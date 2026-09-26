//! Bounded board feedback intake, observation routing, promotion and
//! lead-owned incubator hygiene.
//!
//! Lead and executor observations become `bd` tasks. Listing, merging, voting,
//! promotion, size checks and sweeps invoke only the board CLI: no model calls
//! run in the routine mechanics. Similarity and consequence remain lead
//! judgment, supplied as explicit merge and routing decisions. The controller
//! does not parse the board. Handing a verified procedure to
//! `autonomous-skill-evolution` is a reference only: this module never writes
//! skill packages.
use crate::board_cli::{FEEDBACK_LABEL, INCUBATOR_LABEL, json_ok, json_ok_actor, string_field};
use serde_json::Value;
use std::{io, path::Path};

pub const DEFAULT_FEEDBACK_BATCH_LIMIT: usize = 8;
pub const MAX_OBSERVATION: usize = 512;
pub const MAX_SCOPE: usize = 128;
pub const MAX_REPORTER: usize = 64;
pub const MAX_EPISODE: usize = 64;
pub const MAX_PARENT: usize = 64;
pub const MAX_REASON: usize = 160;

/// Kit default: promote after more than two counted votes.
pub const DEFAULT_VOTE_THRESHOLD: u32 = 3;
pub const DEFAULT_INCUBATOR_SIZE_CAP: usize = 32;

/// Labels owned by this loop on the consuming project's board.
pub const SKILL_EVOLUTION_LABEL: &str = "skill-evolution";
pub const BACKLOG_LABEL: &str = "backlog";
pub const OPENSPEC_LABEL: &str = "openspec";
pub const KIT_FORWARDED_LABEL: &str = "kit-forwarded";
/// Label of sanitized kit-concern items created on the kit's own board.
pub const KIT_FEEDBACK_LABEL: &str = "kit-feedback";

const VOTE_PREFIX: &str = "feedback-vote v1";
const MERGE_PREFIX: &str = "feedback-merge v1";
const ROUTE_PREFIX: &str = "feedback-route v1";
const PROMOTE_PREFIX: &str = "feedback-promote v1";
const ARCHIVE_PREFIX: &str = "feedback-archive v1";
const RESTORE_PREFIX: &str = "feedback-restore v1";
/// Actor recorded for kit-board items created by promotion; carries no
/// consuming-project identity.
const KIT_ROUTING_ACTOR: &str = "feedback-routing";
const MAX_TITLE_BYTES: usize = 72;
/// One OpenSpec change name is a single path segment, never a path.
const MAX_CHANGE_NAME: usize = 96;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReporterKind {
    Lead,
    Executor,
    AutomatedDiagnostic,
}

