//! Hierarchical acceptance for retained structured Lead and Executor work.
//!
//! The accepting Lead or Director names only the board task. This owner derives
//! the assignment, result, child session/turn and current reviewer turn from
//! retained completion state and the native control endpoint, checks the
//! recorded parent relationship, records the exact repository revision set and
//! closes through the existing board owner. It makes no model calls, never
//! fabricates a turn and never releases a workspace.

use super::{control, executor_input, invalid, option_text};
use harness_core::{
    board_cli,
    delivery_reflection::{
        self, ACCEPTANCE_METADATA, AcceptanceBinding, BindingStatus, Purpose, ReflectionController,
    },
    orchestration_config,
    task_orchestrate::{self, RepositoryRevision},
    task_worktree::{self, RetainedCompletion},
};
use serde_json::{Value, json};
use std::{
    env,
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
    process::Command,
};

const HELP: &str = "\
codex-harness executor accept TASK --project DIRECTORY [--source CHECKOUT] [--codex-home DIRECTORY] [--run GENERATION]
  Accept retained structured work as the live immediate parent (Executor work)
  or as the Director (Lead or direct Executor work). TASK is the exact board
  issue. The command authenticates the current managed turn, verifies the
  recorded parent, takes the newest retained completion for TASK unless --run
  names the exact generation, records the exact clean repository revision set,
  writes the acceptance binding through the board owner and closes the task
  with reason 'accepted'. A task with live board dependencies or a Lead block
  with unfinished board children is refused before board effects. Acceptance
  never releases a workspace.
  Repeating the same accepted result is a no-op; --run resolves only an
  ambiguous timestamp tie and never bypasses a newer retained or unresolved
  correction.";

const MAX_CANDIDATES: usize = 6;
const MAX_RECEIPTS: usize = 256;
const MAX_REPOSITORIES: usize = 16;

struct Request {
    task: String,
    project: PathBuf,
    source: Option<PathBuf>,
    codex_home: Option<PathBuf>,
    run: Option<String>,
}

impl Request {
    fn parse(args: &[OsString]) -> io::Result<Self> {
        let mut task = None;
        let mut project = None;
        let mut source = None;
        let mut codex_home = None;
        let mut run = None;
        let mut iter = args.iter();
        let Some(first) = iter.next() else {
            return Err(invalid("executor accept requires the board TASK"));
        };
        let first = option_text(first)?;
        if first.starts_with("--") {
            return Err(invalid(
                "executor accept requires TASK as its first argument",
            ));
        }
        if !first.trim().is_empty() {
            task = Some(first.trim().to_owned());
        }
        while let Some(arg) = iter.next() {
            let key = arg
                .to_str()
                .ok_or_else(|| invalid("invalid native executor accept options"))?;
            let value = iter
                .next()
                .ok_or_else(|| invalid(&format!("{key} requires a value")))?;
            match key {
                "--project" => project = Some(PathBuf::from(value)),
                "--source" => source = Some(PathBuf::from(value)),
                "--codex-home" => codex_home = Some(PathBuf::from(value)),
                "--run" => {
                    let value = option_text(value)?;
                    if value.trim().is_empty() {
                        return Err(invalid("--run requires the exact retained generation"));
                    }
                    run = Some(value.trim().to_owned());
                }
                _ => {
                    return Err(invalid(&format!(
                        "invalid native executor accept option {key}"
                    )));
                }
            }
        }
        let task = task.ok_or_else(|| invalid("executor accept requires a nonempty board TASK"))?;
        if task.chars().any(char::is_whitespace) {
            return Err(invalid(
                "executor accept TASK must be one exact board issue id",
            ));
        }
        let project =
            project.ok_or_else(|| invalid("executor accept requires --project DIRECTORY"))?;
        Ok(Self {
            task,
            project,
            source,
            codex_home,
            run,
        })
    }
}

struct LeadTurn {
    thread: String,
    turn: String,
    role: &'static str,
}

struct Accepted {
    binding: AcceptanceBinding,
    assignment: String,
    result: String,
    result_locator: Option<PathBuf>,
    revision: String,
    revisions: Vec<RepositoryRevision>,
    output_role: String,
    executor_session: Option<String>,
    executor_turn: Option<String>,
    duplicate: bool,
}

