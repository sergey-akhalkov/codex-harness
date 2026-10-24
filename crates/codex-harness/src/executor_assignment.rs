//! Strict versioned structured executor assignments.
//!
//! An assignment file replaces free-text dispatch with declared facts: the
//! objective, the existing inputs it may rely on, the output files it owns,
//! its invariants, its acceptance conditions, the consumer of the returned
//! result and the escalation boundaries that return decisions to that
//! consumer. Validation is deterministic and model-free: the document, its
//! limits and every declared path are checked against the allocated checkout
//! before a model turn opens, and the rendered
//! brief carries the actual checkout, the committed base and the exact
//! relative paths.
//!
//! MCP dispatch and the compatibility CLI render the same small task contract:
//! the caller's literal text or the declared assignment, the named consumer,
//! the MCP escalation tool and the task-specific boundaries. Standing
//! responsibility guidance - the work cycle, base verification, the
//! heavy-command lane, when escalation is justified, the waiting behavior and
//! the compact result shape - is owned once by the executor role fragment and
//! reaches a managed session through its role context instead of being
//! restated in every brief.

use harness_core::{
    board_cli,
    build_identity::hash_bytes,
    delivery_reflection::{self, PURPOSE_METADATA, Purpose},
    task_worktree::{self, SlotAssignmentBinding},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeSet,
    fs, io,
    path::{Path, PathBuf},
    process::Command,
};

/// Version of the assignment document this build accepts.
pub const ASSIGNMENT_SCHEMA: u32 = 1;
/// The assignment file is bounded: its brief becomes the model input.
pub const MAX_ASSIGNMENT_BYTES: u64 = 256 * 1024;
pub const MAX_OBJECTIVE_BYTES: usize = 1024;
pub const MAX_LIST_ITEMS: usize = 64;
pub const MAX_ITEM_BYTES: usize = 512;
pub const MAX_RELATIVE_PATH_BYTES: usize = 240;
/// Bound on the rendered brief handed to the executor.
pub const MAX_BRIEF_BYTES: usize = 16 * 1024;
/// Bounds on the Lead-only change reference and the evidence retained for it.
pub const MAX_LEAD_REPOSITORIES: usize = 16;
pub const MAX_LEAD_EVIDENCE_BYTES: usize = 16 * 1024;

/// Who consumes the returned result when the assignment does not declare a
/// consumer: the lead that dispatched it is always the direct consumer.
pub const DEFAULT_CONSUMER: &str = "the lead that dispatched this assignment";

/// The one escalation channel every assignment names. The installed MCP tool
/// addresses the originating lead itself, so no recipient, slot, session,
/// checkout or endpoint is discovered, supplied or taught here.
pub const LEAD_CHANNEL_TOOL: &str = "executor_ask_lead";

/// The task-specific feedback addressing every assignment carries: who
/// consumes the returned result and the concrete MCP tool that
/// reaches them. The standing exchange rules around it - when escalation is
/// justified, the waiting behavior and the compact result shape - are owned
/// by the executor role fragment, which a managed session receives once, so
/// they are not restated here.
fn push_feedback_address(text: &mut String, consumer: &str) {
    text.push_str(&format!(
        "feedback: return the compact result through the native final response; escalate a decision you cannot resolve to {consumer} through the {LEAD_CHANNEL_TOOL} MCP tool.\n"
    ));
}

/// The free-text assignment one session receives: the caller's own text stays
/// first and literal, followed by the same small task contract every managed
/// assignment renders - the implicit consumer and the MCP escalation tool.
/// Continuation renders through this same function, and a fresh recovery keeps
/// the assignment it recorded, so no path needs a second copy of the standing
/// guidance.
pub fn free_text_brief(text: &str) -> String {
    let mut rendered = text.trim_end().to_owned();
    rendered.push_str("\n\n");
    push_feedback_address(&mut rendered, DEFAULT_CONSUMER);
    rendered
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Assignment {
    pub schema: u32,
    pub objective: String,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub invariants: Vec<String>,
    pub acceptance: Vec<String>,
    /// Consumer of the returned result; omitted means the dispatching lead.
    #[serde(default)]
    pub consumer: Option<String>,
    /// Owned task purpose. Omitted means unspecified rather than ordinary.
    #[serde(default)]
    pub purpose: Option<String>,
    /// The consuming repository's native board issue this assignment executes.
    /// It is the lead's exact issue id, never prose parsed from another field.
    /// Omitted assignments keep the legacy unbound dispatch path.
    #[serde(default)]
    pub board_task: Option<String>,
    /// Additional triggers that return the decision to the consumer; the
    /// standing boundaries apply in every case.
    #[serde(default)]
    pub escalate: Vec<String>,
    /// Lead-only reference to a prepared OpenSpec change. Executor
    /// assignments omit it and keep their existing contract and rendering.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lead_change: Option<LeadChange>,
}

/// The concrete specification-backed input a Lead assignment carries. A
/// planning home may differ from a code repository; both name the exact
/// committed revision they are read at.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LeadChange {
    /// Checkout-relative planning repository root containing `openspec/`.
    pub planning_root: String,
    /// Exact committed revision of the planning repository.
    pub base: String,
    /// Concrete change directory under the planning root, conventionally
    /// `openspec/changes/<change>`.
    pub directory: String,
    /// Selected code repositories and the exact committed base each Lead
    /// workspace is prepared at.
    #[serde(default)]
    pub repositories: Vec<LeadRepository>,
}

/// One selected code repository. `path` is relative to the allocated
/// checkout and must name that repository's root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LeadRepository {
    pub path: String,
    pub base: String,
}

/// Bounded, recordable facts established before a Lead allocation or model
/// turn. Paths are checkout-relative with `/` separators; bases are the exact
/// commits Git resolved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeadChangeEvidence {
    pub change: String,
    pub planning_root: String,
    pub planning_base: String,
    pub artifacts: Vec<String>,
    pub repositories: Vec<LeadRepositoryEvidence>,
    pub board_task: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeadRepositoryEvidence {
    pub path: String,
    pub base: String,
}

/// The actual dispatch context the brief names: the bound checkout, its
/// committed base, the session owner and the source checkout it came from.
pub struct AssignmentContext<'a> {
    pub checkout: &'a Path,
    pub base: &'a str,
    pub owner: &'a str,
    pub source: &'a Path,
}

impl Assignment {
    /// Validates one assignment already decoded from an MCP argument. The
    /// caller supplies literal structured data; no file path or runtime
    /// address is accepted here.
    pub(crate) fn from_value(value: serde_json::Value) -> io::Result<Self> {
        let assignment: Self = serde_json::from_value(value).map_err(|error| {
            invalid(&format!(
                "assignment is not a schema {ASSIGNMENT_SCHEMA} document: {error}"
            ))
        })?;
        assignment.validate()?;
        Ok(assignment)
    }

    /// Reads one assignment file and checks schema, limits and path syntax.
    /// Nothing here touches a checkout or a model.
    pub fn load(path: &Path) -> io::Result<Self> {
        let metadata = fs::metadata(path).map_err(|error| {
            invalid(&format!(
                "assignment file {} is unreadable: {error}",
                path.display()
            ))
        })?;
        if !metadata.is_file() {
            return Err(invalid(&format!(
                "assignment path {} is not a regular file",
                path.display()
            )));
        }
        if metadata.len() > MAX_ASSIGNMENT_BYTES {
            return Err(invalid(&format!(
                "assignment file {} is {} bytes; the limit is {MAX_ASSIGNMENT_BYTES}",
                path.display(),
                metadata.len()
            )));
        }
        let bytes = fs::read(path)?;
        let assignment: Self = serde_json::from_slice(&bytes).map_err(|error| {
            invalid(&format!(
                "assignment file {} is not a schema {ASSIGNMENT_SCHEMA} JSON document: {error}",
                path.display()
            ))
        })?;
        assignment.validate()?;
        Ok(assignment)
    }

    pub(crate) fn validate(&self) -> io::Result<()> {
        if self.schema != ASSIGNMENT_SCHEMA {
            return Err(invalid(&format!(
                "assignment schema {} is unsupported; this build accepts schema {ASSIGNMENT_SCHEMA}",
                self.schema
            )));
        }
        require_text("objective", &self.objective, MAX_OBJECTIVE_BYTES)?;
        require_text_list("invariants", &self.invariants)?;
        require_text_list("acceptance", &self.acceptance)?;
        require_text_list("escalate", &self.escalate)?;
        if let Some(consumer) = &self.consumer {
            require_text("consumer", consumer, MAX_ITEM_BYTES)?;
        }
        if let Some(purpose) = &self.purpose {
            require_text("purpose", purpose, MAX_ITEM_BYTES)?;
            Purpose::parse(purpose)?;
        }
        if let Some(task) = &self.board_task {
            require_text("board_task", task, MAX_ITEM_BYTES)?;
            if task.chars().any(char::is_whitespace) {
                return Err(invalid(
                    "board_task must be one exact board issue id without whitespace; it is never parsed from prose",
                ));
            }
        }
        if self.acceptance.is_empty() {
            return Err(invalid(
                "assignment acceptance is empty; name at least one checkable acceptance item",
            ));
        }
        require_paths("inputs", &self.inputs)?;
        require_paths("outputs", &self.outputs)?;
        if let Some(change) = &self.lead_change {
            change.validate()?;
        }
        Ok(())
    }