impl ReporterKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lead => "lead",
            Self::Executor => "executor",
            Self::AutomatedDiagnostic => "diagnostic",
        }
    }

    /// Parses the caller-supplied reporter kind used by the installed CLI.
    pub fn parse(value: &str) -> io::Result<Self> {
        match value {
            "lead" => Ok(Self::Lead),
            "executor" => Ok(Self::Executor),
            "diagnostic" | "automated-diagnostic" => Ok(Self::AutomatedDiagnostic),
            _ => Err(invalid(format!("unknown reporter kind {value}"))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedbackDraft {
    pub observation: String,
    pub scope: String,
    pub reporter: String,
    pub episode: String,
    pub kind: ReporterKind,
    pub parent_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundedFeedback {
    pub observation: String,
    pub scope: String,
    pub reporter: String,
    pub episode: String,
    pub kind: ReporterKind,
    pub parent_id: String,
}

impl BoundedFeedback {
    pub fn try_from_draft(draft: FeedbackDraft) -> io::Result<Self> {
        Ok(Self {
            observation: require_field("observation", &draft.observation, MAX_OBSERVATION)?,
            scope: require_field("scope", &draft.scope, MAX_SCOPE)?,
            reporter: require_token("reporter", &draft.reporter, MAX_REPORTER)?,
            episode: require_token("episode", &draft.episode, MAX_EPISODE)?,
            kind: draft.kind,
            parent_id: require_token("parent", &draft.parent_id, MAX_PARENT)?,
        })
    }

    pub fn title(&self) -> String {
        format!("Feedback: {}", bounded_title(&self.observation))
    }

    pub fn description(&self) -> String {
        format!(
            "observation: {}\nscope: {}\nreporter: {}\nepisode: {}\nkind: {}\n",
            self.observation,
            self.scope,
            self.reporter,
            self.episode,
            self.kind.as_str()
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedbackRow {
    pub id: String,
    pub feedback: BoundedFeedback,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoteReason {
    Counted,
    Repeat,
    AutomatedDiagnostic,
}

impl VoteReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Counted => "counted",
            Self::Repeat => "repeat",
            Self::AutomatedDiagnostic => "automated-diagnostic",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "counted" => Some(Self::Counted),
            "repeat" => Some(Self::Repeat),
            "automated-diagnostic" => Some(Self::AutomatedDiagnostic),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoteCandidate {
    pub episode: String,
    pub reporter: String,
    pub kind: ReporterKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoteDecision {
    pub counted: bool,
    pub reason: VoteReason,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoteRecord {
    pub episode: String,
    pub reporter: String,
    pub kind: ReporterKind,
    pub counted: bool,
    pub reason: VoteReason,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeRecord {
    pub from: String,
    pub into: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoteLedger {
    pub votes: Vec<VoteRecord>,
    pub merges: Vec<MergeRecord>,
    pub routes: Vec<RouteRecord>,
    pub promotions: Vec<PromotionRecord>,
}

impl VoteLedger {
    pub fn counted(&self) -> usize {
        self.votes.iter().filter(|vote| vote.counted).count()
    }
}

/// How an observation is classified before it competes as an incubator vote.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObservationKind {
    /// A verified reusable procedure in owned skill scope.
    SkillProcedure,
    Process,
    Orchestration,
    Requirement,
    Tool,
    Unclear,
    Material,
    /// Kit-wide instruction, skill or tool demand from a consuming project.
    KitConcern,
}

impl ObservationKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SkillProcedure => "skill-procedure",
            Self::Process => "process",
            Self::Orchestration => "orchestration",
            Self::Requirement => "requirement",
            Self::Tool => "tool",
            Self::Unclear => "unclear",
            Self::Material => "material",
            Self::KitConcern => "kit-concern",
        }
    }

    /// Parses the caller-supplied observation kind used by the installed CLI.
    pub fn parse(value: &str) -> io::Result<Self> {
        match value {
            "skill-procedure" => Ok(Self::SkillProcedure),
            "process" => Ok(Self::Process),
            "orchestration" => Ok(Self::Orchestration),
            "requirement" => Ok(Self::Requirement),
            "tool" => Ok(Self::Tool),
            "unclear" => Ok(Self::Unclear),
            "material" => Ok(Self::Material),
            "kit-concern" => Ok(Self::KitConcern),
            _ => Err(invalid(format!("unknown observation kind {value}"))),
        }
    }

    /// Intake routing: a verified reusable procedure in owned skill scope is
    /// handed to `autonomous-skill-evolution`; every other kind incubates.
    pub fn intake(self) -> IntakeRoute {
        match self {
            Self::SkillProcedure => IntakeRoute::SkillEvolution,
            _ => IntakeRoute::Incubator,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntakeRoute {
    SkillEvolution,
    Incubator,
}

/// Promotion consequence: where a promoted incubator item goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromotionRoute {
    BacklogTask,
    OpenSpecChange,
    KitBacklog,
}

impl PromotionRoute {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::BacklogTask => "backlog-task",
            Self::OpenSpecChange => "openspec-change",
            Self::KitBacklog => "kit-backlog",
        }
    }

    /// Parses the caller-supplied promotion route used by the installed CLI.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "backlog-task" => Some(Self::BacklogTask),
            "openspec-change" => Some(Self::OpenSpecChange),
            "kit-backlog" => Some(Self::KitBacklog),
            _ => None,
        }
    }

    /// Label the promoted item keeps on the consuming project's board.
    pub fn label(self) -> &'static str {
        match self {
            Self::BacklogTask => BACKLOG_LABEL,
            Self::OpenSpecChange => OPENSPEC_LABEL,
            Self::KitBacklog => KIT_FORWARDED_LABEL,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteTarget {
    Incubator,
    SkillEvolution,
}

impl RouteTarget {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Incubator => "incubator",
            Self::SkillEvolution => "skill-evolution",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "incubator" => Some(Self::Incubator),
            "skill-evolution" => Some(Self::SkillEvolution),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteRecord {
    pub kind: ObservationKind,
    pub target: RouteTarget,
    /// The observation that supplied this classification.
    pub item: String,
}

/// Kit-level wording of a kit concern. Only these fields may reach the kit
/// board: no reporter, episode, project path or raw transcript.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KitConcern {
    pub summary: String,
    pub scope: String,
}

impl KitConcern {
    fn bounded(&self) -> io::Result<Self> {
        Ok(Self {
            summary: require_field("summary", &self.summary, MAX_OBSERVATION)?,
            scope: require_field("scope", &self.scope, MAX_SCOPE)?,
        })
    }
}

/// Evidence used to promote an incubator item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromotionEvidence {
    /// The configured vote threshold was reached.
    Votes { threshold: u32 },
    /// Lead consequence override for material correctness, integrity or safety
    /// evidence. Both fields are recorded verbatim.
    ConsequenceOverride { consequence: String, reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromotionOutcome {
    pub item_id: String,
    pub route: PromotionRoute,
    /// `None` for the local backlog, `openspec:<name>` or `kit:<item>`.
    pub target: Option<String>,
    pub counted: usize,
    pub override_used: bool,
    /// True when the board already recorded this exact promotion and the
    /// outcome is confirmed from that history: no promotion record was
    /// written, and only labels a partial run missed may have been reconciled.
    pub already_recorded: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromotionCandidate {
    pub item_id: String,
    pub counted: usize,
    pub kinds: Vec<ObservationKind>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromotionRecord {
    pub route: PromotionRoute,
    pub counted: usize,
    pub override_used: bool,
    pub target: Option<String>,
}

/// Deterministic hygiene triggers observed by the lead session. There is no
/// background scheduler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HygieneTrigger {
    /// The lead closes a stage or epic during acceptance.
    StageOrEpicClosed,
    /// A triage batch found the incubator above its configured size cap.
    IncubatorAboveCap { size: usize, cap: usize },
}

impl HygieneTrigger {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::StageOrEpicClosed => "stage-or-epic-closed",
            Self::IncubatorAboveCap { .. } => "incubator-above-cap",
        }
    }

    fn comment_tail(self) -> String {
        match self {
            Self::StageOrEpicClosed => String::new(),
            Self::IncubatorAboveCap { size, cap } => format!(" size={size} cap={cap}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SweepDecision {
    pub item_id: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SweepOutcome {
    Swept {
        archived: Vec<String>,
    },
    /// No lead session is active; the incubator waits unchanged.
    Deferred {
        size: usize,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriageAction {
    pub feedback_id: String,
    pub merge_into: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedAction {
    pub feedback_id: String,
    pub incubator_id: String,
    pub merged: bool,
    pub vote: VoteDecision,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriageReport {
    pub applied: Vec<AppliedAction>,
    pub deferred: usize,
}

/// A triage action that also carries the observation classification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutedAction {
    pub feedback_id: String,
    pub kind: ObservationKind,
    pub merge_into: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutedOutcome {
    pub feedback_id: String,
    pub route: IntakeRoute,
    /// Incubator item the observation landed on, or the handed-off item.
    pub target_id: String,
    /// `Some` for incubated observations, `None` for a skill-evolution
    /// reference handoff (which records no vote).
    pub vote: Option<VoteDecision>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutedReport {
    pub applied: Vec<RoutedOutcome>,
    pub deferred: usize,
}

pub fn decide_vote(existing: &[VoteRecord], candidate: &VoteCandidate) -> VoteDecision {
    if candidate.kind == ReporterKind::AutomatedDiagnostic {
        return VoteDecision {
            counted: false,
            reason: VoteReason::AutomatedDiagnostic,
        };
    }
    if existing.iter().any(|vote| {
        vote.counted && vote.episode == candidate.episode && vote.reporter == candidate.reporter
    }) {
        return VoteDecision {
            counted: false,
            reason: VoteReason::Repeat,
        };
    }
    VoteDecision {
        counted: true,
        reason: VoteReason::Counted,
    }
}

pub fn parse_ledger(comments: &[String]) -> VoteLedger {
    let mut votes = Vec::new();
    let mut merges = Vec::new();
    let mut routes = Vec::new();
    let mut promotions = Vec::new();
    for comment in comments {
        if let Some(vote) = parse_vote_comment(comment) {
            votes.push(vote);
        } else if let Some(merge) = parse_merge_comment(comment) {
            merges.push(merge);
        } else if let Some(route) = parse_route_comment(comment) {
            routes.push(route);
        } else if let Some(promotion) = parse_promotion_comment(comment) {
            promotions.push(promotion);
        }
    }
    VoteLedger {
        votes,
        merges,
        routes,
        promotions,
    }
}

pub fn format_vote_comment(candidate: &VoteCandidate, decision: &VoteDecision) -> String {
    format!(
        "{VOTE_PREFIX} episode={} reporter={} kind={} counted={} reason={}",
        candidate.episode,
        candidate.reporter,
        candidate.kind.as_str(),
        if decision.counted { "true" } else { "false" },
        decision.reason.as_str()
    )
}

pub fn format_merge_comment(from: &str, into: &str) -> String {
    format!("{MERGE_PREFIX} from={from} into={into}")
}

pub fn format_route_comment(kind: ObservationKind, target: RouteTarget, item: &str) -> String {
    format!(
        "{ROUTE_PREFIX} kind={} target={} item={item}",
        kind.as_str(),
        target.as_str()
    )
}

pub fn record_feedback(
    bd: &Path,
    project: &Path,
    feedback: &BoundedFeedback,
) -> io::Result<String> {
    let title = feedback.title();
    let description = feedback.description();
    let created = json_ok_actor(
        bd,
        project,
        &feedback.reporter,
        &[
            "create",
            &title,
            "--type",
            "task",
            "--labels",
            FEEDBACK_LABEL,
            "--parent",
            &feedback.parent_id,
            "--description",
            &description,
            "--json",
        ],
    )?;
    string_field(&created, "id")
}

pub fn list_feedback(bd: &Path, project: &Path) -> io::Result<Vec<FeedbackRow>> {
    let listed = json_ok(
        bd,
        project,
        &[
            "list",
            "--label",
            FEEDBACK_LABEL,
            "--status",
            "open",
            "--json",
            "--brief",
        ],
    )?;
    let mut rows = Vec::new();
    for id in issue_ids(&listed)? {
        if let Some(feedback) = load_feedback(bd, project, &id)? {
            rows.push(FeedbackRow { id, feedback });
        }
    }
    Ok(rows)
}

pub fn inspect_ledger(bd: &Path, project: &Path, item_id: &str) -> io::Result<VoteLedger> {
    Ok(parse_ledger(&list_comments(bd, project, item_id)?))
}

pub fn apply_triage(
    bd: &Path,
    project: &Path,
    actions: &[TriageAction],
    batch_limit: usize,
) -> io::Result<TriageReport> {
    if batch_limit == 0 {
        return Err(invalid("feedback batch limit must be positive"));
    }
    let deferred = actions.len().saturating_sub(batch_limit);
    let mut applied = Vec::new();
    for action in actions.iter().take(batch_limit) {
        applied.push(apply_action(bd, project, action)?);
    }
    Ok(TriageReport { applied, deferred })
}

/// Applies classified triage actions in one bounded batch. A verified
/// procedure is handed to `autonomous-skill-evolution` as a reference and
/// records no vote; every other observation is admitted (or merged) into the
/// incubator with one counted vote and a visible classification.
pub fn apply_routed_triage(
    bd: &Path,
    project: &Path,
    actions: &[RoutedAction],
    batch_limit: usize,
) -> io::Result<RoutedReport> {
    if batch_limit == 0 {
        return Err(invalid("feedback batch limit must be positive"));
    }
    let deferred = actions.len().saturating_sub(batch_limit);
    let mut applied = Vec::new();
    for action in actions.iter().take(batch_limit) {
        applied.push(apply_routed_action(bd, project, action)?);
    }
    Ok(RoutedReport { applied, deferred })
}

/// The action a bounded batch stopped on and why. The operation text is the
/// exact identity the caller reissues for recovery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchFailure {
    pub operation: String,
    pub error: String,
}

/// A bounded batch that keeps the applied prefix when a later action fails.
/// Recovery reissues the remaining actions: admitted observations keep one
/// counted vote, an existing merge or route record is not repeated, and a
/// non-open merge whose duplicate dependency is already recorded continues.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartialRoutedReport {
    pub applied: Vec<RoutedOutcome>,
    pub deferred: usize,
    pub failure: Option<BatchFailure>,
}

/// Applies classified triage actions in one bounded batch and reports a
/// partial failure instead of discarding the applied prefix.
pub fn apply_routed_triage_recovering(
    bd: &Path,
    project: &Path,
    actions: &[RoutedAction],
    batch_limit: usize,
) -> io::Result<PartialRoutedReport> {
    if batch_limit == 0 {
        return Err(invalid("feedback batch limit must be positive"));
    }
    let deferred = actions.len().saturating_sub(batch_limit);
    let mut applied = Vec::new();
    for action in actions.iter().take(batch_limit) {
        match apply_routed_action(bd, project, action) {
            Ok(outcome) => applied.push(outcome),
            Err(error) => {
                return Ok(PartialRoutedReport {
                    applied,
                    deferred,
                    failure: Some(BatchFailure {
                        operation: routed_operation(action),
                        error: error.to_string(),
                    }),
                });
            }
        }
    }
    Ok(PartialRoutedReport {
        applied,
        deferred,
        failure: None,
    })
}

/// One triage action as the recovery report and the caller identify it.
pub fn routed_operation(action: &RoutedAction) -> String {
    match action.merge_into.as_deref() {
        Some(canonical) => format!(
            "feedback {} kind={} merge_into={canonical}",
            action.feedback_id,
            action.kind.as_str()
        ),
        None => format!(
            "feedback {} kind={}",
            action.feedback_id,
            action.kind.as_str()
        ),
    }
}

fn apply_routed_action(
    bd: &Path,
    project: &Path,
    action: &RoutedAction,
) -> io::Result<RoutedOutcome> {
    match action.kind.intake() {
        IntakeRoute::SkillEvolution => {
            if action.merge_into.is_some() {
                return Err(invalid(format!(
                    "feedback {} is a verified procedure; it is handed to skill-evolution instead of merging into an incubator item",
                    action.feedback_id
                )));
            }
            hand_off_procedure(bd, project, &action.feedback_id)?;
            Ok(RoutedOutcome {
                feedback_id: action.feedback_id.clone(),
                route: IntakeRoute::SkillEvolution,
                target_id: action.feedback_id.clone(),
                vote: None,
            })
        }
        IntakeRoute::Incubator => {
            let triage = TriageAction {
                feedback_id: action.feedback_id.clone(),
                merge_into: action.merge_into.clone(),
            };
            let applied = apply_action_kind(bd, project, &triage, Some(action.kind))?;
            Ok(RoutedOutcome {
                feedback_id: applied.feedback_id,
                route: IntakeRoute::Incubator,
                target_id: applied.incubator_id,
                vote: Some(applied.vote),
            })
        }
    }
}

/// Hands a verified reusable procedure to `autonomous-skill-evolution` as a
/// reference only: one route record plus the handoff label. No skill package
/// is written and `SKILL.md` is never touched.
fn hand_off_procedure(bd: &Path, project: &Path, feedback_id: &str) -> io::Result<()> {
    let snapshot = load_snapshot(bd, project, feedback_id)?
        .ok_or_else(|| invalid(format!("feedback {feedback_id} is missing")))?;
    if snapshot.status != "open" {
        return Err(invalid(format!("feedback {feedback_id} is not open")));
    }
    if snapshot.feedback.is_none() {
        return Err(invalid(format!(
            "feedback {feedback_id} is missing bounded fields"
        )));
    }
    let ledger = inspect_ledger(bd, project, feedback_id)?;
    if ledger
        .routes
        .iter()
        .any(|route| route.target == RouteTarget::SkillEvolution)
    {
        // Repair labels after an interrupted handoff; never duplicate the
        // route record.
        json_ok(
            bd,
            project,
            &["label", "add", feedback_id, SKILL_EVOLUTION_LABEL, "--json"],
        )?;
        json_ok(
            bd,
            project,
            &["label", "remove", feedback_id, FEEDBACK_LABEL, "--json"],
        )?;
        return Ok(());
    }
    if ledger.counted() > 0
        || !ledger.routes.is_empty()
        || snapshot.labels.iter().any(|label| label == INCUBATOR_LABEL)
    {
        return Err(invalid(format!(
            "feedback {feedback_id} already competes as incubator demand"
        )));
    }
    json_ok(
        bd,
        project,
        &[
            "comment",
            feedback_id,
            "--json",
            &format_route_comment(
                ObservationKind::SkillProcedure,
                RouteTarget::SkillEvolution,
                feedback_id,
            ),
        ],
    )?;
    json_ok(
        bd,
        project,
        &["label", "add", feedback_id, SKILL_EVOLUTION_LABEL, "--json"],
    )?;
    json_ok(
        bd,
        project,
        &["label", "remove", feedback_id, FEEDBACK_LABEL, "--json"],
    )?;
    Ok(())
}

fn apply_action(bd: &Path, project: &Path, action: &TriageAction) -> io::Result<AppliedAction> {
    apply_action_kind(bd, project, action, None)
}

fn apply_action_kind(
    bd: &Path,
    project: &Path,
    action: &TriageAction,
    kind: Option<ObservationKind>,
) -> io::Result<AppliedAction> {
    let snapshot = load_snapshot(bd, project, &action.feedback_id)?.ok_or_else(|| {
        invalid(format!(
            "feedback {} is missing bounded fields",
            action.feedback_id
        ))
    })?;
    let incoming = snapshot.feedback.ok_or_else(|| {
        invalid(format!(
            "feedback {} is missing bounded fields",
            action.feedback_id
        ))
    })?;
    // A retry after the duplicate step of a merge must continue, not fail on
    // the closed incoming item: the recorded duplicate dependency proves the
    // merge was already applied.
    let already_merged = action
        .merge_into
        .as_deref()
        .is_some_and(|canonical| snapshot.duplicates.as_deref() == Some(canonical));
    if kind.is_some() && snapshot.status != "open" && !already_merged {
        return Err(invalid(format!(
            "feedback {} is not open",
            action.feedback_id
        )));
    }
    let candidate = VoteCandidate {
        episode: incoming.episode.clone(),
        reporter: incoming.reporter.clone(),
        kind: incoming.kind,
    };
    match action.merge_into.as_deref() {
        None => {
            reject_skill_handoff(bd, project, &action.feedback_id, &snapshot.labels)?;
            json_ok(
                bd,
                project,
                &[
                    "label",
                    "add",
                    &action.feedback_id,
                    INCUBATOR_LABEL,
                    "--json",
                ],
            )?;
            json_ok(
                bd,
                project,
                &[
                    "label",
                    "remove",
                    &action.feedback_id,
                    FEEDBACK_LABEL,
                    "--json",
                ],
            )?;
            let (vote, ledger) = record_vote(bd, project, &action.feedback_id, &candidate)?;
            let applied = AppliedAction {
                feedback_id: action.feedback_id.clone(),
                incubator_id: action.feedback_id.clone(),
                merged: false,
                vote,
            };
            if let Some(kind) = kind {
                record_route_once(
                    bd,
                    project,
                    &applied.incubator_id,
                    kind,
                    &applied.feedback_id,
                    &ledger,
                )?;
            }
            Ok(applied)
        }
        Some(canonical) => {
            require_open_incubator(bd, project, canonical)?;
            let incoming_snapshot =
                load_snapshot(bd, project, &action.feedback_id)?.ok_or_else(|| {
                    invalid(format!(
                        "feedback {} is missing bounded fields",
                        action.feedback_id
                    ))
                })?;
            reject_skill_handoff(bd, project, &action.feedback_id, &incoming_snapshot.labels)?;
            let ledger = inspect_ledger(bd, project, canonical)?;
            // A retry converges instead of duplicating the merge or its
            // record: the recorded merge is the applied prefix of this action.
            if !ledger
                .merges
                .iter()
                .any(|merge| merge.from == action.feedback_id)
            {
                json_ok(
                    bd,
                    project,
                    &[
                        "duplicate",
                        &action.feedback_id,
                        "--of",
                        canonical,
                        "--json",
                    ],
                )?;
                json_ok(
                    bd,
                    project,
                    &[
                        "comment",
                        canonical,
                        "--json",
                        &format_merge_comment(&action.feedback_id, canonical),
                    ],
                )?;
            }
            let (vote, ledger) = record_vote(bd, project, canonical, &candidate)?;
            let applied = AppliedAction {
                feedback_id: action.feedback_id.clone(),
                incubator_id: canonical.to_owned(),
                merged: true,
                vote,
            };
            if let Some(kind) = kind {
                record_route_once(
                    bd,
                    project,
                    &applied.incubator_id,
                    kind,
                    &applied.feedback_id,
                    &ledger,
                )?;
            }
            Ok(applied)
        }
    }
}

fn reject_skill_handoff(
    bd: &Path,
    project: &Path,
    feedback_id: &str,
    labels: &[String],
) -> io::Result<()> {
    if labels.iter().any(|label| label == SKILL_EVOLUTION_LABEL) {
        return Err(invalid(format!(
            "feedback {feedback_id} was handed to skill-evolution; it cannot compete as incubator demand"
        )));
    }
    let ledger = inspect_ledger(bd, project, feedback_id)?;
    if ledger
        .routes
        .iter()
        .any(|route| route.target == RouteTarget::SkillEvolution)
    {
        return Err(invalid(format!(
            "feedback {feedback_id} was handed to skill-evolution; it cannot compete as incubator demand"
        )));
    }
    Ok(())
}

fn record_route(
    bd: &Path,
    project: &Path,
    item_id: &str,
    kind: ObservationKind,
    source_id: &str,
) -> io::Result<()> {
    json_ok(
        bd,
        project,
        &[
            "comment",
            item_id,
            "--json",
            &format_route_comment(kind, RouteTarget::Incubator, source_id),
        ],
    )?;
    Ok(())
}

/// Records an incubator classification once. A retried action that already
/// recorded its route record converges without a duplicate comment.
fn record_route_once(
    bd: &Path,
    project: &Path,
    item_id: &str,
    kind: ObservationKind,
    source_id: &str,
    ledger: &VoteLedger,
) -> io::Result<()> {
    let recorded = ledger.routes.iter().any(|route| {
        route.target == RouteTarget::Incubator && route.kind == kind && route.item == source_id
    });
    if recorded {
        return Ok(());
    }
    record_route(bd, project, item_id, kind, source_id)
}

/// Default consequence route for the recorded observation kinds: a kit concern
/// goes to the kit backlog, a requirement change enters OpenSpec, anything
/// else is a small improvement for the local backlog. `None` when the item is
/// unclassified or merges a kit concern with project-scope observations.
pub fn default_promotion_route(kinds: &[ObservationKind]) -> Option<PromotionRoute> {
    if kinds.is_empty() {
        return None;
    }
    if kinds.contains(&ObservationKind::KitConcern) {
        return kinds
            .iter()
            .all(|kind| *kind == ObservationKind::KitConcern)
            .then_some(PromotionRoute::KitBacklog);
    }
    if kinds.contains(&ObservationKind::Requirement) {
        return Some(PromotionRoute::OpenSpecChange);
    }
    Some(PromotionRoute::BacklogTask)
}

fn check_promotion_route(kinds: &[ObservationKind], route: PromotionRoute) -> io::Result<()> {
    if kinds.is_empty() {
        return Err(invalid(
            "incubator item is not classified; record an observation kind before promotion",
        ));
    }
    if kinds.contains(&ObservationKind::SkillProcedure) {
        return Err(invalid(
            "a verified reusable procedure is handed to skill-evolution, not promoted",
        ));
    }
    if kinds.contains(&ObservationKind::KitConcern) {
        if kinds
            .iter()
            .any(|kind| *kind != ObservationKind::KitConcern)
        {
            return Err(invalid(
                "kit concern is merged with project-scope observations; split the item before promotion",
            ));
        }
        if route != PromotionRoute::KitBacklog {
            return Err(invalid(
                "kit concerns promote to the kit backlog, not to the project backlog",
            ));
        }
        return Ok(());
    }
    if route == PromotionRoute::KitBacklog {
        return Err(invalid(
            "only kit instruction, skill or tool concerns promote to the kit backlog",
        ));
    }
    if kinds.contains(&ObservationKind::Requirement) && route != PromotionRoute::OpenSpecChange {
        return Err(invalid(
            "a behavior or requirement change enters OpenSpec instead of the local backlog",
        ));
    }
    Ok(())
}

/// Lists incubator items at or above the configured promotion threshold using
/// only the board CLI. The lead decides when to run this at a safe boundary.
pub fn promotion_candidates(
    bd: &Path,
    project: &Path,
    threshold: u32,
) -> io::Result<Vec<PromotionCandidate>> {
    if threshold < 2 {
        return Err(invalid("vote threshold must be at least 2"));
    }
    let mut candidates = Vec::new();
    for item_id in list_incubator(bd, project)? {
        let ledger = inspect_ledger(bd, project, &item_id)?;
        let counted = ledger.counted();
        if counted >= threshold as usize {
            candidates.push(PromotionCandidate {
                item_id,
                counted,
                kinds: incubator_kinds(&ledger),
            });
        }
    }
    candidates.sort_by(|left, right| {
        right
            .counted
            .cmp(&left.counted)
            .then_with(|| left.item_id.cmp(&right.item_id))
    });
    Ok(candidates)
}

/// Promotes an incubator item into the local backlog or an existing OpenSpec
/// change entry. History (votes, merges, classifications) stays on the item.
/// `openspec_change` names the intended change for the OpenSpec route; `None`
/// derives the `feedback-<item>` name. The OpenSpec workflow owns change
/// creation and its artifacts: this loop validates and records the reference
/// instead of writing unscaffolded entries, so a retry preserves an existing
/// draft and only reconciles the board.
pub fn promote_item(
    bd: &Path,
    project: &Path,
    item_id: &str,
    route: PromotionRoute,
    evidence: &PromotionEvidence,
    openspec_change: Option<&str>,
) -> io::Result<PromotionOutcome> {
    if route == PromotionRoute::KitBacklog {
        return Err(invalid(
            "kit concerns promote through promote_kit_concern so private consuming-project data stays off the kit board",
        ));
    }
    let snapshot = load_snapshot(bd, project, item_id)?
        .ok_or_else(|| invalid(format!("item {item_id} is missing")))?;
    let ledger = inspect_ledger(bd, project, item_id)?;
    if let Some(record) = ledger.promotions.first() {
        // A recorded promotion is never repeated: confirm the recorded outcome,
        // reconcile any label step a partial run left behind, and refuse a
        // different consequence the caller now asks for.
        let requested_target = match route {
            PromotionRoute::OpenSpecChange => Some(format!(
                "openspec:{}",
                openspec_change_name(item_id, openspec_change)?
            )),
            _ => None,
        };
        return reconcile_promotion(
            bd,
            project,
            item_id,
            &snapshot.labels,
            record,
            Some(route),
            requested_target.as_deref(),
        );
    }
    require_open_incubator_snapshot(&snapshot, item_id)?;
    let kinds = incubator_kinds(&ledger);
    check_promotion_route(&kinds, route)?;
    // The route's structural precondition is caller input: the intended
    // OpenSpec change is validated before the vote evidence, so a missing or
    // malformed reference is reported exactly and nothing is recorded.
    let target = match route {
        PromotionRoute::BacklogTask => None,
        PromotionRoute::OpenSpecChange => {
            let name = openspec_change_name(item_id, openspec_change)?;
            require_openspec_change(project, &name)?;
            Some(format!("openspec:{name}"))
        }
        PromotionRoute::KitBacklog => unreachable!(),
    };
    let (counted, override_note) = validate_evidence(&ledger, evidence)?;
    let comment = PromotionComment {
        route,
        counted,
        threshold: match evidence {
            PromotionEvidence::Votes { threshold } => Some(*threshold),
            PromotionEvidence::ConsequenceOverride { .. } => None,
        },
        target: target.as_deref(),
        override_note: override_note.as_ref(),
    }
    .format();
    json_ok(bd, project, &["comment", item_id, "--json", &comment])?;
    json_ok(
        bd,
        project,
        &["label", "remove", item_id, INCUBATOR_LABEL, "--json"],
    )?;
    json_ok(
        bd,
        project,
        &["label", "add", item_id, route.label(), "--json"],
    )?;
    Ok(PromotionOutcome {
        item_id: item_id.to_owned(),
        route,
        target,
        counted,
        override_used: override_note.is_some(),
        already_recorded: false,
    })
}

/// Promotes a kit concern to the kit's own backlog. Only the explicit
/// kit-level summary and scope reach the kit board: no reporter, episode,
/// project path or raw observation is copied.
pub fn promote_kit_concern(
    bd: &Path,
    project: &Path,
    kit_project: &Path,
    item_id: &str,
    concern: &KitConcern,
    evidence: &PromotionEvidence,
) -> io::Result<PromotionOutcome> {
    let snapshot = load_snapshot(bd, project, item_id)?
        .ok_or_else(|| invalid(format!("item {item_id} is missing")))?;
    let ledger = inspect_ledger(bd, project, item_id)?;
    if let Some(record) = ledger.promotions.first() {
        // The kit task was already created and recorded; repeating the run
        // confirms the recorded outcome and reconciles labels without creating
        // a second kit item.
        return reconcile_promotion(
            bd,
            project,
            item_id,
            &snapshot.labels,
            record,
            Some(PromotionRoute::KitBacklog),
            None,
        );
    }
    require_open_incubator_snapshot(&snapshot, item_id)?;
    let kinds = incubator_kinds(&ledger);
    check_promotion_route(&kinds, PromotionRoute::KitBacklog)?;
    let (counted, override_note) = validate_evidence(&ledger, evidence)?;
    let concern = concern.bounded()?;
    let kit_id = create_kit_item(bd, kit_project, &concern)?;
    let target = format!("kit:{kit_id}");
    let comment = PromotionComment {
        route: PromotionRoute::KitBacklog,
        counted,
        threshold: match evidence {
            PromotionEvidence::Votes { threshold } => Some(*threshold),
            PromotionEvidence::ConsequenceOverride { .. } => None,
        },
        target: Some(target.as_str()),
        override_note: override_note.as_ref(),
    }
    .format();
    json_ok(bd, project, &["comment", item_id, "--json", &comment])?;
    json_ok(
        bd,
        project,
        &["label", "remove", item_id, INCUBATOR_LABEL, "--json"],
    )?;
    json_ok(
        bd,
        project,
        &[
            "label",
            "add",
            item_id,
            PromotionRoute::KitBacklog.label(),
            "--json",
        ],
    )?;
    Ok(PromotionOutcome {
        item_id: item_id.to_owned(),
        route: PromotionRoute::KitBacklog,
        target: Some(target),
        counted,
        override_used: override_note.is_some(),
        already_recorded: false,
    })
}

/// Non-interactive incubator size. The board query is the whole check: no
/// model call is made or needed.
pub fn incubator_size(bd: &Path, project: &Path) -> io::Result<usize> {
    Ok(list_incubator(bd, project)?.len())
}

/// `Some(size)` when the incubator is above its configured cap. Never makes a
/// model call.
pub fn incubator_over_cap(bd: &Path, project: &Path, cap: usize) -> io::Result<Option<usize>> {
    if cap == 0 {
        return Err(invalid("incubator size cap must be positive"));
    }
    let size = incubator_size(bd, project)?;
    Ok((size > cap).then_some(size))
}

/// Lead-owned incubator sweep. A non-lead caller (no lead session active)
/// defers without touching the board; archived items keep their labels,
/// comments and votes so fresh evidence can restore them.
pub fn sweep_incubator(
    bd: &Path,
    project: &Path,
    caller: ReporterKind,
    trigger: HygieneTrigger,
    decisions: &[SweepDecision],
) -> io::Result<SweepOutcome> {
    if caller != ReporterKind::Lead {
        return Ok(SweepOutcome::Deferred {
            size: incubator_size(bd, project)?,
        });
    }
    if let HygieneTrigger::IncubatorAboveCap { size, cap } = trigger {
        if cap == 0 {
            return Err(invalid("incubator size cap must be positive"));
        }
        let actual = incubator_size(bd, project)?;
        if actual <= cap {
            return Err(invalid(format!(
                "incubator size {actual} is not above cap {cap}; the trigger does not match the board"
            )));
        }
        if actual != size {
            return Err(invalid(format!(
                "reported incubator size {size} does not match the board ({actual})"
            )));
        }
    }
    let mut archived = Vec::new();
    for decision in decisions {
        let reason = require_field("reason", &decision.reason, MAX_REASON)?;
        require_open_incubator(bd, project, &decision.item_id)?;
        json_ok(
            bd,
            project,
            &[
                "comment",
                &decision.item_id,
                "--json",
                &format!(
                    "{ARCHIVE_PREFIX} trigger={}{} reason={reason}",
                    trigger.as_str(),
                    trigger.comment_tail()
                ),
            ],
        )?;
        json_ok(
            bd,
            project,
            &[
                "close",
                &decision.item_id,
                "--reason",
                &format!("archived: {reason}"),
                "--json",
            ],
        )?;
        archived.push(decision.item_id.clone());
    }
    Ok(SweepOutcome::Swept { archived })
}

/// Restores an archived item on fresh evidence. The item keeps its incubator
/// label, merge history and votes.
pub fn restore_archived(bd: &Path, project: &Path, item_id: &str, reason: &str) -> io::Result<()> {
    let reason = require_field("reason", reason, MAX_REASON)?;
    let snapshot = load_snapshot(bd, project, item_id)?
        .ok_or_else(|| invalid(format!("item {item_id} is missing")))?;
    if !snapshot.labels.iter().any(|label| label == INCUBATOR_LABEL) {
        return Err(invalid(format!("item {item_id} is not an incubator item")));
    }
    json_ok(
        bd,
        project,
        &[
            "comment",
            item_id,
            "--json",
            &format!("{RESTORE_PREFIX} reason={reason}"),
        ],
    )?;
    json_ok(
        bd,
        project,
        &["reopen", item_id, "--reason", &reason, "--json"],
    )?;
    Ok(())
}

struct PromotionComment<'a> {
    route: PromotionRoute,
    counted: usize,
    threshold: Option<u32>,
    target: Option<&'a str>,
    override_note: Option<&'a (String, String)>,
}

impl PromotionComment<'_> {
    fn format(&self) -> String {
        let basis = if self.override_note.is_some() {
            "override"
        } else {
            "votes"
        };
        let threshold = match self.threshold {
            Some(value) => value.to_string(),
            None => "none".to_owned(),
        };
        let target = self.target.unwrap_or("none");
        let mut text = format!(
            "{PROMOTE_PREFIX} route={} basis={basis} counted={} threshold={threshold} target={target}",
            self.route.as_str(),
            self.counted
        );
        if let Some((consequence, reason)) = self.override_note {
            text.push_str(&format!(" consequence={consequence} reason={reason}"));
        }
        text
    }
}

fn validate_evidence(
    ledger: &VoteLedger,
    evidence: &PromotionEvidence,
) -> io::Result<(usize, Option<(String, String)>)> {
    let counted = ledger.counted();
    match evidence {
        PromotionEvidence::Votes { threshold } => {
            if *threshold < 2 {
                return Err(invalid("vote threshold must be at least 2"));
            }
            if counted < *threshold as usize {
                return Err(invalid(format!(
                    "counted votes {counted} are below the configured threshold {threshold}"
                )));
            }
            Ok((counted, None))
        }
        PromotionEvidence::ConsequenceOverride {
            consequence,
            reason,
        } => {
            let consequence = require_field("consequence", consequence, MAX_REASON)?;
            let reason = require_field("reason", reason, MAX_REASON)?;
            Ok((counted, Some((consequence, reason))))
        }
    }
}

fn incubator_kinds(ledger: &VoteLedger) -> Vec<ObservationKind> {
    let mut kinds = Vec::new();
    for route in &ledger.routes {
        if route.target == RouteTarget::Incubator && !kinds.contains(&route.kind) {
            kinds.push(route.kind);
        }
    }
    kinds
}

fn list_incubator(bd: &Path, project: &Path) -> io::Result<Vec<String>> {
    let listed = json_ok(
        bd,
        project,
        &[
            "list",
            "--label",
            INCUBATOR_LABEL,
            "--status",
            "open",
            "--json",
            "--brief",
        ],
    )?;
    issue_ids(&listed)
}

fn require_open_incubator(bd: &Path, project: &Path, item_id: &str) -> io::Result<IssueSnapshot> {
    let snapshot = load_snapshot(bd, project, item_id)?
        .ok_or_else(|| invalid(format!("item {item_id} is missing")))?;
    require_open_incubator_snapshot(&snapshot, item_id)?;
    Ok(snapshot)
}

/// The open-incubator precondition, shared with callers that already loaded
/// the issue so one board read serves the whole operation.
fn require_open_incubator_snapshot(snapshot: &IssueSnapshot, item_id: &str) -> io::Result<()> {
    if snapshot.status != "open" {
        return Err(invalid(format!("item {item_id} is not open")));
    }
    if !snapshot.labels.iter().any(|label| label == INCUBATOR_LABEL) {
        return Err(invalid(format!("item {item_id} is not an incubator item")));
    }
    Ok(())
}

/// Completes a promotion whose record is already on the board. The recorded
/// history is authoritative: the outcome is returned as recorded and only the
/// label operations a partial run may have missed are reconciled, so a retry
/// never writes a second promotion record or a second kit task. A completed
/// retry confirms that recorded outcome successfully; a different route or a
/// different OpenSpec target is a different consequence and stays refused.
fn reconcile_promotion(
    bd: &Path,
    project: &Path,
    item_id: &str,
    labels: &[String],
    record: &PromotionRecord,
    requested: Option<PromotionRoute>,
    requested_target: Option<&str>,
) -> io::Result<PromotionOutcome> {
    if let Some(requested) = requested.filter(|requested| *requested != record.route) {
        return Err(invalid(format!(
            "incubator item {item_id} is already promoted with route={}; repeat that route instead of {}",
            record.route.as_str(),
            requested.as_str()
        )));
    }
    if let Some(requested_target) = requested_target
        && record.target.as_deref() != Some(requested_target)
    {
        return Err(invalid(format!(
            "incubator item {item_id} is already promoted into {}; the recorded consequence is retained: repeat that target instead of {requested_target}",
            record.target.as_deref().unwrap_or("none")
        )));
    }
    let incubating = labels.iter().any(|label| label == INCUBATOR_LABEL);
    let routed = labels.iter().any(|label| label == record.route.label());
    if incubating {
        json_ok(
            bd,
            project,
            &["label", "remove", item_id, INCUBATOR_LABEL, "--json"],
        )?;
    }
    if !routed {
        json_ok(
            bd,
            project,
            &["label", "add", item_id, record.route.label(), "--json"],
        )?;
    }
    Ok(PromotionOutcome {
        item_id: item_id.to_owned(),
        route: record.route,
        target: record.target.clone(),
        counted: record.counted,
        override_used: record.override_used,
        already_recorded: true,
    })
}

/// The OpenSpec change a requirement promotion enters: the caller's explicit
/// reference, or the derived `feedback-<item>` name the same item would have
/// recorded before. A single path segment only.
fn openspec_change_name(item_id: &str, requested: Option<&str>) -> io::Result<String> {
    match requested {
        Some(name) => {
            let name = name.trim();
            if name.is_empty() {
                return Err(invalid("the OpenSpec change reference is empty"));
            }
            if name.len() > MAX_CHANGE_NAME {
                return Err(invalid(format!(
                    "OpenSpec change reference {name} is {} bytes; the limit is {MAX_CHANGE_NAME}",
                    name.len()
                )));
            }
            if name == "."
                || name == ".."
                || !name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
            {
                return Err(invalid(format!(
                    "OpenSpec change reference {name} must be one path name of ASCII letters, digits, '-', '_' or '.'"
                )));
            }
            Ok(name.to_owned())
        }
        None => {
            let name = format!("feedback-{}", bounded_slug(item_id));
            if name == "feedback-" {
                return Err(invalid(
                    "incubator item id has no usable OpenSpec change name; name the intended change explicitly",
                ));
            }
            Ok(name)
        }
    }
}

/// Validates the change directory a promotion records. Change creation and
/// its artifacts belong to the OpenSpec workflow, so this loop never writes
/// into `openspec/` and never touches templates, configuration or skills; an
/// existing draft is preserved, which is what makes a promotion that stopped
/// after the board read recoverable by rerunning it.
fn require_openspec_change(project: &Path, name: &str) -> io::Result<()> {
    let directory = project.join("openspec/changes").join(name);
    if !directory.is_dir() {
        return Err(invalid(format!(
            "OpenSpec change {name} does not exist at {}; create it through the OpenSpec workflow (for example `openspec new change {name}`) and rerun this promotion: nothing was recorded and no artifact was written",
            directory.display()
        )));
    }
    let recognized =
        directory.join(".openspec.yaml").is_file() || directory.join("proposal.md").is_file();
    if !recognized {
        return Err(invalid(format!(
            "{} exists but is not an OpenSpec change directory (no .openspec.yaml or proposal.md); resolve it before promoting",
            directory.display()
        )));
    }
    Ok(())
}

/// Creates the sanitized kit-backlog task from explicit kit-level wording.
fn create_kit_item(bd: &Path, kit_project: &Path, concern: &KitConcern) -> io::Result<String> {
    if let Some(existing) = find_kit_item(bd, kit_project, concern)? {
        // A previous run created this concern's kit task and stopped before
        // recording the promotion; reuse it instead of duplicating it.
        return Ok(existing);
    }
    let title = format!("Kit feedback: {}", bounded_title(&concern.summary));
    let description = format!(
        "summary: {}\nscope: {}\nkind: kit-concern\n",
        concern.summary, concern.scope
    );
    let created = json_ok_actor(
        bd,
        kit_project,
        KIT_ROUTING_ACTOR,
        &[
            "create",
            &title,
            "--type",
            "task",
            "--labels",
            KIT_FEEDBACK_LABEL,
            "--description",
            &description,
            "--json",
        ],
    )?;
    string_field(&created, "id")
}

/// Finds the open kit item a retry of this exact concern would have created.
/// Kit items are created with kit-level wording only, and the bounded title is
/// a prefix of the summary: two different concerns can share one title, so the
/// recorded sanitized summary and scope must match exactly as well. Reusing a
/// task that only shares the title would silently misroute the other concern.
fn find_kit_item(
    bd: &Path,
    kit_project: &Path,
    concern: &KitConcern,
) -> io::Result<Option<String>> {
    let title = format!("Kit feedback: {}", bounded_title(&concern.summary));
    let listed = json_ok(
        bd,
        kit_project,
        &[
            "list",
            "--label",
            KIT_FEEDBACK_LABEL,
            "--status",
            "open",
            "--json",
            "--brief",
        ],
    )?;
    let rows = match &listed {
        Value::Array(rows) => rows.as_slice(),
        _ => &[],
    };
    for row in rows {
        if row.get("title").and_then(Value::as_str) != Some(title.as_str()) {
            continue;
        }
        let Some(id) = row.get("id").and_then(Value::as_str) else {
            continue;
        };
        if kit_item_is_concern(bd, kit_project, id, concern)? {
            return Ok(Some(id.to_owned()));
        }
    }
    Ok(None)
}

/// Whether one open kit item records exactly this sanitized concern. The
/// description carries only the kit-level summary, scope and kind; nothing
/// private is read or compared.
fn kit_item_is_concern(
    bd: &Path,
    kit_project: &Path,
    id: &str,
    concern: &KitConcern,
) -> io::Result<bool> {
    let shown = json_ok(bd, kit_project, &["show", id, "--json"])?;
    let issue = issue_object(&shown, id)?;
    let mut summary = None;
    let mut scope = None;
    for line in issue_description(issue).lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        match key.trim() {
            "summary" => summary = Some(value.trim().to_owned()),
            "scope" => scope = Some(value.trim().to_owned()),
            _ => {}
        }
    }
    Ok(summary.as_deref() == Some(concern.summary.as_str())
        && scope.as_deref() == Some(concern.scope.as_str()))
}

fn bounded_slug(value: &str) -> String {
    let mut slug = String::new();
    for character in value.chars() {
        if character.is_ascii_alphanumeric() {
            slug.push(character.to_ascii_lowercase());
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    slug.trim_matches('-').to_owned()
}

fn record_vote(
    bd: &Path,
    project: &Path,
    item_id: &str,
    candidate: &VoteCandidate,
) -> io::Result<(VoteDecision, VoteLedger)> {
    let ledger = inspect_ledger(bd, project, item_id)?;
    let decision = decide_vote(&ledger.votes, candidate);
    json_ok(
        bd,
        project,
        &[
            "comment",
            item_id,
            "--json",
            &format_vote_comment(candidate, &decision),
        ],
    )?;
    Ok((decision, ledger))
}

fn load_feedback(bd: &Path, project: &Path, id: &str) -> io::Result<Option<BoundedFeedback>> {
    Ok(load_snapshot(bd, project, id)?.and_then(|snapshot| snapshot.feedback))
}

struct IssueSnapshot {
    status: String,
    labels: Vec<String>,
    feedback: Option<BoundedFeedback>,
    /// The canonical item this issue is recorded as a duplicate of, when the
    /// board carries that dependency. A retried merge uses it to recognize the
    /// duplicate step that already ran.
    duplicates: Option<String>,
}

fn load_snapshot(bd: &Path, project: &Path, id: &str) -> io::Result<Option<IssueSnapshot>> {
    let shown = json_ok(bd, project, &["show", id, "--json"])?;
    let issue = issue_object(&shown, id)?;
    let feedback = parse_description(issue_description(issue)).ok();
    Ok(Some(IssueSnapshot {
        status: issue_status(issue),
        labels: issue_labels(issue),
        feedback,
        duplicates: duplicate_dependency(issue),
    }))
}

fn duplicate_dependency(issue: &Value) -> Option<String> {
    issue
        .get("dependencies")
        .and_then(Value::as_array)?
        .iter()
        .find(|dependency| {
            dependency.get("dependency_type").and_then(Value::as_str) == Some("duplicates")
        })
        .and_then(|dependency| dependency.get("id").and_then(Value::as_str))
        .map(str::to_owned)
}

fn issue_status(issue: &Value) -> String {
    issue
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned()
}

fn issue_labels(issue: &Value) -> Vec<String> {
    match issue.get("labels") {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect(),
        Some(Value::String(text)) => text
            .split(',')
            .map(str::trim)
            .filter(|label| !label.is_empty())
            .map(str::to_owned)
            .collect(),
        _ => Vec::new(),
    }
}

/// The comment texts of one board item, in board order. The feedback CLI
/// parses the same native `bd comments` records the ledger readers use, so
/// pacing and benefit-gate lookups need no second board mirror.
pub fn list_comments(bd: &Path, project: &Path, item_id: &str) -> io::Result<Vec<String>> {
    let value = json_ok(bd, project, &["comments", item_id, "--json"])?;
    Ok(comment_values(&value)
        .iter()
        .filter_map(comment_text)
        .map(str::to_owned)
        .collect())
}

fn parse_description(text: &str) -> io::Result<BoundedFeedback> {
    let mut observation = None;
    let mut scope = None;
    let mut reporter = None;
    let mut episode = None;
    let mut kind = None;
    let mut parent_id = String::new();
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "observation" => observation = Some(value.to_owned()),
            "scope" => scope = Some(value.to_owned()),
            "reporter" => reporter = Some(value.to_owned()),
            "episode" => episode = Some(value.to_owned()),
            "kind" => kind = Some(ReporterKind::parse(value)?),
            "parent" => parent_id = value.to_owned(),
            _ => {}
        }
    }
    BoundedFeedback::try_from_draft(FeedbackDraft {
        observation: observation.ok_or_else(|| invalid("observation is required"))?,
        scope: scope.ok_or_else(|| invalid("scope is required"))?,
        reporter: reporter.ok_or_else(|| invalid("reporter is required"))?,
        episode: episode.ok_or_else(|| invalid("episode is required"))?,
        kind: kind.ok_or_else(|| invalid("kind is required"))?,
        parent_id: if parent_id.is_empty() {
            "unknown".to_owned()
        } else {
            parent_id
        },
    })
}

fn parse_vote_comment(comment: &str) -> Option<VoteRecord> {
    let rest = comment.strip_prefix(VOTE_PREFIX)?.trim();
    let mut episode = None;
    let mut reporter = None;
    let mut kind = None;
    let mut counted = None;
    let mut reason = None;
    for part in rest.split_whitespace() {
        let Some((key, value)) = part.split_once('=') else {
            continue;
        };
        match key {
            "episode" => episode = Some(value.to_owned()),
            "reporter" => reporter = Some(value.to_owned()),
            "kind" => kind = ReporterKind::parse(value).ok(),
            "counted" => counted = Some(value == "true"),
            "reason" => reason = VoteReason::parse(value),
            _ => {}
        }
    }
    Some(VoteRecord {
        episode: episode?,
        reporter: reporter?,
        kind: kind?,
        counted: counted?,
        reason: reason?,
    })
}

fn parse_merge_comment(comment: &str) -> Option<MergeRecord> {
    let rest = comment.strip_prefix(MERGE_PREFIX)?.trim();
    let mut from = None;
    let mut into = None;
    for part in rest.split_whitespace() {
        let Some((key, value)) = part.split_once('=') else {
            continue;
        };
        match key {
            "from" => from = Some(value.to_owned()),
            "into" => into = Some(value.to_owned()),
            _ => {}
        }
    }
    Some(MergeRecord {
        from: from?,
        into: into?,
    })
}

fn parse_route_comment(comment: &str) -> Option<RouteRecord> {
    let rest = comment.strip_prefix(ROUTE_PREFIX)?.trim();
    let mut kind = None;
    let mut target = None;
    let mut item = None;
    for part in rest.split_whitespace() {
        let Some((key, value)) = part.split_once('=') else {
            continue;
        };
        match key {
            "kind" => kind = ObservationKind::parse(value).ok(),
            "target" => target = RouteTarget::parse(value),
            "item" => item = Some(value.to_owned()),
            _ => {}
        }
    }
    Some(RouteRecord {
        kind: kind?,
        target: target?,
        item: item?,
    })
}

fn parse_promotion_comment(comment: &str) -> Option<PromotionRecord> {
    let rest = comment.strip_prefix(PROMOTE_PREFIX)?.trim();
    // Free-text override fields follow the structured tokens and may contain
    // spaces, so only the head before them is parsed.
    let head = rest.split(" consequence=").next()?;
    let mut route = None;
    let mut basis = None;
    let mut counted = None;
    let mut target = None;
    for part in head.split_whitespace() {
        let Some((key, value)) = part.split_once('=') else {
            continue;
        };
        match key {
            "route" => route = PromotionRoute::parse(value),
            "basis" => basis = Some(value.to_owned()),
            "counted" => counted = value.parse::<usize>().ok(),
            "target" => {
                target = match value {
                    "none" => None,
                    _ => Some(value.to_owned()),
                }
            }
            _ => {}
        }
    }
    Some(PromotionRecord {
        route: route?,
        counted: counted?,
        override_used: basis? == "override",
        target,
    })
}

fn issue_ids(value: &Value) -> io::Result<Vec<String>> {
    match value {
        Value::Array(rows) => rows.iter().map(|row| string_field(row, "id")).collect(),
        Value::Object(_) => Ok(vec![string_field(value, "id")?]),
        _ => Err(invalid("feedback list was not an array")),
    }
}

fn issue_object<'a>(value: &'a Value, id: &str) -> io::Result<&'a Value> {
    match value {
        Value::Array(rows) => rows
            .iter()
            .find(|row| row.get("id").and_then(Value::as_str) == Some(id))
            .ok_or_else(|| invalid(format!("issue {id} missing from show output"))),
        Value::Object(_) => Ok(value),
        _ => Err(invalid("show output was not an issue")),
    }
}

fn issue_description(value: &Value) -> &str {
    value
        .get("description")
        .and_then(Value::as_str)
        .or_else(|| value.get("body").and_then(Value::as_str))
        .unwrap_or("")
}

fn comment_values(value: &Value) -> &[Value] {
    match value {
        Value::Array(rows) => rows,
        Value::Object(map) => map
            .get("comments")
            .or_else(|| map.get("items"))
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[]),
        _ => &[],
    }
}

fn comment_text(value: &Value) -> Option<&str> {
    value
        .get("text")
        .and_then(Value::as_str)
        .or_else(|| value.get("body").and_then(Value::as_str))
        .or_else(|| value.get("comment").and_then(Value::as_str))
        .or_else(|| value.get("content").and_then(Value::as_str))
}

fn require_field(name: &str, value: &str, max: usize) -> io::Result<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(invalid(format!("{name} is required")));
    }
    if trimmed.len() > max {
        return Err(invalid(format!("{name} exceeds {max} bytes")));
    }
    if trimmed.contains(['\n', '\r', '\0']) {
        return Err(invalid(format!("{name} must be a single line")));
    }
    Ok(trimmed.to_owned())
}

fn require_token(name: &str, value: &str, max: usize) -> io::Result<String> {
    let trimmed = require_field(name, value, max)?;
    if trimmed.bytes().any(|byte| {
        !(byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b':' | b'@'))
    }) {
        return Err(invalid(format!("{name} contains unsupported characters")));
    }
    Ok(trimmed)
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

/// Truncates on a character boundary so a multi-byte observation cannot panic.
fn bounded_title(observation: &str) -> &str {
    let mut end = 0;
    for character in observation.chars() {
        let next = end + character.len_utf8();
        if next > MAX_TITLE_BYTES {
            break;
        }
        end = next;
    }
    &observation[..end]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board_cli::{self, seed_git};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn candidate(episode: &str, reporter: &str, kind: ReporterKind) -> VoteCandidate {
        VoteCandidate {
            episode: episode.to_owned(),
            reporter: reporter.to_owned(),
            kind,
        }
    }

    fn draft(
        observation: &str,
        reporter: &str,
        episode: &str,
        kind: ReporterKind,
        parent: &str,
    ) -> FeedbackDraft {
        FeedbackDraft {
            observation: observation.to_owned(),
            scope: "dispatch".to_owned(),
            reporter: reporter.to_owned(),
            episode: episode.to_owned(),
            kind,
            parent_id: parent.to_owned(),
        }
    }

    #[test]
    fn bounded_feedback_rejects_transcripts_and_blank_fields() {
        let long = "x".repeat(MAX_OBSERVATION + 1);
        let error = BoundedFeedback::try_from_draft(draft(
            &long,
            "exec-a",
            "e1",
            ReporterKind::Executor,
            "bdct-1.1",
        ))
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert!(error.to_string().contains("observation exceeds"));

        let error = BoundedFeedback::try_from_draft(FeedbackDraft {
            observation: "wait after tools".into(),
            scope: "dispatch\nextra".into(),
            reporter: "exec-a".into(),
            episode: "e1".into(),
            kind: ReporterKind::Executor,
            parent_id: "bdct-1.1".into(),
        })
        .unwrap_err();
        assert!(error.to_string().contains("single line"));
    }

    #[test]
    fn title_truncation_respects_character_boundaries() {
        let observation = format!("{}é-rest", "x".repeat(71));
        let feedback = BoundedFeedback::try_from_draft(draft(
            &observation,
            "exec-a",
            "e1",
            ReporterKind::Executor,
            "bdct-1.1",
        ))
        .unwrap();
        assert_eq!(feedback.title(), format!("Feedback: {}", "x".repeat(71)));

        let observation = format!("{}é-rest", "x".repeat(70));
        let feedback = BoundedFeedback::try_from_draft(draft(
            &observation,
            "exec-a",
            "e1",
            ReporterKind::Executor,
            "bdct-1.1",
        ))
        .unwrap();
        assert_eq!(feedback.title(), format!("Feedback: {}é", "x".repeat(70)));
    }

    #[test]
    fn distinct_reporters_count_once_each() {
        let first = decide_vote(&[], &candidate("e1", "exec-a", ReporterKind::Executor));
        assert_eq!(first.reason, VoteReason::Counted);
        let counted = VoteRecord {
            episode: "e1".into(),
            reporter: "exec-a".into(),
            kind: ReporterKind::Executor,
            counted: true,
            reason: VoteReason::Counted,
        };
        let second = decide_vote(
            &[counted],
            &candidate("e2", "exec-b", ReporterKind::Executor),
        );
        assert!(second.counted);
        assert_eq!(second.reason, VoteReason::Counted);
    }

    #[test]
    fn same_reporter_repeat_is_visible_and_not_counted() {
        let counted = VoteRecord {
            episode: "e1".into(),
            reporter: "exec-a".into(),
            kind: ReporterKind::Executor,
            counted: true,
            reason: VoteReason::Counted,
        };
        let repeat = decide_vote(
            &[counted],
            &candidate("e1", "exec-a", ReporterKind::Executor),
        );
        assert!(!repeat.counted);
        assert_eq!(repeat.reason, VoteReason::Repeat);
        let comment =
            format_vote_comment(&candidate("e1", "exec-a", ReporterKind::Executor), &repeat);
        let ledger = parse_ledger(&[comment]);
        assert_eq!(ledger.counted(), 0);
        assert_eq!(ledger.votes[0].reason, VoteReason::Repeat);
    }

    #[test]
    fn automated_diagnostics_do_not_accumulate_votes() {
        let decision = decide_vote(
            &[],
            &candidate("e1", "health-check", ReporterKind::AutomatedDiagnostic),
        );
        assert!(!decision.counted);
        assert_eq!(decision.reason, VoteReason::AutomatedDiagnostic);
        let comment = format_vote_comment(
            &candidate("e1", "health-check", ReporterKind::AutomatedDiagnostic),
            &decision,
        );
        let ledger = parse_ledger(&[comment]);
        assert_eq!(ledger.counted(), 0);
        assert_eq!(ledger.votes[0].reason, VoteReason::AutomatedDiagnostic);
    }

    #[test]
    fn diagnostic_does_not_block_a_later_human_vote() {
        let diagnostic = VoteRecord {
            episode: "e1".into(),
            reporter: "exec-a".into(),
            kind: ReporterKind::AutomatedDiagnostic,
            counted: false,
            reason: VoteReason::AutomatedDiagnostic,
        };
        let human = decide_vote(
            &[diagnostic],
            &candidate("e1", "exec-a", ReporterKind::Executor),
        );
        assert!(human.counted);
    }

    #[test]
    fn merge_and_vote_comments_round_trip() {
        let merge = format_merge_comment("bdct-1.1.2", "bdct-1.1.1");
        let vote = format_vote_comment(
            &candidate("e2", "exec-b", ReporterKind::Lead),
            &VoteDecision {
                counted: true,
                reason: VoteReason::Counted,
            },
        );
        let ledger = parse_ledger(&[merge, vote]);
        assert_eq!(ledger.merges[0].from, "bdct-1.1.2");
        assert_eq!(ledger.merges[0].into, "bdct-1.1.1");
        assert_eq!(ledger.counted(), 1);
    }

    #[test]
    fn kit_batch_limit_matches_orchestration_knob() {
        let text = include_str!("../../../global/orchestration.toml");
        assert!(text.contains("feedback_batch_limit = 8"));
        assert_eq!(DEFAULT_FEEDBACK_BATCH_LIMIT, 8);
    }

    #[test]
    fn board_and_lead_skills_keep_feedback_off_chat() {
        let board = include_str!("../../../.agents/skills/board-workflow/SKILL.md");
        let lead = include_str!("../../../.agents/skills/team-lead/SKILL.md");
        assert!(board.contains("codex-harness feedback record"));
        assert!(board.contains("Do not use `bd find-duplicates`"));
        assert!(board.contains("codex-harness feedback triage"));
        assert!(board.contains("codex-harness feedback promote"));
        assert!(board.contains("feedback-archive v1"));
        assert!(board.contains("feedback-restore v1"));
        assert!(board.contains("vote_threshold"));
        assert!(board.contains("incubator_size_cap"));
        assert!(board.contains("skill-evolution"));
        assert!(board.contains("stage-or-epic-closed"));
        assert!(board.contains("never writes a skill package"));
        assert!(board.contains("Size checks are board queries"));
        assert!(lead.contains("not real-time chat"));
        assert!(lead.contains("safe boundary"));
        assert!(lead.contains("no model calls"));
        assert!(lead.contains("vote_threshold"));
        assert!(lead.contains("skill-evolution"));
        assert!(lead.contains("incubator_size_cap"));
        assert!(lead.contains("consequence override"));
    }

    fn bd_executable() -> Option<PathBuf> {
        if let Some(value) = std::env::var_os("HARNESS_BD_EXE") {
            let path = PathBuf::from(value);
            if path.is_file() {
                return Some(path);
            }
        }
        if let Some(home) = std::env::var_os("CODEX_HOME") {
            let path = PathBuf::from(home).join("harness/bin").join(bd_name());
            if path.is_file() {
                return Some(path);
            }
        }
        let path = std::env::var_os("PATH")?;
        std::env::split_paths(&path).find_map(|dir| {
            let lower = dir.to_string_lossy().to_ascii_lowercase();
            if lower.ends_with(r"\windowsapps") || lower.contains(r"\windowsapps\") {
                return None;
            }
            let candidate = dir.join(bd_name());
            candidate.is_file().then_some(candidate)
        })
    }

    fn bd_name() -> &'static str {
        if cfg!(windows) { "bd.exe" } else { "bd" }
    }

    fn isolated_feature(bd: &Path) -> (tempfile::TempDir, PathBuf, String) {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let root = tempfile::Builder::new()
            .prefix(&format!("orch-feedback-{stamp}-"))
            .tempdir()
            .unwrap();
        let project = root.path().join("project");
        seed_git(&project).unwrap();
        let init = board_cli::run(
            bd,
            &project,
            &[
                "init",
                "--skip-agents",
                "--non-interactive",
                "--quiet",
                "--prefix",
                "bdct",
            ],
        )
        .unwrap();
        assert!(
            init.status.success(),
            "{}",
            board_cli::failed("init", &init)
        );
        let epic = json_ok(
            bd,
            &project,
            &[
                "create",
                "Stage: synthetic feedback",
                "--type",
                "epic",
                "--json",
            ],
        )
        .unwrap();
        let epic_id = string_field(&epic, "id").unwrap();
        let feature = json_ok(
            bd,
            &project,
            &[
                "create",
                "Spec: feedback intake",
                "--type",
                "feature",
                "--parent",
                &epic_id,
                "--json",
            ],
        )
        .unwrap();
        let feature_id = string_field(&feature, "id").unwrap();
        (root, project, feature_id)
    }

    /// Classifies one recorded observation as an incubator kit concern, the
    /// precondition of a kit-backlog promotion.
    fn incubate_kit_concern(bd: &Path, project: &Path, item: &str) {
        apply_routed_triage(
            bd,
            project,
            &[RoutedAction {
                feedback_id: item.to_owned(),
                kind: ObservationKind::KitConcern,
                merge_into: None,
            }],
            DEFAULT_FEEDBACK_BATCH_LIMIT,
        )
        .unwrap();
    }

    /// The open kit-backlog items of the kit board, as the retry lookup sees
    /// them.
    fn kit_feedback_ids(bd: &Path, kit_project: &Path) -> Vec<String> {
        let listed = json_ok(
            bd,
            kit_project,
            &[
                "list",
                "--label",
                KIT_FEEDBACK_LABEL,
                "--status",
                "open",
                "--json",
                "--brief",
            ],
        )
        .unwrap();
        issue_ids(&listed).unwrap()
    }

    #[test]
    fn lead_and_executor_feedback_is_recorded_and_listed_without_chat() {
        let Some(bd) = bd_executable() else {
            panic!("bd v1.3.0 is required on PATH, CODEX_HOME/harness/bin, or HARNESS_BD_EXE");
        };
        let (_root, project, parent) = isolated_feature(&bd);
        let executor = BoundedFeedback::try_from_draft(draft(
            "dispatch waits after tools",
            "exec-a",
            "e1",
            ReporterKind::Executor,
            &parent,
        ))
        .unwrap();
        let lead = BoundedFeedback::try_from_draft(draft(
            "repeated fixture miss across executors",
            "lead-1",
            "e-lead",
            ReporterKind::Lead,
            &parent,
        ))
        .unwrap();
        let first = record_feedback(&bd, &project, &executor).unwrap();
        let second = record_feedback(&bd, &project, &lead).unwrap();
        let listed = list_feedback(&bd, &project).unwrap();
        assert!(
            listed
                .iter()
                .any(|row| row.id == first && row.feedback.kind == ReporterKind::Executor)
        );
        assert!(
            listed
                .iter()
                .any(|row| row.id == second && row.feedback.kind == ReporterKind::Lead)
        );
        assert_eq!(listed.len(), 2);
    }

    #[test]
    fn lead_batch_triage_merges_similar_feedback_with_one_vote_each() {
        let Some(bd) = bd_executable() else {
            panic!("bd v1.3.0 is required on PATH, CODEX_HOME/harness/bin, or HARNESS_BD_EXE");
        };
        let (_root, project, parent) = isolated_feature(&bd);
        let first = record_feedback(
            &bd,
            &project,
            &BoundedFeedback::try_from_draft(draft(
                "dispatch waits after tools",
                "exec-a",
                "e1",
                ReporterKind::Executor,
                &parent,
            ))
            .unwrap(),
        )
        .unwrap();
        let second = record_feedback(
            &bd,
            &project,
            &BoundedFeedback::try_from_draft(draft(
                "dispatch waits after tools",
                "exec-b",
                "e2",
                ReporterKind::Executor,
                &parent,
            ))
            .unwrap(),
        )
        .unwrap();
        let report = apply_triage(
            &bd,
            &project,
            &[
                TriageAction {
                    feedback_id: first.clone(),
                    merge_into: None,
                },
                TriageAction {
                    feedback_id: second.clone(),
                    merge_into: Some(first.clone()),
                },
            ],
            DEFAULT_FEEDBACK_BATCH_LIMIT,
        )
        .unwrap();
        assert_eq!(report.deferred, 0);
        assert!(!report.applied[0].merged);
        assert!(report.applied[1].merged);
        assert!(report.applied.iter().all(|row| row.vote.counted));
        let remaining = list_feedback(&bd, &project).unwrap();
        assert!(
            remaining
                .iter()
                .all(|row| row.id != first && row.id != second)
        );
        let ledger = inspect_ledger(&bd, &project, &first).unwrap();
        assert_eq!(ledger.counted(), 2);
        assert_eq!(ledger.merges.len(), 1);
        assert_eq!(ledger.merges[0].from, second);
        assert_eq!(ledger.merges[0].into, first);
    }

    #[test]
    fn vote_integrity_rejects_repeats_and_diagnostics_on_the_board() {
        let Some(bd) = bd_executable() else {
            panic!("bd v1.3.0 is required on PATH, CODEX_HOME/harness/bin, or HARNESS_BD_EXE");
        };
        let (_root, project, parent) = isolated_feature(&bd);
        let canonical = record_feedback(
            &bd,
            &project,
            &BoundedFeedback::try_from_draft(draft(
                "dispatch waits after tools",
                "exec-a",
                "e1",
                ReporterKind::Executor,
                &parent,
            ))
            .unwrap(),
        )
        .unwrap();
        let repeat = record_feedback(
            &bd,
            &project,
            &BoundedFeedback::try_from_draft(draft(
                "dispatch waits after tools",
                "exec-a",
                "e1",
                ReporterKind::Executor,
                &parent,
            ))
            .unwrap(),
        )
        .unwrap();
        let diagnostic = record_feedback(
            &bd,
            &project,
            &BoundedFeedback::try_from_draft(draft(
                "dispatch waits after tools",
                "health-check",
                "diag-1",
                ReporterKind::AutomatedDiagnostic,
                &parent,
            ))
            .unwrap(),
        )
        .unwrap();
        let report = apply_triage(
            &bd,
            &project,
            &[
                TriageAction {
                    feedback_id: canonical.clone(),
                    merge_into: None,
                },
                TriageAction {
                    feedback_id: repeat.clone(),
                    merge_into: Some(canonical.clone()),
                },
                TriageAction {
                    feedback_id: diagnostic.clone(),
                    merge_into: Some(canonical.clone()),
                },
            ],
            DEFAULT_FEEDBACK_BATCH_LIMIT,
        )
        .unwrap();
        assert!(report.applied[0].vote.counted);
        assert_eq!(report.applied[1].vote.reason, VoteReason::Repeat);
        assert_eq!(
            report.applied[2].vote.reason,
            VoteReason::AutomatedDiagnostic
        );
        let ledger = inspect_ledger(&bd, &project, &canonical).unwrap();
        assert_eq!(ledger.counted(), 1);
        assert!(
            ledger
                .votes
                .iter()
                .any(|vote| vote.reason == VoteReason::Repeat && !vote.counted)
        );
        assert!(
            ledger
                .votes
                .iter()
                .any(|vote| { vote.reason == VoteReason::AutomatedDiagnostic && !vote.counted })
        );
        assert_eq!(ledger.merges.len(), 2);
    }

    #[test]
    fn triage_defers_work_beyond_the_batch_limit() {
        let Some(bd) = bd_executable() else {
            panic!("bd v1.3.0 is required on PATH, CODEX_HOME/harness/bin, or HARNESS_BD_EXE");
        };
        let (_root, project, parent) = isolated_feature(&bd);
        let mut actions = Vec::new();
        for index in 0..3 {
            let id = record_feedback(
                &bd,
                &project,
                &BoundedFeedback::try_from_draft(draft(
                    &format!("unique wait {index}"),
                    &format!("exec-{index}"),
                    &format!("e{index}"),
                    ReporterKind::Executor,
                    &parent,
                ))
                .unwrap(),
            )
            .unwrap();
            actions.push(TriageAction {
                feedback_id: id,
                merge_into: None,
            });
        }
        let report = apply_triage(&bd, &project, &actions, 2).unwrap();
        assert_eq!(report.applied.len(), 2);
        assert_eq!(report.deferred, 1);
    }

    fn record(
        bd: &Path,
        project: &Path,
        observation: &str,
        reporter: &str,
        episode: &str,
        kind: ReporterKind,
        parent: &str,
    ) -> String {
        record_feedback(
            bd,
            project,
            &BoundedFeedback::try_from_draft(draft(observation, reporter, episode, kind, parent))
                .unwrap(),
        )
        .unwrap()
    }

    fn skills_snapshot(project: &Path) -> Vec<(String, Vec<u8>)> {
        let root = project.join(".agents/skills");
        let mut entries = Vec::new();
        if let Ok(directories) = fs::read_dir(&root) {
            for directory in directories.flatten() {
                let path = directory.path().join("SKILL.md");
                if let Ok(bytes) = fs::read(&path) {
                    entries.push((path.to_string_lossy().into_owned(), bytes));
                }
            }
        }
        entries.sort();
        entries
    }

    fn seed_skill_package(project: &Path) -> Vec<(String, Vec<u8>)> {
        let directory = project.join(".agents/skills/demo");
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("SKILL.md"), "sentinel skill body\n").unwrap();
        skills_snapshot(project)
    }

    #[test]
    fn observation_kinds_route_and_promote_by_consequence() {
        assert_eq!(
            ObservationKind::SkillProcedure.intake(),
            IntakeRoute::SkillEvolution
        );
        for kind in [
            ObservationKind::Process,
            ObservationKind::Orchestration,
            ObservationKind::Requirement,
            ObservationKind::Tool,
            ObservationKind::Unclear,
            ObservationKind::Material,
            ObservationKind::KitConcern,
        ] {
            assert_eq!(kind.intake(), IntakeRoute::Incubator);
        }

        assert_eq!(default_promotion_route(&[]), None);
        assert_eq!(
            default_promotion_route(&[ObservationKind::Process]),
            Some(PromotionRoute::BacklogTask)
        );
        assert_eq!(
            default_promotion_route(&[ObservationKind::Requirement]),
            Some(PromotionRoute::OpenSpecChange)
        );
        assert_eq!(
            default_promotion_route(&[ObservationKind::KitConcern]),
            Some(PromotionRoute::KitBacklog)
        );
        assert_eq!(
            default_promotion_route(&[ObservationKind::KitConcern, ObservationKind::Process]),
            None
        );

        check_promotion_route(&[ObservationKind::KitConcern], PromotionRoute::KitBacklog).unwrap();
        let error =
            check_promotion_route(&[ObservationKind::KitConcern], PromotionRoute::BacklogTask)
                .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("kit concerns promote to the kit backlog")
        );
        let error =
            check_promotion_route(&[ObservationKind::Requirement], PromotionRoute::BacklogTask)
                .unwrap_err();
        assert!(error.to_string().contains("enters OpenSpec"));
        let error = check_promotion_route(
            &[ObservationKind::SkillProcedure],
            PromotionRoute::BacklogTask,
        )
        .unwrap_err();
        assert!(error.to_string().contains("handed to skill-evolution"));
        let error = check_promotion_route(
            &[ObservationKind::KitConcern, ObservationKind::Process],
            PromotionRoute::KitBacklog,
        )
        .unwrap_err();
        assert!(error.to_string().contains("split the item"));
        let error = check_promotion_route(&[], PromotionRoute::BacklogTask).unwrap_err();
        assert!(error.to_string().contains("not classified"));
    }

    #[test]
    fn route_and_promotion_records_round_trip_and_evidence_is_validated() {
        let route = format_route_comment(
            ObservationKind::KitConcern,
            RouteTarget::Incubator,
            "bdct-1.1.2",
        );
        let ledger = parse_ledger(&[route]);
        assert_eq!(ledger.routes[0].kind, ObservationKind::KitConcern);
        assert_eq!(ledger.routes[0].target, RouteTarget::Incubator);
        assert_eq!(ledger.routes[0].item, "bdct-1.1.2");

        let comment = PromotionComment {
            route: PromotionRoute::BacklogTask,
            counted: 3,
            threshold: Some(3),
            target: None,
            override_note: None,
        }
        .format();
        assert!(comment.contains("basis=votes"));
        let ledger = parse_ledger(&[comment]);
        assert_eq!(
            ledger.promotions[0],
            PromotionRecord {
                route: PromotionRoute::BacklogTask,
                counted: 3,
                override_used: false,
                target: None,
            }
        );

        let note = (
            "losing votes loses demand".to_owned(),
            "correctness evidence".to_owned(),
        );
        let comment = PromotionComment {
            route: PromotionRoute::KitBacklog,
            counted: 1,
            threshold: None,
            target: Some("kit:prb-1"),
            override_note: Some(&note),
        }
        .format();
        assert!(
            comment.contains("consequence=losing votes loses demand reason=correctness evidence")
        );
        let ledger = parse_ledger(&[comment]);
        assert!(ledger.promotions[0].override_used);
        assert_eq!(ledger.promotions[0].target.as_deref(), Some("kit:prb-1"));

        let ledger = parse_ledger(&[]);
        let error =
            validate_evidence(&ledger, &PromotionEvidence::Votes { threshold: 2 }).unwrap_err();
        assert!(error.to_string().contains("below the configured threshold"));
        let error =
            validate_evidence(&ledger, &PromotionEvidence::Votes { threshold: 1 }).unwrap_err();
        assert!(error.to_string().contains("at least 2"));
        let error = validate_evidence(
            &ledger,
            &PromotionEvidence::ConsequenceOverride {
                consequence: " ".into(),
                reason: "why".into(),
            },
        )
        .unwrap_err();
        assert!(error.to_string().contains("consequence is required"));
    }

    #[test]
    fn kit_promotion_defaults_match_orchestration_config() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let config = crate::orchestration_config::load(&root).unwrap();
        assert_eq!(config.vote_threshold, DEFAULT_VOTE_THRESHOLD);
        assert_eq!(config.vote_threshold, 3);
        assert_eq!(
            config.incubator_size_cap as usize,
            DEFAULT_INCUBATOR_SIZE_CAP
        );
        let text = include_str!("../../../global/orchestration.toml");
        assert!(text.contains("vote_threshold = 3"));
        assert!(text.contains("incubator_size_cap = 32"));
        assert!(text.contains("more than two distinct votes"));
    }

    #[test]
    fn verified_procedure_is_handed_off_without_votes_or_package_writes() {
        let Some(bd) = bd_executable() else {
            panic!("bd v1.3.0 is required on PATH, CODEX_HOME/harness/bin, or HARNESS_BD_EXE");
        };
        let (_root, project, parent) = isolated_feature(&bd);
        let before = seed_skill_package(&project);

        let procedure = record(
            &bd,
            &project,
            "one investigated failure yields a reusable diagnostic method",
            "exec-a",
            "e1",
            ReporterKind::Executor,
            &parent,
        );
        let report = apply_routed_triage(
            &bd,
            &project,
            &[RoutedAction {
                feedback_id: procedure.clone(),
                kind: ObservationKind::SkillProcedure,
                merge_into: None,
            }],
            DEFAULT_FEEDBACK_BATCH_LIMIT,
        )
        .unwrap();
        assert_eq!(report.applied[0].route, IntakeRoute::SkillEvolution);
        assert!(report.applied[0].vote.is_none());
        assert_eq!(report.applied[0].target_id, procedure);

        let snapshot = load_snapshot(&bd, &project, &procedure).unwrap().unwrap();
        assert!(
            snapshot
                .labels
                .iter()
                .any(|label| label == SKILL_EVOLUTION_LABEL)
        );
        assert!(!snapshot.labels.iter().any(|label| label == FEEDBACK_LABEL));
        assert!(!snapshot.labels.iter().any(|label| label == INCUBATOR_LABEL));
        let ledger = inspect_ledger(&bd, &project, &procedure).unwrap();
        assert_eq!(ledger.counted(), 0);
        assert_eq!(ledger.routes.len(), 1);
        assert_eq!(ledger.routes[0].target, RouteTarget::SkillEvolution);
        assert!(
            !list_feedback(&bd, &project)
                .unwrap()
                .iter()
                .any(|row| row.id == procedure)
        );
        assert!(!list_incubator(&bd, &project).unwrap().contains(&procedure));

        // The same observation is never both handed off and incubator demand.
        let error = apply_routed_triage(
            &bd,
            &project,
            &[RoutedAction {
                feedback_id: procedure.clone(),
                kind: ObservationKind::Process,
                merge_into: None,
            }],
            DEFAULT_FEEDBACK_BATCH_LIMIT,
        )
        .unwrap_err();
        assert!(error.to_string().contains("handed to skill-evolution"));
        assert_eq!(
            inspect_ledger(&bd, &project, &procedure).unwrap().counted(),
            0
        );

        // And an incubating observation is never handed off afterwards.
        let friction = record(
            &bd,
            &project,
            "dispatch waits after tools",
            "exec-b",
            "e2",
            ReporterKind::Executor,
            &parent,
        );
        let report = apply_routed_triage(
            &bd,
            &project,
            &[RoutedAction {
                feedback_id: friction.clone(),
                kind: ObservationKind::Orchestration,
                merge_into: None,
            }],
            DEFAULT_FEEDBACK_BATCH_LIMIT,
        )
        .unwrap();
        assert_eq!(report.applied[0].route, IntakeRoute::Incubator);
        assert_eq!(
            report.applied[0].vote.as_ref().unwrap().reason,
            VoteReason::Counted
        );
        let error = apply_routed_triage(
            &bd,
            &project,
            &[RoutedAction {
                feedback_id: friction.clone(),
                kind: ObservationKind::SkillProcedure,
                merge_into: None,
            }],
            DEFAULT_FEEDBACK_BATCH_LIMIT,
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("already competes as incubator demand")
        );
        assert_eq!(
            inspect_ledger(&bd, &project, &friction).unwrap().counted(),
            1
        );

        assert_eq!(skills_snapshot(&project), before);
    }

    #[test]
    fn vote_threshold_promotes_to_backlog_with_history() {
        let Some(bd) = bd_executable() else {
            panic!("bd v1.3.0 is required on PATH, CODEX_HOME/harness/bin, or HARNESS_BD_EXE");
        };
        let (_root, project, parent) = isolated_feature(&bd);
        let first = record(
            &bd,
            &project,
            "dispatch waits after tools",
            "exec-a",
            "e1",
            ReporterKind::Executor,
            &parent,
        );
        let second = record(
            &bd,
            &project,
            "dispatch waits after tools",
            "exec-b",
            "e2",
            ReporterKind::Executor,
            &parent,
        );
        let third = record(
            &bd,
            &project,
            "dispatch waits after tools",
            "lead-1",
            "e3",
            ReporterKind::Lead,
            &parent,
        );
        let report = apply_routed_triage(
            &bd,
            &project,
            &[
                RoutedAction {
                    feedback_id: first.clone(),
                    kind: ObservationKind::Orchestration,
                    merge_into: None,
                },
                RoutedAction {
                    feedback_id: second.clone(),
                    kind: ObservationKind::Orchestration,
                    merge_into: Some(first.clone()),
                },
                RoutedAction {
                    feedback_id: third.clone(),
                    kind: ObservationKind::Process,
                    merge_into: Some(first.clone()),
                },
            ],
            DEFAULT_FEEDBACK_BATCH_LIMIT,
        )
        .unwrap();
        assert_eq!(report.applied.len(), 3);

        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let threshold = crate::orchestration_config::load(&root)
            .unwrap()
            .vote_threshold;
        let candidates = promotion_candidates(&bd, &project, threshold).unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].item_id, first);
        assert_eq!(candidates[0].counted, 3);
        assert!(
            candidates[0]
                .kinds
                .contains(&ObservationKind::Orchestration)
        );
        assert!(candidates[0].kinds.contains(&ObservationKind::Process));
        assert!(
            promotion_candidates(&bd, &project, threshold + 1)
                .unwrap()
                .is_empty()
        );

        let error = promote_item(
            &bd,
            &project,
            &first,
            PromotionRoute::BacklogTask,
            &PromotionEvidence::Votes {
                threshold: threshold + 1,
            },
            None,
        )
        .unwrap_err();
        assert!(error.to_string().contains("below the configured threshold"));

        let outcome = promote_item(
            &bd,
            &project,
            &first,
            PromotionRoute::BacklogTask,
            &PromotionEvidence::Votes { threshold },
            None,
        )
        .unwrap();
        assert_eq!(outcome.counted, 3);
        assert!(!outcome.override_used);
        assert_eq!(outcome.target, None);

        let snapshot = load_snapshot(&bd, &project, &first).unwrap().unwrap();
        assert!(snapshot.labels.iter().any(|label| label == BACKLOG_LABEL));
        assert!(!snapshot.labels.iter().any(|label| label == INCUBATOR_LABEL));
        let ledger = inspect_ledger(&bd, &project, &first).unwrap();
        assert_eq!(ledger.counted(), 3);
        assert_eq!(ledger.merges.len(), 2);
        assert_eq!(ledger.routes.len(), 3);
        assert_eq!(ledger.promotions.len(), 1);
        assert_eq!(ledger.promotions[0].route, PromotionRoute::BacklogTask);
        assert_eq!(ledger.promotions[0].counted, 3);
        assert!(list_incubator(&bd, &project).unwrap().is_empty());
        let comments = list_comments(&bd, &project, &first).unwrap();
        assert!(comments.iter().any(|comment| {
            comment.contains("route=backlog-task basis=votes counted=3 threshold=3 target=none")
        }));

        // A repeat of a completed promotion confirms the recorded outcome
        // instead of writing a second record or a comment.
        let comments_before = list_comments(&bd, &project, &first).unwrap();
        let repeat = promote_item(
            &bd,
            &project,
            &first,
            PromotionRoute::BacklogTask,
            &PromotionEvidence::Votes { threshold },
            None,
        )
        .unwrap();
        assert!(repeat.already_recorded);
        assert_eq!(repeat.route, PromotionRoute::BacklogTask);
        assert_eq!(repeat.target, None);
        assert_eq!(repeat.counted, 3);
        assert!(!repeat.override_used);
        assert_eq!(
            inspect_ledger(&bd, &project, &first)
                .unwrap()
                .promotions
                .len(),
            1
        );
        assert_eq!(
            list_comments(&bd, &project, &first).unwrap(),
            comments_before
        );
        let snapshot = load_snapshot(&bd, &project, &first).unwrap().unwrap();
        assert!(snapshot.labels.iter().any(|label| label == BACKLOG_LABEL));
        assert!(!snapshot.labels.iter().any(|label| label == INCUBATOR_LABEL));
    }

    #[test]
    fn requirement_change_enters_openspec_with_recorded_override() {
        let Some(bd) = bd_executable() else {
            panic!("bd v1.3.0 is required on PATH, CODEX_HOME/harness/bin, or HARNESS_BD_EXE");
        };
        let (_root, project, parent) = isolated_feature(&bd);
        let item = record(
            &bd,
            &project,
            "backlog must record the promotion rationale",
            "lead-1",
            "e1",
            ReporterKind::Lead,
            &parent,
        );
        apply_routed_triage(
            &bd,
            &project,
            &[RoutedAction {
                feedback_id: item.clone(),
                kind: ObservationKind::Requirement,
                merge_into: None,
            }],
            DEFAULT_FEEDBACK_BATCH_LIMIT,
        )
        .unwrap();

        let error = promote_item(
            &bd,
            &project,
            &item,
            PromotionRoute::BacklogTask,
            &PromotionEvidence::Votes { threshold: 2 },
            None,
        )
        .unwrap_err();
        assert!(error.to_string().contains("enters OpenSpec"));

        // The OpenSpec workflow owns the change: promoting without an
        // existing intended change records nothing and names the remedy.
        let derived = format!("feedback-{}", bounded_slug(&item));
        let error = promote_item(
            &bd,
            &project,
            &item,
            PromotionRoute::OpenSpecChange,
            &PromotionEvidence::Votes { threshold: 2 },
            None,
        )
        .unwrap_err();
        let message = error.to_string();
        assert!(message.contains(&derived), "{message}");
        assert!(message.contains("openspec new change"), "{message}");
        assert!(message.contains("nothing was recorded"), "{message}");
        let ledger = inspect_ledger(&bd, &project, &item).unwrap();
        assert!(ledger.promotions.is_empty());
        let snapshot = load_snapshot(&bd, &project, &item).unwrap().unwrap();
        assert!(snapshot.labels.iter().any(|label| label == INCUBATOR_LABEL));

        // The OpenSpec workflow creates the intended change; promotion
        // validates it and preserves the draft exactly.
        let change = project.join("openspec/changes").join(&derived);
        fs::create_dir_all(&change).unwrap();
        let draft = change.join("proposal.md");
        fs::write(&draft, format!("# Draft for {item}\n")).unwrap();
        let draft_before = fs::read(&draft).unwrap();

        let evidence = PromotionEvidence::ConsequenceOverride {
            consequence: "accepted behavior would silently change".to_owned(),
            reason: "material correctness evidence".to_owned(),
        };
        let outcome = promote_item(
            &bd,
            &project,
            &item,
            PromotionRoute::OpenSpecChange,
            &evidence,
            None,
        )
        .unwrap();
        assert!(outcome.override_used);
        assert_eq!(outcome.counted, 1);
        assert_eq!(
            outcome.target.as_deref(),
            Some(format!("openspec:{derived}").as_str())
        );
        assert_eq!(fs::read(&draft).unwrap(), draft_before);

        let snapshot = load_snapshot(&bd, &project, &item).unwrap().unwrap();
        assert!(snapshot.labels.iter().any(|label| label == OPENSPEC_LABEL));
        assert!(!snapshot.labels.iter().any(|label| label == INCUBATOR_LABEL));
        let ledger = inspect_ledger(&bd, &project, &item).unwrap();
        assert_eq!(ledger.counted(), 1);
        assert_eq!(ledger.promotions.len(), 1);
        assert!(ledger.promotions[0].override_used);
        assert_eq!(
            ledger.promotions[0].target.as_deref(),
            Some(format!("openspec:{derived}").as_str())
        );
        let comments = list_comments(&bd, &project, &item).unwrap();
        assert!(comments.iter().any(|comment| {
            comment.contains("route=openspec-change basis=override counted=1 threshold=none")
                && comment.contains("consequence=accepted behavior would silently change")
                && comment.contains("reason=material correctness evidence")
        }));

        // A completed promotion confirms its recorded outcome instead of
        // writing another record, and the draft stays exactly as the OpenSpec
        // workflow wrote it.
        let comments_before = list_comments(&bd, &project, &item).unwrap();
        let repeat = promote_item(
            &bd,
            &project,
            &item,
            PromotionRoute::OpenSpecChange,
            &evidence,
            None,
        )
        .unwrap();
        assert!(repeat.already_recorded);
        assert_eq!(
            repeat.target.as_deref(),
            Some(format!("openspec:{derived}").as_str())
        );
        assert_eq!(
            inspect_ledger(&bd, &project, &item)
                .unwrap()
                .promotions
                .len(),
            1
        );
        assert_eq!(
            list_comments(&bd, &project, &item).unwrap(),
            comments_before
        );
        assert_eq!(fs::read(&draft).unwrap(), draft_before);

        // A different intended change is a different consequence: it stays
        // refused even when that change exists, and the refusal names the
        // recorded target.
        let other = project.join("openspec/changes/other-intent");
        fs::create_dir_all(&other).unwrap();
        fs::write(other.join("proposal.md"), "# Other intent\n").unwrap();
        let error = promote_item(
            &bd,
            &project,
            &item,
            PromotionRoute::OpenSpecChange,
            &evidence,
            Some("other-intent"),
        )
        .unwrap_err();
        let message = error.to_string();
        assert!(message.contains("already promoted"), "{message}");
        assert!(
            message.contains(&format!("openspec:{derived}")),
            "{message}"
        );
        assert!(message.contains("other-intent"), "{message}");
        assert_eq!(
            inspect_ledger(&bd, &project, &item)
                .unwrap()
                .promotions
                .len(),
            1
        );
    }

    #[test]
    fn interrupted_openspec_promotion_reconciles_without_rewriting_the_draft() {
        let Some(bd) = bd_executable() else {
            panic!("bd v1.3.0 is required on PATH, CODEX_HOME/harness/bin, or HARNESS_BD_EXE");
        };
        let (_root, project, parent) = isolated_feature(&bd);
        let item = record(
            &bd,
            &project,
            "requirement needs an OpenSpec change",
            "lead-1",
            "e1",
            ReporterKind::Lead,
            &parent,
        );
        apply_routed_triage(
            &bd,
            &project,
            &[RoutedAction {
                feedback_id: item.clone(),
                kind: ObservationKind::Requirement,
                merge_into: None,
            }],
            DEFAULT_FEEDBACK_BATCH_LIMIT,
        )
        .unwrap();

        // The OpenSpec workflow created the intended change before the
        // promotion ran; its draft must survive every retry untouched.
        let change = project.join("openspec/changes/lead-intent");
        fs::create_dir_all(&change).unwrap();
        fs::write(change.join("proposal.md"), "# Lead intent\n").unwrap();

        // A run that recorded the promotion and then failed before its label
        // steps leaves exactly this comment; a retry reads it as the applied
        // prefix and only reconciles the board.
        json_ok(
            &bd,
            &project,
            &[
                "comment",
                &item,
                "--json",
                "feedback-promote v1 route=openspec-change basis=votes counted=1 threshold=2 target=openspec:lead-intent",
            ],
        )
        .unwrap();
        let snapshot = load_snapshot(&bd, &project, &item).unwrap().unwrap();
        assert!(snapshot.labels.iter().any(|label| label == INCUBATOR_LABEL));

        let outcome = promote_item(
            &bd,
            &project,
            &item,
            PromotionRoute::OpenSpecChange,
            &PromotionEvidence::Votes { threshold: 2 },
            Some("lead-intent"),
        )
        .unwrap();
        assert_eq!(outcome.target.as_deref(), Some("openspec:lead-intent"));
        let ledger = inspect_ledger(&bd, &project, &item).unwrap();
        assert_eq!(
            ledger.promotions.len(),
            1,
            "the retry must not record the promotion twice"
        );
        let snapshot = load_snapshot(&bd, &project, &item).unwrap().unwrap();
        assert!(snapshot.labels.iter().any(|label| label == OPENSPEC_LABEL));
        assert!(!snapshot.labels.iter().any(|label| label == INCUBATOR_LABEL));
        assert_eq!(
            fs::read_to_string(change.join("proposal.md")).unwrap(),
            "# Lead intent\n"
        );

        // A reference that names no change is refused before any board write.
        let other = record(
            &bd,
            &project,
            "another requirement",
            "lead-1",
            "e2",
            ReporterKind::Lead,
            &parent,
        );
        apply_routed_triage(
            &bd,
            &project,
            &[RoutedAction {
                feedback_id: other.clone(),
                kind: ObservationKind::Requirement,
                merge_into: None,
            }],
            DEFAULT_FEEDBACK_BATCH_LIMIT,
        )
        .unwrap();
        let error = promote_item(
            &bd,
            &project,
            &other,
            PromotionRoute::OpenSpecChange,
            &PromotionEvidence::Votes { threshold: 2 },
            Some("absent-change"),
        )
        .unwrap_err();
        let message = error.to_string();
        assert!(message.contains("absent-change"), "{message}");
        assert!(message.contains("nothing was recorded"), "{message}");
        assert!(
            inspect_ledger(&bd, &project, &other)
                .unwrap()
                .promotions
                .is_empty()
        );
        assert!(
            load_snapshot(&bd, &project, &other)
                .unwrap()
                .unwrap()
                .labels
                .iter()
                .any(|label| label == INCUBATOR_LABEL)
        );

        // A reference that is a path instead of one name is rejected too.
        let error = promote_item(
            &bd,
            &project,
            &other,
            PromotionRoute::OpenSpecChange,
            &PromotionEvidence::Votes { threshold: 2 },
            Some("../escape"),
        )
        .unwrap_err();
        assert!(
            error.to_string().contains("must be one path name"),
            "{error}"
        );
    }

    #[test]
    fn kit_retry_reuses_only_the_exact_sanitized_concern() {
        let Some(bd) = bd_executable() else {
            panic!("bd v1.3.0 is required on PATH, CODEX_HOME/harness/bin, or HARNESS_BD_EXE");
        };
        let (_root, project, parent) = isolated_feature(&bd);
        let (_kit_root, kit_project, _kit_parent) = isolated_feature(&bd);
        // Two different concerns whose bounded titles are identical: only the
        // full sanitized summary and scope distinguish them.
        let shared = "s".repeat(80);
        let first = KitConcern {
            summary: format!("{shared} first concern"),
            scope: "kit skill: board-workflow".to_owned(),
        };
        let second = KitConcern {
            summary: format!("{shared} second concern"),
            scope: "kit skill: board-workflow".to_owned(),
        };
        assert_eq!(
            bounded_title(&first.summary),
            bounded_title(&second.summary)
        );
        let existing = json_ok(
            &bd,
            &kit_project,
            &[
                "create",
                &format!("Kit feedback: {}", bounded_title(&first.summary)),
                "--type",
                "task",
                "--labels",
                KIT_FEEDBACK_LABEL,
                "--description",
                &format!(
                    "summary: {}\nscope: {}\nkind: kit-concern\n",
                    first.summary, first.scope
                ),
                "--json",
            ],
        )
        .unwrap();
        let existing_id = string_field(&existing, "id").unwrap();

        // A previous partial run of a different concern left that one task:
        // promoting the second concern must add its own task instead of
        // adopting a task that only shares the bounded title.
        let item = record(
            &bd,
            &project,
            "second kit concern",
            "lead-1",
            "e1",
            ReporterKind::Lead,
            &parent,
        );
        incubate_kit_concern(&bd, &project, &item);
        let evidence = PromotionEvidence::ConsequenceOverride {
            consequence: "kit-level wording must stay distinguishable".to_owned(),
            reason: "synthetic counterexample".to_owned(),
        };
        let outcome =
            promote_kit_concern(&bd, &project, &kit_project, &item, &second, &evidence).unwrap();
        let second_id = outcome
            .target
            .as_deref()
            .unwrap()
            .strip_prefix("kit:")
            .unwrap()
            .to_owned();
        assert_ne!(second_id, existing_id);
        let listed = kit_feedback_ids(&bd, &kit_project);
        assert_eq!(listed.len(), 2, "{listed:?}");
        assert!(listed.contains(&existing_id) && listed.contains(&second_id));

        // An exact retry of the same concern reuses the task it created.
        let retry = record(
            &bd,
            &project,
            "second kit concern again",
            "lead-1",
            "e2",
            ReporterKind::Lead,
            &parent,
        );
        incubate_kit_concern(&bd, &project, &retry);
        let outcome =
            promote_kit_concern(&bd, &project, &kit_project, &retry, &second, &evidence).unwrap();
        assert_eq!(
            outcome.target.as_deref(),
            Some(format!("kit:{second_id}").as_str())
        );
        assert_eq!(kit_feedback_ids(&bd, &kit_project).len(), 2);
    }

    #[test]
    fn kit_concern_promotes_without_private_data_or_package_writes() {
        let Some(bd) = bd_executable() else {
            panic!("bd v1.3.0 is required on PATH, CODEX_HOME/harness/bin, or HARNESS_BD_EXE");
        };
        let (_root, project, parent) = isolated_feature(&bd);
        let (_kit_root, kit_project, _kit_parent) = isolated_feature(&bd);
        let before = seed_skill_package(&project);

        let observation = "kit board-workflow skill confuses our internal dispatch naming";
        let first = record(
            &bd,
            &project,
            observation,
            "exec-k",
            "e7",
            ReporterKind::Executor,
            &parent,
        );
        let second = record(
            &bd,
            &project,
            observation,
            "exec-l",
            "e8",
            ReporterKind::Executor,
            &parent,
        );
        let third = record(
            &bd,
            &project,
            observation,
            "lead-2",
            "e9",
            ReporterKind::Lead,
            &parent,
        );
        apply_routed_triage(
            &bd,
            &project,
            &[
                RoutedAction {
                    feedback_id: first.clone(),
                    kind: ObservationKind::KitConcern,
                    merge_into: None,
                },
                RoutedAction {
                    feedback_id: second.clone(),
                    kind: ObservationKind::KitConcern,
                    merge_into: Some(first.clone()),
                },
                RoutedAction {
                    feedback_id: third.clone(),
                    kind: ObservationKind::KitConcern,
                    merge_into: Some(first.clone()),
                },
            ],
            DEFAULT_FEEDBACK_BATCH_LIMIT,
        )
        .unwrap();

        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let threshold = crate::orchestration_config::load(&root)
            .unwrap()
            .vote_threshold;
        let error = promote_item(
            &bd,
            &project,
            &first,
            PromotionRoute::KitBacklog,
            &PromotionEvidence::Votes { threshold },
            None,
        )
        .unwrap_err();
        assert!(error.to_string().contains("promote_kit_concern"));

        let concern = KitConcern {
            summary: "board-workflow skill: document promotion and hygiene labels".to_owned(),
            scope: "kit skill: board-workflow".to_owned(),
        };
        let outcome = promote_kit_concern(
            &bd,
            &project,
            &kit_project,
            &first,
            &concern,
            &PromotionEvidence::Votes { threshold },
        )
        .unwrap();
        let kit_id = outcome
            .target
            .as_deref()
            .unwrap()
            .strip_prefix("kit:")
            .unwrap()
            .to_owned();
        assert_eq!(outcome.counted, 3);

        let shown = json_ok(&bd, &kit_project, &["show", &kit_id, "--json"]).unwrap();
        let kit_item = issue_object(&shown, &kit_id).unwrap();
        let title = kit_item["title"].as_str().unwrap();
        let description = issue_description(kit_item);
        assert!(title.starts_with("Kit feedback: "));
        assert!(
            description.contains("board-workflow skill: document promotion and hygiene labels")
        );
        assert!(description.contains("scope: kit skill: board-workflow"));
        for private in [observation, "exec-k", "e7", &parent] {
            assert!(!description.contains(private), "kit item leaked {private}");
        }
        assert!(
            issue_labels(kit_item)
                .iter()
                .any(|label| label == KIT_FEEDBACK_LABEL)
        );

        let snapshot = load_snapshot(&bd, &project, &first).unwrap().unwrap();
        assert!(
            snapshot
                .labels
                .iter()
                .any(|label| label == KIT_FORWARDED_LABEL)
        );
        assert!(!snapshot.labels.iter().any(|label| label == INCUBATOR_LABEL));
        let comments = list_comments(&bd, &project, &first).unwrap();
        assert!(comments.iter().any(|comment| {
            comment.contains("route=kit-backlog basis=votes counted=3")
                && comment.contains(&format!("target=kit:{kit_id}"))
        }));
        assert_eq!(skills_snapshot(&project), before);
    }

    #[test]
    fn lead_sweep_is_deterministic_restorable_and_defers_without_lead() {
        let Some(bd) = bd_executable() else {
            panic!("bd v1.3.0 is required on PATH, CODEX_HOME/harness/bin, or HARNESS_BD_EXE");
        };
        let (_root, project, parent) = isolated_feature(&bd);
        let mut items = Vec::new();
        for (index, kind) in [
            ObservationKind::Process,
            ObservationKind::Tool,
            ObservationKind::Unclear,
        ]
        .into_iter()
        .enumerate()
        {
            let id = record(
                &bd,
                &project,
                &format!("stale friction {index}"),
                &format!("exec-{index}"),
                &format!("e{index}"),
                ReporterKind::Executor,
                &parent,
            );
            apply_routed_triage(
                &bd,
                &project,
                &[RoutedAction {
                    feedback_id: id.clone(),
                    kind,
                    merge_into: None,
                }],
                DEFAULT_FEEDBACK_BATCH_LIMIT,
            )
            .unwrap();
            items.push(id);
        }

        assert_eq!(incubator_size(&bd, &project).unwrap(), 3);
        assert_eq!(incubator_over_cap(&bd, &project, 3).unwrap(), None);
        assert_eq!(incubator_over_cap(&bd, &project, 2).unwrap(), Some(3));

        // No lead session: the sweep defers without touching the board.
        let decision = SweepDecision {
            item_id: items[0].clone(),
            reason: "superseded by a newer observation".to_owned(),
        };
        let outcome = sweep_incubator(
            &bd,
            &project,
            ReporterKind::Executor,
            HygieneTrigger::IncubatorAboveCap { size: 3, cap: 2 },
            std::slice::from_ref(&decision),
        )
        .unwrap();
        assert_eq!(outcome, SweepOutcome::Deferred { size: 3 });
        assert_eq!(incubator_size(&bd, &project).unwrap(), 3);
        assert_eq!(
            load_snapshot(&bd, &project, &items[0])
                .unwrap()
                .unwrap()
                .status,
            "open"
        );

        // The lead sweeps on the trigger with visible reasons and history.
        let outcome = sweep_incubator(
            &bd,
            &project,
            ReporterKind::Lead,
            HygieneTrigger::IncubatorAboveCap { size: 3, cap: 2 },
            std::slice::from_ref(&decision),
        )
        .unwrap();
        assert_eq!(
            outcome,
            SweepOutcome::Swept {
                archived: items[..1].to_vec()
            }
        );
        assert_eq!(incubator_size(&bd, &project).unwrap(), 2);
        let snapshot = load_snapshot(&bd, &project, &items[0]).unwrap().unwrap();
        assert_eq!(snapshot.status, "closed");
        assert!(snapshot.labels.iter().any(|label| label == INCUBATOR_LABEL));
        let comments = list_comments(&bd, &project, &items[0]).unwrap();
        assert!(comments.iter().any(|comment| {
            comment.contains("feedback-archive v1 trigger=incubator-above-cap size=3 cap=2")
                && comment.contains("reason=superseded by a newer observation")
        }));
        assert_eq!(
            inspect_ledger(&bd, &project, &items[0]).unwrap().counted(),
            1
        );

        // A trigger that no longer matches the board is refused.
        let error = sweep_incubator(
            &bd,
            &project,
            ReporterKind::Lead,
            HygieneTrigger::IncubatorAboveCap { size: 3, cap: 2 },
            &[],
        )
        .unwrap_err();
        assert!(error.to_string().contains("does not match the board"));

        // Fresh evidence restores the item with its history visible.
        restore_archived(
            &bd,
            &project,
            &items[0],
            "fresh report of the same friction",
        )
        .unwrap();
        assert_eq!(incubator_size(&bd, &project).unwrap(), 3);
        let snapshot = load_snapshot(&bd, &project, &items[0]).unwrap().unwrap();
        assert_eq!(snapshot.status, "open");
        assert_eq!(
            inspect_ledger(&bd, &project, &items[0]).unwrap().counted(),
            1
        );

        // The stage/epic-closure trigger records itself.
        let outcome = sweep_incubator(
            &bd,
            &project,
            ReporterKind::Lead,
            HygieneTrigger::StageOrEpicClosed,
            &[SweepDecision {
                item_id: items[1].clone(),
                reason: "stale after stage acceptance".to_owned(),
            }],
        )
        .unwrap();
        assert_eq!(
            outcome,
            SweepOutcome::Swept {
                archived: vec![items[1].clone()]
            }
        );
        let comments = list_comments(&bd, &project, &items[1]).unwrap();
        assert!(comments.iter().any(|comment| {
            comment.contains("feedback-archive v1 trigger=stage-or-epic-closed")
        }));

        // Non-incubator items are never swept.
        let plain = json_ok(
            &bd,
            &project,
            &["create", "Plain task", "--type", "task", "--json"],
        )
        .unwrap();
        let plain_id = string_field(&plain, "id").unwrap();
        let error = sweep_incubator(
            &bd,
            &project,
            ReporterKind::Lead,
            HygieneTrigger::StageOrEpicClosed,
            &[SweepDecision {
                item_id: plain_id,
                reason: "not incubator".to_owned(),
            }],
        )
        .unwrap_err();
        assert!(error.to_string().contains("not an incubator item"));
    }

    #[test]
    fn pacing_observations_decisions_and_gate_records_round_trip_on_the_board() {
        use crate::benefit_gate::{default_allowed, parse_gate_comments};
        use crate::pacing::{applicable, parse_pacing_comments};
        use crate::scoped_observations::{account_view, parse_observation_comments};

        let Some(bd) = bd_executable() else {
            panic!("bd v1.3.0 is required on PATH, CODEX_HOME/harness/bin, or HARNESS_BD_EXE");
        };
        let (_root, project, parent) = isolated_feature(&bd);
        let item = json_ok(
            &bd,
            &project,
            &[
                "create",
                "Pacing: account window for the stage",
                "--type",
                "task",
                "--parent",
                &parent,
                "--description",
                "Synthetic pacing item.",
                "--json",
            ],
        )
        .unwrap();
        let item_id = string_field(&item, "id").unwrap();
        let now = 1_789_853_000;
        let reset = now + 1800;
        let comment = |text: &str| {
            json_ok(&bd, &project, &["comment", &item_id, "--json", text]).unwrap();
        };

        // The documented board-comment format, as the board-workflow skill
        // records it, is the only input the ledger reads back.
        comment(&format!(
            "pacing-observation v1 scope=gpt source=dashboard-snapshot used=93 resets_at={reset} window_minutes=10080 refusals=0 observed_at={} max_age=3600",
            now - 30
        ));
        comment(&format!(
            "pacing-observation v1 scope=xai source=provider-refusal used=unknown resets_at=unknown window_minutes=unknown refusals=1 observed_at={} max_age=3600",
            now - 60
        ));
        let observations =
            parse_observation_comments(&list_comments(&bd, &project, &item_id).unwrap());
        assert_eq!(observations.len(), 2);
        let gpt = account_view(&observations, "gpt", now);
        assert_eq!(gpt.used_percent, Some(93));
        assert_eq!(gpt.resets_at, Some(reset));
        let xai = account_view(&observations, "xai", now);
        assert_eq!(xai.used_percent, None);
        assert_eq!(xai.refusals, 1);

        comment(
            "pacing-decision v1 id=gpt:concurrency scope=gpt knob=concurrency from=4 to=1 expires_at=none reason=pressure basis=dashboard",
        );
        comment(
            "pacing-decision v1 id=gpt:effort scope=gpt knob=effort from=xhigh to=high expires_at=none reason=pressure basis=dashboard",
        );
        comment(&format!(
            "pacing-decision v1 id=gpt:new-assignments scope=gpt knob=new-assignments from=admit to=defer:2 expires_at={} reason=critical basis=dashboard",
            now - 1
        ));
        comment("pacing-revoke v1 id=gpt:effort reason=observation superseded");
        let records = parse_pacing_comments(&list_comments(&bd, &project, &item_id).unwrap());
        assert_eq!(records.decisions.len(), 3);
        assert_eq!(records.revoked, vec!["gpt:effort".to_owned()]);
        let usable = applicable(&records, now);
        assert_eq!(
            usable.len(),
            1,
            "withdrawn and expired decisions stop applying"
        );
        assert_eq!(usable[0].id(), "gpt:concurrency");
        assert!(usable[0].to_comment().contains("from=4 to=1"));

        for (outcome, quality) in [("reject", "regressed"), ("adopt", "unchanged")] {
            comment(&format!(
                "benefit-gate v1 item={item_id} improvement=lane-reuse outcome={outcome} quality={quality} matched=2 tolerance_percent=10.0 baseline_seconds=100.0 candidate_seconds=99.0 regression_percent=-1.0 baseline=direct candidate=lane accounting=check+coordination+rework detail=measured over two matched tasks"
            ));
        }
        let gate = parse_gate_comments(&list_comments(&bd, &project, &item_id).unwrap());
        assert_eq!(gate.len(), 2);
        assert!(default_allowed(&gate, &item_id), "the latest adoption wins");
        assert!(!default_allowed(&gate, "codex-harness-qr6.1"));
    }
}
