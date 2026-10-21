//! Strict parent-side validation of one independent semantic acceptance
//! judgment. The reviewer's source content is data: this module checks the
//! document identity, schema, declared locations, physical lines, evidence
//! binding and retained bytes. It derives no acceptance from reviewer prose.

use harness_core::build_identity::hash_bytes;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Component, Path};

pub(super) const REVIEW_SCHEMA: u32 = 1;
pub(super) const MAX_FINDINGS: usize = 64;
pub(super) const MAX_UNRESOLVED_ISSUES: usize = 64;
pub(super) const MAX_JUDGMENT_BYTES: u64 = 256 * 1024;
pub(super) const MAX_REVIEW_FILE_BYTES: u64 = 4 * 1024 * 1024;
pub(super) const MAX_PACKET_BYTES: u64 = 32 * 1024 * 1024;
pub(super) const MAX_ACTION_RECORDS_PER_TURN: usize = 512;
pub(super) const MAX_ACTION_RECORDS_TOTAL: usize = 2048;
pub(super) const MAX_ACTION_COMMAND_BYTES: usize = 2048;
pub(super) const MAX_ACTION_TARGET_BYTES: usize = 512;
pub(super) const MAX_ACTION_ARGUMENTS_BYTES: usize = 2048;
pub(super) const MAX_ACTION_OUTPUT_BYTES: usize = 1024;
pub(super) const MAX_ACTION_ERROR_BYTES: usize = 1024;
const MAX_TEXT_BYTES: usize = 4096;
pub(super) const MAX_RUN_ID_BYTES: usize = 200;
pub(super) const MAX_PATH_BYTES: usize = 240;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct ReviewManifest {
    pub schema: u32,
    pub run_id: String,
    pub submission_revision: String,
    pub base_revision: String,
    pub task: ReviewFileIdentity,
    pub criteria: ReviewFileIdentity,
    pub submission: ReviewFileIdentity,
    pub actions: ReviewFileIdentity,
    pub sources: Vec<ReviewFileIdentity>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct ReviewFileIdentity {
    pub path: String,
    pub sha256: String,
    /// Physical newline-delimited lines, not sentence or visual-wrap lines.
    pub lines: u64,
}

impl ReviewFileIdentity {
    pub(super) fn of(path: &str, bytes: &[u8]) -> io::Result<Self> {
        let path = normalize_relative(path)?;
        Ok(Self {
            path,
            sha256: hash_bytes(bytes),
            lines: physical_lines(bytes),
        })
    }
}

/// The bounded visible action evidence supplied to the reviewer. It carries
/// no arm identity, expected verdict, sibling outcome or reflection context.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct ReviewActions {
    pub schema: u32,
    pub run_id: String,
    pub task: String,
    pub submitted_revision: String,
    pub base_revision: String,
    pub writable_scope: Vec<String>,
    pub changed_paths: Vec<String>,
    pub worktree_clean: bool,
    pub report_sha256: String,
    pub participants: Vec<ReviewParticipant>,
    /// The exact retained visible tool and item events, attributed to the
    /// participant attempt and native turn that produced them.
    #[serde(default)]
    pub action_evidence: Vec<ReviewAction>,
    /// One explicit coverage record per participant attempt. A missing or
    /// incomplete raw stream is recorded here; it is never reported as an
    /// empty action list.
    #[serde(default)]
    pub action_coverage: Vec<ReviewActionCoverage>,
    /// True only when every required participant turn had a complete,
    /// readable raw action stream with a terminal turn event.
    #[serde(default)]
    pub action_evidence_complete: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReviewParticipant {
    /// The exact qualification attempt identity. It makes repeated turns in
    /// one native session distinguishable.
    #[serde(default)]
    pub attempt: String,
    pub role: String,
    pub session: String,
    pub profile: String,
    pub state: String,
}

/// One visible tool or item event retained from a participant's raw native
/// detail stream. Opaque reasoning and compaction items are never included.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReviewAction {
    pub attempt: String,
    pub role: String,
    pub session: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn: Option<String>,
    pub item: String,
    pub phase: String,
    /// `tool` for an executed command/tool/file action; `item` for another
    /// visible non-message item.
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    /// Visible native error text or an explicit MCP `result.isError` marker.
    /// A completed status alone is not treated as success.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// A bounded field was shortened; the action identity and outcome remain
    /// intact and the truncation is explicit to the reviewer.
    #[serde(default)]
    pub text_truncated: bool,
}