    /// Validates the declared paths against the allocated checkout. Existing
    /// inputs must be regular files inside the checkout; output paths may not
    /// exist yet, but every existing ancestor must stay inside the checkout,
    /// so traversal and symlink or reparse escapes are rejected before the
    /// dispatched session starts.
    pub fn validate_paths(&self, checkout: &Path) -> io::Result<()> {
        let root = checkout.canonicalize().map_err(|error| {
            invalid(&format!(
                "the allocated checkout {} cannot be resolved: {error}",
                checkout.display()
            ))
        })?;
        for (index, declared) in self.inputs.iter().enumerate() {
            let parts = relative_parts(&format!("inputs[{index}]"), declared)?;
            resolve_input(&root, declared, &parts)?;
        }
        for (index, declared) in self.outputs.iter().enumerate() {
            let parts = relative_parts(&format!("outputs[{index}]"), declared)?;
            resolve_output(&root, declared, &parts)?;
        }
        Ok(())
    }

    /// The content identity retained beside a consumer-board binding. It is
    /// the semantic assignment document after schema validation, so a resume
    /// can prove it continues the same declaration rather than a same-named
    /// task with different work.
    pub fn content_sha256(&self) -> io::Result<String> {
        Ok(hash_bytes(&serde_json::to_vec(self).map_err(|error| {
            io::Error::new(io::ErrorKind::InvalidData, error)
        })?))
    }
}

impl LeadChange {
    /// The checkout-independent part of a Lead reference: field bounds, plain
    /// relative paths, a concrete OpenSpec change directory and at least one
    /// distinct repository. Existence, commits and the board are established
    /// against the checkout by [`validate_lead_assignment`].
    fn validate(&self) -> io::Result<()> {
        relative_directory("lead_change.planning_root", &self.planning_root)?;
        require_text("lead_change.base", &self.base, MAX_ITEM_BYTES)?;
        require_revision_text("lead_change.base", &self.base)?;
        let parts = relative_parts("lead_change.directory", &self.directory)?;
        if parts.len() < 3 || parts[0] != "openspec" || parts[1] != "changes" {
            return Err(invalid(
                "lead_change.directory must name a concrete OpenSpec change directory under the planning root as openspec/changes/<change>; a title or board card alone is not a specification",
            ));
        }
        if self.repositories.is_empty() {
            return Err(invalid(
                "lead_change.repositories is empty; a Lead assignment names at least one code repository and its committed base",
            ));
        }
        if self.repositories.len() > MAX_LEAD_REPOSITORIES {
            return Err(invalid(&format!(
                "lead_change.repositories has {} entries; the limit is {MAX_LEAD_REPOSITORIES}",
                self.repositories.len()
            )));
        }
        let mut seen = BTreeSet::new();
        for (index, repository) in self.repositories.iter().enumerate() {
            let field = format!("lead_change.repositories[{index}]");
            let parts = relative_directory(&format!("{field}.path"), &repository.path)?;
            let normalized = normalized_relative(&parts);
            if !seen.insert(normalized.clone()) {
                return Err(invalid(&format!(
                    "assignment {field}.path repeats the already declared repository: {normalized}"
                )));
            }
            require_text(&format!("{field}.base"), &repository.base, MAX_ITEM_BYTES)?;
            require_revision_text(&format!("{field}.base"), &repository.base)?;
        }
        Ok(())
    }
}

/// Resolves the assignment's declared `board_task` in the consuming
/// repository's native board, before any slot claim or model effect. The
/// returned binding carries the canonical board scope, the exact issue, a
/// sha256 of this assignment document and the effective purpose. An explicit
/// purpose is the lead's declaration; a known nonordinary ancestor purpose is
/// inherited; contradictions and unavailable ancestors refuse. A task with no
/// known purpose stays explicitly unknown rather than becoming ordinary.
pub fn bind_board_task(
    assignment: &Assignment,
    repository: &Path,
    bd: &Path,
) -> io::Result<SlotAssignmentBinding> {
    let issue_id = assignment.board_task.as_deref().ok_or_else(|| {
        invalid("bind_board_task requires assignment.board_task; legacy assignments have no board binding")
    })?;
    if issue_id.trim().is_empty() || issue_id.chars().any(char::is_whitespace) {
        return Err(invalid(
            "board_task must be one exact board issue id without whitespace; it is never parsed from prose",
        ));
    }
    let before = delivery_reflection::board_source_identity(bd, repository)?;
    if !task_worktree::same_repository(Path::new(&before.repo_root), repository) {
        return Err(invalid(&format!(
            "board task '{issue_id}' resolved from board scope {} (repo_root {}), which is not the consuming repository {}; refusing a stale or foreign board binding",
            before.project_id,
            before.repo_root,
            repository.display()
        )));
    }
    let issue = board_cli::show_issue(bd, repository, issue_id)?;
    let own = issue_purpose(&issue)?;
    let inherited = inherited_nonordinary_purpose(bd, repository, &issue)?;
    let declared = assignment
        .purpose
        .as_deref()
        .map(Purpose::parse)
        .transpose()?;
    if let Some(declared) = declared
        && declared != Purpose::Ordinary
        && own.is_none()
        && inherited.is_none()
    {
        return Err(invalid(&format!(
            "assignment declares nonordinary purpose '{}' but board task '{issue_id}' has no board or ancestor purpose; nonordinary purposes are inherited, while only an ordinary declaration belongs to the lead",
            declared.as_str()
        )));
    }
    if let (Some(declared), Some(own)) = (declared, own)
        && declared != own
    {
        return Err(invalid(&format!(
            "assignment declares purpose '{}' but board task '{issue_id}' records purpose '{}'",
            declared.as_str(),
            own.as_str()
        )));
    }
    if let Some((ancestor, ancestor_id)) = inherited.as_ref() {
        if let Some(declared) = declared
            && declared != *ancestor
        {
            return Err(invalid(&format!(
                "assignment declares purpose '{}' but ancestor '{ancestor_id}' declares nonordinary purpose '{}'; a descendant does not silently leave its purpose lineage",
                declared.as_str(),
                ancestor.as_str()
            )));
        }
        if let Some(own) = own
            && own != *ancestor
        {
            return Err(invalid(&format!(
                "board task '{issue_id}' records purpose '{}' but ancestor '{ancestor_id}' declares nonordinary purpose '{}'",
                own.as_str(),
                ancestor.as_str()
            )));
        }
    }
    let (effective, purpose_source) = match (declared, own, inherited.as_ref()) {
        (Some(purpose), _, _) => (Some(purpose), "declared".to_owned()),
        (None, Some(purpose), _) => (Some(purpose), "board".to_owned()),
        (None, None, Some((purpose, ancestor_id))) => {
            (Some(*purpose), format!("ancestor:{ancestor_id}"))
        }
        (None, None, None) => (None, "unknown".to_owned()),
    };
    if let Some(effective) = effective
        && own.is_none()
    {
        let before_metadata = board_cli::issue_metadata(&issue);
        let refreshed = board_cli::set_purpose_metadata(bd, repository, issue_id, effective)?;
        let after_metadata = board_cli::issue_metadata(&refreshed);
        for (key, value) in &before_metadata {
            if key != PURPOSE_METADATA && after_metadata.get(key) != Some(value) {
                return Err(invalid(&format!(
                    "recording harness.purpose on board task '{issue_id}' changed unrelated metadata '{key}'; the dispatch is refused instead of trusting a rewritten task"
                )));
            }
        }
    }
    // The metadata write advances the board revision; the retained source
    // observation names the revision after the write. Scope churn during
    // resolution is a conflict, not a binding.
    let after = delivery_reflection::board_source_identity(bd, repository)?;
    if !before.same_scope(&after) {
        return Err(invalid(&format!(
            "board scope changed while resolving task '{issue_id}'; retry the dispatch"
        )));
    }
    let assignment_sha256 = assignment.content_sha256()?;
    Ok(SlotAssignmentBinding {
        source: after,
        issue: issue_id.to_owned(),
        assignment_sha256,
        purpose: effective.map(|purpose| purpose.as_str().to_owned()),
        purpose_source,
    })
}

fn issue_purpose(issue: &Value) -> io::Result<Option<Purpose>> {
    match board_cli::issue_metadata(issue).get(PURPOSE_METADATA) {
        Some(raw) => Ok(Some(Purpose::parse(raw)?)),
        None => Ok(None),
    }
}

