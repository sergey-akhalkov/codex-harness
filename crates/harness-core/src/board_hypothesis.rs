//! Hypothesis cards on the existing Beads board.
//!
//! The self-improvement loop keeps one durable `task` labeled `hypothesis`
//! per hypothesis. The card description carries the admitted identity
//! (mechanism, applicability conditions, evidence basis, prediction,
//! counterexample, acceptance and the linked OpenSpec change); native
//! comments carry the lifecycle events: experiment trials with their
//! nonblocking candidate/workload relationship, implementation references,
//! reconsiderations, retentions of completed real tasks and the user's scoped
//! removal decisions. A retention carries the replayable frozen task identity
//! with its independent oracle and acceptance references - never a solution,
//! patch or summary. No second journal, vote or approval tracker is
//! introduced - every operation here is a bounded `bd` call through the
//! existing board adapter.
//!
//! Admission searches open, closed and deferred cards first: a prior
//! same-condition conclusion is reused as recorded, and reconsidering it
//! records a fresh evidential basis on the same card instead of creating a
//! duplicate. Removal authority is read from the latest matching
//! `removal-decision v1` comment and never inferred from a benefit verdict,
//! run-start authority or an older approval.

use crate::benefit_gate;
use crate::board_cli::{json_ok_actor, string_field};
use crate::board_feedback;
use serde_json::Value;
use std::{io, path::Path, process::Command};

/// The label marking a durable hypothesis card.
pub const HYPOTHESIS_LABEL: &str = "hypothesis";
/// The description header of an admitted hypothesis card.
pub const ADMISSION_HEADER: &str = "hypothesis: 1";
/// One recorded experiment trial on a candidate card.
pub const TRIAL_PREFIX: &str = "hypothesis-trial v1";
/// One recorded implementation reference (candidate branch/worktree).
pub const IMPLEMENTATION_PREFIX: &str = "hypothesis-implementation v1";
/// One recorded retention of a completed real task on its owning card.
pub const RETENTION_PREFIX: &str = "hypothesis-retention v1";
/// One recorded reconsideration of an earlier conclusion.
pub const RECONSIDERATION_PREFIX: &str = "hypothesis-reconsideration v1";
/// One recorded removal proposal awaiting the user's decision.
pub const REMOVAL_PROPOSAL_PREFIX: &str = "removal-proposal v1";
/// One recorded explicit user removal approval, refusal or withdrawal.
pub const REMOVAL_DECISION_PREFIX: &str = "removal-decision v1";

const ACTOR: &str = "hypothesis-loop";
const MAX_MECHANISM: usize = 96;
const MAX_CONDITIONS: usize = 96;
const MAX_LOCATOR: usize = 192;
const MAX_STATEMENT: usize = 256;
const MAX_SPEC: usize = 128;
const MAX_BASIS: usize = 128;
const MAX_EXPERIMENT: usize = 128;
const MAX_REFERENCE: usize = 160;
const MAX_TARGET: usize = 96;
const MAX_REASON: usize = 160;
const MAX_DETAIL: usize = 512;
const MAX_TITLE_BYTES: usize = 72;

pub(crate) fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

/// A required single-line value; control characters and blank values are
/// refused so a recorded field cannot corrupt the comment/description shape.
pub(crate) fn require_line(name: &str, value: &str, max: usize) -> io::Result<String> {
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

/// A single whitespace-free token used as a `key=value` field. The character
/// set keeps recorded comments machine-readable and free of the ` detail=`
/// separator; full prose belongs in the bounded `detail` tail.
pub(crate) fn require_token(name: &str, value: &str, max: usize) -> io::Result<String> {
    let trimmed = require_line(name, value, max)?;
    if trimmed.bytes().any(|byte| {
        !(byte.is_ascii_alphanumeric()
            || matches!(
                byte,
                b'.' | b'_' | b'-' | b':' | b'@' | b'+' | b'/' | b'\\' | b',' | b';' | b'~' | b'#'
            ))
    }) {
        return Err(invalid(format!("{name} contains unsupported characters")));
    }
    Ok(trimmed)
}

fn optional_token(name: &str, value: Option<&str>, max: usize) -> io::Result<Option<String>> {
    let Some(value) = value else {
        return Ok(None);
    };
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    require_token(name, value, max).map(Some)
}

fn optional_detail(name: &str, value: Option<&str>, max: usize) -> io::Result<Option<String>> {
    let Some(value) = value else {
        return Ok(None);
    };
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    require_line(name, value, max).map(Some)
}

/// Truncates on a character boundary so a multi-byte mechanism cannot panic.
fn bounded_title(value: &str) -> &str {
    let mut end = 0;
    for character in value.chars() {
        let next = end + character.len_utf8();
        if next > MAX_TITLE_BYTES {
            break;
        }
        end = next;
    }
    &value[..end]
}

/// One candidate hypothesis before admission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HypothesisDraft {
    /// The causal mechanism: what change is expected to remove the burden.
    pub mechanism: String,
    /// Applicability conditions under which the mechanism is claimed to hold.
    pub conditions: String,
    /// The attributable observation and its evidence locator.
    pub observation: String,
    /// The predicted quality/time/resource effect.
    pub predicted: String,
    /// A plausible counterexample or disconfirming scenario.
    pub counterexample: String,
    /// The predeclared acceptance statement.
    pub acceptance: String,
    /// The linked OpenSpec change reference.
    pub spec: String,
    /// The evidential basis this admission relies on.
    pub basis: String,
}

/// A validated hypothesis admission record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundedHypothesis {
    pub mechanism: String,
    pub conditions: String,
    pub observation: String,
    pub predicted: String,
    pub counterexample: String,
    pub acceptance: String,
    pub spec: String,
    pub basis: String,
}

impl BoundedHypothesis {
    pub fn try_from_draft(draft: HypothesisDraft) -> io::Result<Self> {
        Ok(Self {
            mechanism: require_token("mechanism", &draft.mechanism, MAX_MECHANISM)?,
            conditions: require_token("conditions", &draft.conditions, MAX_CONDITIONS)?,
            observation: require_token("observation", &draft.observation, MAX_LOCATOR)?,
            predicted: require_line("predicted", &draft.predicted, MAX_STATEMENT)?,
            counterexample: require_line("counterexample", &draft.counterexample, MAX_STATEMENT)?,
            acceptance: require_line("acceptance", &draft.acceptance, MAX_STATEMENT)?,
            spec: require_token("spec", &draft.spec, MAX_SPEC)?,
            basis: require_token("basis", &draft.basis, MAX_BASIS)?,
        })
    }

    pub fn title(&self) -> String {
        format!("Hypothesis: {}", bounded_title(&self.mechanism))
    }

    pub fn description(&self) -> String {
        format!(
            "{ADMISSION_HEADER}\nmechanism: {}\nconditions: {}\nobservation: {}\npredicted: {}\ncounterexample: {}\nacceptance: {}\nspec: {}\nbasis: {}\n",
            self.mechanism,
            self.conditions,
            self.observation,
            self.predicted,
            self.counterexample,
            self.acceptance,
            self.spec,
            self.basis
        )
    }
}

/// The admitted identity parsed from a card description. `None` when the
/// document carries no recognized admission header; individual fields stay
/// optional so a foreign hypothesis-labeled card is still listed honestly.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HypothesisRecord {
    pub mechanism: Option<String>,
    pub conditions: Option<String>,
    pub observation: Option<String>,
    pub predicted: Option<String>,
    pub counterexample: Option<String>,
    pub acceptance: Option<String>,
    pub spec: Option<String>,
    pub basis: Option<String>,
}

pub fn parse_admission(description: &str) -> Option<HypothesisRecord> {
    let mut header = None;
    let mut record = HypothesisRecord::default();
    for line in description.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "hypothesis" => header = Some(value.to_owned()),
            "mechanism" => record.mechanism = Some(value.to_owned()),
            "conditions" => record.conditions = Some(value.to_owned()),
            "observation" => record.observation = Some(value.to_owned()),
            "predicted" => record.predicted = Some(value.to_owned()),
            "counterexample" => record.counterexample = Some(value.to_owned()),
            "acceptance" => record.acceptance = Some(value.to_owned()),
            "spec" => record.spec = Some(value.to_owned()),
            "basis" => record.basis = Some(value.to_owned()),
            _ => {}
        }
    }
    (header.as_deref() == Some("1")).then_some(record)
}

/// The latest recorded decision for a card, as read from its benefit-gate
/// comments. `outcome` is the recorded token; an unreadable record keeps
/// `None` so the caller can tell "no decision" from "unreadable decision".
/// `scope` is the decision scope the record binds (for example the task and
/// model scope), so prior-result search reads back the scope the conclusion
/// applies to and not just its verdict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecisionSummary {
    pub outcome: Option<String>,
    pub quality: Option<String>,
    pub experiment: Option<String>,
    pub scope: Option<String>,
    pub reason: Option<String>,
}

/// One hypothesis card with its recorded lifecycle summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HypothesisCard {
    pub id: String,
    pub title: String,
    pub status: String,
    pub created_at: String,
    pub mechanism: Option<String>,
    pub conditions: Option<String>,
    pub spec: Option<String>,
    pub basis: Option<String>,
    pub decisions: usize,
    pub latest_decision: Option<DecisionSummary>,
    pub reconsiderations: usize,
    pub trials: usize,
    pub implementations: usize,
    /// Retained completed real tasks recorded under this owner.
    pub retentions: usize,
}

/// What an admission search restricts to; both fields are exact matches
/// against the admitted identity.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchFilter {
    pub mechanism: Option<String>,
    pub conditions: Option<String>,
}

impl SearchFilter {
    fn matches(&self, card: &HypothesisCard) -> bool {
        self.mechanism
            .as_deref()
            .is_none_or(|value| card.mechanism.as_deref() == Some(value))
            && self
                .conditions
                .as_deref()
                .is_none_or(|value| card.conditions.as_deref() == Some(value))
    }
}