/// The exact coverage state of one participant attempt's raw action stream.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReviewActionCoverage {
    pub attempt: String,
    pub role: String,
    pub session: String,
    pub state: String,
    /// The exact native turn the retained stream completed. Every action of a
    /// complete coverage record must carry this identity; a foreign turn is a
    /// gap, never a silent fallback.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn: Option<String>,
    /// Digest of the retained raw bytes copied before the mutable pool slot
    /// could be reused; absent when no bytes were retained.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_sha256: Option<String>,
    /// Total raw stream length when it was readable, in bytes.
    pub source_bytes: u64,
    /// Bytes retained in the frozen action snapshot.
    pub retained_bytes: u64,
    /// Action records emitted for this attempt into `action_evidence`.
    pub action_records: u64,
    /// Action records omitted at the declared per-turn or total bound.
    pub omitted_records: u64,
    pub malformed_records: u64,
    pub partial_records: u64,
    /// Terminal turn events observed in the retained stream.
    pub terminal_events: u64,
    pub truncated: bool,
    /// Complete only when the whole raw stream was readable, parseable and
    /// ended with a terminal turn event; otherwise `note` states the gap.
    pub complete: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// One named frozen input. Evidence validation reads only these bytes.
#[derive(Clone, Debug)]
pub(super) struct PacketFile {
    pub path: String,
    pub bytes: Vec<u8>,
}

impl PacketFile {
    pub(super) fn lines(&self) -> u64 {
        physical_lines(&self.bytes)
    }
}