/// The nearest nonordinary purpose declared anywhere in the parent chain.
/// Ordinary ancestor work is never inherited: a child does not become
/// ordinary merely because a task above it exists. Two different nonordinary
/// ancestor declarations refuse instead of choosing one.
fn inherited_nonordinary_purpose(
    bd: &Path,
    repository: &Path,
    issue: &Value,
) -> io::Result<Option<(Purpose, String)>> {
    let mut parent = issue
        .get("parent")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let mut seen = BTreeSet::new();
    let mut depth = 0usize;
    let mut found = None;
    while let Some(parent_id) = parent.take() {
        if depth >= delivery_reflection::MAX_ANCESTORS {
            return Err(invalid(&format!(
                "purpose lineage exceeds {} ancestors at '{parent_id}'",
                delivery_reflection::MAX_ANCESTORS
            )));
        }
        if !seen.insert(parent_id.clone()) {
            return Err(invalid(&format!(
                "purpose lineage repeats ancestor '{parent_id}'"
            )));
        }
        let ancestor = board_cli::show_issue(bd, repository, &parent_id).map_err(|error| {
            invalid(&format!(
                "board task ancestor '{parent_id}' is unavailable: {error}"
            ))
        })?;
        if let Some(purpose) = issue_purpose(&ancestor)?
            && purpose != Purpose::Ordinary
        {
            match &found {
                None => found = Some((purpose, parent_id.clone())),
                Some((existing, _)) if *existing != purpose => {
                    return Err(invalid(&format!(
                        "board task ancestors declare conflicting nonordinary purposes '{}' and '{}'",
                        existing.as_str(),
                        purpose.as_str()
                    )));
                }
                Some(_) => {}
            }
        }
        parent = ancestor
            .get("parent")
            .and_then(Value::as_str)
            .map(str::to_owned);
        depth += 1;
    }
    Ok(found)
}

/// Validates a specification-backed Lead assignment against the allocated
/// checkout before any slot is claimed or model turn is opened. It resolves
/// the OpenSpec change, requires the applicable planning artifacts at the
/// exact committed planning revision, checks every selected repository and
/// base, and reads the board reference without mutating it. The returned
/// bounded evidence is the value the dispatch owner retains.
///
/// Direct Executor assignments do not call this function and keep their
/// existing contract.
pub fn validate_lead_assignment(
    assignment: &Assignment,
    context: &AssignmentContext<'_>,
    bd: &Path,
) -> io::Result<LeadChangeEvidence> {
    assignment.validate()?;
    assignment.validate_paths(context.checkout)?;
    let change = assignment.lead_change.as_ref().ok_or_else(|| {
        invalid(
            "a Lead assignment requires lead_change naming a concrete OpenSpec change directory; a title or board card alone is not a specification",
        )
    })?;
    let board_task = assignment.board_task.as_deref().ok_or_else(|| {
        invalid(
            "a Lead assignment requires board_task naming the exact board issue it owns; a change reference alone does not supply board ownership",
        )
    })?;
    let root = context.checkout.canonicalize().map_err(|error| {
        invalid(&format!(
            "the allocated checkout {} cannot be resolved: {error}",
            context.checkout.display()
        ))
    })?;
    let planning_parts = relative_directory("lead_change.planning_root", &change.planning_root)?;
    let planning_root = resolve_declared_directory(
        &root,
        &planning_parts,
        "lead_change.planning_root",
        &change.planning_root,
        "checkout",
    )?;
    let change_parts = relative_parts("lead_change.directory", &change.directory)?;
    let change_dir = resolve_declared_directory(
        &planning_root,
        &change_parts,
        "lead_change.directory",
        &change.directory,
        "planning root",
    )?;

    let mut artifacts = Vec::new();
    let mut frozen = Vec::new();
    add_required_artifact(
        &change_dir,
        "proposal.md",
        &root,
        &mut artifacts,
        &mut frozen,
    )?;
    add_required_artifact(&change_dir, "tasks.md", &root, &mut artifacts, &mut frozen)?;
    let schema = change_schema(&change_dir, &planning_root, &root, &mut frozen)?;
    if schema == "spec-driven" || change_dir.join("design.md").is_file() {
        add_required_artifact(&change_dir, "design.md", &root, &mut artifacts, &mut frozen)?;
    }
    for spec in spec_artifacts(&change_dir, &root)? {
        artifacts.push(path_relative(&root, &spec)?);
        frozen.push(spec);
    }
    if artifacts.len() > MAX_LIST_ITEMS {
        return Err(invalid(&format!(
            "the change declares {} planning artifacts; the limit is {MAX_LIST_ITEMS}",
            artifacts.len()
        )));
    }

    let planning_repository = planning_repository(&planning_root, &root)?;
    let planning_base = resolve_commit(&planning_repository, &change.base, "lead_change.base")?;
    verify_committed(&planning_repository, &planning_base, &frozen)?;
    let repositories = validate_lead_repositories(&root, &change.repositories)?;

    let board = delivery_reflection::board_source_identity(bd, context.checkout).map_err(|error| {
        invalid(&format!(
            "Lead board reference '{board_task}' cannot be read from {}: {error}; the dispatch is refused before allocation or model work",
            context.checkout.display()
        ))
    })?;
    if !task_worktree::same_repository(Path::new(&board.repo_root), context.checkout) {
        return Err(invalid(&format!(
            "Lead board task '{board_task}' belongs to repository {}, not the allocated checkout {}; refusing a foreign board reference",
            board.repo_root,
            context.checkout.display()
        )));
    }
    board_cli::show_issue(bd, context.checkout, board_task).map_err(|error| {
        invalid(&format!(
            "Lead board task '{board_task}' cannot be resolved in {}: {error}",
            context.checkout.display()
        ))
    })?;

    let evidence = LeadChangeEvidence {
        change: path_relative(&root, &change_dir)?,
        planning_root: normalized_relative(&planning_parts),
        planning_base,
        artifacts,
        repositories,
        board_task: board_task.to_owned(),
    };
    let encoded = serde_json::to_vec(&evidence).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Lead change evidence cannot be encoded: {error}"),
        )
    })?;
    if encoded.len() > MAX_LEAD_EVIDENCE_BYTES {
        return Err(invalid(&format!(
            "the Lead change evidence is {} bytes; the limit is {MAX_LEAD_EVIDENCE_BYTES}",
            encoded.len()
        )));
    }
    Ok(evidence)
}

/// Validates the declared paths and renders the brief an executor session
/// receives. The same function serves dispatch and the model-free `executor
/// assignment` path, so a session-role caller can exercise the exact rendering
/// without a model request.
pub fn brief(assignment: &Assignment, context: &AssignmentContext<'_>) -> io::Result<String> {
    render_brief(
        assignment,
        context,
        "Structured executor assignment (schema 1)",
    )
}

/// Renders the same declared contract under the lead-role heading. A bounded
/// experimental lead conversation receives the identical brief - objective,
/// checkout, base, owned paths, checks, consumer, feedback address and
/// boundaries - and its heading names its own role instead of presenting the
/// conversation as an executor session.
pub fn lead_brief(assignment: &Assignment, context: &AssignmentContext<'_>) -> io::Result<String> {
    render_brief(assignment, context, "Structured lead assignment (schema 1)")
}

const FRAMING_CONTRACT_EXAMPLE: &str = r#"{
  "schema": 1,
  "objective": "State the exact bounded task outcome.",
  "inputs": [],
  "outputs": [],
  "invariants": ["Preserve the frozen task and checkout scope."],
  "acceptance": ["Name at least one checkable acceptance item."],
  "consumer": "qualification controller",
  "escalate": []
}"#;

/// The authoritative prompt contract for an ordinary lead's framing turn.
/// Both the initial request and its one bounded format correction render this
/// text, so schema facts and limits have one owner.
pub fn framing_contract() -> String {
    let mut text = String::new();
    text.push_str(
        "Return exactly one JSON object, with no prose or code fences. Assignment schema 1 is strict and rejects unknown fields.\n",
    );
    text.push_str(&format!(
        "- Required fields are \"schema\", \"objective\", \"inputs\", \"outputs\", \"invariants\", and \"acceptance\".\n\
         - Optional fields are \"consumer\", \"purpose\", \"board_task\", and \"escalate\".\n\
         - The complete document must be at most {MAX_ASSIGNMENT_BYTES} bytes; the rendered assignment brief must be at most {MAX_BRIEF_BYTES} bytes.\n\
         - \"objective\" is a non-empty string of at most {MAX_OBJECTIVE_BYTES} bytes.\n\
         - \"inputs\" and \"outputs\" must be JSON arrays of plain checkout-relative path strings, never objects. Each array has at most {MAX_LIST_ITEMS} entries, each path is at most {MAX_RELATIVE_PATH_BYTES} bytes, and paths must not be rooted, traverse with \"..\", contain \":\", name an empty file or repeat within their array.\n\
         - Every input must already exist as a regular file inside the checkout. An output may be new, but it must remain inside the checkout.\n\
         - For a read-only task, \"outputs\" must be the empty array [].\n\
         - \"invariants\", \"acceptance\", and \"escalate\" must be arrays of strings. Each array has at most {MAX_LIST_ITEMS} entries and each string is at most {MAX_ITEM_BYTES} bytes; \"acceptance\" must contain at least one item.\n\
         - When present, \"consumer\", \"purpose\", and \"board_task\" are non-empty strings of at most {MAX_ITEM_BYTES} bytes; \"purpose\" is exactly \"ordinary\", \"reflection\" or \"improvement\"; \"board_task\" is one exact issue id without whitespace.\n\
         \nExample shape (the empty arrays are a read-only default; replace values with the actual task facts):\n"
    ));
    text.push_str(FRAMING_CONTRACT_EXAMPLE);
    text.push('\n');
    text
}