pub(super) fn run(args: &[OsString]) -> io::Result<i32> {
    if args.first().is_some_and(|arg| arg == "--help") {
        println!("{HELP}");
        return Ok(0);
    }
    let request = Request::parse(args)?;
    let home = executor_input::codex_home(request.codex_home)?;
    let source = request.source.unwrap_or_else(|| request.project.clone());
    let bd = crate::feedback_cli::discover_bd_in(&home)?;

    let lead = current_lead(&home)?;
    let issue = board_cli::show_issue(&bd, &request.project, &request.task)?;
    let purpose = board_purpose(&issue, &request.task)?;

    if issue.get("status").and_then(serde_json::Value::as_str) == Some("closed") {
        let controller = ReflectionController::new(&bd, &request.project, &home);
        let reflection = controller.check_active_binding(&lead.thread, None)?;
        refuse_unusable_reflection(&reflection)?;
        let binding = recorded_acceptance(&issue, &request.task)?;
        let accepted = validate_recorded_acceptance(
            &home,
            &source,
            &bd,
            &request.project,
            &issue,
            &request.task,
            purpose,
            &lead,
            request.run.as_deref(),
            binding,
        )?;
        return receipt(&request.task, purpose, &lead, &reflection, &accepted);
    }
    if board_cli::issue_metadata(&issue).contains_key(ACCEPTANCE_METADATA) {
        return Err(board_cli::open_acceptance_refusal(&request.task));
    }
    refuse_blocked_dependencies(&issue, &request.task)?;
    let config_source = if source.join("global/roles.toml").is_file() {
        source.clone()
    } else {
        crate::feedback_cli::installed_kit_source_in(&home).ok_or_else(|| {
            invalid(
                "ordinary acceptance has no installed kit source; repair the installation before reading self_reflection_enabled",
            )
        })?
    };
    let self_reflection_enabled =
        orchestration_config::load(&config_source)?.self_reflection_enabled;
    if self_reflection_enabled {
        // Prepare before observing the binding and before the board close. The
        // activation checkpoint is therefore at or before the accepted event,
        // so a fast first acceptance remains eligible for exactly one
        // reflection instead of starting the journal at a later head.
        crate::self_reflection_cli::lifecycle::prepare_automatic(
            &home,
            &config_source,
            &request.project,
            &lead.thread,
            crate::self_reflection_cli::lifecycle::PreparationTrigger::Acceptance,
        )?;
    }
    let controller = ReflectionController::new(&bd, &request.project, &home);
    let reflection = controller.check_active_binding(&lead.thread, None)?;
    refuse_unusable_reflection(&reflection)?;

    // Dispatch preparation publishes its claim and startup reservation under
    // the pool-wide admission lock; assignment binding and the run receipt
    // follow after it is released. Hold the same lock across the final
    // deterministic reads and the board close so a new same-task reservation
    // cannot appear between the observation and the close. Reflection and
    // endpoint activity above stay outside this guard.
    let admission = task_worktree::begin_pool_admission(&home, &source)?;
    let retained = task_worktree::retained_completions_for_task(&home, &source, &request.task)?;
    let selected = select_completion(
        &home,
        &source,
        &request.task,
        request.run.as_deref(),
        &retained,
    )?;
    let run_receipt = validate_hierarchy(&home, &selected, &lead)?;
    validate_completion(&bd, &request.project, &issue, &selected, purpose, &retained)?;
    refuse_stale_explicit_run(request.run.as_deref(), &selected, &retained)?;
    refuse_unresolved_correction(&home, &source, &request.task, &selected, &retained)?;

    let assignment = selected
        .assignment
        .as_ref()
        .expect("validated completion has an assignment binding");
    let result = selected
        .result
        .as_ref()
        .expect("validated completion has a retained result");
    let executor_session = selected
        .session
        .clone()
        .expect("validated completion has an executor session");
    let executor_turn = selected
        .turn
        .clone()
        .expect("validated completion has an executor turn");
    let revision = selected
        .source
        .revision
        .clone()
        .expect("validated completion has a source revision");
    let output_role = selected
        .role
        .as_deref()
        .expect("validated completion has a run role");
    if output_role == "lead" {
        refuse_unfinished_parent(&bd, &request.project, &request.task)?;
    }
    let revisions = acceptance_revisions(&selected, run_receipt.as_ref())?;

    let binding = AcceptanceBinding::new(
        request.task.clone(),
        selected.run_generation.clone(),
        assignment.assignment_sha256.clone(),
        result.sha256.clone(),
        revision.clone(),
        vec![executor_session.clone()],
        lead.thread.clone(),
        lead.thread.clone(),
        purpose,
    )
    .with_turns(
        Some(executor_session.clone()),
        Some(executor_turn.clone()),
        Some(lead.turn.clone()),
    )
    .with_self_reflection_enabled(self_reflection_enabled);
    let mut metadata = delivery_reflection::acceptance_metadata(&binding)?;
    metadata.insert(
        task_orchestrate::ACCEPTANCE_REVISIONS_METADATA.to_owned(),
        serde_json::to_string(&revisions)?,
    );
    let closed = board_cli::close_accepted(
        &bd,
        &request.project,
        &lead.thread,
        &request.task,
        &metadata,
    )?;
    drop(admission);

    let accepted = Accepted {
        binding,
        assignment: assignment.assignment_sha256.clone(),
        result: result.sha256.clone(),
        result_locator: Some(result.locator.clone()),
        revision,
        revisions,
        output_role: output_role.to_owned(),
        executor_session: Some(executor_session),
        executor_turn: Some(executor_turn),
        duplicate: closed.duplicate,
    };
    receipt(&request.task, purpose, &lead, &reflection, &accepted)
}

fn receipt(
    task: &str,
    purpose: Purpose,
    lead: &LeadTurn,
    reflection: &delivery_reflection::BindingCheck,
    accepted: &Accepted,
) -> io::Result<i32> {
    let exact = accepted.executor_session.is_some()
        && accepted.executor_turn.is_some()
        && accepted.binding.lead_turn.is_some();
    let self_reflection_enabled = accepted.binding.self_reflection_enabled;
    let automatic_trigger = reflection.status == BindingStatus::Active
        && purpose.triggers_reflection()
        && exact
        && self_reflection_enabled;
    let detail = if !self_reflection_enabled {
        format!(
            "{}; self_reflection_enabled = false at acceptance, so this accepted closure is not eligible for automatic reflection",
            reflection.detail
        )
    } else if purpose.triggers_reflection() && !exact {
        format!(
            "{}; the accepted record has no exact executor session, completed executor turn and accepting lead turn, so automatic reflection is ineligible",
            reflection.detail
        )
    } else if reflection.status == BindingStatus::Active && !purpose.triggers_reflection() {
        format!(
            "{}; {} work never auto-triggers reflection",
            reflection.detail,
            purpose.as_str()
        )
    } else {
        reflection.detail.clone()
    };
    let acceptance_id = accepted.binding.acceptance_id()?;
    let receipt = json!({
        "schema": 1,
        "status": "accepted",
        "duplicate": accepted.duplicate,
        "task": task,
        "purpose": purpose.as_str(),
        "acceptanceId": acceptance_id,
        "run": accepted.binding.run,
        "assignment": accepted.assignment,
        "result": accepted.result,
        "resultLocator": accepted
            .result_locator
            .as_ref()
            .map(|path| path.display().to_string()),
        "revision": accepted.revision,
        "revisions": accepted.revisions,
        "review": {
            "outputRole": accepted.output_role,
            "acceptingRole": lead.role,
            "parent": lead.thread,
            "workspaceRelease": "held",
        },
        "workspaceHeld": true,
        "executorSession": accepted.executor_session,
        "executorTurn": accepted.executor_turn,
        "leadSession": accepted.binding.lead_session,
        "leadTurn": accepted.binding.lead_turn,
        "currentLeadTurn": lead.turn,
        "reflection": {
            "status": reflection.status,
            "detail": detail,
            "bindingId": reflection.binding_id,
            "revision": reflection.revision,
            "selfReflectionEnabled": self_reflection_enabled,
            "automaticTrigger": automatic_trigger,
        },
        "modelCalls": 0,
    });
    println!("{}", serde_json::to_string_pretty(&receipt)?);
    Ok(0)
}