/// The settled, parent-validated judgment. `kind` is derived only from the
/// retained structural facts: findings mean rejection, unresolved obstacles
/// without findings mean the judgment remains unknown, and only a clean pair
/// is acceptance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Judgment {
    pub kind: &'static str,
    pub findings: usize,
    pub unresolved_issues: usize,
    pub run_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawJudgment {
    run_id: String,
    findings: Vec<RawFinding>,
    unresolved_issues: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFinding {
    path: String,
    line: u64,
    description: String,
    evidence: String,
}

/// Read and verify the exact frozen packet named by its manifest. The manifest
/// itself is an allowed input, so a reviewer can cite it without a second
/// copy of its bytes.
pub(super) fn load_packet(
    root: &Path,
    manifest_path: &Path,
) -> io::Result<(ReviewManifest, Vec<PacketFile>)> {
    let metadata = fs::symlink_metadata(manifest_path).map_err(|error| {
        invalid(format!(
            "the semantic review manifest {} is unavailable: {error}",
            manifest_path.display()
        ))
    })?;
    if !metadata.is_file() {
        return Err(invalid(format!(
            "the semantic review manifest {} is not an ordinary file",
            manifest_path.display()
        )));
    }
    if metadata.len() > MAX_JUDGMENT_BYTES {
        return Err(invalid(
            "the semantic review manifest exceeds its byte bound",
        ));
    }
    let manifest_bytes = fs::read(manifest_path)?;
    let manifest: ReviewManifest = serde_json::from_slice(&manifest_bytes).map_err(|error| {
        invalid(format!(
            "the semantic review manifest is not a strict schema {REVIEW_SCHEMA} document: {error}"
        ))
    })?;
    if manifest.schema != REVIEW_SCHEMA {
        return Err(invalid(format!(
            "the semantic review manifest declares schema {}; this controller reads schema {REVIEW_SCHEMA}",
            manifest.schema
        )));
    }
    bounded_text("semantic review run_id", &manifest.run_id, MAX_RUN_ID_BYTES)?;
    for revision in [&manifest.submission_revision, &manifest.base_revision] {
        if !is_commit_identity(revision) {
            return Err(invalid(
                "the semantic review manifest must name exact commit revisions",
            ));
        }
    }
    let mut identities = vec![
        manifest.task.clone(),
        manifest.criteria.clone(),
        manifest.submission.clone(),
        manifest.actions.clone(),
    ];
    identities.extend(manifest.sources.iter().cloned());
    let mut seen = std::collections::BTreeSet::new();
    let mut files = Vec::with_capacity(identities.len() + 1);
    let mut total = u64::try_from(manifest_bytes.len()).unwrap_or(u64::MAX);
    for identity in identities {
        let path = normalize_relative(&identity.path)?;
        if !seen.insert(path.clone()) {
            return Err(invalid(format!(
                "the semantic review manifest declares {path} more than once"
            )));
        }
        let file = root.join(&path);
        let metadata = fs::symlink_metadata(&file).map_err(|error| {
            invalid(format!(
                "the frozen semantic review input {} is unavailable: {error}",
                file.display()
            ))
        })?;
        if !metadata.is_file() {
            return Err(invalid(format!(
                "the frozen semantic review input {} is not an ordinary file",
                file.display()
            )));
        }
        if metadata.len() > MAX_REVIEW_FILE_BYTES {
            return Err(invalid(format!(
                "the frozen semantic review input {path} exceeds its byte bound"
            )));
        }
        total = total.saturating_add(metadata.len());
        if total > MAX_PACKET_BYTES {
            return Err(invalid(
                "the frozen semantic review packet exceeds its byte bound",
            ));
        }
        let bytes = fs::read(&file)?;
        if hash_bytes(&bytes) != identity.sha256 {
            return Err(invalid(format!(
                "the frozen semantic review input {path} changed after the packet was committed"
            )));
        }
        if physical_lines(&bytes) != identity.lines {
            return Err(invalid(format!(
                "the frozen semantic review input {path} line count changed after the packet was committed"
            )));
        }
        files.push(PacketFile { path, bytes });
    }
    files.push(PacketFile {
        path: "manifest.json".to_owned(),
        bytes: manifest_bytes,
    });
    Ok((manifest, files))
}

/// Validate one retained final result against the exact frozen packet. A
/// returned error is a structural/output defect that may use the declared
/// same-reviewer correction; packet hash or identity failures are refused
/// before this function is called.
pub(super) fn validate(
    bytes: &[u8],
    expected_run_id: &str,
    files: &[PacketFile],
) -> io::Result<Judgment> {
    if bytes.len() as u64 > MAX_JUDGMENT_BYTES {
        return Err(invalid(
            "the semantic review judgment exceeds its byte bound",
        ));
    }
    let raw: RawJudgment = serde_json::from_slice(bytes).map_err(|error| {
        invalid(format!(
            "the semantic review result is not one strict JSON object with only run_id, findings and unresolved_issues: {error}"
        ))
    })?;
    if raw.run_id != expected_run_id {
        return Err(invalid(format!(
            "the semantic review run_id is {:?}; the frozen packet identity is {:?}",
            raw.run_id, expected_run_id
        )));
    }
    if raw.findings.len() > MAX_FINDINGS {
        return Err(invalid(format!(
            "the semantic review returned {} findings; the bound is {MAX_FINDINGS}",
            raw.findings.len()
        )));
    }
    if raw.unresolved_issues.len() > MAX_UNRESOLVED_ISSUES {
        return Err(invalid(format!(
            "the semantic review returned {} unresolved issues; the bound is {MAX_UNRESOLVED_ISSUES}",
            raw.unresolved_issues.len()
        )));
    }
    for (index, finding) in raw.findings.iter().enumerate() {
        let path = normalize_relative(&finding.path)
            .map_err(|error| invalid(format!("findings[{index}].path is invalid: {error}")))?;
        let file = files.iter().find(|file| file.path == path).ok_or_else(|| {
            invalid(format!(
                "findings[{index}].path {path:?} is not one of the frozen review inputs"
            ))
        })?;
        if finding.line == 0 || finding.line > file.lines() {
            return Err(invalid(format!(
                "findings[{index}].line {} is not a physical line of {path} (1..={})",
                finding.line,
                file.lines()
            )));
        }
        bounded_text(
            &format!("findings[{index}].description"),
            &finding.description,
            MAX_TEXT_BYTES,
        )?;
        bounded_text(
            &format!("findings[{index}].evidence"),
            &finding.evidence,
            MAX_TEXT_BYTES,
        )?;
        if !evidence_has_location(&finding.evidence, files) {
            return Err(invalid(format!(
                "findings[{index}].evidence names no physical frozen input location of the form path:line or path:first-last"
            )));
        }
    }
    for (index, issue) in raw.unresolved_issues.iter().enumerate() {
        bounded_text(
            &format!("unresolved_issues[{index}]"),
            issue,
            MAX_TEXT_BYTES,
        )?;
    }
    let kind = if !raw.findings.is_empty() {
        "reject"
    } else if !raw.unresolved_issues.is_empty() {
        "unknown"
    } else {
        "accept"
    };
    Ok(Judgment {
        kind,
        findings: raw.findings.len(),
        unresolved_issues: raw.unresolved_issues.len(),
        run_id: raw.run_id,
    })
}

/// Validate the bounded action evidence as one internally consistent document.
/// An incomplete coverage record is structurally valid because it names a real
/// gap. The controller refuses to freeze or accept such a packet: an
/// unavailable action view is never evidence that no effect occurred.
pub(super) fn validate_action_evidence(actions: &ReviewActions) -> io::Result<()> {
    if actions.action_coverage.len() != actions.participants.len() {
        return Err(invalid(format!(
            "the semantic review action evidence has {} coverage record(s) for {} participant turn(s); every required turn needs one explicit record",
            actions.action_coverage.len(),
            actions.participants.len()
        )));
    }
    for (index, (coverage, participant)) in actions
        .action_coverage
        .iter()
        .zip(&actions.participants)
        .enumerate()
    {
        bounded_text(
            &format!("action_coverage[{index}].attempt"),
            &coverage.attempt,
            MAX_RUN_ID_BYTES,
        )?;
        bounded_text(
            &format!("action_coverage[{index}].role"),
            &coverage.role,
            MAX_PATH_BYTES,
        )?;
        bounded_text(
            &format!("action_coverage[{index}].session"),
            &coverage.session,
            MAX_RUN_ID_BYTES,
        )?;
        bounded_text(
            &format!("action_coverage[{index}].state"),
            &coverage.state,
            MAX_PATH_BYTES,
        )?;
        if let Some(turn) = coverage.turn.as_deref() {
            bounded_text(
                &format!("action_coverage[{index}].turn"),
                turn,
                MAX_RUN_ID_BYTES,
            )?;
        }
        if coverage.attempt != participant.attempt
            || coverage.role != participant.role
            || coverage.session != participant.session
            || coverage.state != participant.state
        {
            return Err(invalid(format!(
                "action_coverage[{index}] does not bind the exact participant turn {} role {} session {}",
                participant.attempt, participant.role, participant.session
            )));
        }
        if coverage.complete && coverage.turn.is_none() {
            return Err(invalid(format!(
                "action_coverage[{index}] is marked complete without the exact completed native turn identity"
            )));
        }
        if coverage.complete {
            if coverage.truncated
                || coverage.malformed_records != 0
                || coverage.partial_records != 0
                || coverage.omitted_records != 0
                || coverage.terminal_events == 0
                || coverage.retained_bytes == 0
                || coverage.retained_bytes != coverage.source_bytes
            {
                return Err(invalid(format!(
                    "action_coverage[{index}] is marked complete but records a truncated, malformed, partial, omitted or non-terminal stream"
                )));
            }
            let digest = coverage.source_sha256.as_deref().ok_or_else(|| {
                invalid(format!(
                    "action_coverage[{index}] is marked complete without a raw stream digest"
                ))
            })?;
            if !is_sha256(digest) {
                return Err(invalid(format!(
                    "action_coverage[{index}] carries an invalid raw stream digest"
                )));
            }
        } else {
            let note = coverage.note.as_deref().ok_or_else(|| {
                invalid(format!(
                    "action_coverage[{index}] is incomplete without an explicit note naming the gap"
                ))
            })?;
            if note.trim().is_empty() {
                return Err(invalid(format!(
                    "action_coverage[{index}] is incomplete without an explicit note naming the gap"
                )));
            }
            bounded_text(
                &format!("action_coverage[{index}].note"),
                note,
                MAX_TEXT_BYTES,
            )?;
        }
    }
    let computed_complete = actions.action_coverage.iter().all(|entry| entry.complete);
    if actions.action_evidence_complete != computed_complete {
        return Err(invalid(
            "the semantic review action_evidence_complete flag disagrees with its explicit coverage records",
        ));
    }
    let mut counts = std::collections::BTreeMap::<&str, u64>::new();
    for (index, action) in actions.action_evidence.iter().enumerate() {
        bounded_text(
            &format!("action_evidence[{index}].attempt"),
            &action.attempt,
            MAX_RUN_ID_BYTES,
        )?;
        bounded_text(
            &format!("action_evidence[{index}].role"),
            &action.role,
            MAX_PATH_BYTES,
        )?;
        bounded_text(
            &format!("action_evidence[{index}].session"),
            &action.session,
            MAX_RUN_ID_BYTES,
        )?;
        bounded_text(
            &format!("action_evidence[{index}].item"),
            &action.item,
            MAX_RUN_ID_BYTES,
        )?;
        bounded_text(
            &format!("action_evidence[{index}].phase"),
            &action.phase,
            MAX_PATH_BYTES,
        )?;
        match action.kind.as_str() {
            "tool" | "item" => {}
            other => {
                return Err(invalid(format!(
                    "action_evidence[{index}].kind {other:?} is not tool or item"
                )));
            }
        }
        let coverage = actions
            .action_coverage
            .iter()
            .find(|coverage| coverage.attempt == action.attempt)
            .ok_or_else(|| {
                invalid(format!(
                    "action_evidence[{index}] names attempt {:?} with no coverage record",
                    action.attempt
                ))
            })?;
        if coverage.role != action.role || coverage.session != action.session {
            return Err(invalid(format!(
                "action_evidence[{index}] does not bind the exact participant session and role of its coverage record"
            )));
        }
        if coverage.complete {
            let turn = coverage.turn.as_deref().ok_or_else(|| {
                invalid(format!(
                    "action_evidence[{index}] has a complete coverage record without an exact native turn identity"
                ))
            })?;
            if action.turn.as_deref() != Some(turn) {
                return Err(invalid(format!(
                    "action_evidence[{index}] turn {:?} does not match the exact completed turn {turn:?} for attempt {}",
                    action.turn, action.attempt
                )));
            }
        }
        for (name, value, max) in [
            ("turn", action.turn.as_deref(), MAX_RUN_ID_BYTES),
            ("tool", action.tool.as_deref(), MAX_PATH_BYTES),
            (
                "command",
                action.command.as_deref(),
                MAX_ACTION_COMMAND_BYTES,
            ),
            ("target", action.target.as_deref(), MAX_ACTION_TARGET_BYTES),
            (
                "arguments",
                action.arguments.as_deref(),
                MAX_ACTION_ARGUMENTS_BYTES,
            ),
            ("output", action.output.as_deref(), MAX_ACTION_OUTPUT_BYTES),
            ("error", action.error.as_deref(), MAX_ACTION_ERROR_BYTES),
        ] {
            if let Some(value) = value {
                bounded_text(&format!("action_evidence[{index}].{name}"), value, max)?;
            }
        }
        *counts.entry(action.attempt.as_str()).or_default() += 1;
    }
    for coverage in &actions.action_coverage {
        let actual = counts.get(coverage.attempt.as_str()).copied().unwrap_or(0);
        if coverage.action_records != actual {
            return Err(invalid(format!(
                "action_coverage for attempt {} declares {} action record(s) but {} are present",
                coverage.attempt, coverage.action_records, actual
            )));
        }
        if coverage.action_records > MAX_ACTION_RECORDS_PER_TURN as u64 {
            return Err(invalid(format!(
                "action_coverage for attempt {} declares {} action record(s), exceeding the per-turn bound of {MAX_ACTION_RECORDS_PER_TURN}",
                coverage.attempt, coverage.action_records
            )));
        }
    }
    if actions.action_evidence.len() > MAX_ACTION_RECORDS_TOTAL {
        return Err(invalid(format!(
            "the semantic review action evidence exceeds its {MAX_ACTION_RECORDS_TOTAL}-record bound"
        )));
    }
    Ok(())
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub(super) fn physical_lines(bytes: &[u8]) -> u64 {
    if bytes.is_empty() {
        return 0;
    }
    let newlines = bytes.iter().filter(|byte| **byte == b'\n').count() as u64;
    newlines + u64::from(!bytes.ends_with(b"\n"))
}

fn normalize_relative(value: &str) -> io::Result<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.len() > MAX_PATH_BYTES {
        return Err(invalid(
            "semantic review paths must be bounded relative input paths",
        ));
    }
    let normalized = trimmed.replace('\\', "/");
    let path = Path::new(&normalized);
    if path.is_absolute() {
        return Err(invalid(format!(
            "semantic review path {trimmed} must be relative"
        )));
    }
    for component in path.components() {
        match component {
            Component::Normal(name) => {
                let name = name.to_string_lossy();
                if matches!(name.as_ref(), ".git" | ".beads") {
                    return Err(invalid(format!(
                        "semantic review path {trimmed} may not name repository metadata"
                    )));
                }
            }
            _ => {
                return Err(invalid(format!(
                    "semantic review path {trimmed} contains traversal or a non-relative component"
                )));
            }
        }
    }
    Ok(normalized)
}

fn bounded_text(name: &str, value: &str, max: usize) -> io::Result<()> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(invalid(format!("{name} must not be empty")));
    }
    if trimmed.len() > max {
        return Err(invalid(format!("{name} exceeds its {max} byte bound")));
    }
    Ok(())
}

