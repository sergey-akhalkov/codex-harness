//! Instruction-refresh succession for orchestrated workers.
//!
//! One exact session's CLI process is replaced through the verified
//! `codex exec resume <SESSION_ID>` path only at a safe boundary after its
//! in-flight tool effects have completed. The predecessor's durable task
//! context is handed over first and its recorded process is stopped before the
//! successor starts. The successor is only accepted when its own session
//! rollout shows that the current instructions and skills were reloaded.
//!
//! This module is deterministic mechanics only: no model calls, no interactive
//! picker, no model substitution, no catalogue delivery and no same-session
//! compact recovery (owned by `autonomous-skill-evolution`). The compact
//! accepted-revision identity published by skill-evolution
//! (`name`, canonical `path`, `revision`, `operation`) is consumed read-only.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
};

/// Exact wording required when reload verification fails.
pub const NOT_ESTABLISHED: &str = "succession not established";
/// Stable marker that identifies the successor continuation turn in a rollout.
pub const MARKER: &str = "SUCCESSION_CONTINUATION";
/// Instruction content beyond this bound is verified through its prefix only.
const INSTRUCTION_LIMIT: usize = 32 * 1024;
/// Bounded context fields in the continuation prompt.
const FIELD_LIMIT: usize = 2048;
const PROMPT_LIMIT: usize = 8192;
/// Bounded search of the session rollout directory.
const ROLLOUT_SCAN_LIMIT: usize = 4096;
const ROLLOUT_TAIL_LIMIT: u64 = 4 * 1024 * 1024;
const ROLLOUT_CANDIDATES: usize = 64;
const SANDBOX_MODES: [&str; 3] = ["read-only", "workspace-write", "danger-full-access"];
const APPROVAL_POLICIES: [&str; 2] = ["on-request", "never"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RevisionIdentity {
    pub name: String,
    pub path: String,
    pub revision: String,
    pub operation: String,
}

/// Accepts the flat `skills identity` report and the journaled awareness
/// record that nests the same compact identity under `identity`.
pub fn parse_revision(bytes: &[u8]) -> io::Result<RevisionIdentity> {
    let value: Value =
        serde_json::from_slice(bytes).map_err(|_| invalid("revision identity is not JSON"))?;
    let source = match value.get("identity") {
        Some(identity) if identity.is_object() => identity.clone(),
        _ => value,
    };
    let identity: RevisionIdentity =
        serde_json::from_value(source).map_err(|_| invalid("revision identity is incomplete"))?;
    for (field, text) in [
        ("name", &identity.name),
        ("path", &identity.path),
        ("revision", &identity.revision),
        ("operation", &identity.operation),
    ] {
        if text.trim().is_empty() || text.len() > 512 || text.contains('\0') {
            return Err(invalid(&format!("revision identity {field} is invalid")));
        }
    }
    if !Path::new(&identity.path).is_absolute() {
        return Err(invalid(
            "revision identity path must be canonical and absolute",
        ));
    }
    Ok(identity)
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: u32,
    pub session: String,
    pub profile: String,
    pub codex_home: PathBuf,
    pub workspace: PathBuf,
    #[serde(default)]
    pub state: Option<PathBuf>,
    #[serde(default)]
    pub executable: Option<PathBuf>,
    pub revision: RevisionIdentity,
    #[serde(default)]
    pub instructions: Option<PathBuf>,
    #[serde(default)]
    pub task: Option<String>,
    #[serde(default)]
    pub assignment: Option<String>,
    pub requirements: String,
    pub partial_work: String,
    #[serde(default)]
    pub sandbox: Option<String>,
    #[serde(default)]
    pub approval: Option<String>,
    #[serde(default)]
    pub record: Option<PathBuf>,
    pub evidence: PathBuf,
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,
}

fn default_timeout() -> u64 {
    120
}

impl Request {
    pub fn validate(&self) -> io::Result<()> {
        if self.schema != 1 {
            return Err(invalid("unsupported succession request schema"));
        }
        let session = self.session.trim();
        if session.is_empty()
            || session.len() > 256
            || session.starts_with('-')
            || session.contains(char::is_whitespace)
        {
            return Err(invalid(
                "succession requires one exact session id; --last and picker selection are refused",
            ));
        }
        if self.profile.trim().is_empty() {
            return Err(invalid("succession profile is missing"));
        }
        for (name, path) in [
            ("codex_home", &self.codex_home),
            ("workspace", &self.workspace),
            ("evidence", &self.evidence),
        ] {
            if !path.is_absolute() {
                return Err(invalid(&format!("succession {name} must be absolute")));
            }
        }
        for (name, value) in [
            ("state", self.state.as_ref()),
            ("executable", self.executable.as_ref()),
            ("instructions", self.instructions.as_ref()),
            ("record", self.record.as_ref()),
        ] {
            if value
                .is_some_and(|path| !path.is_absolute() || path.to_string_lossy().contains('\0'))
            {
                return Err(invalid(&format!("succession {name} must be absolute")));
            }
        }
        for (name, text) in [
            ("requirements", &self.requirements),
            ("partial_work", &self.partial_work),
        ] {
            if text.trim().is_empty() || text.len() > FIELD_LIMIT {
                return Err(invalid(&format!(
                    "succession {name} is empty or exceeds its bound"
                )));
            }
        }
        if let Some(sandbox) = &self.sandbox
            && !SANDBOX_MODES.contains(&sandbox.as_str())
        {
            return Err(invalid("succession sandbox mode is unsupported"));
        }
        if let Some(approval) = &self.approval
            && !APPROVAL_POLICIES.contains(&approval.as_str())
        {
            return Err(invalid("succession approval policy is unsupported"));
        }
        if self.timeout_seconds == 0 || self.timeout_seconds > 3600 {
            return Err(invalid(
                "succession timeout must be between 1 and 3600 seconds",
            ));
        }
        Ok(())
    }

    /// Instruction source used for reload verification.
    pub fn instruction_path(&self) -> PathBuf {
        match &self.instructions {
            Some(path) => path.clone(),
            None => {
                let workspace = self.workspace.join("AGENTS.md");
                if workspace.is_file() {
                    workspace
                } else {
                    self.codex_home.join("AGENTS.md")
                }
            }
        }
    }

    /// Durable handover record path; private state, never a shared source.
    pub fn record_path(&self) -> PathBuf {
        self.record.clone().unwrap_or_else(|| match &self.task {
            Some(task) if !task.trim().is_empty() => self
                .codex_home
                .join("harness/orchestration")
                .join(format!("{task}.succession.json")),
            _ => self.evidence.join("succession-handover.json"),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionFacts {
    pub thread_present: bool,
    pub thread_settled: bool,
    pub last_turn_in_progress: bool,
    pub active_turn: Option<String>,
    pub active_operations: Vec<String>,
    pub pending_requests: usize,
    pub recovery_pending: Vec<String>,
    pub handoff_phase: Option<String>,
    pub client_closed: bool,
    pub closed: bool,
    /// `None` when the recorded view process cannot be observed.
    pub predecessor_running: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeFacts {
    /// Exact native background terminal inventory for the session is empty.
    pub terminals_empty: bool,
    /// The session's latest native turn is not in progress.
    pub latest_turn_settled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SafeBoundary {
    pub predecessor_running: bool,
    /// `None` when the predecessor already exited and its controller recorded
    /// the settled closure; no live native inventory exists in that case.
    pub native: Option<NativeFacts>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Boundary {
    Safe(SafeBoundary),
    Deferred(String),
}

/// Reads the predecessor's private controller artifacts. Missing optional
/// artifacts stay unknown rather than guessing safety.
pub fn read_session_facts(root: &Path) -> io::Result<SessionFacts> {
    let task: Value = read_json(&root.join("task.json"))?;
    let vis: Value = read_optional_json(&root.join("visibility.json"))?.unwrap_or(json!({}));
    let handoff: Value = read_optional_json(&root.join("handoff.json"))?.unwrap_or(json!({}));
    let closed = root.join("closed.json").is_file();
    let client_closed = root.join("client-closed.json").is_file();
    // The caller observes the recorded view process; the artifacts alone do
    // not establish liveness. A settled closure is the only local proof.
    let predecessor_running = closed.then_some(false);
    let threads = task["threads"].as_object();
    let thread = threads.and_then(|threads| threads.values().next());
    let thread_present = thread.is_some();
    let status = thread.map(|thread| thread["status"]["type"].as_str().unwrap_or("unknown"));
    let thread_settled = matches!(status, Some("idle" | "notLoaded"));
    let last_turn = thread
        .and_then(|thread| thread["turns"].as_array())
        .and_then(|turns| turns.last());
    let last_turn_in_progress = last_turn.is_some_and(|turn| turn["status"] == "inProgress");
    let keys = |value: &Value, key: &str| -> Vec<String> {
        value[key]
            .as_object()
            .map(|map| map.keys().cloned().collect())
            .unwrap_or_default()
    };
    let handoff_phase = handoff["transfer"]["phase"].as_str().map(str::to_owned);
    Ok(SessionFacts {
        thread_present,
        thread_settled,
        last_turn_in_progress,
        active_turn: keys(&vis, "activeTurns").into_iter().next(),
        active_operations: keys(&vis, "activeOperations"),
        pending_requests: vis["pendingRequests"]
            .as_object()
            .map_or(0, |map| map.len()),
        recovery_pending: keys(&vis, "recoveryPending"),
        handoff_phase,
        client_closed,
        closed,
        predecessor_running,
    })
}

/// Decides whether replacement may proceed. Unknown observations defer the
/// succession; they never authorize a stop or a spawn.
pub fn boundary(facts: &SessionFacts, native: Option<&NativeFacts>) -> Boundary {
    if facts.closed {
        return Boundary::Safe(SafeBoundary {
            predecessor_running: false,
            native: None,
        });
    }
    if facts.predecessor_running == Some(false) {
        return Boundary::Deferred(
            "predecessor process is gone but its work never settled; reconcile before replacement"
                .into(),
        );
    }
    if facts.predecessor_running.is_none() {
        return Boundary::Deferred(
            "predecessor process state is unknown; inspect the session before replacement".into(),
        );
    }
    if !facts.thread_present {
        return Boundary::Deferred(
            "predecessor session state is unavailable; boundary unknown".into(),
        );
    }
    if !facts.thread_settled || facts.last_turn_in_progress {
        return Boundary::Deferred("the predecessor turn is still active".into());
    }
    if let Some(turn) = &facts.active_turn {
        return Boundary::Deferred(format!("turn {turn} is still active"));
    }
    if !facts.active_operations.is_empty() {
        return Boundary::Deferred(format!(
            "in-flight tool effects are not complete: {}",
            facts.active_operations.join(", ")
        ));
    }
    if facts.pending_requests > 0 {
        return Boundary::Deferred("native observation requests are still pending".into());
    }
    if !facts.recovery_pending.is_empty() {
        return Boundary::Deferred("visibility recovery is still pending".into());
    }
    if let Some(phase) = &facts.handoff_phase
        && phase != "waiting"
        && phase != "running"
    {
        return Boundary::Deferred(format!(
            "leadership transfer phase '{phase}' requires reconciliation"
        ));
    }
    let Some(native) = native else {
        return Boundary::Deferred(
            "native background inventory is unknown; boundary not established".into(),
        );
    };
    if !native.latest_turn_settled {
        return Boundary::Deferred("the predecessor native turn is still running".into());
    }
    if !native.terminals_empty {
        return Boundary::Deferred(
            "predecessor background terminals are still running; effects are in flight".into(),
        );
    }
    Boundary::Safe(SafeBoundary {
        predecessor_running: true,
        native: Some(native.clone()),
    })
}

/// Recorded binding of the predecessor session (`leader.json`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionBinding {
    pub model: Option<String>,
    pub model_provider: Option<String>,
    pub reasoning_effort: Option<String>,
    pub approval: Option<String>,
    pub sandbox: Option<String>,
    pub cwd: Option<PathBuf>,
}

pub fn binding_from_leader(root: &Path) -> io::Result<Option<SessionBinding>> {
    let Some(leader) = read_optional_json::<Value>(&root.join("leader.json"))? else {
        return Ok(None);
    };
    let native = &leader["native"];
    let text = |value: &Value, key: &str| value[key].as_str().map(str::to_owned);
    let sandbox = native["sandbox"]["type"]
        .as_str()
        .or_else(|| leader["sandbox"]["type"].as_str())
        .and_then(sandbox_mode);
    let cwd = native["cwd"]
        .as_str()
        .or_else(|| leader["cwd"].as_str())
        .map(PathBuf::from);
    Ok(Some(SessionBinding {
        model: text(native, "model").or_else(|| text(&leader, "model")),
        model_provider: text(native, "modelProvider").or_else(|| text(&leader, "modelProvider")),
        reasoning_effort: text(native, "reasoningEffort"),
        approval: text(native, "approvalPolicy"),
        sandbox,
        cwd,
    }))
}

/// The native thread result reports sandbox types in its own vocabulary; the
/// CLI takes the kebab-case modes. Unsupported values stay unknown so the
/// successor is never started with a guessed authorization.
fn sandbox_mode(value: &str) -> Option<String> {
    let mode = match value {
        "dangerFullAccess" => "danger-full-access",
        "readOnly" => "read-only",
        "workspaceWrite" => "workspace-write",
        other if SANDBOX_MODES.contains(&other) => other,
        _ => return None,
    };
    Some(mode.to_owned())
}

/// The successor must keep the predecessor's binding and authorization.
pub fn verify_binding(
    request: &Request,
    binding: &SessionBinding,
    profile: &crate::orchestration_config::ProfileBinding,
) -> io::Result<()> {
    if let (Some(recorded), Some(configured)) = (&binding.model, &profile.model)
        && recorded != configured
    {
        return Err(invalid(
            "configured profile model differs from the recorded session; refusing substitution",
        ));
    }
    if let (Some(recorded), Some(configured)) = (&binding.model_provider, &profile.model_provider)
        && recorded != configured
    {
        return Err(invalid(
            "configured profile provider differs from the recorded session; refusing substitution",
        ));
    }
    if let (Some(recorded), Some(configured)) =
        (&binding.reasoning_effort, &profile.reasoning_effort)
        && recorded != configured
    {
        return Err(invalid(
            "configured profile effort differs from the recorded session; refusing substitution",
        ));
    }
    if let (Some(recorded), Some(requested)) = (&binding.sandbox, &request.sandbox)
        && recorded != requested
    {
        return Err(invalid(
            "requested sandbox differs from the recorded session authorization",
        ));
    }
    if let (Some(recorded), Some(requested)) = (&binding.approval, &request.approval)
        && recorded != requested
    {
        return Err(invalid(
            "requested approval policy differs from the recorded session authorization",
        ));
    }
    if let Some(cwd) = &binding.cwd {
        let recorded = cwd.canonicalize().unwrap_or_else(|_| cwd.clone());
        let requested = request
            .workspace
            .canonicalize()
            .unwrap_or_else(|_| request.workspace.clone());
        if recorded != requested {
            return Err(invalid(
                "requested workspace differs from the recorded session workspace",
            ));
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SuccessorPlan {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub prompt: String,
    #[serde(skip)]
    pub sandbox: Option<String>,
    #[serde(skip)]
    pub approval: Option<String>,
}

/// Builds the verified successor invocation: one exact session id, never a
/// picker, never `--last`, keeping the recorded sandbox and approval policy.
pub fn successor_plan(
    request: &Request,
    binding: Option<&SessionBinding>,
) -> io::Result<SuccessorPlan> {
    let profile_args = crate::orchestration_config::executor_session_args(&request.profile)?;
    let sandbox = binding
        .and_then(|binding| binding.sandbox.clone())
        .or_else(|| request.sandbox.clone());
    let approval = binding
        .and_then(|binding| binding.approval.clone())
        .or_else(|| request.approval.clone());
    let cwd = binding
        .and_then(|binding| binding.cwd.clone())
        .unwrap_or_else(|| request.workspace.clone());
    let prompt = continuation_prompt(request, sandbox.as_deref(), approval.as_deref());
    if prompt.len() > PROMPT_LIMIT {
        return Err(invalid("succession continuation prompt exceeds its bound"));
    }
    let program = request
        .executable
        .clone()
        .unwrap_or_else(|| request.codex_home.join("harness/bin/codex.exe"));
    if !program.is_absolute() {
        return Err(invalid("successor executable must be absolute"));
    }
    let mut args: Vec<String> = profile_args;
    args.extend([
        "exec".into(),
        "--skip-git-repo-check".into(),
        "--json".into(),
    ]);
    if let Some(sandbox) = &sandbox {
        args.extend(["--sandbox".into(), sandbox.clone()]);
    }
    args.extend(["-C".into(), cwd.to_string_lossy().into_owned()]);
    if let Some(approval) = &approval {
        args.extend(["-c".into(), format!("approval_policy={approval}")]);
    }
    args.extend([
        "resume".into(),
        request.session.trim().to_owned(),
        prompt.clone(),
    ]);
    for forbidden in ["--last", "--all", "--remote", "--worktree", "fork"] {
        if args.iter().any(|arg| arg == forbidden) {
            return Err(invalid(
                "successor invocation must select the exact session without a picker",
            ));
        }
    }
    let resume = args
        .iter()
        .position(|arg| arg == "resume")
        .ok_or_else(|| invalid("successor invocation lost its resume path"))?;
    if args.get(resume + 1).map(String::as_str) != Some(request.session.trim()) {
        return Err(invalid("successor invocation must name the exact session"));
    }
    Ok(SuccessorPlan {
        program,
        args,
        prompt,
        sandbox,
        approval,
    })
}

fn bounded_field(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.len() <= FIELD_LIMIT {
        return trimmed.to_owned();
    }
    let mut end = FIELD_LIMIT;
    while !trimmed.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}… (truncated)", &trimmed[..end])
}

/// Bounded continuation prompt for the successor process with the effective
/// (recorded) authorization text.
pub fn continuation_prompt(
    request: &Request,
    sandbox: Option<&str>,
    approval: Option<&str>,
) -> String {
    let revision = &request.revision;
    format!(
        "{MARKER} {session} {name}@{rev}\n\
         You are the successor process for the same already authorized task, resumed under refreshed instructions and skills. Continue only that task with its original constraints and acceptance.\n\
         Durable handover record: {record}\n\
         Task: {task}; assignment: {assignment}\n\
         Requirements: {requirements}\n\
         Authorization (unchanged): {authorization}\n\
         Published revision: skill={name} path={path} revision={rev} operation={operation}\n\
         Partial work to preserve: {partial}\n\
         First reconcile the workspace state and the visible conversation history. Read existing artifacts before writing and never blindly replay an operation whose outcome is uncertain. Do not spawn hidden agents.\n",
        session = request.session.trim(),
        name = revision.name,
        rev = revision.revision,
        record = request.record_path().display(),
        task = request.task.as_deref().unwrap_or("unrecorded"),
        assignment = request.assignment.as_deref().unwrap_or("unrecorded"),
        requirements = bounded_field(&request.requirements),
        authorization = authorization_text(request, sandbox, approval),
        path = revision.path,
        operation = revision.operation,
        partial = bounded_field(&request.partial_work),
    )
}

fn authorization_text(request: &Request, sandbox: Option<&str>, approval: Option<&str>) -> String {
    let sandbox = sandbox.unwrap_or("unchanged");
    let approval = approval.unwrap_or("unchanged");
    format!(
        "sandbox={sandbox} approval={approval} workspace={}",
        request.workspace.display()
    )
}

/// Durable handover record written before the predecessor is stopped.
pub fn handover_record(
    request: &Request,
    binding: Option<&SessionBinding>,
    profile: &crate::orchestration_config::ProfileBinding,
    predecessor_stop: &str,
    state: Option<&Path>,
) -> Value {
    json!({
        "schema": 1,
        "kind": "instructionRefreshSuccession",
        "session": request.session.trim(),
        "profile": request.profile,
        "workspace": request.workspace,
        "revision": request.revision,
        "task": request.task,
        "assignment": request.assignment,
        "requirements": request.requirements,
        "partialWork": request.partial_work,
        "authorization": {
            "sandbox": binding.and_then(|binding| binding.sandbox.clone()).or_else(|| request.sandbox.clone()),
            "approval": binding.and_then(|binding| binding.approval.clone()).or_else(|| request.approval.clone()),
            "model": profile.model,
            "modelProvider": profile.model_provider,
            "reasoningEffort": profile.reasoning_effort,
        },
        "predecessor": {
            "state": state,
            "stop": predecessor_stop,
        },
        "successor": "exec resume; reconcile partial effects before writing; never replay uncertain operations",
        "modelCalls": 0,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reload {
    Verified,
    NotVerified(String),
}

pub struct ReloadExpectation<'a> {
    pub instruction_path: &'a Path,
    pub instruction_text: &'a str,
    pub skill_name: &'a str,
    pub skill_description: &'a str,
    pub session: &'a str,
}

fn normalize(text: &str) -> String {
    text.replace("\r\n", "\n")
}

fn instruction_needle(text: &str) -> String {
    let normalized = normalize(text);
    let trimmed = normalized.trim();
    if trimmed.len() <= INSTRUCTION_LIMIT {
        return trimmed.to_owned();
    }
    let mut end = INSTRUCTION_LIMIT;
    while !trimmed.is_char_boundary(end) {
        end -= 1;
    }
    trimmed[..end].to_owned()
}

/// Verifies that the successor's own preserved rollout re-injected the current
/// instructions and the accepted skill revision in its continuation turn.
pub fn verify_reload(rollout: &str, expectation: &ReloadExpectation<'_>) -> Reload {
    let needle = instruction_needle(expectation.instruction_text);
    if needle.is_empty() {
        return Reload::NotVerified("current instructions are empty".into());
    }
    let marker = format!("{MARKER} {}", expectation.session);
    let skill = format!(
        "- {}: {}",
        expectation.skill_name, expectation.skill_description
    );
    let mut marker_line = None;
    let mut skills_line = None;
    let mut instructions_line = None;
    for (index, line) in rollout.lines().enumerate() {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if value["type"] != "response_item" || value["payload"]["type"] != "message" {
            continue;
        }
        let Some(parts) = value["payload"]["content"].as_array() else {
            continue;
        };
        let text = parts
            .iter()
            .filter_map(|part| part["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let text = normalize(&text);
        let role = value["payload"]["role"].as_str();
        if role == Some("user") && text.contains(&marker) {
            marker_line = Some(index);
        } else if role == Some("user")
            && text.contains("# AGENTS.md instructions")
            && text.contains(&needle)
        {
            instructions_line = Some(index);
        } else if role == Some("developer")
            && text.contains("<skills_instructions>")
            && text.contains(&skill)
        {
            skills_line = Some(index);
        }
    }
    let Some(marker_line) = marker_line else {
        return Reload::NotVerified(
            "the successor continuation turn is missing from the session rollout".into(),
        );
    };
    let window = marker_line.saturating_sub(40);
    if skills_line.is_none_or(|index| index < window || index > marker_line) {
        return Reload::NotVerified(format!(
            "the successor skills list does not contain {} at the published revision",
            expectation.skill_name
        ));
    }
    if instructions_line.is_none_or(|index| index < window || index > marker_line) {
        return Reload::NotVerified(format!(
            "the successor turn did not reload {}",
            expectation.instruction_path.display()
        ));
    }
    Reload::Verified
}

/// Finds the session's rollout under `CODEX_HOME/sessions`. The most recently
/// modified file carrying the successor continuation marker wins; a session
/// without a marked rollout is reported as missing evidence, never guessed.
pub fn find_rollout(codex_home: &Path, session: &str, marker: &str) -> io::Result<Option<PathBuf>> {
    let root = codex_home.join("sessions");
    let mut pending = vec![(root.clone(), 0usize)];
    let mut candidates: Vec<(u64, PathBuf)> = Vec::new();
    let mut visited = 0usize;
    while let Some((directory, depth)) = pending.pop() {
        if depth > 4 || visited > ROLLOUT_SCAN_LIMIT {
            continue;
        }
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        for entry in entries {
            let entry = entry?;
            visited += 1;
            let path = entry.path();
            if path.is_dir() {
                pending.push((path, depth + 1));
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.starts_with("rollout-") || !name.contains(session) {
                continue;
            }
            let modified = entry
                .metadata()?
                .modified()?
                .duration_since(std::time::UNIX_EPOCH)
                .map(|value| value.as_millis() as u64)
                .unwrap_or(0);
            candidates.push((modified, path));
        }
    }
    candidates.sort_by_key(|entry| std::cmp::Reverse(entry.0));
    candidates.truncate(ROLLOUT_CANDIDATES);
    for (_, path) in candidates {
        if let Ok(tail) = read_bounded_tail(&path)
            && tail.contains(marker)
        {
            return Ok(Some(path));
        }
    }
    Ok(None)
}

/// Bounded read of the rollout tail; the successor turn is retained whole.
pub fn read_bounded_tail(path: &Path) -> io::Result<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = fs::File::open(path)?;
    let length = file.metadata()?.len();
    let start = length.saturating_sub(ROLLOUT_TAIL_LIMIT);
    if start > 0 {
        file.seek(SeekFrom::Start(start))?;
    }
    let mut bytes = Vec::new();
    file.take(ROLLOUT_TAIL_LIMIT).read_to_end(&mut bytes)?;
    if start > 0
        && let Some(newline) = bytes.iter().position(|byte| *byte == b'\n')
    {
        bytes.drain(..=newline);
    }
    String::from_utf8(bytes).map_err(|_| invalid("session rollout is not UTF-8"))
}

pub fn hash_text(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

/// Session pointer recorded by the managed runtime for exact-session lookup.
pub fn session_pointer_path(codex_home: &Path, session: &str) -> PathBuf {
    codex_home
        .join("harness/task-sessions")
        .join(format!("{session}.json"))
}

pub fn read_session_pointer(codex_home: &Path, session: &str) -> io::Result<Option<PathBuf>> {
    let Some(value) = read_optional_json::<Value>(&session_pointer_path(codex_home, session))?
    else {
        return Ok(None);
    };
    let root = value["root"]
        .as_str()
        .map(PathBuf::from)
        .ok_or_else(|| invalid("session pointer does not name a state root"))?;
    if !root.is_absolute() {
        return Err(invalid("session pointer state root must be absolute"));
    }
    Ok(Some(root))
}

/// Reads one bounded JSON artifact.
pub(crate) fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> io::Result<T> {
    let bytes = fs::read(path)?;
    serde_json::from_slice(&bytes).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{}: {error}", path.display()),
        )
    })
}

fn read_optional_json<T: serde::de::DeserializeOwned>(path: &Path) -> io::Result<Option<T>> {
    match read_json(path) {
        Ok(value) => Ok(Some(value)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

pub(crate) fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

/// Exact allowed session-id shape; names are refused so selection stays exact.
pub fn exact_session_id(session: &str) -> io::Result<&str> {
    let session = session.trim();
    if session.len() < 8
        || session.len() > 256
        || !session
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '-')
    {
        return Err(invalid(
            "succession session must be one exact session id, not a picker selection",
        ));
    }
    Ok(session)
}

/// Unique marker text used to locate the successor turn evidence.
pub fn marker_for(session: &str) -> String {
    format!("{MARKER} {session}")
}

/// Ordered argument text for receipts.
pub fn argv_text(program: &Path, args: &[String]) -> Vec<String> {
    let mut argv: Vec<String> = vec![program.to_string_lossy().into_owned()];
    argv.extend(args.iter().cloned());
    argv
}

/// os string argv for spawning.
pub fn os_argv(plan: &SuccessorPlan) -> Vec<OsString> {
    plan.args.iter().map(OsString::from).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orchestration_config::ProfileBinding;

    fn request(root: &Path) -> Request {
        Request {
            schema: 1,
            session: "01a0bb1b-c508-7f83-9567-c916cfa1770c".into(),
            profile: "ds".into(),
            codex_home: root.join("home"),
            workspace: root.join("workspace"),
            state: Some(root.join("state")),
            executable: None,
            revision: RevisionIdentity {
                name: "demo".into(),
                path: root.join("skills/demo").to_string_lossy().into_owned(),
                revision: "rev-2".into(),
                operation: "update".into(),
            },
            instructions: None,
            task: Some("task-1".into()),
            assignment: Some("a1".into()),
            requirements: "Deliver the result with checks".into(),
            partial_work: "proof.txt holds one completed effect".into(),
            sandbox: Some("read-only".into()),
            approval: Some("never".into()),
            record: None,
            evidence: root.join("evidence"),
            timeout_seconds: 60,
        }
    }

    fn binding(root: &Path) -> SessionBinding {
        SessionBinding {
            model: Some("gpt-6-astra".into()),
            model_provider: Some("fixture".into()),
            reasoning_effort: Some("low".into()),
            approval: Some("never".into()),
            sandbox: Some("read-only".into()),
            cwd: Some(root.join("workspace")),
        }
    }

    fn profile_binding() -> ProfileBinding {
        ProfileBinding {
            profile: "ds".into(),
            model: Some("gpt-6-astra".into()),
            model_provider: Some("fixture".into()),
            reasoning_effort: Some("low".into()),
        }
    }

    #[test]
    fn request_validation_refuses_pickers_and_unknown_modes() {
        let owned = tempfile::tempdir().unwrap();
        let root = owned.path();
        let valid = request(root);
        valid.validate().unwrap();
        for invalid_session in ["", "  ", "-1", "--last", "last session"] {
            let mut request = valid.clone();
            request.session = invalid_session.into();
            assert!(request.validate().is_err(), "{invalid_session}");
        }
        let mut relative = valid.clone();
        relative.workspace = PathBuf::from("relative");
        assert!(relative.validate().is_err());
        let mut sandbox = valid.clone();
        sandbox.sandbox = Some("anything".into());
        assert!(sandbox.validate().is_err());
        let mut timeout = valid.clone();
        timeout.timeout_seconds = 0;
        assert!(timeout.validate().is_err());
        let mut empty = valid.clone();
        empty.requirements = "   ".into();
        assert!(empty.validate().is_err());
        let session = exact_session_id(&valid.session).unwrap();
        assert_eq!(session, valid.session);
    }

    #[test]
    fn revision_identity_accepts_the_published_flat_and_nested_forms() {
        let flat = json!({"name":"demo","path":"C:\\skills\\demo","revision":"abc","operation":"update","observed":true});
        let parsed = parse_revision(serde_json::to_vec(&flat).unwrap().as_slice()).unwrap();
        assert_eq!(parsed.name, "demo");
        let nested = json!({"identity":{"name":"demo","path":"C:\\skills\\demo","revision":"abc","operation":"update"},"stale":false});
        let parsed = parse_revision(serde_json::to_vec(&nested).unwrap().as_slice()).unwrap();
        assert_eq!(parsed.revision, "abc");
        assert!(parse_revision(b"{}").is_err());
        assert!(parse_revision(br#"{"name":"demo"}"#).is_err());
    }

    #[test]
    fn successor_plan_selects_the_exact_session_and_preserves_authorization() {
        let owned = tempfile::tempdir().unwrap();
        let root = owned.path();
        let request = request(root);
        let binding = binding(root);
        let plan = successor_plan(&request, Some(&binding)).unwrap();
        assert_eq!(plan.args[0], "--profile");
        assert_eq!(plan.args[1], "ds");
        assert_eq!(plan.args[2], "-c");
        assert_eq!(plan.args[3], "agents.enabled=false");
        assert_eq!(plan.args[4], "exec");
        assert!(plan.args.contains(&"--skip-git-repo-check".to_string()));
        assert!(plan.args.contains(&"--json".to_string()));
        let sandbox = plan.args.iter().position(|arg| arg == "--sandbox").unwrap();
        assert_eq!(plan.args[sandbox + 1], "read-only");
        let approval = plan
            .args
            .windows(2)
            .position(|pair| pair[0] == "-c" && pair[1].starts_with("approval_policy="))
            .unwrap();
        assert_eq!(plan.args[approval + 1], "approval_policy=never");
        let resume = plan.args.iter().position(|arg| arg == "resume").unwrap();
        assert_eq!(plan.args[resume + 1], request.session);
        for forbidden in ["--last", "--all", "--remote", "--worktree", "fork"] {
            assert!(!plan.args.iter().any(|arg| arg == forbidden), "{forbidden}");
        }
        assert!(plan.prompt.contains(MARKER));
        assert!(plan.prompt.contains(&request.session));
        assert!(plan.prompt.contains("demo@rev-2"));
        assert!(plan.prompt.contains("never blindly replay"));
        assert!(plan.prompt.contains("proof.txt holds one completed effect"));

        let mut default_profile = request.clone();
        default_profile.profile = "default".into();
        let plan = successor_plan(&default_profile, None).unwrap();
        assert_eq!(plan.args[0], "-c");
        assert_eq!(plan.args[1], "agents.enabled=false");
        assert_eq!(plan.args[2], "exec");

        let mut substituted = binding.clone();
        substituted.model = Some("other-model".into());
        assert!(verify_binding(&request, &substituted, &profile_binding()).is_err());
        let mut sandbox_conflict = request.clone();
        sandbox_conflict.sandbox = Some("danger-full-access".into());
        assert!(verify_binding(&sandbox_conflict, &binding, &profile_binding()).is_err());
        let mut foreign = binding.clone();
        foreign.cwd = Some(root.join("elsewhere"));
        assert!(verify_binding(&request, &foreign, &profile_binding()).is_err());
        verify_binding(&request, &binding, &profile_binding()).unwrap();
    }

    #[test]
    fn boundary_defers_unknown_and_in_flight_effects() {
        let settled = SessionFacts {
            thread_present: true,
            thread_settled: true,
            last_turn_in_progress: false,
            active_turn: None,
            active_operations: vec![],
            pending_requests: 0,
            recovery_pending: vec![],
            handoff_phase: Some("waiting".into()),
            client_closed: false,
            closed: false,
            predecessor_running: Some(true),
        };
        let native = NativeFacts {
            terminals_empty: true,
            latest_turn_settled: true,
        };
        assert!(matches!(
            boundary(&settled, Some(&native)),
            Boundary::Safe(SafeBoundary {
                predecessor_running: true,
                ..
            })
        ));
        assert!(matches!(boundary(&settled, None), Boundary::Deferred(_)));
        let mut active = settled.clone();
        active.active_operations = vec!["item-1".into()];
        assert!(matches!(
            boundary(&active, Some(&native)),
            Boundary::Deferred(_)
        ));
        let mut turn = settled.clone();
        turn.active_turn = Some("turn-1".into());
        assert!(matches!(
            boundary(&turn, Some(&native)),
            Boundary::Deferred(_)
        ));
        let mut pending = settled.clone();
        pending.pending_requests = 2;
        assert!(matches!(
            boundary(&pending, Some(&native)),
            Boundary::Deferred(_)
        ));
        let mut recovery = settled.clone();
        recovery.recovery_pending = vec!["lead".into()];
        assert!(matches!(
            boundary(&recovery, Some(&native)),
            Boundary::Deferred(_)
        ));
        let mut blocked = settled.clone();
        blocked.handoff_phase = Some("blocked".into());
        assert!(matches!(
            boundary(&blocked, Some(&native)),
            Boundary::Deferred(_)
        ));
        let mut unknown_process = settled.clone();
        unknown_process.predecessor_running = None;
        assert!(matches!(
            boundary(&unknown_process, Some(&native)),
            Boundary::Deferred(_)
        ));
        let mut busy_terminals = native.clone();
        busy_terminals.terminals_empty = false;
        assert!(matches!(
            boundary(&settled, Some(&busy_terminals)),
            Boundary::Deferred(_)
        ));
        let closed = SessionFacts {
            predecessor_running: Some(false),
            closed: true,
            ..settled.clone()
        };
        assert!(matches!(
            boundary(&closed, None),
            Boundary::Safe(SafeBoundary {
                predecessor_running: false,
                native: None
            })
        ));
    }

    #[test]
    fn session_facts_read_the_controller_artifacts() {
        let owned = tempfile::tempdir().unwrap();
        let root = owned.path();
        fs::write(
            root.join("task.json"),
            serde_json::to_vec(&json!({"schema":1,"threads":{"lead":{
                "id":"lead","status":{"type":"idle"},
                "turns":[{"id":"t1","status":"completed"}]}},"completedItems":{}}))
            .unwrap(),
        )
        .unwrap();
        fs::write(
            root.join("visibility.json"),
            serde_json::to_vec(
                &json!({"schema":1,"visible":true,"conversations":{"lead":true},
                "activeTurns":{},"activeOperations":{},"interruptionRequested":{},
                "recoveryPending":[],"pendingRequests":{}}),
            )
            .unwrap(),
        )
        .unwrap();
        fs::write(root.join("view.json"), b"{}").unwrap();
        let facts = read_session_facts(root).unwrap();
        assert!(facts.thread_present);
        assert!(facts.thread_settled);
        assert!(!facts.last_turn_in_progress);
        assert_eq!(facts.active_operations.len(), 0);
    }

    #[test]
    fn recorded_native_binding_is_normalized_for_the_cli() {
        let owned = tempfile::tempdir().unwrap();
        let root = owned.path();
        fs::write(
            root.join("leader.json"),
            serde_json::to_vec(&json!({"schema":1,"threadId":"lead","model":"gpt-6-astra",
                "modelProvider":"fixture","native":{"model":"gpt-6-astra","modelProvider":"fixture",
                    "approvalPolicy":"never","sandbox":{"type":"dangerFullAccess"},
                    "cwd":root.to_string_lossy()}}))
            .unwrap(),
        )
        .unwrap();
        let binding = binding_from_leader(root).unwrap().unwrap();
        assert_eq!(binding.sandbox.as_deref(), Some("danger-full-access"));
        assert_eq!(binding.approval.as_deref(), Some("never"));
        assert_eq!(binding.cwd.as_deref(), Some(root));
        assert_eq!(sandbox_mode("readOnly").as_deref(), Some("read-only"));
        assert_eq!(
            sandbox_mode("workspaceWrite").as_deref(),
            Some("workspace-write")
        );
        assert_eq!(sandbox_mode("mystery"), None);
    }

    #[test]
    fn reload_verification_uses_the_successor_turn_evidence() {
        let rolled = |role: &str, text: &str| {
            json!({"type":"response_item","payload":{"type":"message","role":role,
                "content":[{"type":"input_text","text":text}]}})
            .to_string()
        };
        let session = "01a0bb1b-c508-7f83-9567-c916cfa1770c";
        let instruction = "# Project instructions\n\nMARKER=reload-9a1e\n";
        let expected = "- demo: Relay marker reload-9a1e. (file: r0/demo/SKILL.md)".to_string();
        let stale = "- demo: Relay marker seed-0000. (file: r0/demo/SKILL.md)".to_string();
        let marker = format!("{MARKER} {session} demo@rev-2");
        let good = [
            rolled("developer", "<skills_instructions>\n### Available skills\n"),
            rolled("user", &format!("# AGENTS.md instructions for C:\\ws\n\n<INSTRUCTIONS>\n{instruction}</INSTRUCTIONS>")),
            rolled("developer", &format!("<skills_instructions>\n### Available skills\n{expected}\n")),
            rolled("user", &marker),
        ]
        .join("\n");
        let expectation = ReloadExpectation {
            instruction_path: Path::new(r"C:\ws\AGENTS.md"),
            instruction_text: instruction,
            skill_name: "demo",
            skill_description: "Relay marker reload-9a1e.",
            session,
        };
        assert_eq!(verify_reload(&good, &expectation), Reload::Verified);

        let stale_skills = good.replace(&expected, &stale);
        assert!(matches!(
            verify_reload(&stale_skills, &expectation),
            Reload::NotVerified(_)
        ));
        let stale_instructions = [
            rolled("developer", "<skills_instructions>\n### Available skills\n"),
            rolled(
                "user",
                "# AGENTS.md instructions for C:\\ws\n\n<INSTRUCTIONS>\n# Old instructions\n\nMARKER=seed-0000\n</INSTRUCTIONS>",
            ),
            rolled(
                "developer",
                &format!("<skills_instructions>\n### Available skills\n{expected}\n"),
            ),
            rolled("user", &marker),
        ]
        .join("\n");
        assert!(matches!(
            verify_reload(&stale_instructions, &expectation),
            Reload::NotVerified(_)
        ));
        let no_turn = good.replace(&marker, "SUCCESSION_CONTINUATION other");
        assert!(matches!(
            verify_reload(&no_turn, &expectation),
            Reload::NotVerified(_)
        ));
        let mut truncated = good.clone();
        truncated.truncate(good.find(&expected).unwrap() + 5);
        assert!(matches!(
            verify_reload(&truncated, &expectation),
            Reload::NotVerified(_)
        ));
    }

    #[test]
    fn handover_record_carries_context_without_private_history() {
        let owned = tempfile::tempdir().unwrap();
        let root = owned.path();
        let request = request(root);
        let record = handover_record(
            &request,
            Some(&binding(root)),
            &profile_binding(),
            "confirmed",
            Some(Path::new(r"C:\state")),
        );
        assert_eq!(record["kind"], "instructionRefreshSuccession");
        assert_eq!(record["session"], request.session);
        assert_eq!(record["requirements"], "Deliver the result with checks");
        assert_eq!(record["authorization"]["sandbox"], "read-only");
        assert_eq!(record["authorization"]["approval"], "never");
        assert_eq!(record["revision"]["revision"], "rev-2");
        assert_eq!(record["modelCalls"], 0);
        let text = record.to_string();
        for history in ["visibleHistory", "aggregatedOutput", "encryptedContent"] {
            assert!(!text.contains(history), "no opaque history in handover");
        }
    }
}