/// Lists every hypothesis card, including closed and deferred ones, from the
/// board's own label view; comments are not read here.
pub fn list_hypothesis_cards(bd: &Path, project: &Path) -> io::Result<Vec<HypothesisCard>> {
    let listed = json_ok_actor(
        bd,
        project,
        ACTOR,
        &["list", "--label", HYPOTHESIS_LABEL, "--all", "--json"],
    )?;
    let rows = listed.as_array().ok_or_else(|| {
        invalid("bd list --label hypothesis --all was not a JSON array".to_owned())
    })?;
    let mut cards = Vec::with_capacity(rows.len());
    for row in rows {
        let id = string_field(row, "id")?;
        let status = row
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_owned();
        let created_at = row
            .get("created_at")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        let title = row
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        let record = parse_admission(
            row.get("description")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        )
        .unwrap_or_default();
        cards.push(HypothesisCard {
            id,
            title,
            status,
            created_at,
            mechanism: record.mechanism,
            conditions: record.conditions,
            spec: record.spec,
            basis: record.basis,
            decisions: 0,
            latest_decision: None,
            reconsiderations: 0,
            trials: 0,
            implementations: 0,
            retentions: 0,
        });
    }
    Ok(cards)
}

/// Lists hypothesis cards matching the filter, reading each matching card's
/// comments for its decision, reconsideration, trial, implementation and
/// retention counts. Closed and deferred cards participate exactly like open
/// ones.
pub fn search_hypotheses(
    bd: &Path,
    project: &Path,
    filter: &SearchFilter,
) -> io::Result<Vec<HypothesisCard>> {
    let mut cards = list_hypothesis_cards(bd, project)?;
    cards.retain(|card| filter.matches(card));
    for card in &mut cards {
        let comments = board_feedback::list_comments(bd, project, &card.id)?;
        let (decisions, latest) = summarize_decisions(&comments, &card.id);
        card.decisions = decisions;
        card.latest_decision = latest;
        card.reconsiderations = comments
            .iter()
            .filter(|comment| comment.starts_with(RECONSIDERATION_PREFIX))
            .count();
        card.trials = comments
            .iter()
            .filter(|comment| comment.starts_with(TRIAL_PREFIX))
            .count();
        card.implementations = comments
            .iter()
            .filter(|comment| comment.starts_with(IMPLEMENTATION_PREFIX))
            .count();
        card.retentions = comments
            .iter()
            .filter(|comment| comment.starts_with(RETENTION_PREFIX))
            .count();
    }
    Ok(cards)
}

fn summarize_decisions(comments: &[String], item: &str) -> (usize, Option<DecisionSummary>) {
    let mut decisions = 0;
    let mut latest = None;
    for record in benefit_gate::parse_gate_comments(comments) {
        if record.item != item {
            continue;
        }
        decisions += 1;
        latest = Some(DecisionSummary {
            outcome: record.outcome.clone(),
            quality: record.quality.clone(),
            experiment: record.experiment.clone(),
            scope: record.scope.clone(),
            reason: record.reason.clone(),
        });
    }
    (decisions, latest)
}

/// The latest recorded decision for one card, or `None` when no readable
/// benefit-gate record is attributed to it. Callers that reconcile an
/// unadopted decision with its OpenSpec change read the decision scope and
/// experiment from here instead of trusting caller-supplied prose.
pub fn latest_decision(
    bd: &Path,
    project: &Path,
    item: &str,
) -> io::Result<Option<DecisionSummary>> {
    let comments = board_feedback::list_comments(bd, project, item)?;
    Ok(summarize_decisions(&comments, item).1)
}

/// The outcome of an admission attempt against the existing board.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Admission {
    /// A new card was created for this hypothesis.
    Created { id: String },
    /// A matching card is already active (open, deferred, in review...); no
    /// duplicate card is created.
    Existing { id: String, status: String },
    /// A prior same-condition rejection is reused as recorded; a fresh
    /// evidential basis is required before reconsidering it.
    ReusedRejection {
        id: String,
        experiment: Option<String>,
        reason: Option<String>,
        basis: Option<String>,
    },
    /// A prior same-condition inconclusive result is reused as recorded.
    ReusedInconclusive {
        id: String,
        experiment: Option<String>,
        reason: Option<String>,
        basis: Option<String>,
    },
    /// A fresh basis was recorded on the existing card and the card was
    /// reactivated; earlier conclusions are preserved.
    Reconsidered {
        id: String,
        basis: String,
        prior_outcome: String,
        prior_experiment: Option<String>,
    },
}

/// Admits a hypothesis. The search covers open, closed and deferred cards for
/// the same mechanism and conditions before any creation:
///
/// - an active card is returned as [`Admission::Existing`];
/// - a terminal card whose latest conclusion is `reject` or `inconclusive`
///   reuses that conclusion unless `fresh_basis` names evidence not already
///   recorded on the card, in which case a reconsideration comment is
///   recorded and the card is reactivated;
/// - otherwise one new `task` labeled `hypothesis` is created.
pub fn admit_hypothesis(
    bd: &Path,
    project: &Path,
    draft: &BoundedHypothesis,
    fresh_basis: Option<&str>,
) -> io::Result<Admission> {
    let fresh = fresh_basis
        .map(|value| require_token("fresh basis", value, MAX_BASIS))
        .transpose()?;
    let filter = SearchFilter {
        mechanism: Some(draft.mechanism.clone()),
        conditions: Some(draft.conditions.clone()),
    };
    let cards = search_hypotheses(bd, project, &filter)?;
    let Some(card) = cards
        .iter()
        .max_by(|left, right| left.created_at.cmp(&right.created_at))
    else {
        let created = json_ok_actor(
            bd,
            project,
            ACTOR,
            &[
                "create",
                &draft.title(),
                "--type",
                "task",
                "--labels",
                HYPOTHESIS_LABEL,
                "--description",
                &draft.description(),
                "--json",
            ],
        )?;
        return Ok(Admission::Created {
            id: string_field(&created, "id")?,
        });
    };

    let terminal = matches!(card.status.as_str(), "closed" | "deferred");
    let outcome = card
        .latest_decision
        .as_ref()
        .and_then(|decision| decision.outcome.clone());
    if !terminal || !matches!(outcome.as_deref(), Some("reject" | "inconclusive")) {
        return Ok(Admission::Existing {
            id: card.id.clone(),
            status: card.status.clone(),
        });
    }

    let experiment = card
        .latest_decision
        .as_ref()
        .and_then(|decision| decision.experiment.clone());
    let reason = card
        .latest_decision
        .as_ref()
        .and_then(|decision| decision.reason.clone());
    let comments = board_feedback::list_comments(bd, project, &card.id)?;
    let reuse = || match outcome.as_deref() {
        Some("inconclusive") => Admission::ReusedInconclusive {
            id: card.id.clone(),
            experiment: experiment.clone(),
            reason: reason.clone(),
            basis: card.basis.clone(),
        },
        _ => Admission::ReusedRejection {
            id: card.id.clone(),
            experiment: experiment.clone(),
            reason: reason.clone(),
            basis: card.basis.clone(),
        },
    };
    let Some(basis) = fresh else {
        return Ok(reuse());
    };
    let mut used_bases: Vec<String> = card.basis.clone().into_iter().collect();
    for comment in &comments {
        if let Some(record) = parse_reconsideration(comment)
            && record.item == card.id
        {
            used_bases.push(record.basis);
        }
    }
    if used_bases.contains(&basis) {
        return Ok(reuse());
    }

    let prior_outcome = outcome.unwrap_or_else(|| "inconclusive".to_owned());
    let text = format!(
        "{RECONSIDERATION_PREFIX} item={} basis={basis} prior={} conclusion={prior_outcome}",
        card.id,
        experiment.as_deref().unwrap_or("none")
    );
    write_comment_once(bd, project, &card.id, &comments, &text)?;
    let reason = format!("hypothesis reconsideration basis={basis}");
    if card.status == "closed" {
        json_ok_actor(
            bd,
            project,
            ACTOR,
            &["reopen", &card.id, "--reason", &reason, "--json"],
        )?;
    } else {
        json_ok_actor(
            bd,
            project,
            ACTOR,
            &["update", &card.id, "--defer", "", "--json"],
        )?;
    }
    Ok(Admission::Reconsidered {
        id: card.id.clone(),
        basis,
        prior_outcome,
        prior_experiment: experiment,
    })
}

/// The role one hypothesis plays in a recorded experiment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HypothesisRole {
    Candidate,
    Workload,
}

impl HypothesisRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Candidate => "candidate",
            Self::Workload => "workload",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "candidate" => Some(Self::Candidate),
            "workload" => Some(Self::Workload),
            _ => None,
        }
    }
}

/// One experiment trial: the candidate hypothesis and the workload hypothesis
/// it is evaluated on, tied by a nonblocking `related` edge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrialDraft {
    pub experiment: String,
    pub role: HypothesisRole,
    pub counterpart: String,
    pub evidence: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundedTrial {
    pub experiment: String,
    pub role: HypothesisRole,
    pub counterpart: String,
    pub evidence: Option<String>,
}

impl BoundedTrial {
    pub fn try_from_draft(draft: TrialDraft) -> io::Result<Self> {
        Ok(Self {
            experiment: require_token("experiment", &draft.experiment, MAX_EXPERIMENT)?,
            role: draft.role,
            counterpart: require_token("counterpart", &draft.counterpart, MAX_EXPERIMENT)?,
            evidence: optional_token("evidence", draft.evidence.as_deref(), MAX_REFERENCE)?,
        })
    }

    fn comment(&self, item: &str) -> String {
        format!(
            "{TRIAL_PREFIX} item={item} experiment={} role={} counterpart={} evidence={}",
            self.experiment,
            self.role.as_str(),
            self.counterpart,
            self.evidence.as_deref().unwrap_or("none")
        )
    }
}

/// The result of recording a trial.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrialRecord {
    pub experiment: String,
    pub role: HypothesisRole,
    pub counterpart: String,
    /// A new trial comment was written; `false` when the identical trial was
    /// already recorded.
    pub recorded: bool,
    /// The nonblocking related edge was established by this call.
    pub related: bool,
}