fn current_lead(home: &Path) -> io::Result<LeadTurn> {
    let thread = env::var("CODEX_THREAD_ID")
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            invalid(
                "executor accept needs the current managed lead conversation: CODEX_THREAD_ID is missing or blank",
            )
        })?;
    let endpoint = match control::registered_lead(home, &thread) {
        control::RegisteredLead::Live(endpoint) => endpoint,
        control::RegisteredLead::Unavailable(reason) => {
            return Err(invalid(&format!(
                "executor accept cannot authenticate the current lead turn: {reason}"
            )));
        }
    };
    let mut conversation = control::Conversation::attach(&endpoint, control::DEFAULT_BOUND)?;
    let record = conversation.read_thread_record(&thread)?;
    let turns = record
        .get("turns")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| {
            invalid(&format!(
                "lead thread {thread} has no native turn list; acceptance cannot establish the exact current lead turn and refuses before changing the board"
            ))
        })?;
    // Resumed history can retain unfinished turns; only the newest turn can approve.
    let latest = turns.last().ok_or_else(|| {
        invalid(&format!(
            "lead thread {thread} has no native turns; acceptance refuses before changing the board"
        ))
    })?;
    if latest.get("status").and_then(serde_json::Value::as_str) != Some("inProgress") {
        return Err(invalid(&format!(
            "lead thread {thread}'s latest native turn is not in progress; acceptance refuses before changing the board"
        )));
    }
    let turn = latest
        .get("id")
        .and_then(serde_json::Value::as_str)
        .filter(|id| !id.trim().is_empty())
        .ok_or_else(|| {
            invalid(&format!(
                "lead thread {thread} has an in-progress native turn without a usable exact turn identity; acceptance refuses before changing the board"
            ))
        })?
        .to_owned();
    let role = accepting_role(home, &thread)?;
    Ok(LeadTurn { thread, turn, role })
}

/// The accepting conversation is identified from the same retained MCP run
/// receipts that establish dispatch hierarchy. An unrecorded registered root is
/// the Director; a recorded Lead is a delegated Lead; a recorded Executor has no
/// acceptance authority.
fn accepting_role(home: &Path, thread: &str) -> io::Result<&'static str> {
    let own: Vec<Value> = receipt_values(home)?
        .into_iter()
        .filter(|receipt| receipt["executorThreadId"].as_str() == Some(thread))
        .collect();
    match own.as_slice() {
        [] => Ok("director"),
        [receipt] => {
            if !receipt["release"].is_null() {
                return Err(invalid(
                    "the current conversation's run was released and cannot accept work",
                ));
            }
            match receipt["role"].as_str() {
                Some("lead") => Ok("lead"),
                Some("executor") => Err(invalid(
                    "the current conversation is a recorded Executor and cannot accept its own or another run's work",
                )),
                Some(role) => Err(invalid(&format!(
                    "the current conversation records unknown run role '{role}'; acceptance authority is unavailable"
                ))),
                None => Err(invalid(
                    "the current conversation's run receipt records no role; acceptance authority is unavailable",
                )),
            }
        }
        _ => Err(invalid(
            "the current conversation is recorded as the child of more than one run; refusing ambiguous acceptance authority",
        )),
    }
}

fn validate_hierarchy(
    home: &Path,
    retained: &RetainedCompletion,
    lead: &LeadTurn,
) -> io::Result<Option<Value>> {
    let output_role = retained.role.as_deref().ok_or_else(|| {
        invalid(
            "the retained completion records no run role; hierarchical acceptance is unavailable",
        )
    })?;
    let receipt = run_receipt(home, &retained.run_generation)?;
    let parent = match receipt.as_ref() {
        Some(receipt) => {
            if receipt["role"].as_str() != Some(output_role) {
                return Err(invalid(&format!(
                    "retained run {} records role {}, but its MCP receipt records role {}; refusing a changed hierarchy identity",
                    retained.run_generation,
                    output_role,
                    receipt["role"].as_str().unwrap_or("unrecorded")
                )));
            }
            if receipt["executorThreadId"].as_str() != retained.session.as_deref()
                || receipt["turnId"].as_str() != retained.turn.as_deref()
            {
                return Err(invalid(&format!(
                    "retained run {} does not match the exact session and completed turn in its MCP receipt; refusing fabricated turn provenance",
                    retained.run_generation
                )));
            }
            let parent = receipt["parentThreadId"]
                .as_str()
                .or_else(|| receipt["leadThreadId"].as_str())
                .ok_or_else(|| {
                    invalid(&format!(
                        "retained run {} has no recorded parent thread; hierarchical acceptance cannot establish ownership",
                        retained.run_generation
                    ))
                })?;
            if let Some(lead_thread) = receipt["leadThreadId"].as_str()
                && lead_thread != parent
            {
                return Err(invalid(&format!(
                    "retained run {} records conflicting parent and lead threads; refusing ambiguous ownership",
                    retained.run_generation
                )));
            }
            Some(parent)
        }
        None => None,
    };
    if receipt.is_none() && output_role == "lead" {
        return Err(invalid(&format!(
            "retained run {} is a Lead output without an MCP receipt; the Director's exact parent relationship cannot be established",
            retained.run_generation
        )));
    }
    task_orchestrate::validate_acceptance_relation(output_role, lead.role, parent, &lead.thread)
        .map_err(|error| invalid(&format!("acceptance hierarchy refused: {error}")))?;
    Ok(receipt)
}