fn is_commit_identity(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Evidence must name at least one exact frozen input at a physical line or
/// line range. This binds a finding to observable packet evidence without
/// asking the parent to reinterpret the reviewer's prose.
fn evidence_has_location(evidence: &str, files: &[PacketFile]) -> bool {
    for (colon, _) in evidence.match_indices(':') {
        let prefix = &evidence[..colon];
        let start = prefix
            .rfind(|character: char| {
                character.is_whitespace()
                    || matches!(
                        character,
                        '(' | '[' | '{' | '<' | ',' | ';' | ':' | '"' | '\'' | '`'
                    )
            })
            .map(|index| index + 1)
            .unwrap_or(0);
        let candidate = prefix[start..].trim_matches(|character: char| {
            matches!(character, '`' | '"' | '\'' | '(' | '[' | '{')
        });
        let Some(file) = files.iter().find(|file| file.path == candidate) else {
            continue;
        };
        let rest = &evidence[colon + 1..];
        let first_len = rest.bytes().take_while(u8::is_ascii_digit).count();
        if first_len == 0 {
            continue;
        }
        let Ok(first) = rest[..first_len].parse::<u64>() else {
            continue;
        };
        let tail = &rest[first_len..];
        let last = if let Some(tail) = tail.strip_prefix('-') {
            let length = tail.bytes().take_while(u8::is_ascii_digit).count();
            if length == 0 {
                continue;
            }
            tail[..length].parse::<u64>().unwrap_or(0)
        } else {
            first
        };
        if first >= 1 && last >= first && last <= file.lines() {
            return true;
        }
    }
    false
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str, text: &str) -> PacketFile {
        PacketFile {
            path: path.to_owned(),
            bytes: text.as_bytes().to_vec(),
        }
    }

    #[test]
    fn physical_lines_are_newline_delimited() {
        assert_eq!(physical_lines(b""), 0);
        assert_eq!(physical_lines(b"one"), 1);
        assert_eq!(physical_lines(b"one\n"), 1);
        assert_eq!(physical_lines(b"one\ntwo\r\n"), 2);
    }

    #[test]
    fn findings_derive_rejection_and_clean_output_derives_acceptance() {
        let files = vec![file("submission.txt", "claim\n")];
        let accepted = validate(
            br#"{"run_id":"r","findings":[],"unresolved_issues":[]}"#,
            "r",
            &files,
        )
        .unwrap();
        assert_eq!(accepted.kind, "accept");
        let rejected = validate(
            br#"{"run_id":"r","findings":[{"path":"submission.txt","line":1,"description":"unsupported claim","evidence":"submission.txt:1 records the claim"}],"unresolved_issues":[]}"#,
            "r",
            &files,
        )
        .unwrap();
        assert_eq!(rejected.kind, "reject");
    }

    #[test]
    fn malformed_location_and_evidence_are_refused() {
        let files = vec![file("submission.txt", "claim\n")];
        for output in [
            br#"{"run_id":"r","findings":[{"path":"submission.txt","line":2,"description":"x","evidence":"submission.txt:1"}],"unresolved_issues":[]}"#.as_slice(),
            br#"{"run_id":"r","findings":[{"path":"other.txt","line":1,"description":"x","evidence":"submission.txt:1"}],"unresolved_issues":[]}"#.as_slice(),
            br#"{"run_id":"r","findings":[{"path":"submission.txt","line":1,"description":"x","evidence":"no location"}],"unresolved_issues":[]}"#.as_slice(),
            br#"{"run_id":"wrong","findings":[],"unresolved_issues":[]}"#.as_slice(),
        ] {
            assert!(validate(output, "r", &files).is_err());
        }
    }

    fn actions_with_turn(coverage_turn: Option<&str>, action_turn: Option<&str>) -> ReviewActions {
        ReviewActions {
            schema: 1,
            run_id: "run-1".to_owned(),
            task: "task".to_owned(),
            submitted_revision: "a".repeat(40),
            base_revision: "b".repeat(40),
            writable_scope: vec!["solution.txt".to_owned()],
            changed_paths: vec!["solution.txt".to_owned()],
            worktree_clean: true,
            report_sha256: "c".repeat(64),
            participants: vec![ReviewParticipant {
                attempt: "executor-1".to_owned(),
                role: "executor".to_owned(),
                session: "session-1".to_owned(),
                profile: "ds".to_owned(),
                state: "completed".to_owned(),
            }],
            action_evidence: vec![ReviewAction {
                attempt: "executor-1".to_owned(),
                role: "executor".to_owned(),
                session: "session-1".to_owned(),
                turn: action_turn.map(str::to_owned),
                item: "cmd-1".to_owned(),
                phase: "completed".to_owned(),
                kind: "tool".to_owned(),
                tool: Some("command".to_owned()),
                command: Some("git status --short".to_owned()),
                target: None,
                arguments: None,
                status: Some("completed".to_owned()),
                exit_code: Some(0),
                output: Some("clean".to_owned()),
                error: None,
                text_truncated: false,
            }],
            action_coverage: vec![ReviewActionCoverage {
                attempt: "executor-1".to_owned(),
                role: "executor".to_owned(),
                session: "session-1".to_owned(),
                state: "completed".to_owned(),
                turn: coverage_turn.map(str::to_owned),
                source_sha256: Some("d".repeat(64)),
                source_bytes: 128,
                retained_bytes: 128,
                action_records: 1,
                omitted_records: 0,
                malformed_records: 0,
                partial_records: 0,
                terminal_events: 1,
                truncated: false,
                complete: true,
                note: None,
            }],
            action_evidence_complete: true,
        }
    }

    #[test]
    fn complete_action_evidence_requires_the_exact_native_turn() {
        assert!(
            validate_action_evidence(&actions_with_turn(Some("turn-a"), Some("turn-a"))).is_ok()
        );
        for (coverage_turn, action_turn) in [
            (Some("turn-a"), Some("turn-b")),
            (Some("turn-a"), None),
            (None, Some("turn-a")),
            (None, None),
        ] {
            assert!(
                validate_action_evidence(&actions_with_turn(coverage_turn, action_turn)).is_err(),
                "coverage turn {coverage_turn:?} action turn {action_turn:?} must not be complete"
            );
        }
    }
}