/// Records one experiment trial on a hypothesis card and establishes the
/// nonblocking candidate/workload relationship. Repeating the identical
/// trial adds no duplicate comment; the queue is never blocked through a
/// `blocks` edge.
pub fn record_trial(
    bd: &Path,
    project: &Path,
    item: &str,
    trial: &BoundedTrial,
) -> io::Result<TrialRecord> {
    require_hypothesis_card(bd, project, item)?;
    if item == trial.counterpart {
        return Err(invalid(
            "a hypothesis cannot be its own experiment counterpart",
        ));
    }
    require_hypothesis_card(bd, project, &trial.counterpart)?;
    let existing = board_feedback::list_comments(bd, project, item)?;
    let already = existing.iter().any(|comment| {
        parse_trial(comment).is_some_and(|record| {
            record.item == item
                && record.experiment == trial.experiment
                && record.role == Some(trial.role)
                && record.counterpart.as_deref() == Some(trial.counterpart.as_str())
        })
    });
    if !already {
        write_comment_once(bd, project, item, &existing, &trial.comment(item))?;
    }
    let (candidate, workload) = match trial.role {
        HypothesisRole::Candidate => (item, trial.counterpart.as_str()),
        HypothesisRole::Workload => (trial.counterpart.as_str(), item),
    };
    let related = ensure_related(bd, project, candidate, workload)?;
    Ok(TrialRecord {
        experiment: trial.experiment.clone(),
        role: trial.role,
        counterpart: trial.counterpart.clone(),
        recorded: !already,
        related,
    })
}

/// One implementation reference bound to a hypothesis card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImplementationDraft {
    pub role: HypothesisRole,
    pub branch: String,
    pub base: String,
    pub revision: String,
    pub worktree: String,
    pub runtime: Option<String>,
    pub baseline_runtime: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundedImplementation {
    pub role: HypothesisRole,
    pub branch: String,
    pub base: String,
    pub revision: String,
    pub worktree: String,
    pub runtime: Option<String>,
    pub baseline_runtime: Option<String>,
}

impl BoundedImplementation {
    pub fn try_from_draft(draft: ImplementationDraft) -> io::Result<Self> {
        Ok(Self {
            role: draft.role,
            branch: require_token("branch", &draft.branch, MAX_LOCATOR)?,
            base: require_token("base", &draft.base, MAX_LOCATOR)?,
            revision: require_token("revision", &draft.revision, MAX_LOCATOR)?,
            worktree: require_token("worktree", &draft.worktree, MAX_LOCATOR)?,
            runtime: optional_token("runtime", draft.runtime.as_deref(), MAX_REFERENCE)?,
            baseline_runtime: optional_token(
                "baseline runtime",
                draft.baseline_runtime.as_deref(),
                MAX_REFERENCE,
            )?,
        })
    }

    fn comment(&self, item: &str) -> String {
        format!(
            "{IMPLEMENTATION_PREFIX} item={item} role={} branch={} base={} revision={} worktree={} runtime={} baseline={}",
            self.role.as_str(),
            self.branch,
            self.base,
            self.revision,
            self.worktree,
            self.runtime.as_deref().unwrap_or("none"),
            self.baseline_runtime.as_deref().unwrap_or("none")
        )
    }
}

/// The result of recording an implementation reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImplementationRecord {
    pub role: HypothesisRole,
    pub branch: String,
    pub revision: String,
    pub recorded: bool,
}

/// Records a candidate branch/base/revision/worktree and prepared runtime
/// identities on the card. An un-evaluated implementation stays a pending
/// state on the same card: nothing here records adoption or closes the
/// hypothesis.
pub fn record_implementation(
    bd: &Path,
    project: &Path,
    item: &str,
    implementation: &BoundedImplementation,
) -> io::Result<ImplementationRecord> {
    let snapshot = require_hypothesis_card(bd, project, item)?;
    if snapshot.status == "closed" {
        return Err(invalid(format!(
            "hypothesis card {item} is closed; a closed investigation needs a recorded reconsideration basis before new implementation work"
        )));
    }
    let text = implementation.comment(item);
    let comments = board_feedback::list_comments(bd, project, item)?;
    let already = comments.iter().any(|comment| comment == &text);
    if !already {
        write_comment_once(bd, project, item, &comments, &text)?;
    }
    Ok(ImplementationRecord {
        role: implementation.role,
        branch: implementation.branch.clone(),
        revision: implementation.revision.clone(),
        recorded: !already,
    })
}

/// One retention declaration for a completed real task, recorded on the card
/// that already owns it. The record carries bounded references only - the
/// frozen task identity, the retained pristine pre-solution replay copy and
/// the independent oracle and acceptance references - so reading it back
/// cannot hand a solution to a fresh executor. A missing oracle, acceptance or
/// replay reference is refused: no summary substitutes for retained evidence.
/// [`record_retention`] additionally resolves the frozen git tree object id
/// from the retained replay copy and records it, so a later run can rebuild
/// the task verifiably from the board alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetentionDraft {
    pub case_id: String,
    pub experiment: String,
    pub mechanism: String,
    pub conditions: String,
    /// The committed source revision the completed real task ran.
    pub revision: String,
    /// The frozen root commit materialized from that revision.
    pub frozen: String,
    /// Content digest over the frozen tree entries.
    pub tree: String,
    pub oracle: String,
    pub acceptance: String,
    /// The retained pristine pre-solution copy used for replay.
    pub replay: String,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundedRetention {
    pub case_id: String,
    pub experiment: String,
    pub mechanism: String,
    pub conditions: String,
    pub revision: String,
    pub frozen: String,
    pub tree: String,
    pub oracle: String,
    pub acceptance: String,
    pub replay: String,
    pub detail: Option<String>,
}

impl BoundedRetention {
    pub fn try_from_draft(draft: RetentionDraft) -> io::Result<Self> {
        Ok(Self {
            case_id: require_token("case", &draft.case_id, MAX_EXPERIMENT)?,
            experiment: require_token("experiment", &draft.experiment, MAX_EXPERIMENT)?,
            mechanism: require_token("mechanism", &draft.mechanism, MAX_MECHANISM)?,
            conditions: require_token("conditions", &draft.conditions, MAX_CONDITIONS)?,
            revision: require_token("revision", &draft.revision, MAX_LOCATOR)?,
            frozen: require_token("frozen", &draft.frozen, MAX_LOCATOR)?,
            tree: require_token("tree", &draft.tree, MAX_LOCATOR)?,
            oracle: require_token("oracle", &draft.oracle, MAX_LOCATOR)?,
            acceptance: require_token("acceptance", &draft.acceptance, MAX_LOCATOR)?,
            replay: require_token("replay", &draft.replay, MAX_LOCATOR)?,
            detail: optional_detail("detail", draft.detail.as_deref(), MAX_DETAIL)?,
        })
    }

    fn comment(&self, item: &str, tree_object: &str) -> String {
        let mut text = format!(
            "{RETENTION_PREFIX} item={item} case={} experiment={} mechanism={} conditions={} revision={} frozen={} tree={} tree_object={} oracle={} acceptance={} replay={}",
            self.case_id,
            self.experiment,
            self.mechanism,
            self.conditions,
            self.revision,
            self.frozen,
            self.tree,
            tree_object,
            self.oracle,
            self.acceptance,
            self.replay
        );
        if let Some(detail) = &self.detail {
            text.push_str(&format!(" detail={detail}"));
        }
        text
    }
}

/// The result of recording a retention.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetentionRecord {
    pub case_id: String,
    /// A new retention comment was written; `false` when the identical
    /// retention was already recorded.
    pub recorded: bool,
}

/// Records the retained replayable identity of one completed real task on the
/// card that already owns it. The card must exist: retention never creates a
/// card, a synthetic task or a second task store. The frozen git tree object
/// id is resolved from the retained replay copy and recorded with the bounded
/// references; a retention whose frozen identity cannot be resolved is refused
/// and not recorded, so a later run never reads an identity the retained
/// artifact cannot support.
pub fn record_retention(
    bd: &Path,
    project: &Path,
    item: &str,
    retention: &BoundedRetention,
) -> io::Result<RetentionRecord> {
    require_hypothesis_card(bd, project, item)?;
    let tree_object = frozen_tree_object(&retention.replay, &retention.frozen)?;
    let text = retention.comment(item, &tree_object);
    let comments = board_feedback::list_comments(bd, project, item)?;
    let already = comments.iter().any(|comment| comment == &text);
    if !already {
        write_comment_once(bd, project, item, &comments, &text)?;
    }
    Ok(RetentionRecord {
        case_id: retention.case_id.clone(),
        recorded: !already,
    })
}

/// Resolve the frozen git tree object id recorded for a retained replay copy:
/// the recorded frozen root commit must exist in the retained repository and
/// record exactly this tree. A missing or unreadable artifact refuses the
/// retention instead of recording an identity a later run cannot rebuild.
fn frozen_tree_object(replay: &str, frozen: &str) -> io::Result<String> {
    let path = Path::new(replay);
    if !path.is_dir() {
        return Err(invalid(format!(
            "the retained replay copy {replay} is missing; its frozen tree object id cannot be resolved and the retention is not recorded"
        )));
    }
    let out = Command::new("git")
        .args([
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("{frozen}^{{tree}}"),
        ])
        .current_dir(path)
        .output()
        .map_err(|error| {
            invalid(format!(
                "the retained replay copy {replay} could not be read for its frozen tree object id: {error}"
            ))
        })?;
    let object = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    if !out.status.success()
        || !(40..=64).contains(&object.len())
        || !object
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    {
        return Err(invalid(format!(
            "the retained replay copy {replay} does not contain frozen commit {frozen}; its frozen tree object id cannot be resolved and the retention is not recorded"
        )));
    }
    Ok(object)
}

/// One reviewable removal proposal recorded before the user's decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemovalProposalDraft {
    /// The reviewed proposal/source basis (for example the linked OpenSpec
    /// change).
    pub proposal: String,
    /// The named removal target.
    pub target: String,
    /// The evidence locator behind the proposal.
    pub evidence: String,
    /// The behavior loss this removal would cause, as a bounded label.
    pub loss: String,
    /// The unapplied preview locator.
    pub preview: Option<String>,
    /// Bounded prose detail; the board record stays a reference, not a
    /// transcript.
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundedRemovalProposal {
    pub proposal: String,
    pub target: String,
    pub evidence: String,
    pub loss: String,
    pub preview: Option<String>,
    pub detail: Option<String>,
}