fn run_receipt(home: &Path, generation: &str) -> io::Result<Option<Value>> {
    let matches: Vec<Value> = receipt_values(home)?
        .into_iter()
        .filter(|receipt| {
            receipt["runGeneration"].as_str() == Some(generation)
                || receipt["run"].as_str() == Some(generation)
        })
        .collect();
    match matches.as_slice() {
        [] => Ok(None),
        [receipt] => Ok(Some(receipt.clone())),
        _ => Err(invalid(&format!(
            "run generation {generation} matches more than one MCP receipt; refusing ambiguous parent or revision evidence"
        ))),
    }
}

fn receipt_values(home: &Path) -> io::Result<Vec<Value>> {
    let root = home.join("harness/executor-pool");
    if !root.is_dir() {
        return Ok(Vec::new());
    }
    let mut receipts = Vec::new();
    for directory in fs::read_dir(&root)? {
        let directory = directory?;
        if !directory.file_type()?.is_dir() {
            continue;
        }
        for entry in fs::read_dir(directory.path())? {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if !name.starts_with("mcp-") || !name.ends_with(".json") {
                continue;
            }
            if receipts.len() >= MAX_RECEIPTS {
                return Err(invalid(&format!(
                    "more than {MAX_RECEIPTS} MCP run receipts are present; refusing an unbounded acceptance scan"
                )));
            }
            let value = serde_json::from_slice(&fs::read(entry.path())?).map_err(|error| {
                invalid(&format!(
                    "MCP run receipt {} is not valid JSON: {error}",
                    entry.path().display()
                ))
            })?;
            receipts.push(value);
        }
    }
    Ok(receipts)
}

fn acceptance_revisions(
    selected: &RetainedCompletion,
    receipt: Option<&Value>,
) -> io::Result<Vec<RepositoryRevision>> {
    if receipt.is_none() {
        // Legacy single-level Executor acceptance keeps its retained exact
        // revision even after an explicit release reused the slot. That
        // revision, not the slot's later HEAD, is the accepted input.
        return Ok(vec![RepositoryRevision {
            repository: selected.checkout.display().to_string(),
            checkout: selected.checkout.display().to_string(),
            revision: selected
                .source
                .revision
                .clone()
                .expect("validated completion has a source revision"),
        }]);
    }
    let receipt = receipt.expect("non-legacy acceptance has a run receipt");
    let mut repositories = Vec::new();
    if let Some(workspaces) = receipt["workspaces"].as_array() {
        for workspace in workspaces {
            let repository = required_revision_path(workspace, "repository")?;
            let checkout = required_revision_path(workspace, "checkout")?;
            repositories.push((repository, checkout));
        }
    } else {
        let repository = receipt["repository"]
            .as_str()
            .map(PathBuf::from)
            .unwrap_or_else(|| selected.checkout.clone());
        repositories.push((repository, selected.checkout.clone()));
    }
    if !repositories
        .iter()
        .any(|(_, checkout)| same_path(checkout, &selected.checkout))
    {
        let repository = receipt["repository"]
            .as_str()
            .map(PathBuf::from)
            .unwrap_or_else(|| selected.checkout.clone());
        repositories.push((repository, selected.checkout.clone()));
    }
    let evidence = task_orchestrate::repository_revisions(&repositories)?;
    let primary = evidence
        .iter()
        .find(|revision| same_path(Path::new(&revision.checkout), &selected.checkout))
        .ok_or_else(|| {
            invalid(&format!(
                "accepted revision evidence does not name the completed checkout {}",
                selected.checkout.display()
            ))
        })?;
    if selected.source.revision.as_deref() != Some(primary.revision.as_str()) {
        return Err(invalid(&format!(
            "completed checkout {} is now at {}, not the retained completion revision {}; refusing changed acceptance input",
            selected.checkout.display(),
            primary.revision,
            selected.source.revision.as_deref().unwrap_or("unrecorded")
        )));
    }
    Ok(evidence)
}

fn required_revision_path(value: &Value, field: &str) -> io::Result<PathBuf> {
    value[field]
        .as_str()
        .filter(|text| !text.trim().is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| {
            invalid(&format!(
                "accepted workspace evidence records no usable {field}; refusing an incomplete repository revision set"
            ))
        })
}

/// A board `blocks` relationship is separate from the parent-child tree. The
/// board owner refuses the accepted close while a blocker is open; refuse
/// before metadata is written so a waiting dependency has no false completion
/// and can be accepted normally after its prerequisite is verified.
fn refuse_blocked_dependencies(issue: &Value, task: &str) -> io::Result<()> {
    let blockers: Vec<&str> = issue["dependencies"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|dependency| dependency["dependency_type"].as_str() == Some("blocks"))
        .filter(|dependency| dependency["status"].as_str() != Some("closed"))
        .filter_map(|dependency| dependency["id"].as_str())
        .collect();
    if blockers.is_empty() {
        return Ok(());
    }
    Err(invalid(&format!(
        "dependent task {task} is blocked by prerequisite contract {}; verify and record the prerequisite, then retry. No acceptance metadata or board closure was written.",
        blockers.join(", ")
    )))
}