fn render_brief(
    assignment: &Assignment,
    context: &AssignmentContext<'_>,
    heading: &str,
) -> io::Result<String> {
    assignment.validate_paths(context.checkout)?;
    let consumer = consumer_text(assignment);
    let purpose = assignment
        .purpose
        .as_deref()
        .map(Purpose::parse)
        .transpose()?;
    let boundaries = assignment
        .escalate
        .iter()
        .map(|item| item.trim().to_owned())
        .collect::<Vec<_>>();
    let mut text = String::new();
    text.push_str(heading);
    text.push('\n');
    text.push_str(&format!("objective: {}\n", assignment.objective.trim()));
    text.push_str(&format!("checkout: {}\n", context.checkout.display()));
    text.push_str(&format!("base: {}\n", context.base));
    text.push_str(&format!("source: {}\n", context.source.display()));
    text.push_str(&format!("owner: {}\n", context.owner));
    text.push_str(&format!("consumer: {consumer}\n"));
    if let Some(purpose) = purpose {
        text.push_str(&format!("purpose: {}\n", purpose.as_str()));
    }
    push_list(
        &mut text,
        "inputs (declared; verified regular files inside the checkout)",
        &assignment.inputs,
    );
    push_list(
        &mut text,
        "outputs (owned by this assignment; new files are allowed)",
        &assignment.outputs,
    );
    push_list(&mut text, "invariants", &assignment.invariants);
    push_list(&mut text, "acceptance", &assignment.acceptance);
    push_list(
        &mut text,
        &format!("escalate to {consumer} (additional boundaries)"),
        &boundaries,
    );
    push_feedback_address(&mut text, consumer);
    if text.len() > MAX_BRIEF_BYTES {
        return Err(invalid(&format!(
            "the rendered assignment brief is {} bytes; the limit is {MAX_BRIEF_BYTES}",
            text.len()
        )));
    }
    Ok(text)
}

fn add_required_artifact(
    change_dir: &Path,
    name: &str,
    checkout: &Path,
    artifacts: &mut Vec<String>,
    frozen: &mut Vec<PathBuf>,
) -> io::Result<()> {
    let resolved = resolve_planning_file(&change_dir.join(name), checkout, name)?;
    artifacts.push(path_relative(checkout, &resolved)?);
    frozen.push(resolved);
    Ok(())
}

/// Resolves a declared relative directory, rejecting traversal, missing
/// paths and symlink escapes outside `root`.
fn resolve_declared_directory(
    root: &Path,
    parts: &[String],
    field: &str,
    declared: &str,
    scope: &str,
) -> io::Result<PathBuf> {
    let joined = joined_path(root, parts);
    match fs::symlink_metadata(&joined) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(invalid(&format!(
                "{field} {declared} is missing from the checkout at {}; prepare it before dispatch",
                joined.display()
            )));
        }
        Err(error) => {
            return Err(invalid(&format!(
                "{field} {declared} cannot be inspected at {}: {error}",
                joined.display()
            )));
        }
        Ok(_) => {}
    }
    let resolved = fs::canonicalize(&joined).map_err(|error| {
        invalid(&format!(
            "{field} {declared} cannot be resolved at {}: {error}",
            joined.display()
        ))
    })?;
    if !resolved.starts_with(root) {
        return Err(invalid(&format!(
            "{field} {declared} resolves outside the declared {scope} to {}; the assignment is rejected without reading it",
            resolved.display()
        )));
    }
    let metadata = fs::metadata(&resolved).map_err(|error| {
        invalid(&format!(
            "{field} {declared} is unreadable at {}: {error}",
            resolved.display()
        ))
    })?;
    if !metadata.is_dir() {
        return Err(invalid(&format!(
            "{field} {declared} is not a directory at {}",
            resolved.display()
        )));
    }
    Ok(resolved)
}

/// Resolves one existing, non-empty planning artifact and proves it stays
/// inside the allocated checkout.
fn resolve_planning_file(path: &Path, checkout: &Path, label: &str) -> io::Result<PathBuf> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(invalid(&format!(
                "Lead planning artifact {label} is missing at {}; prepare the change before Lead dispatch",
                path.display()
            )));
        }
        Err(error) => {
            return Err(invalid(&format!(
                "Lead planning artifact {label} cannot be inspected at {}: {error}",
                path.display()
            )));
        }
        Ok(_) => {}
    }
    let resolved = fs::canonicalize(path).map_err(|error| {
        invalid(&format!(
            "Lead planning artifact {label} cannot be resolved at {}: {error}",
            path.display()
        ))
    })?;
    if !resolved.starts_with(checkout) {
        return Err(invalid(&format!(
            "Lead planning artifact {label} resolves outside the checkout to {}; the assignment is rejected without reading it",
            resolved.display()
        )));
    }
    let metadata = fs::metadata(&resolved).map_err(|error| {
        invalid(&format!(
            "Lead planning artifact {label} is unreadable at {}: {error}",
            resolved.display()
        ))
    })?;
    if !metadata.is_file() {
        return Err(invalid(&format!(
            "Lead planning artifact {label} is not a regular file at {}",
            resolved.display()
        )));
    }
    if metadata.len() == 0 {
        return Err(invalid(&format!(
            "Lead planning artifact {label} is empty at {}; a prepared change carries content",
            resolved.display()
        )));
    }
    Ok(resolved)
}

/// Determines the change's OpenSpec schema. The change-local file wins, then
/// the planning root's config; the default schema requires a design.
///
/// shortcut: only a top-level `schema:` line is read, upgrade if OpenSpec
/// stores schema selection in nested metadata.
fn change_schema(
    change_dir: &Path,
    planning_root: &Path,
    checkout: &Path,
    frozen: &mut Vec<PathBuf>,
) -> io::Result<String> {
    for path in [
        change_dir.join(".openspec.yaml"),
        planning_root.join("openspec/config.yaml"),
    ] {
        if !path.is_file() {
            continue;
        }
        let resolved = resolve_planning_file(&path, checkout, "change schema")?;
        let text = fs::read_to_string(&resolved).map_err(|error| {
            invalid(&format!(
                "change schema {} cannot be read: {error}",
                resolved.display()
            ))
        })?;
        frozen.push(resolved);
        if let Some(schema) = top_level_schema(&text) {
            return Ok(schema);
        }
    }
    Ok("spec-driven".to_owned())
}

fn top_level_schema(text: &str) -> Option<String> {
    for line in text.lines() {
        let Some(value) = line.trim().strip_prefix("schema:") else {
            continue;
        };
        let value = value.split('#').next().unwrap_or_default().trim();
        let value = value.trim_matches(['"', '\'']);
        if !value.is_empty() {
            return Some(value.to_owned());
        }
    }
    None
}

fn spec_artifacts(change_dir: &Path, checkout: &Path) -> io::Result<Vec<PathBuf>> {
    let specs = change_dir.join("specs");
    let entries = fs::read_dir(&specs).map_err(|error| {
        invalid(&format!(
            "the change at {} has no readable specs directory for specs/*/spec.md: {error}",
            change_dir.display()
        ))
    })?;
    let mut found = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| {
            invalid(&format!(
                "the change specs directory {} cannot be read: {error}",
                specs.display()
            ))
        })?;
        if !entry
            .file_type()
            .map_err(|error| {
                invalid(&format!(
                    "cannot inspect {}: {error}",
                    entry.path().display()
                ))
            })?
            .is_dir()
        {
            continue;
        }
        let spec = entry.path().join("spec.md");
        if !spec.exists() {
            continue;
        }
        let label = format!("specs/{}/spec.md", entry.file_name().to_string_lossy());
        found.push(resolve_planning_file(&spec, checkout, &label)?);
        if found.len() >= MAX_LIST_ITEMS {
            return Err(invalid(&format!(
                "the change at {} declares at least {MAX_LIST_ITEMS} capability specifications; the limit is {MAX_LIST_ITEMS}",
                change_dir.display()
            )));
        }
    }
    found.sort();
    if found.is_empty() {
        return Err(invalid(&format!(
            "the change at {} has no non-empty specs/*/spec.md; a proposal and tasks alone are not a complete specification",
            change_dir.display()
        )));
    }
    Ok(found)
}