impl BoundedRemovalProposal {
    pub fn try_from_draft(draft: RemovalProposalDraft) -> io::Result<Self> {
        Ok(Self {
            proposal: require_token("proposal", &draft.proposal, MAX_REFERENCE)?,
            target: require_token("target", &draft.target, MAX_TARGET)?,
            evidence: require_token("evidence", &draft.evidence, MAX_REFERENCE)?,
            loss: require_token("loss", &draft.loss, MAX_TARGET)?,
            preview: optional_token("preview", draft.preview.as_deref(), MAX_REFERENCE)?,
            detail: optional_detail("detail", draft.detail.as_deref(), MAX_DETAIL)?,
        })
    }

    fn comment(&self, item: &str) -> String {
        let mut text = format!(
            "{REMOVAL_PROPOSAL_PREFIX} item={item} proposal={} target={} evidence={} loss={} preview={}",
            self.proposal,
            self.target,
            self.evidence,
            self.loss,
            self.preview.as_deref().unwrap_or("none")
        );
        if let Some(detail) = &self.detail {
            text.push_str(&format!(" detail={detail}"));
        }
        text
    }
}

/// The result of recording a removal proposal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemovalProposalRecord {
    pub proposal: String,
    pub target: String,
    pub recorded: bool,
}

/// Records one removal proposal version on the card. The proposal is a
/// reference for the user's decision; nothing here applies the removal.
///
/// A changed version (different loss, evidence, preview or detail) for the
/// same proposal and target is appended as the new current version and
/// reported as recorded: an approval is bound to the exact reviewed content,
/// so a changed proposal needs a fresh decision before any further removal
/// effect. Repeating the current version adds no comment.
pub fn record_removal_proposal(
    bd: &Path,
    project: &Path,
    item: &str,
    proposal: &BoundedRemovalProposal,
) -> io::Result<RemovalProposalRecord> {
    require_hypothesis_card(bd, project, item)?;
    let text = proposal.comment(item);
    let expected = parse_removal_proposal(&text).expect("self-parsed removal proposal");
    let comments = board_feedback::list_comments(bd, project, item)?;
    let current = scoped_proposal_evidence(&comments, item, &proposal.proposal, &proposal.target)
        .as_ref()
        .and_then(ProposalEvidence::complete);
    let recorded = current.as_ref() != Some(&expected);
    if recorded {
        json_ok_actor(bd, project, ACTOR, &["comment", item, "--json", &text])?;
    }
    Ok(RemovalProposalRecord {
        proposal: proposal.proposal.clone(),
        target: proposal.target.clone(),
        recorded,
    })
}

/// The user's explicit decision about a presented removal proposal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemovalDecisionKind {
    Approve,
    Refuse,
    Withdraw,
}

impl RemovalDecisionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Approve => "approve",
            Self::Refuse => "refuse",
            Self::Withdraw => "withdraw",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "approve" => Some(Self::Approve),
            "refuse" => Some(Self::Refuse),
            "withdraw" => Some(Self::Withdraw),
            _ => None,
        }
    }
}

/// One stage the user's approval can expressly cover.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RemovalAction {
    Experiment,
    Integration,
    Publication,
}

impl RemovalAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Experiment => "experiment",
            Self::Integration => "integration",
            Self::Publication => "publication",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "experiment" => Some(Self::Experiment),
            "integration" => Some(Self::Integration),
            "publication" => Some(Self::Publication),
            _ => None,
        }
    }
}

/// One explicit user removal decision bound to a proposal, target, covered
/// actions and the user's decision reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemovalDecisionDraft {
    pub decision: RemovalDecisionKind,
    pub proposal: String,
    pub target: String,
    pub actions: Vec<RemovalAction>,
    pub loss: Option<String>,
    pub basis: Option<String>,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundedRemovalDecision {
    pub decision: RemovalDecisionKind,
    pub proposal: String,
    pub target: String,
    pub actions: Vec<RemovalAction>,
    pub loss: Option<String>,
    pub basis: Option<String>,
    pub detail: Option<String>,
}

impl BoundedRemovalDecision {
    pub fn try_from_draft(draft: RemovalDecisionDraft) -> io::Result<Self> {
        let mut actions = draft.actions;
        actions.sort_unstable();
        actions.dedup();
        match draft.decision {
            RemovalDecisionKind::Approve if actions.is_empty() => {
                return Err(invalid(
                    "an approval must name the covered actions (experiment, integration, publication)",
                ));
            }
            RemovalDecisionKind::Refuse | RemovalDecisionKind::Withdraw if !actions.is_empty() => {
                return Err(invalid(
                    "covered actions apply only to an approval; a refusal or withdrawal authorizes nothing",
                ));
            }
            _ => {}
        }
        Ok(Self {
            decision: draft.decision,
            proposal: require_token("proposal", &draft.proposal, MAX_REFERENCE)?,
            target: require_token("target", &draft.target, MAX_TARGET)?,
            actions,
            loss: optional_token("loss", draft.loss.as_deref(), MAX_TARGET)?,
            basis: optional_token("basis", draft.basis.as_deref(), MAX_REFERENCE)?,
            detail: optional_detail("detail", draft.detail.as_deref(), MAX_DETAIL)?,
        })
    }

    /// Binds the decision text to the reviewed proposal content: the behavior
    /// loss, evidence and unapplied preview are copied exactly as reviewed and
    /// `reviewed=` carries the digest over every reviewed field, including the
    /// bounded prose detail, so any later proposal change is detectable and
    /// needs a fresh decision.
    fn comment(&self, item: &str, reviewed: &RecordedRemovalProposal) -> String {
        let actions = if self.actions.is_empty() {
            "none".to_owned()
        } else {
            self.actions
                .iter()
                .map(|action| action.as_str())
                .collect::<Vec<_>>()
                .join("+")
        };
        let mut text = format!(
            "{REMOVAL_DECISION_PREFIX} item={item} decision={} proposal={} target={} actions={actions} loss={} evidence={} preview={} reviewed={} basis={}",
            self.decision.as_str(),
            reviewed.proposal,
            reviewed.target,
            reviewed.loss,
            reviewed.evidence,
            reviewed.preview.as_deref().unwrap_or("none"),
            reviewed_proposal_digest(reviewed),
            self.basis.as_deref().unwrap_or("none")
        );
        if let Some(detail) = &self.detail {
            text.push_str(&format!(" detail={detail}"));
        }
        text
    }
}

/// A parsed removal decision; unreadable fields stay `None` so the latest
/// record still supersedes an older approval instead of being skipped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemovalDecisionRecord {
    pub item: String,
    pub decision: Option<RemovalDecisionKind>,
    pub proposal: Option<String>,
    pub target: Option<String>,
    pub actions: Vec<RemovalAction>,
    /// The reviewed behavior loss, copied from the proposal at decision time.
    pub loss: Option<String>,
    /// The reviewed evidence reference, copied from the proposal.
    pub evidence: Option<String>,
    /// The reviewed unapplied preview reference, copied from the proposal.
    pub preview: Option<String>,
    /// The digest over every reviewed proposal field, including the bounded
    /// prose detail; the authority comparison uses this binding.
    pub reviewed: Option<String>,
    pub basis: Option<String>,
    pub detail: Option<String>,
}

pub fn parse_removal_decisions(comments: &[String]) -> Vec<RemovalDecisionRecord> {
    comments
        .iter()
        .filter_map(|comment| parse_removal_decision(comment))
        .collect()
}

fn parse_removal_decision(comment: &str) -> Option<RemovalDecisionRecord> {
    let rest = comment.strip_prefix(REMOVAL_DECISION_PREFIX)?.trim();
    let (head, detail) = split_detail(rest);
    let mut item = None;
    let mut decision = None;
    let mut proposal = None;
    let mut target = None;
    let mut actions = Vec::new();
    let mut loss = None;
    let mut evidence = None;
    let mut preview = None;
    let mut reviewed = None;
    let mut basis = None;
    for part in head.split_whitespace() {
        let Some((key, value)) = part.split_once('=') else {
            continue;
        };
        match key {
            "item" => item = Some(value.to_owned()),
            "decision" => decision = RemovalDecisionKind::parse(value),
            "proposal" => proposal = Some(value.to_owned()),
            "target" => target = Some(value.to_owned()),
            "actions" => {
                actions = value.split('+').filter_map(RemovalAction::parse).collect();
            }
            "loss" => loss = non_none(value),
            "evidence" => evidence = non_none(value),
            "preview" => preview = non_none(value),
            "reviewed" => reviewed = Some(value.to_owned()),
            "basis" => basis = non_none(value),
            _ => {}
        }
    }
    Some(RemovalDecisionRecord {
        item: item.filter(|value| !value.is_empty())?,
        decision,
        proposal,
        target,
        actions,
        loss,
        evidence,
        preview,
        reviewed,
        basis,
        detail: detail.map(str::to_owned),
    })
}