fn refuse_unfinished_parent(bd: &Path, project: &Path, task: &str) -> io::Result<()> {
    let output = Command::new(bd)
        .args(["children", task, "--json"])
        .current_dir(project)
        .output()?;
    if !output.status.success() {
        return Err(invalid(&format!(
            "cannot read board children of Lead task {task}: {}; acceptance changed no board state",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let value: Value = serde_json::from_slice(&output.stdout).map_err(|error| {
        invalid(&format!(
            "board children of Lead task {task} are not valid JSON: {error}; acceptance changed no board state"
        ))
    })?;
    let children = value
        .as_array()
        .cloned()
        .or_else(|| {
            value
                .get("issues")
                .and_then(Value::as_array)
                .cloned()
        })
        .ok_or_else(|| {
            invalid(&format!(
                "board children of Lead task {task} are not an issue list; acceptance changed no board state"
            ))
        })?;
    task_orchestrate::refuse_unfinished_parent(
        task,
        &task_orchestrate::unfinished_children(&children),
    )
    .map_err(|error| invalid(&error.to_string()))
}

fn recorded_revisions(issue: &Value) -> io::Result<Option<Vec<RepositoryRevision>>> {
    let metadata = board_cli::issue_metadata(issue);
    let Some(raw) = metadata.get(task_orchestrate::ACCEPTANCE_REVISIONS_METADATA) else {
        return Ok(None);
    };
    let revisions: Vec<RepositoryRevision> = serde_json::from_str(raw).map_err(|error| {
        invalid(&format!(
            "recorded acceptance repository revisions are malformed: {error}"
        ))
    })?;
    if revisions.is_empty() || revisions.len() > MAX_REPOSITORIES {
        return Err(invalid(&format!(
            "recorded acceptance names {} repositories; the supported range is 1..={MAX_REPOSITORIES}",
            revisions.len()
        )));
    }
    for revision in &revisions {
        if !(revision.revision.len() == 40 || revision.revision.len() == 64)
            || !revision
                .revision
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(invalid(&format!(
                "recorded acceptance repository {} has no exact commit revision",
                revision.repository
            )));
        }
    }
    Ok(Some(revisions))
}

fn same_path(left: &Path, right: &Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left
            .to_string_lossy()
            .eq_ignore_ascii_case(&right.to_string_lossy()),
        _ => left
            .to_string_lossy()
            .trim_end_matches(['\\', '/'])
            .eq_ignore_ascii_case(right.to_string_lossy().trim_end_matches(['\\', '/'])),
    }
}

fn recorded_acceptance(issue: &serde_json::Value, task: &str) -> io::Result<AcceptanceBinding> {
    let status = issue
        .get("status")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    if status != "closed" {
        return Err(invalid(&format!("task {task} is not closed as accepted")));
    }
    let reason = issue
        .get("close_reason")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .trim();
    if reason != delivery_reflection::ACCEPTED_CLOSE_REASON {
        return Err(invalid(&format!(
            "task {task} is already closed with reason '{reason}', not accepted; a cancelled, reopened or lead-review closure is never overwritten"
        )));
    }
    let raw = board_cli::issue_metadata(issue)
        .get(ACCEPTANCE_METADATA)
        .cloned()
        .ok_or_else(|| {
            invalid(&format!(
                "task {task} is closed as accepted but records no {ACCEPTANCE_METADATA}; the acceptance cannot be replayed"
            ))
        })?;
    let binding = serde_json::from_str::<AcceptanceBinding>(&raw)
        .map_err(|error| {
            invalid(&format!(
                "task {task} has malformed acceptance metadata: {error}"
            ))
        })?
        .normalized()?;
    if binding.task != task {
        return Err(invalid(&format!(
            "task {task} records acceptance for task {}; refusing a foreign acceptance",
            binding.task
        )));
    }
    Ok(binding)
}

#[allow(clippy::too_many_arguments)]
fn validate_recorded_acceptance(
    home: &Path,
    source: &Path,
    bd: &Path,
    project: &Path,
    issue: &serde_json::Value,
    task: &str,
    purpose: Purpose,
    lead: &LeadTurn,
    requested_run: Option<&str>,
    binding: AcceptanceBinding,
) -> io::Result<Accepted> {
    if binding.purpose != purpose {
        return Err(invalid(&format!(
            "task {task} records acceptance purpose {}, but the board now records {}; stale purpose is never repeated",
            binding.purpose.as_str(),
            purpose.as_str()
        )));
    }
    if binding.lead_session != lead.thread || binding.approved_by != lead.thread {
        return Err(invalid(&format!(
            "task {task} was accepted by lead {}, not the current lead {}; a foreign lead cannot replay this acceptance",
            binding.lead_session, lead.thread
        )));
    }
    if let Some(run) = requested_run
        && run != binding.run
    {
        return Err(invalid(&format!(
            "--run {run} does not match the recorded accepted run {}; an accepted task is never rebound to another generation",
            binding.run
        )));
    }

    let retained = task_worktree::load_retained_completion(home, source, &binding.run)?
        .ok_or_else(|| {
            invalid(&format!(
                "task {task} records accepted run {}, but no retained completion for that exact generation is available; the acceptance cannot be verified",
                binding.run
            ))
        })?;
    let candidates = vec![retained.clone()];
    validate_hierarchy(home, &retained, lead)?;
    validate_completion(bd, project, issue, &retained, purpose, &candidates)?;

    let assignment = retained
        .assignment
        .as_ref()
        .expect("validated completion has an assignment binding");
    let result = retained
        .result
        .as_ref()
        .expect("validated completion has a retained result");
    let executor_session = retained
        .session
        .clone()
        .expect("validated completion has an executor session");
    let executor_turn = retained
        .turn
        .clone()
        .expect("validated completion has an executor turn");
    let revision = retained
        .source
        .revision
        .clone()
        .expect("validated completion has a source revision");

    if assignment.assignment_sha256 != binding.assignment {
        return Err(invalid(&format!(
            "accepted run {} now resolves assignment {}, not the recorded {}; changed acceptance input is never repeated",
            binding.run, assignment.assignment_sha256, binding.assignment
        )));
    }
    if result.sha256 != binding.result {
        return Err(invalid(&format!(
            "accepted run {} now resolves result {}, not the recorded {}; changed acceptance input is never repeated",
            binding.run, result.sha256, binding.result
        )));
    }
    if revision != binding.revision {
        return Err(invalid(&format!(
            "accepted run {} now resolves revision {}, not the recorded {}; changed acceptance input is never repeated",
            binding.run, revision, binding.revision
        )));
    }
    if !binding
        .executor_sessions
        .iter()
        .any(|session| session == &executor_session)
    {
        return Err(invalid(&format!(
            "accepted run {} now resolves executor session {executor_session}, which the recorded acceptance did not bind",
            binding.run
        )));
    }
    if let Some(recorded) = &binding.executor_session
        && recorded != &executor_session
    {
        return Err(invalid(&format!(
            "accepted run {} now resolves executor session {executor_session}, not the recorded {recorded}",
            binding.run
        )));
    }
    if let Some(recorded) = &binding.executor_turn
        && recorded != &executor_turn
    {
        return Err(invalid(&format!(
            "accepted run {} now resolves executor turn {executor_turn}, not the recorded {recorded}",
            binding.run
        )));
    }
    let output_role = retained
        .role
        .as_deref()
        .expect("validated completion has a run role");
    let revisions = match recorded_revisions(issue)? {
        Some(revisions) => revisions,
        None => vec![RepositoryRevision {
            repository: retained.checkout.display().to_string(),
            checkout: retained.checkout.display().to_string(),
            revision: binding.revision.clone(),
        }],
    };
    if !revisions
        .iter()
        .any(|revision| revision.revision == binding.revision)
    {
        return Err(invalid(&format!(
            "task {task} records accepted revision {}, but its repository revision set does not contain that revision; refusing incomplete evidence",
            binding.revision
        )));
    }

    Ok(Accepted {
        executor_session: binding.executor_session.clone(),
        executor_turn: binding.executor_turn.clone(),
        binding,
        assignment: assignment.assignment_sha256.clone(),
        result: result.sha256.clone(),
        result_locator: Some(result.locator.clone()),
        revision,
        revisions,
        output_role: output_role.to_owned(),
        duplicate: true,
    })
}

fn board_purpose(issue: &serde_json::Value, task: &str) -> io::Result<Purpose> {
    let raw = board_cli::issue_metadata(issue)
        .get(delivery_reflection::PURPOSE_METADATA)
        .cloned()
        .ok_or_else(|| {
            invalid(&format!(
                "board task {task} records no harness.purpose; unknown purpose never becomes ordinary or reflection-eligible"
            ))
        })?;
    Purpose::parse(&raw).map_err(|error| {
        invalid(&format!(
            "board task {task} records an invalid harness.purpose: {error}"
        ))
    })
}

fn refuse_unusable_reflection(check: &delivery_reflection::BindingCheck) -> io::Result<()> {
    match check.status {
        BindingStatus::Active | BindingStatus::Inactive | BindingStatus::Stopped => Ok(()),
        other => Err(invalid(&format!(
            "delivery reflection is {other:?} and cannot accept this closure: {}; the board was not changed",
            check.detail
        ))),
    }
}

fn select_completion(
    home: &Path,
    source: &Path,
    task: &str,
    run: Option<&str>,
    retained: &[RetainedCompletion],
) -> io::Result<RetainedCompletion> {
    if let Some(run) = run {
        let selected = task_worktree::load_retained_completion(home, source, run)?.ok_or_else(|| {
            invalid(&format!(
                "run {run} has no retained completion; pass a generation listed by the task's retained completions"
            ))
        })?;
        if selected
            .assignment
            .as_ref()
            .is_none_or(|binding| binding.issue != task)
        {
            return Err(invalid(&format!(
                "run {run} is retained for another board task, not {task}; refusing a foreign assignment"
            )));
        }
        return Ok(selected);
    }
    let Some(newest) = retained.last() else {
        return Err(invalid(&format!(
            "task {task} has no retained completion; retain the completed structured run before accepting it"
        )));
    };
    let newest_count = retained
        .iter()
        .filter(|record| record.recorded_ms == newest.recorded_ms)
        .count();
    if newest_count > 1 {
        return Err(invalid(&format!(
            "task {task} has {newest_count} retained completions with the same newest timestamp; choosing one would guess. Pass --run GENERATION for the exact accepted run.{}",
            candidate_note(retained)
        )));
    }
    Ok(newest.clone())
}

/// An exact generation resolves a retention timestamp tie; it does not make a
/// superseded result current again. A later completion, even one that is
/// ineligible, must be resolved rather than silently skipped.
fn refuse_stale_explicit_run(
    requested: Option<&str>,
    selected: &RetainedCompletion,
    retained: &[RetainedCompletion],
) -> io::Result<()> {
    if requested.is_none() {
        return Ok(());
    }
    let Some(newer) = retained.iter().rev().find(|record| {
        record.run_generation != selected.run_generation
            && record.recorded_ms > selected.recorded_ms
    }) else {
        return Ok(());
    };
    Err(invalid(&format!(
        "run {} was retained at {}ms, before newer completion {} at {}ms for the same task; --run does not bypass a newer correction. Resolve or accept the newest result. No board metadata was changed.{}",
        selected.run_generation,
        selected.recorded_ms,
        newer.run_generation,
        newer.recorded_ms,
        candidate_note(retained)
    )))
}

/// A correction dispatched for the same board issue has no completion while it
/// is active, starting or suspended. The board-bound slot record is the durable
/// owner of that state. A retained record represents the binding only when its
/// task, generation handle and assignment identity all match: a new structured
/// correction may reuse the same task and handle with a different assignment
/// before its run receipt exists. Ignore only an occupied slot with no startup
/// reservation whose current run receipt resolves to a retained completion;
/// any other same-issue binding is unfinished work.
fn refuse_unresolved_correction(
    home: &Path,
    source: &Path,
    task: &str,
    selected: &RetainedCompletion,
    retained: &[RetainedCompletion],
) -> io::Result<()> {
    for active in task_worktree::unresolved_assignments_for_issue(home, source, task)? {
        let represented = retained.iter().any(|record| {
            record.task.as_deref() == Some(active.task.as_str())
                && record.handle.as_deref() == Some(active.handle.as_str())
                && record
                    .assignment
                    .as_ref()
                    .is_some_and(|assignment| assignment.same_binding(&active.assignment))
        });
        let current_run = current_run_generation(home, source, active.slot)?;
        let current_is_retained = current_run
            .as_deref()
            .is_some_and(|run| retained.iter().any(|record| record.run_generation == run));
        if represented && current_is_retained && !active.starting {
            continue;
        }
        if matches!(
            active.state,
            task_worktree::SlotState::Free | task_worktree::SlotState::Released
        ) {
            continue;
        }
        let state = slot_state_label(active.state, active.starting);
        let startup = if active.starting {
            "; a startup reservation is recorded, so the current run may not have written its receipt yet"
        } else {
            ""
        };
        let current = current_run
            .as_deref()
            .map(|run| format!("current run {run}"))
            .unwrap_or_else(|| "no current run generation is recorded".to_owned());
        return Err(invalid(&format!(
            "task {task} has an unresolved executor correction in slot {}: task '{}', handle '{}', state {state}, assignment {}, {current}{startup}. Retained completion {} cannot be accepted while that same-task work is unfinished; finish or release that slot, then accept the newest result. Explicit --run does not bypass it. No board metadata was changed.",
            active.slot,
            active.task,
            active.handle,
            active.assignment.assignment_sha256,
            selected.run_generation
        )));
    }
    // The assignment is bound after the startup reservation is published, so
    // the retained slot can briefly carry the new same-task reservation with
    // no board assignment yet. A fresh claim may also reuse the slot task with
    // a new generation handle, so this observation binds the durable task and
    // publication state, not the retained completion's handle.
    refuse_unpublished_same_task_correction(home, source, task, selected)
}

fn slot_state_label(state: task_worktree::SlotState, starting: bool) -> &'static str {
    match state {
        task_worktree::SlotState::Synchronizing => "startup",
        task_worktree::SlotState::Occupied if starting => "starting",
        task_worktree::SlotState::Occupied => "active",
        task_worktree::SlotState::AwaitingReview => "awaiting review",
        task_worktree::SlotState::Free | task_worktree::SlotState::Released => "released",
    }
}

fn refuse_unpublished_same_task_correction(
    home: &Path,
    source: &Path,
    task: &str,
    selected: &RetainedCompletion,
) -> io::Result<()> {
    let Some(record) = task_worktree::load_slot_record(home, source, selected.slot)? else {
        return Ok(());
    };
    if matches!(
        record.state,
        task_worktree::SlotState::Free | task_worktree::SlotState::Released
    ) || record.task.as_deref() != selected.task.as_deref()
    {
        return Ok(());
    }
    // A settled binding is fully owned by the assignment-aware check above.
    // This observation only covers the claim-to-bind window and a recorded
    // startup reservation, where no current receipt can prove completion.
    if record.assignment.is_some() && record.startup.is_none() {
        return Ok(());
    }
    let current_run = current_run_generation(home, source, selected.slot)?;
    let current = current_run
        .as_deref()
        .map(|run| format!("current run {run}"))
        .unwrap_or_else(|| "no current run generation is recorded".to_owned());
    let assignment = match record.assignment.as_deref() {
        Some(active) => format!("assignment {}", active.assignment_sha256),
        None => "no board assignment is published".to_owned(),
    };
    let startup = if record.startup.is_some() {
        "; a startup reservation is recorded, so the current run may not have written its receipt yet"
    } else {
        ""
    };
    Err(invalid(&format!(
        "task {task} has an unresolved executor correction in slot {}: task '{}', handle '{}', state {}, {assignment}, {current}{startup}. Retained completion {} cannot be accepted while that same-task work is unfinished; finish or release that slot, then accept the newest result. Explicit --run does not bypass it. No board metadata was changed.",
        selected.slot,
        record.task.as_deref().unwrap_or("none"),
        record.handle.as_deref().unwrap_or("none"),
        slot_state_label(record.state, record.startup.is_some()),
        selected.run_generation
    )))
}

fn current_run_generation(home: &Path, source: &Path, slot: u32) -> io::Result<Option<String>> {
    let receipt = task_worktree::pool_state_dir(home, source)?.join(format!("spawn-{slot}.json"));
    let bytes = match fs::read(&receipt) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|error| {
        invalid(&format!(
            "slot {slot} has an unresolved board assignment, but its current run receipt {} is not readable: {error}",
            receipt.display()
        ))
    })?;
    let generation = value
        .get("originatingLead")
        .and_then(|lead| lead.get("runGeneration"))
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|generation| !generation.is_empty())
        .ok_or_else(|| {
            invalid(&format!(
                "slot {slot} has an unresolved board assignment, but its current run receipt {} records no exact generation",
                receipt.display()
            ))
        })?;
    Ok(Some(generation.to_owned()))
}