/// The repository that owns the planning root. A planning home may differ
/// from a selected code repository, but its committed inputs must still be
/// bound inside the allocated checkout.
fn planning_repository(planning_root: &Path, checkout: &Path) -> io::Result<PathBuf> {
    let output = run_git(planning_root, &["rev-parse", "--show-toplevel"])?;
    if !output.status.success() {
        return Err(invalid(&format!(
            "lead_change.planning_root {} is not inside a Git repository: {}",
            planning_root.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let top = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
    let top = fs::canonicalize(&top).map_err(|error| {
        invalid(&format!(
            "the planning repository {} cannot be resolved: {error}",
            top.display()
        ))
    })?;
    if !top.starts_with(checkout) {
        return Err(invalid(&format!(
            "the planning repository {} is outside the allocated checkout {}; a planning home must remain a declared local input",
            top.display(),
            checkout.display()
        )));
    }
    Ok(top)
}

/// Resolves one declared base to the exact commit Git names. Nonzero or
/// unparsable answers are refusals, never silent fallbacks.
fn resolve_commit(repository: &Path, reference: &str, field: &str) -> io::Result<String> {
    let reference = reference.trim();
    let output = run_git(
        repository,
        &["rev-parse", "--verify", &format!("{reference}^{{commit}}")],
    )?;
    if !output.status.success() {
        return Err(invalid(&format!(
            "{field} '{reference}' is not a committed revision in {}; name an exact commit that exists there",
            repository.display()
        )));
    }
    let commit = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if !matches!(commit.len(), 40 | 64) || !commit.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(invalid(&format!(
            "{field} '{reference}' did not resolve to a commit in {}: {commit}",
            repository.display()
        )));
    }
    Ok(commit)
}

/// Proves every planning artifact is present with content at the exact
/// planning base and that the allocated checkout still carries those same
/// bytes. A later unannounced edit is refused rather than silently redefining
/// the Lead's scope.
fn verify_committed(repository: &Path, base: &str, paths: &[PathBuf]) -> io::Result<()> {
    let mut relatives = Vec::with_capacity(paths.len());
    for path in paths {
        let relative = path.strip_prefix(repository).map_err(|_| {
            invalid(&format!(
                "planning artifact {} is outside the planning repository {}; the planning base cannot bind it",
                path.display(),
                repository.display()
            ))
        })?;
        let relative = slash_path(relative);
        if relative.is_empty() {
            return Err(invalid(
                "a planning artifact resolves to the planning repository root instead of a file",
            ));
        }
        let size = run_git(
            repository,
            &["cat-file", "-s", &format!("{base}:{relative}")],
        )?;
        if !size.status.success() {
            return Err(invalid(&format!(
                "planning artifact {relative} is not committed at planning base {base} in {}; commit the prepared change or name its exact committed revision",
                repository.display()
            )));
        }
        let bytes = String::from_utf8_lossy(&size.stdout)
            .trim()
            .parse::<u64>()
            .map_err(|error| {
                invalid(&format!(
                    "planning artifact {relative} has no readable size at base {base}: {error}"
                ))
            })?;
        if bytes == 0 {
            return Err(invalid(&format!(
                "planning artifact {relative} is empty at planning base {base}; a prepared change carries content"
            )));
        }
        relatives.push(relative);
    }

    let mut args = vec!["diff", "--quiet", base, "--"];
    args.extend(relatives.iter().map(String::as_str));
    let diff = run_git(repository, &args)?;
    if diff.status.code() == Some(1) {
        return Err(invalid(&format!(
            "planning artifacts in {} differ from committed base {base}; the checkout has uncommitted or changed planning inputs. Commit the exact inputs or dispatch the committed revision",
            repository.display()
        )));
    }
    if !diff.status.success() {
        return Err(invalid(&format!(
            "planning artifacts in {} cannot be compared with base {base}: {}",
            repository.display(),
            String::from_utf8_lossy(&diff.stderr).trim()
        )));
    }
    Ok(())
}

fn validate_lead_repositories(
    checkout: &Path,
    repositories: &[LeadRepository],
) -> io::Result<Vec<LeadRepositoryEvidence>> {
    let mut seen = BTreeSet::new();
    let mut evidence = Vec::with_capacity(repositories.len());
    for (index, repository) in repositories.iter().enumerate() {
        let field = format!("lead_change.repositories[{index}]");
        let parts = relative_directory(&format!("{field}.path"), &repository.path)?;
        let joined = joined_path(checkout, &parts);
        let resolved = fs::canonicalize(&joined).map_err(|error| {
            invalid(&format!(
                "{field}.path {} is missing from the checkout: {error}",
                repository.path
            ))
        })?;
        if !resolved.starts_with(checkout) {
            return Err(invalid(&format!(
                "{field}.path {} resolves outside the checkout to {}",
                repository.path,
                resolved.display()
            )));
        }
        let metadata = fs::metadata(&resolved).map_err(|error| {
            invalid(&format!(
                "{field}.path {} is unreadable: {error}",
                resolved.display()
            ))
        })?;
        if !metadata.is_dir() {
            return Err(invalid(&format!(
                "{field}.path {} is not a directory",
                resolved.display()
            )));
        }
        if !seen.insert(resolved.clone()) {
            return Err(invalid(&format!(
                "{field}.path {} resolves to a repository already declared by another entry; declare it once",
                repository.path
            )));
        }
        let top = run_git(&resolved, &["rev-parse", "--show-toplevel"])?;
        if !top.status.success() {
            return Err(invalid(&format!(
                "{field}.path {} is not a Git repository: {}",
                resolved.display(),
                String::from_utf8_lossy(&top.stderr).trim()
            )));
        }
        let top = PathBuf::from(String::from_utf8_lossy(&top.stdout).trim());
        let top = fs::canonicalize(&top).map_err(|error| {
            invalid(&format!(
                "{field}.path {} has no resolvable repository root: {error}",
                resolved.display()
            ))
        })?;
        if top != resolved {
            return Err(invalid(&format!(
                "{field}.path {} is not the repository root; name {} instead",
                resolved.display(),
                top.display()
            )));
        }
        let base = resolve_commit(&resolved, &repository.base, &format!("{field}.base"))?;
        evidence.push(LeadRepositoryEvidence {
            path: normalized_relative(&parts),
            base,
        });
    }
    Ok(evidence)
}

fn run_git(cwd: &Path, args: &[&str]) -> io::Result<std::process::Output> {
    Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .map_err(|error| {
            invalid(&format!(
                "git {} at {} cannot start: {error}",
                args.join(" "),
                cwd.display()
            ))
        })
}

fn normalized_relative(parts: &[String]) -> String {
    if parts.is_empty() {
        ".".to_owned()
    } else {
        parts.join("/")
    }
}

fn path_relative(root: &Path, path: &Path) -> io::Result<String> {
    let relative = path.strip_prefix(root).map_err(|_| {
        invalid(&format!(
            "{} is outside the allocated checkout {}",
            path.display(),
            root.display()
        ))
    })?;
    let relative = slash_path(relative);
    if relative.is_empty() {
        return Err(invalid(&format!(
            "{} names the checkout root instead of a file or change directory",
            path.display()
        )));
    }
    Ok(relative)
}

fn slash_path(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn require_revision_text(field: &str, value: &str) -> io::Result<()> {
    if value.contains('\0') || value.chars().any(char::is_whitespace) {
        return Err(invalid(&format!(
            "assignment {field} must be one exact Git revision without whitespace"
        )));
    }
    if value.trim_start().starts_with('-') {
        return Err(invalid(&format!(
            "assignment {field} must be a Git revision, not an option: {value}"
        )));
    }
    Ok(())
}

/// The declared consumer, or the dispatching lead when the field is omitted
/// or blank. `validate` rejects a declared blank value, and the fallback keeps
/// a directly constructed assignment rendering a named consumer.
fn consumer_text(assignment: &Assignment) -> &str {
    assignment
        .consumer
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_CONSUMER)
}

fn push_list(text: &mut String, title: &str, values: &[String]) {
    if values.is_empty() {
        text.push_str(&format!("{title}: none declared\n"));
        return;
    }
    text.push_str(&format!("{title}:\n"));
    for value in values {
        text.push_str(&format!("- {}\n", value.trim()));
    }
}

fn resolve_input(root: &Path, declared: &str, parts: &[String]) -> io::Result<()> {
    let joined = joined_path(root, parts);
    match fs::symlink_metadata(&joined) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Err(invalid(&format!(
            "declared input {declared} is missing from the checkout: {} does not exist; correct the assignment or the checkout before dispatching",
            joined.display()
        ))),
        Err(error) => Err(invalid(&format!(
            "declared input {declared} cannot be inspected at {}: {error}",
            joined.display()
        ))),
        Ok(_) => {
            let resolved = fs::canonicalize(&joined).map_err(|error| {
                invalid(&format!(
                    "declared input {declared} cannot be resolved: {error}"
                ))
            })?;
            if !resolved.starts_with(root) {
                return Err(invalid(&format!(
                    "declared input {declared} resolves outside the checkout to {}; the assignment is rejected without reading the target",
                    resolved.display()
                )));
            }
            let metadata = fs::metadata(&resolved).map_err(|error| {
                invalid(&format!("declared input {declared} is unreadable: {error}"))
            })?;
            if !metadata.is_file() {
                return Err(invalid(&format!(
                    "declared input {declared} is not a regular file"
                )));
            }
            Ok(())
        }
    }
}

/// Walks the existing prefix of an output path. Every entry that exists is
/// resolved with links followed and must land inside the checkout; the first
/// missing component ends the walk, because the rest of the path is new.
fn resolve_output(root: &Path, declared: &str, parts: &[String]) -> io::Result<()> {
    let mut current = root.to_path_buf();
    for (index, part) in parts.iter().enumerate() {
        current.push(part);
        match fs::symlink_metadata(&current) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => {
                return Err(invalid(&format!(
                    "declared output {declared} cannot be inspected at {}: {error}",
                    current.display()
                )));
            }
            Ok(_) => {
                let resolved = fs::canonicalize(&current).map_err(|error| {
                    invalid(&format!(
                        "declared output {declared} has an unresolvable entry at {}: {error}",
                        current.display()
                    ))
                })?;
                if !resolved.starts_with(root) {
                    return Err(invalid(&format!(
                        "declared output {declared} resolves outside the checkout through {}; the assignment is rejected",
                        current.display()
                    )));
                }
                let last = index + 1 == parts.len();
                if !last && !resolved.is_dir() {
                    return Err(invalid(&format!(
                        "declared output {declared} has an existing non-directory ancestor {}",
                        current.display()
                    )));
                }
                if last && resolved.is_dir() {
                    return Err(invalid(&format!(
                        "declared output {declared} is an existing directory; name the file to write"
                    )));
                }
                current = resolved;
            }
        }
    }
    Ok(())
}

fn joined_path(root: &Path, parts: &[String]) -> PathBuf {
    let mut path = root.to_path_buf();
    for part in parts {
        path.push(part);
    }
    path
}

fn require_text(field: &str, value: &str, max: usize) -> io::Result<()> {
    if value.trim().is_empty() {
        return Err(invalid(&format!("assignment {field} is empty")));
    }
    if value.len() > max {
        return Err(invalid(&format!(
            "assignment {field} is {} bytes; the limit is {max}",
            value.len()
        )));
    }
    Ok(())
}

fn require_text_list(field: &str, values: &[String]) -> io::Result<()> {
    if values.len() > MAX_LIST_ITEMS {
        return Err(invalid(&format!(
            "assignment {field} has {} entries; the limit is {MAX_LIST_ITEMS}",
            values.len()
        )));
    }
    for (index, value) in values.iter().enumerate() {
        require_text(&format!("{field}[{index}]"), value, MAX_ITEM_BYTES)?;
    }
    Ok(())
}

fn require_paths(field: &str, values: &[String]) -> io::Result<()> {
    if values.len() > MAX_LIST_ITEMS {
        return Err(invalid(&format!(
            "assignment {field} has {} paths; the limit is {MAX_LIST_ITEMS}",
            values.len()
        )));
    }
    let mut seen = BTreeSet::new();
    for (index, value) in values.iter().enumerate() {
        let name = format!("{field}[{index}]");
        let parts = relative_parts(&name, value)?;
        if !seen.insert(parts.join("/")) {
            return Err(invalid(&format!(
                "assignment {name} repeats an already declared path: {value}"
            )));
        }
    }
    Ok(())
}

/// Splits a declared path into plain relative components. Both separators are
/// accepted so a declaration reads the same in either convention; rooted,
/// traversing, drive-relative and alternate-stream names are rejected before
/// any filesystem lookup.
fn relative_parts(field: &str, value: &str) -> io::Result<Vec<String>> {
    split_relative(field, value, false)
}

/// The same lexical rules for a directory that may name the checkout root.
fn relative_directory(field: &str, value: &str) -> io::Result<Vec<String>> {
    split_relative(field, value, true)
}

fn split_relative(field: &str, value: &str, allow_current: bool) -> io::Result<Vec<String>> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(invalid(&format!("assignment {field} is empty")));
    }
    if value.len() > MAX_RELATIVE_PATH_BYTES {
        return Err(invalid(&format!(
            "assignment {field} is {} bytes; the limit is {MAX_RELATIVE_PATH_BYTES}",
            value.len()
        )));
    }
    if value.contains('\0') {
        return Err(invalid(&format!(
            "assignment {field} contains a NUL character"
        )));
    }
    if trimmed.starts_with('/') || trimmed.starts_with('\\') {
        return Err(invalid(&format!(
            "assignment {field} must be relative to the checkout, not rooted: {value}"
        )));
    }
    let mut parts = Vec::new();
    let mut saw_current = false;
    for part in trimmed.split(['/', '\\']) {
        if part.is_empty() {
            continue;
        }
        if part == "." {
            saw_current = true;
            continue;
        }
        if part == ".." {
            return Err(invalid(&format!(
                "assignment {field} must not traverse with '..': {value}"
            )));
        }
        if part.contains(':') {
            return Err(invalid(&format!(
                "assignment {field} must be a plain relative path without ':': {value}"
            )));
        }
        parts.push(part.to_owned());
    }
    if parts.is_empty() && !(allow_current && saw_current) {
        return Err(invalid(&format!(
            "assignment {field} names no file: {value}"
        )));
    }
    Ok(parts)
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn document(objective: &str, inputs: &[&str], outputs: &[&str]) -> String {
        serde_json::to_string_pretty(&serde_json::json!({
            "schema": 1,
            "objective": objective,
            "inputs": inputs,
            "outputs": outputs,
            "invariants": ["keep the change inside the checkout"],
            "acceptance": ["the named check passes"],
        }))
        .unwrap()
    }

    fn temp_root(name: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("executor-assignment-{name}-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn checked_in(root: &Path, name: &str, document: &str) -> Assignment {
        let path = root.join(name);
        fs::write(&path, document).unwrap();
        Assignment::load(&path).unwrap()
    }

    /// The escalation wording an earlier renderer used. The rule must leave
    /// rather than grow beside the final contract, so the renderer keeps no
    /// copy.
    const SUPERSEDED_ESCALATION_HEADING: &str = "escalate to {consumer} (everything else, including ordinary implementation errors, is yours to resolve)";

    /// The exact feedback addressing a brief must render: the named consumer
    /// and the concrete installed command, so a dispatched session knows where
    /// a result or an escalated decision returns.
    fn expected_feedback_line(consumer: &str) -> String {
        format!(
            "feedback: return the compact result through the native final response; escalate a decision you cannot resolve to {consumer} through the executor_ask_lead MCP tool.\n"
        )
    }

    /// Standing exchange guidance owned by the executor role fragment. The
    /// role file carries these obligations; the assignment renderer must not
    /// restate them in any brief.
    const ROLE_OWNED_STANDING_PHRASES: [&str; 9] = [
        "It addresses the originating lead without a recipient",
        "returns a stable request id",
        "Ask only after investigating the available facts",
        "Ordinary local errors and routine progress remain yours",
        "durable blockers, decisions and results stay on the bd issue",
        "no polling, keep-alive loop or resume",
        "An action-required question is not completion",
        "report a compact result: done and remaining work",
        "Do not invent a transport file",
    ];

    /// The small task contract every brief renders with no standing exchange
    /// guidance: the role fragment is the single owner of the standing rules.
    fn assert_small_task_contract(text: &str, consumer: &str) {
        assert!(text.contains(&expected_feedback_line(consumer)), "{text}");
        for standing in ROLE_OWNED_STANDING_PHRASES {
            assert!(!text.contains(standing), "{standing}\n{text}");
        }
    }

    #[test]
    fn schema_version_fields_and_limits_are_strict() {
        let root = temp_root("strict");
        let path = root.join("assignment.json");
        fs::write(&path, document("Do the work", &["input.txt"], &["out.txt"])).unwrap();
        fs::write(root.join("input.txt"), "input\n").unwrap();
        let assignment = Assignment::load(&path).unwrap();
        assert_eq!(assignment.schema, 1);
        assignment.validate_paths(&root).unwrap();

        let mut unknown: serde_json::Value =
            serde_json::from_str(&document("Do the work", &[], &[])).unwrap();
        unknown["extra"] = serde_json::json!(true);
        fs::write(&path, serde_json::to_string(&unknown).unwrap()).unwrap();
        assert!(
            Assignment::load(&path)
                .unwrap_err()
                .to_string()
                .contains("unknown field")
        );

        let mut unsupported: serde_json::Value =
            serde_json::from_str(&document("Do the work", &[], &[])).unwrap();
        unsupported["schema"] = serde_json::json!(2);
        fs::write(&path, serde_json::to_string(&unsupported).unwrap()).unwrap();
        assert!(
            Assignment::load(&path)
                .unwrap_err()
                .to_string()
                .contains("schema 2 is unsupported")
        );

        let mut missing: serde_json::Value =
            serde_json::from_str(&document("Do the work", &[], &[])).unwrap();
        missing.as_object_mut().unwrap().remove("acceptance");
        fs::write(&path, serde_json::to_string(&missing).unwrap()).unwrap();
        assert!(
            Assignment::load(&path)
                .unwrap_err()
                .to_string()
                .contains("missing field")
        );

        let text = document(&"x".repeat(MAX_OBJECTIVE_BYTES + 1), &[], &[]);
        fs::write(&path, text).unwrap();
        assert!(
            Assignment::load(&path)
                .unwrap_err()
                .to_string()
                .contains("the limit is")
        );

        for declared in ["..\\outside.txt", "C:\\outside.txt", "/outside.txt", ""] {
            let text = document("Do the work", &[declared], &[]);
            fs::write(&path, text).unwrap();
            assert!(
                Assignment::load(&path).is_err(),
                "{declared} must be rejected"
            );
        }
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn inputs_must_exist_and_outputs_may_be_new() {
        let root = temp_root("paths");
        fs::create_dir_all(root.join("crates")).unwrap();
        fs::write(root.join("crates/input.rs"), "fn main() {}\n").unwrap();
        let assignment = checked_in(
            &root,
            "assignment.json",
            &document(
                "Extend the module",
                &["crates/input.rs"],
                &["crates/new/module.rs"],
            ),
        );
        assignment.validate_paths(&root).unwrap();

        let missing = checked_in(
            &root,
            "missing.json",
            &document("Extend the module", &["crates/absent.rs"], &[]),
        );
        let error = missing.validate_paths(&root).unwrap_err().to_string();
        assert!(error.contains("crates/absent.rs"), "{error}");
        assert!(error.contains("is missing"), "{error}");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn the_brief_names_the_checkout_base_and_exact_paths() {
        let root = temp_root("brief");
        fs::write(root.join("input.txt"), "input\n").unwrap();
        let assignment = checked_in(
            &root,
            "assignment.json",
            &document("Ship the outcome", &["input.txt"], &["out/result.txt"]),
        );
        let source = root.join("source");
        let text = brief(
            &assignment,
            &AssignmentContext {
                checkout: &root,
                base: "0123456789abcdef0123456789abcdef01234567",
                owner: "exec-ds-7",
                source: &source,
            },
        )
        .unwrap();
        assert!(text.contains("Structured executor assignment (schema 1)"));
        assert!(text.contains(&format!("checkout: {}", root.display())));
        assert!(text.contains("base: 0123456789abcdef0123456789abcdef01234567"));
        assert!(text.contains("owner: exec-ds-7"));
        assert!(text.contains("- input.txt"));
        assert!(text.contains("- out/result.txt"));
        assert!(text.contains("- the named check passes"));
        assert!(text.contains(&format!("source: {}", source.display())));
        assert_eq!(text.matches("source:").count(), 1, "{text}");
        // The consumer and the concrete feedback address are part of the
        // rendered contract, not of the caller's prose. The standing exchange
        // guidance around them lives in the executor role fragment and is not
        // restated in the brief.
        assert!(
            text.contains(&format!("consumer: {DEFAULT_CONSUMER}")),
            "{text}"
        );
        assert_small_task_contract(&text, DEFAULT_CONSUMER);
        // The standing work-cycle, base-verification and heavy-command text is
        // not restated in the brief: a managed session receives it once
        // through its role context.
        for standing in [
            "work cycle (yours)",
            "verify the checkout is at the base above",
            "Run heavy builds and checks through codex-harness heavy",
        ] {
            assert!(!text.contains(standing), "{standing}\n{text}");
        }
        let _ = fs::remove_dir_all(root);
    }

    /// The bounded lead conversation receives the same declared contract
    /// under its own heading: one rendering path serves both roles, only the
    /// first line names the role the conversation actually runs as.
    #[test]
    fn the_lead_brief_keeps_the_contract_under_its_own_heading() {
        let root = temp_root("lead-brief");
        fs::write(root.join("input.txt"), "input\n").unwrap();
        let assignment = checked_in(
            &root,
            "assignment.json",
            &document("Bound the lead route", &["input.txt"], &["out/result.txt"]),
        );
        let context = AssignmentContext {
            checkout: &root,
            base: "0123456789abcdef0123456789abcdef01234567",
            owner: "lead-exp-1",
            source: &root,
        };
        let lead = lead_brief(&assignment, &context).unwrap();
        let executor = brief(&assignment, &context).unwrap();
        assert!(
            lead.starts_with("Structured lead assignment (schema 1)\n"),
            "{lead}"
        );
        assert_eq!(
            lead.strip_prefix("Structured lead assignment (schema 1)\n"),
            executor.strip_prefix("Structured executor assignment (schema 1)\n"),
            "the contract below the heading is identical"
        );
        assert!(lead.contains("owner: lead-exp-1"), "{lead}");
        assert!(lead.contains("- input.txt"), "{lead}");
        assert!(lead.contains("- out/result.txt"), "{lead}");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn consumer_and_escalation_are_optional_and_validated() {
        let root = temp_root("contract-fields");
        fs::write(root.join("input.txt"), "input\n").unwrap();
        let source = root.join("source");
        let path = root.join("assignment.json");
        let context = AssignmentContext {
            checkout: &root,
            base: "0123456789abcdef0123456789abcdef01234567",
            owner: "exec-ds-7",
            source: &source,
        };

        // Omission keeps a schema-1 document valid: the dispatching lead
        // consumes the result and the standing boundaries still apply.
        let minimal = checked_in(
            &root,
            "assignment.json",
            &document("Ship the outcome", &["input.txt"], &[]),
        );
        assert_eq!(minimal.consumer, None);
        assert!(minimal.escalate.is_empty());
        let text = brief(&minimal, &context).unwrap();
        assert_small_task_contract(&text, DEFAULT_CONSUMER);

        // A declared consumer and extra triggers are carried into the brief,
        // and the standing boundaries are not replaced by them.
        let mut declared: serde_json::Value =
            serde_json::from_str(&document("Ship the outcome", &["input.txt"], &[])).unwrap();
        declared["consumer"] = serde_json::json!("the lead of epic sample-3mu");
        declared["escalate"] = serde_json::json!(["any change to the synthetic crate layout"]);
        fs::write(&path, serde_json::to_string(&declared).unwrap()).unwrap();
        let assignment = Assignment::load(&path).unwrap();
        let text = brief(&assignment, &context).unwrap();
        assert!(
            text.contains("consumer: the lead of epic sample-3mu"),
            "{text}"
        );
        assert_small_task_contract(&text, "the lead of epic sample-3mu");
        assert!(
            text.contains("escalate to the lead of epic sample-3mu (additional boundaries)"),
            "{text}"
        );
        assert!(
            text.contains("- any change to the synthetic crate layout"),
            "{text}"
        );
        // Standing boundaries stay owned by the role fragment: declared
        // triggers extend that contract instead of copying it into the brief.
        assert!(
            !text.contains("- a material architecture or design change"),
            "{text}"
        );

        // Malformed values fail before any rendering, with the field named.
        let too_long_consumer = "x".repeat(MAX_ITEM_BYTES + 1);
        for (field, value, expected) in [
            (
                "consumer",
                serde_json::json!(""),
                "assignment consumer is empty",
            ),
            (
                "consumer",
                serde_json::json!(too_long_consumer),
                "the limit is",
            ),
            (
                "escalate",
                serde_json::json!([""]),
                "assignment escalate[0] is empty",
            ),
            (
                "escalate",
                serde_json::json!(vec!["escalate this"; MAX_LIST_ITEMS + 1]),
                "the limit is",
            ),
        ] {
            let mut broken: serde_json::Value =
                serde_json::from_str(&document("Ship the outcome", &["input.txt"], &[])).unwrap();
            broken[field] = value;
            fs::write(&path, serde_json::to_string(&broken).unwrap()).unwrap();
            let error = Assignment::load(&path).unwrap_err().to_string();
            assert!(error.contains(expected), "{field}: {error}");
        }
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_document_within_field_limits_can_still_exceed_the_brief_budget() {
        let root = temp_root("brief-budget");
        fs::write(root.join("input.txt"), "input\n").unwrap();
        let mut assignment = checked_in(
            &root,
            "assignment.json",
            &document("Ship the outcome", &["input.txt"], &[]),
        );
        // Every list and item stays inside its own limit; the rendered brief
        // still cannot fit the assignment budget.
        let item = format!("keep the change inside the checkout: {}", "x".repeat(400));
        assignment.invariants = vec![item.clone(); MAX_LIST_ITEMS];
        assignment.acceptance = vec![item; MAX_LIST_ITEMS];
        let error = brief(
            &assignment,
            &AssignmentContext {
                checkout: &root,
                base: "0123456789abcdef0123456789abcdef01234567",
                owner: "exec-ds-7",
                source: &root,
            },
        )
        .unwrap_err()
        .to_string();
        assert!(
            error.contains("the rendered assignment brief is"),
            "{error}"
        );
        assert!(
            error.contains(&format!("the limit is {MAX_BRIEF_BYTES}")),
            "{error}"
        );
        let _ = fs::remove_dir_all(root);
    }

    /// The task contract an executor receives must stay a small, bounded
    /// addition to one fixed payload: this test binds the canonical payload,
    /// the rendered size, the small feedback addressing and the removal of the
    /// standing exchange wording, so the instruction-load accounting can be
    /// reproduced from the source.
    #[test]
    fn one_fixed_payload_keeps_the_rendered_guidance_concise() {
        let root = std::env::temp_dir().join("executor-assignment-measure-fixed");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        for input in [
            "openspec/changes/sample/design.md",
            "crates/sample/src/first.rs",
            "crates/sample/src/second.rs",
            "crates/sample/tests/first.rs",
        ] {
            let path = root.join(input);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, "// declared input\n").unwrap();
        }
        let document = serde_json::to_string_pretty(&serde_json::json!({
            "schema": 1,
            "objective": "Implement the structured-renderer half of sample change 9.9: add concise installed guidance to generated structured assignment briefs and leave the free-text renderer to the other worker.",
            "inputs": [
                "openspec/changes/sample/design.md",
                "crates/sample/src/first.rs",
                "crates/sample/src/second.rs",
                "crates/sample/tests/first.rs"
            ],
            "outputs": ["crates/sample/src/first.rs"],
            "invariants": [
                "Rendered structured briefs must name the task-specific consumer and the concrete feedback address without restating the standing exchange guidance owned by the executor role.",
                "Guidance is exceptional only: material ambiguity, an authority or access boundary, or an unresolvable dependency after investigation; ordinary implementation errors and routine progress stay off-channel.",
                "Do not teach address discovery, receipt editing, process identity checks, resume rituals, recursive delegation or re-enabling native agent tools.",
                "Keep the addition within MAX_BRIEF_BYTES and replace superseded wording rather than appending a second workflow where possible.",
                "Do not edit the free-text renderer: another worker owns it and the free-text half remains for lead integration."
            ],
            "acceptance": [
                "Existing and new assignment tests prove the exact rendered guidance, limits, Unicode and default consumer behavior.",
                "cargo fmt --all -- --check passes.",
                "codex-harness heavy -- cargo test --locked -p codex-harness executor_assignment --jobs 1 -- --test-threads=1 passes."
            ]
        }))
        .unwrap();
        let assignment = checked_in(&root, "assignment.json", &document);
        let text = brief(
            &assignment,
            &AssignmentContext {
                checkout: &root,
                base: "d01df098da47c0d87e5f6268210ab1f2401e152d",
                owner: "exec-measure",
                source: &root,
            },
        )
        .unwrap();
        let guidance = expected_feedback_line(DEFAULT_CONSUMER);
        println!("CANONICAL_BRIEF_BYTES {}", text.len());
        println!("CANONICAL_BRIEF_WORDS {}", text.split_whitespace().count());
        println!("CANONICAL_GUIDANCE_BYTES {}", guidance.len());
        println!(
            "CANONICAL_GUIDANCE_WORDS {}",
            guidance.split_whitespace().count()
        );
        assert!(
            text.len() <= MAX_BRIEF_BYTES,
            "the canonical brief is {} bytes",
            text.len()
        );
        assert!(
            guidance.len() <= 1024,
            "the added guidance is {} bytes; keep it concise",
            guidance.len()
        );
        assert!(
            !text.contains(SUPERSEDED_ESCALATION_HEADING),
            "the superseded escalation wording is replaced, not kept beside the contract: {text}"
        );
        assert_small_task_contract(&text, DEFAULT_CONSUMER);
        let _ = fs::remove_dir_all(root);
    }

    /// The one feedback address a real dispatch renders: the named consumer
    /// and the concrete installed command, with none of the standing exchange
    /// rules - when to ask, the waiting behavior or the result shape - which
    /// the executor role fragment owns.
    #[test]
    fn the_feedback_address_is_rendered_without_standing_guidance() {
        let root = temp_root("guidance");
        fs::write(root.join("input.txt"), "input\n").unwrap();
        let assignment = checked_in(
            &root,
            "assignment.json",
            &document("Ship the outcome", &["input.txt"], &[]),
        );
        let text = brief(
            &assignment,
            &AssignmentContext {
                checkout: &root,
                base: "0123456789abcdef0123456789abcdef01234567",
                owner: "exec-ds-7",
                source: &root,
            },
        )
        .unwrap();
        assert_small_task_contract(&text, DEFAULT_CONSUMER);
        assert_eq!(text.matches("feedback:").count(), 1, "{text}");
        // The contract teaches no address or session discovery, receipt
        // editing, process identity checks, a resume ritual, recursive
        // delegation, a message size rule, a transport detail or a
        // caller-created message file.
        for forbidden in [
            "--session",
            "CODEX_",
            "PID",
            "endpoint-",
            "turn/steer",
            "executor spawn",
            "sub-agent",
            "agent tool",
            "receipt",
            "reply-to",
            "--file",
            "temporary",
            "KiB",
            "MiB",
        ] {
            assert!(!text.contains(forbidden), "{forbidden}\n{text}");
        }
        let _ = fs::remove_dir_all(root);
    }

    /// Free text is a supported dispatch path, so it renders the same small
    /// task contract: the caller's text stays first and literal, and the
    /// implicit consumer plus the concrete feedback address follow it. Neither
    /// form restates the standing guidance owned by the executor role
    /// fragment.
    #[test]
    fn free_text_and_structured_assignments_render_one_task_contract() {
        let literal = "Réparer le contrat - keep 'quoted' text and --notify literal, суммарный контроль\nsecond line";
        let free = free_text_brief(literal);
        assert!(free.starts_with(literal), "{free}");
        assert_small_task_contract(&free, DEFAULT_CONSUMER);
        assert_eq!(free.matches("feedback:").count(), 1, "{free}");

        // The same feedback contract reaches the structured path, so an
        // executor cannot receive two versions of the address.
        let root = temp_root("one-rule");
        fs::write(root.join("input.txt"), "input\n").unwrap();
        let assignment = checked_in(
            &root,
            "assignment.json",
            &document("Ship the outcome", &["input.txt"], &[]),
        );
        let structured = brief(
            &assignment,
            &AssignmentContext {
                checkout: &root,
                base: "0123456789abcdef0123456789abcdef01234567",
                owner: "exec-ds-7",
                source: &root,
            },
        )
        .unwrap();
        assert_small_task_contract(&structured, DEFAULT_CONSUMER);
        assert!(
            structured.contains(&expected_feedback_line(DEFAULT_CONSUMER)),
            "{structured}"
        );
        // A caller's trailing blank line is dropped instead of being rendered
        // into the contract; nothing else about the text is rewritten.
        let padded = free_text_brief("do the work\n\n");
        assert!(padded.starts_with("do the work\n"), "{padded}");
        let _ = fs::remove_dir_all(root);
    }

    /// Every limit is a byte limit. These two documents hold the same number of
    /// characters per item; the multibyte one is refused for its bytes while
    /// its ASCII twin renders, so the budget cannot be mistaken for characters.
    #[test]
    fn multibyte_assignment_content_is_measured_in_bytes() {
        let root = temp_root("multibyte");
        fs::write(root.join("input.txt"), "input\n").unwrap();
        let context = AssignmentContext {
            checkout: &root,
            base: "0123456789abcdef0123456789abcdef01234567",
            owner: "exec-ds-7",
            source: &root,
        };
        let items = 60;
        let mut wide = checked_in(
            &root,
            "wide.json",
            &document("Ship the outcome", &["input.txt"], &[]),
        );
        wide.invariants = vec!["é".repeat(200); items];
        assert_eq!(wide.invariants[0].chars().count(), 200);
        assert!(wide.invariants[0].len() <= MAX_ITEM_BYTES);
        let error = brief(&wide, &context).unwrap_err().to_string();
        assert!(
            error.contains("the rendered assignment brief is"),
            "{error}"
        );
        assert!(
            error.contains(&format!("the limit is {MAX_BRIEF_BYTES}")),
            "{error}"
        );

        let mut ascii = checked_in(
            &root,
            "ascii.json",
            &document("Ship the outcome", &["input.txt"], &[]),
        );
        ascii.invariants = vec!["x".repeat(200); items];
        assert_eq!(ascii.invariants[0].chars().count(), 200);
        let text = brief(&ascii, &context).unwrap();
        assert!(text.len() <= MAX_BRIEF_BYTES, "{}", text.len());
        assert_small_task_contract(&text, DEFAULT_CONSUMER);
        let _ = fs::remove_dir_all(root);
    }
}