/// Records the user's explicit removal decision on the card, bound to the
/// exact reviewed proposal content: every reviewed field, including the
/// bounded prose detail, enters the recorded `reviewed=` digest.
///
/// - An approval must name the reviewed behavior loss and it must equal the
///   recorded proposal's loss; a mismatched loss cannot claim the reviewed
///   scope.
/// - The latest proposal version for the scope must be complete; an
///   incomplete newer record cannot silently fall back to an older proposal,
///   so the complete proposal must be recorded first.
/// - The decision is idempotent only against the *latest* decision: repeating
///   the current decision confirms it without a new comment, while a decision
///   that differs from the current one is appended and truthfully reported as
///   a new recorded decision (even when an older comment carries the same
///   text), so it becomes the current authority.
pub fn record_removal_decision(
    bd: &Path,
    project: &Path,
    item: &str,
    decision: &BoundedRemovalDecision,
) -> io::Result<bool> {
    require_hypothesis_card(bd, project, item)?;
    let comments = board_feedback::list_comments(bd, project, item)?;
    let Some(evidence) =
        scoped_proposal_evidence(&comments, item, &decision.proposal, &decision.target)
    else {
        return Err(invalid(format!(
            "no removal proposal with proposal={} target={} is recorded on {item}; record the reviewed proposal before the user's decision",
            decision.proposal, decision.target
        )));
    };
    let Some(reviewed) = evidence.complete() else {
        return Err(invalid(format!(
            "the latest removal proposal record for proposal={} target={} on {item} is incomplete; it supersedes earlier versions, so record the complete reviewed proposal before the user's decision",
            decision.proposal, decision.target
        )));
    };
    if let Some(loss) = decision.loss.as_deref()
        && loss != reviewed.loss
    {
        return Err(invalid(format!(
            "the decision's loss {loss} does not match the recorded proposal's behavior loss {}; a decision cannot claim a scope the user did not review",
            reviewed.loss
        )));
    }
    if decision.decision == RemovalDecisionKind::Approve && decision.loss.is_none() {
        return Err(invalid(
            "an approval must identify the reviewed behavior loss (loss) so it binds the reviewed proposal content",
        ));
    }
    let text = decision.comment(item, &reviewed);
    let expected = parse_removal_decision(&text).expect("self-parsed removal decision");
    let latest = parse_removal_decisions(&comments)
        .into_iter()
        .rfind(|record| record.item == item);
    if latest.as_ref() == Some(&expected) {
        return Ok(false);
    }
    json_ok_actor(bd, project, ACTOR, &["comment", item, "--json", &text])?;
    Ok(true)
}

/// One requested stage for which the recorded authority is resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorityRequest {
    pub proposal: String,
    pub target: String,
    pub action: RemovalAction,
}

/// The resolved removal authority for one exact proposal/target/action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemovalAuthority {
    /// The latest decision is an approval that covers the requested scope.
    Authorized { record: RemovalDecisionRecord },
    /// The latest decision for this scope is the user's refusal.
    Refused { record: RemovalDecisionRecord },
    /// The latest decision withdrew earlier approval for this scope.
    Withdrawn { record: RemovalDecisionRecord },
    /// No removal decision is recorded for this card.
    Missing,
    /// A decision exists but does not cover the requested scope - including
    /// an unreadable latest record, which never falls back to an older
    /// approval.
    NotCovered {
        reason: String,
        record: Option<RemovalDecisionRecord>,
    },
}

/// Resolves the current removal authority from the card's decision comments
/// and the current proposal content. The latest attributable decision
/// controls, and it is bound to the exact reviewed proposal:
///
/// - an unreadable or out-of-scope newer decision leaves the request
///   uncovered rather than restoring an older approval;
/// - a proposal version recorded after the decision with any changed reviewed
///   field - loss, evidence, preview or the bounded prose detail (for example
///   a newly discovered consumer) - needs a fresh decision, since the old
///   consent covered different content;
/// - an incomplete or unreadable newer proposal version for the scope is not
///   skipped in favor of an older complete version;
/// - a benefit verdict is not consulted at all.
pub fn removal_authority(
    comments: &[String],
    item: &str,
    request: &AuthorityRequest,
) -> RemovalAuthority {
    let records = parse_removal_decisions(comments);
    let Some(latest) = records
        .iter()
        .rev()
        .find(|record| record.item == item)
        .cloned()
    else {
        return RemovalAuthority::Missing;
    };
    let Some(kind) = latest.decision else {
        return RemovalAuthority::NotCovered {
            reason: "the latest removal decision is unreadable: no recognized decision".to_owned(),
            record: Some(latest),
        };
    };
    let same_scope = latest.proposal.as_deref() == Some(request.proposal.as_str())
        && latest.target.as_deref() == Some(request.target.as_str());
    if !same_scope {
        return RemovalAuthority::NotCovered {
            reason: format!(
                "the latest removal {} addresses proposal={} target={}; {} / {} has no current decision",
                kind.as_str(),
                latest.proposal.as_deref().unwrap_or("absent"),
                latest.target.as_deref().unwrap_or("absent"),
                request.proposal,
                request.target
            ),
            record: Some(latest),
        };
    }
    let Some(evidence) =
        scoped_proposal_evidence(comments, item, &request.proposal, &request.target)
    else {
        return RemovalAuthority::NotCovered {
            reason: format!(
                "no current removal proposal record exists for proposal={} target={}",
                request.proposal, request.target
            ),
            record: Some(latest),
        };
    };
    let Some(current) = evidence.complete() else {
        return RemovalAuthority::NotCovered {
            reason: format!(
                "the latest removal proposal record for proposal={} target={} is incomplete; it supersedes earlier versions, so a fresh complete proposal and decision are required",
                request.proposal, request.target
            ),
            record: Some(latest),
        };
    };
    let current_digest = reviewed_proposal_digest(&current);
    match latest.reviewed.as_deref() {
        Some(reviewed) if reviewed == current_digest => {}
        Some(reviewed) => {
            return RemovalAuthority::NotCovered {
                reason: format!(
                    "the reviewed proposal changed after the latest decision (reviewed content {reviewed}; current content {current_digest}: loss={}, evidence={}, preview={}); a fresh decision is required for the changed proposal",
                    current.loss,
                    current.evidence,
                    current.preview.as_deref().unwrap_or("none")
                ),
                record: Some(latest),
            };
        }
        None => {
            return RemovalAuthority::NotCovered {
                reason:
                    "the latest decision does not bind the reviewed proposal content; a fresh decision is required"
                        .to_owned(),
                record: Some(latest),
            };
        }
    }
    match kind {
        RemovalDecisionKind::Refuse => RemovalAuthority::Refused { record: latest },
        RemovalDecisionKind::Withdraw => RemovalAuthority::Withdrawn { record: latest },
        RemovalDecisionKind::Approve if !latest.actions.contains(&request.action) => {
            let covered = if latest.actions.is_empty() {
                "none".to_owned()
            } else {
                latest
                    .actions
                    .iter()
                    .map(|action| action.as_str())
                    .collect::<Vec<_>>()
                    .join("+")
            };
            RemovalAuthority::NotCovered {
                reason: format!(
                    "the latest approval covers actions={covered}; {} is not covered",
                    request.action.as_str()
                ),
                record: Some(latest),
            }
        }
        RemovalDecisionKind::Approve => RemovalAuthority::Authorized { record: latest },
    }
}

/// Closes a completed hypothesis investigation with its outcome, cause and
/// experiment reference in the close reason.
pub fn close_hypothesis(
    bd: &Path,
    project: &Path,
    item: &str,
    outcome: &str,
    reason: &str,
    experiment: Option<&str>,
) -> io::Result<()> {
    let outcome = match outcome.trim() {
        "adopt" | "reject" | "inconclusive" => outcome.trim(),
        other => {
            return Err(invalid(format!(
                "outcome {other} is not adopt, reject or inconclusive"
            )));
        }
    };
    let reason = require_line("reason", reason, MAX_REASON)?;
    let experiment = experiment
        .map(|value| require_token("experiment", value, MAX_EXPERIMENT))
        .transpose()?;
    require_hypothesis_card(bd, project, item)?;
    let text = format!(
        "hypothesis {outcome}: {reason} (experiment {})",
        experiment.as_deref().unwrap_or("none")
    );
    json_ok_actor(
        bd,
        project,
        ACTOR,
        &["close", item, "--reason", &text, "--json"],
    )?;
    Ok(())
}

/// Defers a hypothesis whose missing observation or next check is not
/// currently actionable.
pub fn defer_hypothesis(bd: &Path, project: &Path, item: &str, until: &str) -> io::Result<()> {
    let until = require_line("defer until", until, 64)?;
    require_hypothesis_card(bd, project, item)?;
    json_ok_actor(
        bd,
        project,
        ACTOR,
        &["update", item, "--defer", &until, "--json"],
    )?;
    Ok(())
}

/// A single card read from `bd show`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardSnapshot {
    pub id: String,
    pub status: String,
    pub labels: Vec<String>,
    pub description: String,
    pub dependencies: Vec<Dependency>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dependency {
    pub id: String,
    pub dependency_type: String,
}

