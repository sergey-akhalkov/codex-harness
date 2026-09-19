//! Bounded board feedback intake and model-free triage mechanics.
//!
//! Lead and executor observations become `bd` tasks. Listing, merging and
//! voting invoke only the board CLI. Similarity remains lead judgment, supplied
//! as explicit merge decisions. The controller does not parse the board.
use crate::board_cli::{FEEDBACK_LABEL, INCUBATOR_LABEL, json_ok, json_ok_actor, string_field};
use serde_json::Value;
use std::{io, path::Path};

pub const DEFAULT_FEEDBACK_BATCH_LIMIT: usize = 8;
pub const MAX_OBSERVATION: usize = 512;
pub const MAX_SCOPE: usize = 128;
pub const MAX_REPORTER: usize = 64;
pub const MAX_EPISODE: usize = 64;
pub const MAX_PARENT: usize = 64;

const VOTE_PREFIX: &str = "feedback-vote v1";
const MERGE_PREFIX: &str = "feedback-merge v1";
const MAX_TITLE_BYTES: usize = 72;

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

    fn parse(value: &str) -> io::Result<Self> {
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
}

impl VoteLedger {
    pub fn counted(&self) -> usize {
        self.votes.iter().filter(|vote| vote.counted).count()
    }
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
    for comment in comments {
        if let Some(vote) = parse_vote_comment(comment) {
            votes.push(vote);
        } else if let Some(merge) = parse_merge_comment(comment) {
            merges.push(merge);
        }
    }
    VoteLedger { votes, merges }
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

fn apply_action(bd: &Path, project: &Path, action: &TriageAction) -> io::Result<AppliedAction> {
    let incoming = load_feedback(bd, project, &action.feedback_id)?.ok_or_else(|| {
        invalid(format!(
            "feedback {} is missing bounded fields",
            action.feedback_id
        ))
    })?;
    let candidate = VoteCandidate {
        episode: incoming.episode.clone(),
        reporter: incoming.reporter.clone(),
        kind: incoming.kind,
    };
    match action.merge_into.as_deref() {
        None => {
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
            let vote = record_vote(bd, project, &action.feedback_id, &candidate)?;
            Ok(AppliedAction {
                feedback_id: action.feedback_id.clone(),
                incubator_id: action.feedback_id.clone(),
                merged: false,
                vote,
            })
        }
        Some(canonical) => {
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
            let vote = record_vote(bd, project, canonical, &candidate)?;
            Ok(AppliedAction {
                feedback_id: action.feedback_id.clone(),
                incubator_id: canonical.to_owned(),
                merged: true,
                vote,
            })
        }
    }
}

fn record_vote(
    bd: &Path,
    project: &Path,
    item_id: &str,
    candidate: &VoteCandidate,
) -> io::Result<VoteDecision> {
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
    Ok(decision)
}

fn load_feedback(bd: &Path, project: &Path, id: &str) -> io::Result<Option<BoundedFeedback>> {
    let shown = json_ok(bd, project, &["show", id, "--json"])?;
    let issue = issue_object(&shown, id)?;
    let description = issue_description(issue);
    match parse_description(description) {
        Ok(feedback) => Ok(Some(feedback)),
        Err(_) => Ok(None),
    }
}

fn list_comments(bd: &Path, project: &Path, item_id: &str) -> io::Result<Vec<String>> {
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
        assert!(board.contains("kind: lead|executor|diagnostic"));
        assert!(board.contains("Do not use `bd find-duplicates`"));
        assert!(lead.contains("not real-time chat"));
        assert!(lead.contains("safe boundary"));
        assert!(lead.contains("no model calls"));
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
}