fn candidate_note(retained: &[RetainedCompletion]) -> String {
    if retained.is_empty() {
        return String::new();
    }
    let mut rows: Vec<String> = retained
        .iter()
        .rev()
        .take(MAX_CANDIDATES)
        .map(|record| {
            let purpose = record
                .assignment
                .as_ref()
                .and_then(|binding| binding.purpose.as_deref())
                .unwrap_or("unknown");
            format!(
                "{} recorded={}ms eligible={} purpose={} reasons={}",
                record.run_generation,
                record.recorded_ms,
                record.eligibility.eligible,
                purpose,
                if record.eligibility.reasons.is_empty() {
                    "none".to_owned()
                } else {
                    record.eligibility.reasons.join("; ")
                }
            )
        })
        .collect();
    if retained.len() > MAX_CANDIDATES {
        rows.push(format!(
            "{} older retained completions omitted",
            retained.len() - MAX_CANDIDATES
        ));
    }
    format!(
        "\n  --run candidates (newest first):\n    {}",
        rows.join("\n    ")
    )
}

fn validate_completion(
    bd: &Path,
    project: &Path,
    issue: &serde_json::Value,
    retained: &RetainedCompletion,
    purpose: Purpose,
    candidates: &[RetainedCompletion],
) -> io::Result<()> {
    let assignment = retained.assignment.as_ref().ok_or_else(|| {
        invalid(&format!(
            "retained completion {} has no verified board assignment",
            retained.run_generation
        ))
    })?;
    let output_role = match retained.role.as_deref() {
        Some(role @ ("executor" | "lead")) => role,
        Some(role) => {
            return Err(ineligible(
                retained,
                candidates,
                &format!("the run role '{role}' is not an executor or Lead output"),
            ));
        }
        None => {
            return Err(ineligible(
                retained,
                candidates,
                "the run records no output role",
            ));
        }
    };
    if retained.state != "completed" || retained.exit_code != Some(0) {
        return Err(ineligible(
            retained,
            candidates,
            "the run is not a completed zero-exit assignment",
        ));
    }
    if retained
        .task
        .as_deref()
        .is_none_or(|task| task.trim().is_empty())
        || retained
            .handle
            .as_deref()
            .is_none_or(|handle| handle.trim().is_empty())
    {
        return Err(ineligible(
            retained,
            candidates,
            "the run records no durable task binding and generation handle",
        ));
    }
    if assignment.issue != retained_task(issue) {
        return Err(invalid(&format!(
            "retained completion {} is bound to board task {}, not {}",
            retained.run_generation,
            assignment.issue,
            retained_task(issue)
        )));
    }
    if assignment.assignment_sha256.len() != 64
        || !assignment
            .assignment_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(ineligible(
            retained,
            candidates,
            "the assignment binding has no exact sha256 assignment identity",
        ));
    }
    if assignment
        .effective_purpose()
        .map_err(|error| invalid(&format!("retained assignment purpose is invalid: {error}")))?
        != Some(purpose)
    {
        return Err(invalid(&format!(
            "retained completion {} records purpose {}, but board task {} records {}; stale purpose is never accepted",
            retained.run_generation,
            assignment.purpose.as_deref().unwrap_or("unknown"),
            assignment.issue,
            purpose.as_str()
        )));
    }
    let current = delivery_reflection::board_source_identity(bd, project)?;
    if !assignment.source.same_scope(&current) {
        return Err(invalid(&format!(
            "retained completion {} was bound to board scope {} / {} / {} (repo_root {}), not the current {} / {} / {} (repo_root {}); stale board source is never accepted",
            retained.run_generation,
            assignment.source.project_id,
            assignment.source.beads_dir,
            assignment.source.database,
            assignment.source.repo_root,
            current.project_id,
            current.beads_dir,
            current.database,
            current.repo_root
        )));
    }
    match retained.submitted_assignment.as_ref() {
        Some(submitted)
            if submitted.kind == task_worktree::AssignmentSubmission::STRUCTURED
                && submitted.assignment_sha256.as_deref()
                    == Some(assignment.assignment_sha256.as_str()) => {}
        _ => {
            return Err(ineligible(
                retained,
                candidates,
                "the run did not submit the bound structured assignment",
            ));
        }
    }
    if retained
        .session
        .as_deref()
        .is_none_or(|session| session.trim().is_empty())
        || retained
            .turn
            .as_deref()
            .is_none_or(|turn| turn.trim().is_empty())
    {
        return Err(ineligible(
            retained,
            candidates,
            "the run records no exact executor session and completed turn",
        ));
    }
    let Some(result) = retained.result.as_ref() else {
        return Err(ineligible(
            retained,
            candidates,
            "the run retains no readable result artifact",
        ));
    };
    if result.sha256.len() != 64
        || !result.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
        || retained
            .source
            .revision
            .as_deref()
            .is_none_or(|revision| revision.trim().is_empty())
        || retained.source.dirty != Some(false)
    {
        return Err(ineligible(
            retained,
            candidates,
            "the run records no clean accepted revision and result identity",
        ));
    }
    let mut tolerated = Vec::new();
    if output_role == "lead" {
        tolerated.push("the run role is 'lead', not an ordinary executor".to_owned());
    }
    if purpose != Purpose::Ordinary {
        tolerated.push(format!(
            "board task '{}' purpose '{}' is not ordinary",
            assignment.issue,
            purpose.as_str()
        ));
    }
    let hierarchy_eligible = if tolerated.is_empty() {
        retained.eligibility.eligible && retained.eligibility.reasons.is_empty()
    } else {
        !retained.eligibility.eligible
            && retained.eligibility.reasons.as_slice() == tolerated.as_slice()
    };
    if !hierarchy_eligible {
        let reasons = retained.eligibility.reasons.join("; ");
        return Err(ineligible(
            retained,
            candidates,
            &format!(
                "the retained {output_role} completion has acceptance-ineligible facts beyond the allowed hierarchy and purpose: {}",
                if reasons.is_empty() {
                    "eligibility is true without the expected role or purpose fact".to_owned()
                } else {
                    reasons
                }
            ),
        ));
    }
    Ok(())
}

fn retained_task(issue: &serde_json::Value) -> &str {
    issue
        .get("id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
}

fn ineligible(
    retained: &RetainedCompletion,
    candidates: &[RetainedCompletion],
    reason: &str,
) -> io::Error {
    invalid(&format!(
        "retained completion {} is not acceptable: {reason}. No board metadata was changed.{}",
        retained.run_generation,
        candidate_note(candidates)
    ))
}