/// Reads one card; a missing or unreadable item is reported explicitly.
pub fn load_card(bd: &Path, project: &Path, id: &str) -> io::Result<CardSnapshot> {
    let shown = json_ok_actor(bd, project, ACTOR, &["show", id, "--json"])
        .map_err(|error| invalid(format!("board item {id} is not readable: {error}")))?;
    let value = match shown {
        Value::Array(rows) => rows
            .into_iter()
            .next()
            .ok_or_else(|| invalid(format!("board item {id} is not readable: no row returned")))?,
        other => other,
    };
    Ok(CardSnapshot {
        id: string_field(&value, "id")?,
        status: value
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_owned(),
        labels: value
            .get("labels")
            .and_then(Value::as_array)
            .map(|labels| {
                labels
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default(),
        description: value
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        dependencies: value
            .get("dependencies")
            .and_then(Value::as_array)
            .map(|dependencies| {
                dependencies
                    .iter()
                    .filter_map(|dependency| {
                        Some(Dependency {
                            id: dependency.get("id").and_then(Value::as_str)?.to_owned(),
                            dependency_type: dependency
                                .get("dependency_type")
                                .and_then(Value::as_str)
                                .unwrap_or("")
                                .to_owned(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
    })
}

pub(crate) fn require_hypothesis_card(
    bd: &Path,
    project: &Path,
    id: &str,
) -> io::Result<CardSnapshot> {
    let snapshot = load_card(bd, project, id)?;
    if !snapshot
        .labels
        .iter()
        .any(|label| label == HYPOTHESIS_LABEL)
    {
        return Err(invalid(format!(
            "item {id} is not a hypothesis card: the {HYPOTHESIS_LABEL} label is missing"
        )));
    }
    Ok(snapshot)
}

/// The `related` edge is bd's nonblocking relationship; a scheduling `blocks`
/// edge is never used for an evaluation workload.
fn ensure_related(bd: &Path, project: &Path, candidate: &str, workload: &str) -> io::Result<bool> {
    let snapshot = load_card(bd, project, candidate)?;
    if snapshot
        .dependencies
        .iter()
        .any(|dependency| dependency.id == workload && dependency.dependency_type == "related")
    {
        return Ok(false);
    }
    json_ok_actor(
        bd,
        project,
        ACTOR,
        &["link", candidate, workload, "--type", "related", "--json"],
    )?;
    Ok(true)
}

fn write_comment_once(
    bd: &Path,
    project: &Path,
    item: &str,
    comments: &[String],
    text: &str,
) -> io::Result<bool> {
    if comments.iter().any(|comment| comment == text) {
        return Ok(false);
    }
    json_ok_actor(bd, project, ACTOR, &["comment", item, "--json", text])?;
    Ok(true)
}

/// Splits a comment into its key/value head and the bounded ` detail=` tail.
fn split_detail(rest: &str) -> (&str, Option<&str>) {
    match rest.split_once(" detail=") {
        Some((head, detail)) => (head, Some(detail)),
        None => (rest, None),
    }
}

fn non_none(value: &str) -> Option<String> {
    (value != "none").then(|| value.to_owned())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TrialComment {
    item: String,
    experiment: String,
    role: Option<HypothesisRole>,
    counterpart: Option<String>,
}

fn parse_trial(comment: &str) -> Option<TrialComment> {
    let rest = comment.strip_prefix(TRIAL_PREFIX)?.trim();
    let (head, _) = split_detail(rest);
    let mut item = None;
    let mut experiment = None;
    let mut role = None;
    let mut counterpart = None;
    for part in head.split_whitespace() {
        let Some((key, value)) = part.split_once('=') else {
            continue;
        };
        match key {
            "item" => item = Some(value.to_owned()),
            "experiment" => experiment = Some(value.to_owned()),
            "role" => role = HypothesisRole::parse(value),
            "counterpart" => counterpart = non_none(value),
            _ => {}
        }
    }
    Some(TrialComment {
        item: item?,
        experiment: experiment?,
        role,
        counterpart,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReconsiderationComment {
    item: String,
    basis: String,
}

fn parse_reconsideration(comment: &str) -> Option<ReconsiderationComment> {
    let rest = comment.strip_prefix(RECONSIDERATION_PREFIX)?.trim();
    let (head, _) = split_detail(rest);
    let mut item = None;
    let mut basis = None;
    for part in head.split_whitespace() {
        let Some((key, value)) = part.split_once('=') else {
            continue;
        };
        match key {
            "item" => item = Some(value.to_owned()),
            "basis" => basis = Some(value.to_owned()),
            _ => {}
        }
    }
    Some(ReconsiderationComment {
        item: item?,
        basis: basis?,
    })
}

/// One recorded retention of a completed real task, as read back from its
/// owning card. The fields are references: no solution, patch, answer or
/// acceptance content can be carried here. `tree_object` is the frozen git
/// tree object id; records written before it was retained carry `None`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedRetention {
    pub item: String,
    pub case_id: String,
    pub experiment: String,
    pub mechanism: String,
    pub conditions: String,
    pub revision: String,
    pub frozen: String,
    /// Content digest over the frozen tree entries.
    pub tree: String,
    /// The frozen git tree object id when the record carries it.
    pub tree_object: Option<String>,
    pub oracle: String,
    pub acceptance: String,
    pub replay: String,
    pub detail: Option<String>,
}

/// Parses recorded retentions in board order. A record missing any required
/// reference - including the independent oracle or acceptance evidence - is
/// not a retention, so an absent-evidence summary cannot be read back as one.
pub fn parse_retentions(comments: &[String]) -> Vec<RecordedRetention> {
    comments
        .iter()
        .filter_map(|comment| parse_retention(comment))
        .collect()
}

fn parse_retention(comment: &str) -> Option<RecordedRetention> {
    let rest = comment.strip_prefix(RETENTION_PREFIX)?.trim();
    let (head, detail) = split_detail(rest);
    let mut item = None;
    let mut case_id = None;
    let mut experiment = None;
    let mut mechanism = None;
    let mut conditions = None;
    let mut revision = None;
    let mut frozen = None;
    let mut tree = None;
    let mut tree_object = None;
    let mut oracle = None;
    let mut acceptance = None;
    let mut replay = None;
    for part in head.split_whitespace() {
        let Some((key, value)) = part.split_once('=') else {
            continue;
        };
        match key {
            "item" => item = non_none(value),
            "case" => case_id = non_none(value),
            "experiment" => experiment = non_none(value),
            "mechanism" => mechanism = non_none(value),
            "conditions" => conditions = non_none(value),
            "revision" => revision = non_none(value),
            "frozen" => frozen = non_none(value),
            "tree" => tree = non_none(value),
            "tree_object" => tree_object = non_none(value),
            "oracle" => oracle = non_none(value),
            "acceptance" => acceptance = non_none(value),
            "replay" => replay = non_none(value),
            _ => {}
        }
    }
    Some(RecordedRetention {
        item: item?,
        case_id: case_id?,
        experiment: experiment?,
        mechanism: mechanism?,
        conditions: conditions?,
        revision: revision?,
        frozen: frozen?,
        tree: tree?,
        tree_object,
        oracle: oracle?,
        acceptance: acceptance?,
        replay: replay?,
        detail: detail.map(str::to_owned),
    })
}

/// One recorded removal proposal version. The decisions bind to these fields,
/// and the controller can retain the preview and full references from here
/// without another tracker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedRemovalProposal {
    pub item: String,
    pub proposal: String,
    pub target: String,
    pub evidence: String,
    pub loss: String,
    pub preview: Option<String>,
    pub detail: Option<String>,
}

/// Parses recorded removal proposal versions in board order (oldest first),
/// so the last matching record is the current version.
pub fn parse_removal_proposals(comments: &[String]) -> Vec<RecordedRemovalProposal> {
    comments
        .iter()
        .filter_map(|comment| parse_removal_proposal(comment))
        .collect()
}

/// A stable 128-bit content digest over every reviewed proposal field:
/// proposal, target, evidence, loss, preview and the bounded prose detail.
/// A decision's `reviewed=` value is this digest, so changing any field -
/// including the detail - invalidates the earlier consent.
pub fn reviewed_proposal_digest(proposal: &RecordedRemovalProposal) -> String {
    const OFFSET: u128 = 0x6c62272e07bb014262b821756295c58d;
    const PRIME: u128 = 0x0000_0000_0100_0000_0000_0000_0000_013b;
    let mut hash = OFFSET;
    {
        let mut feed = |bytes: &[u8]| {
            for byte in bytes {
                hash ^= u128::from(*byte);
                hash = hash.wrapping_mul(PRIME);
            }
        };
        for part in [
            Some(proposal.proposal.as_str()),
            Some(proposal.target.as_str()),
            Some(proposal.evidence.as_str()),
            Some(proposal.loss.as_str()),
            proposal.preview.as_deref(),
            proposal.detail.as_deref(),
        ] {
            match part {
                Some(value) => {
                    feed(&[1]);
                    feed(&(value.len() as u64).to_le_bytes());
                    feed(value.as_bytes());
                }
                None => feed(&[0]),
            }
        }
    }
    format!("{hash:032x}")
}

/// A removal proposal comment with only the fields it actually recorded. A
/// record is attributable to a scope when it names the item, proposal and
/// target, even if other fields are incomplete: an incomplete newer record
/// supersedes earlier versions instead of being skipped for them.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ProposalEvidence {
    item: String,
    proposal: Option<String>,
    target: Option<String>,
    evidence: Option<String>,
    loss: Option<String>,
    preview: Option<String>,
    detail: Option<String>,
}

impl ProposalEvidence {
    fn complete(&self) -> Option<RecordedRemovalProposal> {
        Some(RecordedRemovalProposal {
            item: self.item.clone(),
            proposal: self.proposal.clone()?,
            target: self.target.clone()?,
            evidence: self.evidence.clone()?,
            loss: self.loss.clone()?,
            preview: self.preview.clone(),
            detail: self.detail.clone(),
        })
    }
}

/// The latest attributable proposal evidence for one exact scope. Only a
/// record that names the proposal and target can be attributed to it.
fn scoped_proposal_evidence(
    comments: &[String],
    item: &str,
    proposal: &str,
    target: &str,
) -> Option<ProposalEvidence> {
    comments
        .iter()
        .filter_map(|comment| parse_proposal_evidence(comment))
        .rfind(|record| {
            record.item == item
                && record.proposal.as_deref() == Some(proposal)
                && record.target.as_deref() == Some(target)
        })
}

fn parse_removal_proposal(comment: &str) -> Option<RecordedRemovalProposal> {
    parse_proposal_evidence(comment)?.complete()
}

fn parse_proposal_evidence(comment: &str) -> Option<ProposalEvidence> {
    let rest = comment.strip_prefix(REMOVAL_PROPOSAL_PREFIX)?.trim();
    let (head, detail) = split_detail(rest);
    let mut item = None;
    let mut proposal = None;
    let mut target = None;
    let mut evidence = None;
    let mut loss = None;
    let mut preview = None;
    for part in head.split_whitespace() {
        let Some((key, value)) = part.split_once('=') else {
            continue;
        };
        match key {
            "item" => item = Some(value.to_owned()),
            "proposal" => proposal = Some(value.to_owned()),
            "target" => target = Some(value.to_owned()),
            "evidence" => evidence = Some(value.to_owned()),
            "loss" => loss = Some(value.to_owned()),
            "preview" => preview = non_none(value),
            _ => {}
        }
    }
    Some(ProposalEvidence {
        item: item?,
        proposal,
        target,
        evidence,
        loss,
        preview,
        detail: detail.map(str::to_owned),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft() -> BoundedHypothesis {
        BoundedHypothesis::try_from_draft(HypothesisDraft {
            mechanism: "bounded-output".to_owned(),
            conditions: "local-tool-runs".to_owned(),
            observation: "token-audit:findings#12".to_owned(),
            predicted: "less repeated context loading".to_owned(),
            counterexample: "diagnostics vanish on failure".to_owned(),
            acceptance: "diagnostic preservation check passes".to_owned(),
            spec: "openspec/changes/add-bounded-output".to_owned(),
            basis: "evidence-bounded-output".to_owned(),
        })
        .unwrap()
    }

    #[test]
    fn admission_round_trips_through_the_description() {
        let bounded = draft();
        let record = parse_admission(&bounded.description()).unwrap();
        assert_eq!(record.mechanism.as_deref(), Some("bounded-output"));
        assert_eq!(record.conditions.as_deref(), Some("local-tool-runs"));
        assert_eq!(
            record.observation.as_deref(),
            Some("token-audit:findings#12")
        );
        assert_eq!(
            record.predicted.as_deref(),
            Some("less repeated context loading")
        );
        assert_eq!(
            record.counterexample.as_deref(),
            Some("diagnostics vanish on failure")
        );
        assert_eq!(
            record.acceptance.as_deref(),
            Some("diagnostic preservation check passes")
        );
        assert_eq!(
            record.spec.as_deref(),
            Some("openspec/changes/add-bounded-output")
        );
        assert_eq!(record.basis.as_deref(), Some("evidence-bounded-output"));
        assert_eq!(bounded.title(), "Hypothesis: bounded-output");

        assert!(parse_admission("observation: unrelated\n").is_none());
        assert!(parse_admission("").is_none());
    }

    #[test]
    fn blank_or_multiline_fields_are_refused() {
        let error = BoundedHypothesis::try_from_draft(HypothesisDraft {
            mechanism: "bounded output".to_owned(),
            ..draft_components()
        })
        .unwrap_err();
        assert!(error.to_string().contains("unsupported characters"));
        let error = BoundedHypothesis::try_from_draft(HypothesisDraft {
            predicted: "two\nlines".to_owned(),
            ..draft_components()
        })
        .unwrap_err();
        assert!(error.to_string().contains("single line"));
    }

    fn draft_components() -> HypothesisDraft {
        HypothesisDraft {
            mechanism: "bounded-output".to_owned(),
            conditions: "local-tool-runs".to_owned(),
            observation: "token-audit:findings#12".to_owned(),
            predicted: "less repeated context loading".to_owned(),
            counterexample: "diagnostics vanish on failure".to_owned(),
            acceptance: "diagnostic preservation check passes".to_owned(),
            spec: "openspec/changes/add-bounded-output".to_owned(),
            basis: "evidence-bounded-output".to_owned(),
        }
    }

    fn proposal_text(loss: &str, evidence: &str, preview: &str, detail: &str) -> String {
        format!(
            "removal-proposal v1 item=bdct-h1 proposal=openspec/changes/remove-x target=skill-x evidence={evidence} loss={loss} preview={preview} detail={detail}"
        )
    }

    fn proposal_record(
        loss: &str,
        evidence: &str,
        preview: &str,
        detail: &str,
    ) -> RecordedRemovalProposal {
        parse_removal_proposal(&proposal_text(loss, evidence, preview, detail))
            .expect("complete proposal")
    }

    fn decision_text(
        decision: &str,
        actions: &str,
        loss: &str,
        evidence: &str,
        preview: &str,
        detail: &str,
    ) -> String {
        let reviewed = proposal_record(loss, evidence, preview, detail);
        format!(
            "removal-decision v1 item=bdct-h1 decision={decision} proposal=openspec/changes/remove-x target=skill-x actions={actions} loss={loss} evidence={evidence} preview={preview} reviewed={} basis=user-turn-7 detail=presented proposal",
            reviewed_proposal_digest(&reviewed)
        )
    }

    const DETAIL: &str = "consumer list: none known";

    #[test]
    fn removal_authority_requires_a_matching_scoped_approval() {
        let request = AuthorityRequest {
            proposal: "openspec/changes/remove-x".to_owned(),
            target: "skill-x".to_owned(),
            action: RemovalAction::Experiment,
        };
        assert_eq!(
            removal_authority(&[], "bdct-h1", &request),
            RemovalAuthority::Missing
        );

        let approved = vec![
            proposal_text("skill-x", "ev-1", "preview-1", DETAIL),
            decision_text(
                "approve",
                "experiment",
                "skill-x",
                "ev-1",
                "preview-1",
                DETAIL,
            ),
        ];
        assert!(matches!(
            removal_authority(&approved, "bdct-h1", &request),
            RemovalAuthority::Authorized { .. }
        ));
        let integration = AuthorityRequest {
            action: RemovalAction::Integration,
            ..request.clone()
        };
        let authority = removal_authority(&approved, "bdct-h1", &integration);
        match authority {
            RemovalAuthority::NotCovered { reason, .. } => {
                assert!(reason.contains("integration is not covered"), "{reason}");
            }
            other => panic!("expected not-covered, got {other:?}"),
        }

        let refused = vec![
            proposal_text("skill-x", "ev-1", "preview-1", DETAIL),
            decision_text("refuse", "none", "skill-x", "ev-1", "preview-1", DETAIL),
        ];
        assert!(matches!(
            removal_authority(&refused, "bdct-h1", &request),
            RemovalAuthority::Refused { .. }
        ));
        let withdrawn = vec![
            proposal_text("skill-x", "ev-1", "preview-1", DETAIL),
            decision_text("withdraw", "none", "skill-x", "ev-1", "preview-1", DETAIL),
        ];
        assert!(matches!(
            removal_authority(&withdrawn, "bdct-h1", &request),
            RemovalAuthority::Withdrawn { .. }
        ));

        // A proposal version recorded after the decision invalidates the old
        // consent: any changed reviewed field, including the bounded prose
        // detail, needs a fresh decision before further removal effect.
        let mut changed_loss = approved.clone();
        changed_loss.push(proposal_text(
            "skill-x+recovery",
            "ev-2",
            "preview-2",
            DETAIL,
        ));
        match removal_authority(&changed_loss, "bdct-h1", &request) {
            RemovalAuthority::NotCovered { reason, .. } => {
                assert!(reason.contains("changed"), "{reason}");
            }
            other => panic!("expected not-covered for a changed proposal, got {other:?}"),
        }
        let mut changed_detail = approved.clone();
        changed_detail.push(proposal_text(
            "skill-x",
            "ev-1",
            "preview-1",
            "consumer list: recovery path uses it",
        ));
        assert!(matches!(
            removal_authority(&changed_detail, "bdct-h1", &request),
            RemovalAuthority::NotCovered { .. }
        ));
        // An incomplete newer scoped record supersedes the earlier proposal
        // instead of being skipped for it.
        let mut incomplete = approved.clone();
        incomplete.push(
            "removal-proposal v1 item=bdct-h1 proposal=openspec/changes/remove-x target=skill-x loss=skill-x"
                .to_owned(),
        );
        match removal_authority(&incomplete, "bdct-h1", &request) {
            RemovalAuthority::NotCovered { reason, .. } => {
                assert!(reason.contains("incomplete"), "{reason}");
            }
            other => panic!("expected not-covered for an incomplete proposal, got {other:?}"),
        }
        // ...including for an earlier refusal.
        let mut refused_then_changed = refused.clone();
        refused_then_changed.push(proposal_text(
            "skill-x+recovery",
            "ev-2",
            "preview-2",
            DETAIL,
        ));
        assert!(matches!(
            removal_authority(&refused_then_changed, "bdct-h1", &request),
            RemovalAuthority::NotCovered { .. }
        ));
        // A complete proposal and a fresh decision for the changed content
        // restore authority.
        let mut re_decided = changed_loss.clone();
        re_decided.push(decision_text(
            "approve",
            "experiment",
            "skill-x+recovery",
            "ev-2",
            "preview-2",
            DETAIL,
        ));
        assert!(matches!(
            removal_authority(&re_decided, "bdct-h1", &request),
            RemovalAuthority::Authorized { .. }
        ));
        // A decision that does not bind the reviewed content never authorizes.
        let unbound = vec![
            proposal_text("skill-x", "ev-1", "preview-1", DETAIL),
            "removal-decision v1 item=bdct-h1 decision=approve proposal=openspec/changes/remove-x target=skill-x actions=experiment loss=skill-x basis=user-turn-7".to_owned(),
        ];
        assert!(matches!(
            removal_authority(&unbound, "bdct-h1", &request),
            RemovalAuthority::NotCovered { .. }
        ));

        // A decision for another target never covers this one.
        let other_target = vec![
            proposal_text("skill-x", "ev-1", "preview-1", DETAIL),
            decision_text(
                "approve",
                "experiment+integration",
                "skill-x",
                "ev-1",
                "preview-1",
                DETAIL,
            )
            .replace("target=skill-x", "target=skill-y"),
        ];
        assert!(matches!(
            removal_authority(&other_target, "bdct-h1", &request),
            RemovalAuthority::NotCovered { .. }
        ));

        // The latest decision controls: an unreadable newer record cannot
        // recover the older approval, and a later approval supersedes a
        // withdrawal.
        let mut mixed = approved.clone();
        mixed.push("removal-decision v1 item=bdct-h1".to_owned());
        assert!(matches!(
            removal_authority(&mixed, "bdct-h1", &request),
            RemovalAuthority::NotCovered { .. }
        ));
        let mut reapplied = withdrawn.clone();
        reapplied.push(decision_text(
            "approve",
            "experiment",
            "skill-x",
            "ev-1",
            "preview-1",
            DETAIL,
        ));
        assert!(matches!(
            removal_authority(&reapplied, "bdct-h1", &request),
            RemovalAuthority::Authorized { .. }
        ));

        // An unattributable comment names no card and cannot shadow anyone.
        let mut unattributable = approved.clone();
        unattributable.push("removal-decision v1 decision=withdraw".to_owned());
        assert!(matches!(
            removal_authority(&unattributable, "bdct-h1", &request),
            RemovalAuthority::Authorized { .. }
        ));

        // Decisions for another card never apply here.
        let other_item = vec![
            proposal_text("skill-x", "ev-1", "preview-1", DETAIL),
            decision_text("refuse", "none", "skill-x", "ev-1", "preview-1", DETAIL)
                .replace("item=bdct-h1", "item=bdct-h2"),
        ];
        assert_eq!(
            removal_authority(&other_item, "bdct-h1", &request),
            RemovalAuthority::Missing
        );
    }

    #[test]
    fn reviewed_proposal_digest_covers_every_field_and_is_stable() {
        let base = proposal_record("skill-x", "ev-1", "preview-1", DETAIL);
        assert_eq!(
            reviewed_proposal_digest(&base),
            reviewed_proposal_digest(&base)
        );
        assert_eq!(reviewed_proposal_digest(&base).len(), 32);
        for changed in [
            proposal_record("skill-x+recovery", "ev-1", "preview-1", DETAIL),
            proposal_record("skill-x", "ev-2", "preview-1", DETAIL),
            proposal_record("skill-x", "ev-1", "preview-2", DETAIL),
            proposal_record("skill-x", "ev-1", "preview-1", "changed detail"),
            RecordedRemovalProposal {
                item: "bdct-h1".to_owned(),
                proposal: "openspec/changes/remove-y".to_owned(),
                ..base.clone()
            },
            RecordedRemovalProposal {
                item: "bdct-h1".to_owned(),
                target: "skill-y".to_owned(),
                ..base.clone()
            },
        ] {
            assert_ne!(
                reviewed_proposal_digest(&changed),
                reviewed_proposal_digest(&base),
                "{changed:?}"
            );
        }
        // A missing optional field is distinct from any recorded value.
        let without_detail = RecordedRemovalProposal {
            item: "bdct-h1".to_owned(),
            proposal: "openspec/changes/remove-x".to_owned(),
            target: "skill-x".to_owned(),
            evidence: "ev-1".to_owned(),
            loss: "skill-x".to_owned(),
            preview: Some("preview-1".to_owned()),
            detail: None,
        };
        assert_ne!(
            reviewed_proposal_digest(&without_detail),
            reviewed_proposal_digest(&base)
        );
    }

    #[test]
    fn refusal_and_withdrawal_must_not_claim_covered_actions() {
        let error = BoundedRemovalDecision::try_from_draft(RemovalDecisionDraft {
            decision: RemovalDecisionKind::Refuse,
            proposal: "openspec/changes/remove-x".to_owned(),
            target: "skill-x".to_owned(),
            actions: vec![RemovalAction::Experiment],
            loss: None,
            basis: None,
            detail: None,
        })
        .unwrap_err();
        assert!(error.to_string().contains("only to an approval"));
        let error = BoundedRemovalDecision::try_from_draft(RemovalDecisionDraft {
            decision: RemovalDecisionKind::Approve,
            proposal: "openspec/changes/remove-x".to_owned(),
            target: "skill-x".to_owned(),
            actions: Vec::new(),
            loss: None,
            basis: None,
            detail: None,
        })
        .unwrap_err();
        assert!(error.to_string().contains("covered actions"));
    }

    #[test]
    fn trial_and_reconsideration_comments_parse() {
        let trial = BoundedTrial::try_from_draft(TrialDraft {
            experiment: "exp-1".to_owned(),
            role: HypothesisRole::Candidate,
            counterpart: "bdct-b".to_owned(),
            evidence: Some("runs/exp-1".to_owned()),
        })
        .unwrap();
        let comment = trial.comment("bdct-a");
        assert_eq!(
            comment,
            "hypothesis-trial v1 item=bdct-a experiment=exp-1 role=candidate counterpart=bdct-b evidence=runs/exp-1"
        );
        let parsed = parse_trial(&comment).unwrap();
        assert_eq!(parsed.item, "bdct-a");
        assert_eq!(parsed.experiment, "exp-1");
        assert_eq!(parsed.role, Some(HypothesisRole::Candidate));
        assert_eq!(parsed.counterpart.as_deref(), Some("bdct-b"));

        let reconsideration = parse_reconsideration(
            "hypothesis-reconsideration v1 item=bdct-a basis=evidence-2 prior=exp-1 conclusion=reject",
        )
        .unwrap();
        assert_eq!(reconsideration.item, "bdct-a");
        assert_eq!(reconsideration.basis, "evidence-2");
        assert!(parse_reconsideration("hypothesis-trial v1 item=bdct-a").is_none());
    }

    #[test]
    fn implementation_comment_round_trips() {
        let implementation = BoundedImplementation::try_from_draft(ImplementationDraft {
            role: HypothesisRole::Workload,
            branch: "hypothesis/b".to_owned(),
            base: "0b960b4c87f21a38f5ade8b8d27e374bacb81b8a".to_owned(),
            revision: "abc1234".to_owned(),
            worktree: "wt-loc-1".to_owned(),
            runtime: Some("runtime-candidate".to_owned()),
            baseline_runtime: Some("runtime-baseline".to_owned()),
        })
        .unwrap();
        assert_eq!(
            implementation.comment("bdct-b"),
            "hypothesis-implementation v1 item=bdct-b role=workload branch=hypothesis/b base=0b960b4c87f21a38f5ade8b8d27e374bacb81b8a revision=abc1234 worktree=wt-loc-1 runtime=runtime-candidate baseline=runtime-baseline"
        );
    }

    fn retention_components() -> RetentionDraft {
        RetentionDraft {
            case_id: "case-b".to_owned(),
            experiment: "exp-1".to_owned(),
            mechanism: "bounded-output".to_owned(),
            conditions: "local-tool-runs".to_owned(),
            revision: "0b960b4c87f21a38f5ade8b8d27e374bacb81b8a".to_owned(),
            frozen: "aa11bb22cc33dd44ee55ff66aa77bb88cc99dd00".to_owned(),
            tree: "d1g3e5s7t9a1b3c5d7e9f1a3b5c7d9e1f3a5b7c9".to_owned(),
            oracle: "oracle-7".to_owned(),
            acceptance: "acceptance/run-9".to_owned(),
            replay: r"C:\state\replay-1".to_owned(),
            detail: None,
        }
    }

    #[test]
    fn retention_comment_round_trips_references_only() {
        let retention = BoundedRetention::try_from_draft(RetentionDraft {
            detail: Some("accepted completed real task".to_owned()),
            ..retention_components()
        })
        .unwrap();
        let tree_object = "bb11cc22dd33ee44ff55aa66bb77cc88dd99ee00";
        let comment = retention.comment("bdct-h1", tree_object);
        assert_eq!(
            comment,
            r"hypothesis-retention v1 item=bdct-h1 case=case-b experiment=exp-1 mechanism=bounded-output conditions=local-tool-runs revision=0b960b4c87f21a38f5ade8b8d27e374bacb81b8a frozen=aa11bb22cc33dd44ee55ff66aa77bb88cc99dd00 tree=d1g3e5s7t9a1b3c5d7e9f1a3b5c7d9e1f3a5b7c9 tree_object=bb11cc22dd33ee44ff55aa66bb77cc88dd99ee00 oracle=oracle-7 acceptance=acceptance/run-9 replay=C:\state\replay-1 detail=accepted completed real task"
        );
        let parsed = parse_retention(&comment).unwrap();
        assert_eq!(parsed.item, "bdct-h1");
        assert_eq!(parsed.case_id, "case-b");
        assert_eq!(parsed.experiment, "exp-1");
        assert_eq!(parsed.mechanism, "bounded-output");
        assert_eq!(parsed.conditions, "local-tool-runs");
        assert_eq!(parsed.frozen, "aa11bb22cc33dd44ee55ff66aa77bb88cc99dd00");
        assert_eq!(parsed.tree_object.as_deref(), Some(tree_object));
        assert_eq!(parsed.oracle, "oracle-7");
        assert_eq!(parsed.acceptance, "acceptance/run-9");
        assert_eq!(parsed.replay, r"C:\state\replay-1");
        assert_eq!(
            parsed.detail.as_deref(),
            Some("accepted completed real task")
        );

        // A record written before the frozen tree object id was retained
        // stays readable with no object identity to rebuild from.
        let legacy = comment.replace(&format!(" tree_object={tree_object}"), "");
        let legacy = parse_retention(&legacy).unwrap();
        assert_eq!(legacy.case_id, "case-b");
        assert_eq!(legacy.tree_object, None);

        // Absent evidence is not a retention: a summary cannot stand in for
        // the oracle, the acceptance reference or the frozen replay identity.
        let summary = "hypothesis-retention v1 item=bdct-h1 case=case-b experiment=exp-1 mechanism=bounded-output conditions=local-tool-runs revision=abc frozen=def tree=tre oracle=none acceptance=acceptance/run-9 replay=C:/state/replay-1 detail=claimed saving";
        assert!(parse_retention(summary).is_none());
        let no_acceptance = summary.replace("acceptance/run-9", "none");
        assert!(parse_retention(&no_acceptance).is_none());
        let no_summary_evidence =
            "hypothesis-retention v1 item=bdct-h1 case=case-b detail=claimed saving";
        assert!(parse_retention(no_summary_evidence).is_none());

        // A mixed comment list keeps only complete retention records.
        let records = parse_retentions(&[
            summary.to_owned(),
            comment.clone(),
            "hypothesis-trial v1 item=bdct-h1 experiment=exp-1 role=workload counterpart=bdct-b"
                .to_owned(),
        ]);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].case_id, "case-b");
    }

    #[test]
    fn retention_requires_bounded_single_line_evidence() {
        let error = BoundedRetention::try_from_draft(RetentionDraft {
            acceptance: "   ".to_owned(),
            ..retention_components()
        })
        .unwrap_err();
        assert!(
            error.to_string().contains("acceptance is required"),
            "{error}"
        );
        let error = BoundedRetention::try_from_draft(RetentionDraft {
            oracle: "line one\nline two".to_owned(),
            ..retention_components()
        })
        .unwrap_err();
        assert!(error.to_string().contains("single line"), "{error}");
        let error = BoundedRetention::try_from_draft(RetentionDraft {
            mechanism: "bounded output".to_owned(),
            ..retention_components()
        })
        .unwrap_err();
        assert!(
            error.to_string().contains("unsupported characters"),
            "{error}"
        );
        let error = BoundedRetention::try_from_draft(RetentionDraft {
            detail: Some("two\nlines".to_owned()),
            ..retention_components()
        })
        .unwrap_err();
        assert!(error.to_string().contains("single line"), "{error}");
    }
}
