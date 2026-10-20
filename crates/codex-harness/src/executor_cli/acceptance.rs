//! Normal lead acceptance for retained structured executor work.
//!
//! The live lead names only the board task. This owner derives the assignment,
//! result, executor session/turn and current lead turn from retained completion
//! state and the native control endpoint, then closes through the existing
//! board owner. It makes no model calls and never fabricates a turn or a
//! successful automatic reflection.

use super::{control, executor_input, invalid, option_text};
use harness_core::{
    board_cli,
    delivery_reflection::{
        self, ACCEPTANCE_METADATA, AcceptanceBinding, BindingStatus, Purpose, ReflectionController,
    },
    orchestration_config,
    task_worktree::{self, RetainedCompletion},
};
use serde_json::json;
use std::{
    env,
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
};

const HELP: &str = "\
codex-harness executor accept TASK --project DIRECTORY [--source CHECKOUT] [--codex-home DIRECTORY] [--run GENERATION]
  Accept retained structured executor work as the live lead. TASK is the exact
  board issue. The command authenticates the current managed lead turn, takes
  the newest retained completion for TASK unless --run names the exact
  generation, writes the acceptance binding through the board owner and closes
  the task with reason 'accepted'. It reports whether automatic reflection is
  eligible. Repeating the same accepted result is a no-op; --run resolves only
  an ambiguous timestamp tie and never bypasses a newer retained or unresolved
  correction.";

const MAX_CANDIDATES: usize = 6;

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
}

struct Accepted {
    binding: AcceptanceBinding,
    assignment: String,
    result: String,
    result_locator: Option<PathBuf>,
    revision: String,
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

    let controller = ReflectionController::new(&bd, &request.project, &home);
    let reflection = controller.check_active_binding(&lead.thread, None)?;
    refuse_unusable_reflection(&reflection)?;

    if issue.get("status").and_then(serde_json::Value::as_str) == Some("closed") {
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
    let metadata = delivery_reflection::acceptance_metadata(&binding)?;
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
    let active = record
        .get("turns")
        .and_then(serde_json::Value::as_array)
        .map(|turns| {
            turns
                .iter()
                .filter(|turn| {
                    turn.get("status").and_then(serde_json::Value::as_str) == Some("inProgress")
                })
                .collect::<Vec<_>>()
        })
        .ok_or_else(|| {
            invalid(&format!(
                "lead thread {thread} has no native turn list; acceptance cannot establish the exact current lead turn and refuses before changing the board"
            ))
        })?;
    if active.len() != 1 {
        return Err(invalid(&format!(
            "lead thread {thread} has {} in-progress native turns; acceptance requires exactly one exact current lead turn and refuses before changing the board",
            active.len()
        )));
    }
    let turn = control::active_turn(&record).ok_or_else(|| {
        invalid(&format!(
            "lead thread {thread} has an in-progress native turn without a usable exact turn identity; acceptance refuses before changing the board"
        ))
    })?;
    if active[0].get("id").and_then(serde_json::Value::as_str) != Some(turn.as_str()) {
        return Err(invalid(&format!(
            "lead thread {thread} has an in-progress native turn without a usable exact turn identity; acceptance refuses before changing the board"
        )));
    }
    Ok(LeadTurn { thread, turn })
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

    Ok(Accepted {
        executor_session: binding.executor_session.clone(),
        executor_turn: binding.executor_turn.clone(),
        binding,
        assignment: assignment.assignment_sha256.clone(),
        result: result.sha256.clone(),
        result_locator: Some(result.locator.clone()),
        revision,
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
    if retained.role.as_deref() != Some("executor") {
        return Err(ineligible(
            retained,
            candidates,
            "the run role is not executor",
        ));
    }
    if retained.state != "completed" || retained.exit_code != Some(0) {
        return Err(ineligible(
            retained,
            candidates,
            "the run is not a completed zero-exit executor run",
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
    if purpose == Purpose::Ordinary {
        if !retained.eligibility.eligible || !retained.eligibility.reasons.is_empty() {
            let reasons = retained.eligibility.reasons.join("; ");
            return Err(ineligible(
                retained,
                candidates,
                &format!(
                    "the retained completion is not ordinary acceptance-eligible: {}",
                    if reasons.is_empty() {
                        "eligibility is false without a reason".to_owned()
                    } else {
                        reasons
                    }
                ),
            ));
        }
    } else {
        let tolerated = format!(
            "board task '{}' purpose '{}' is not ordinary",
            assignment.issue,
            purpose.as_str()
        );
        if retained.eligibility.eligible
            || retained.eligibility.reasons.as_slice() != [tolerated.as_str()]
        {
            let reasons = retained.eligibility.reasons.join("; ");
            return Err(ineligible(
                retained,
                candidates,
                &format!(
                    "the retained {} completion has acceptance-ineligible facts beyond its nonordinary purpose: {}",
                    purpose.as_str(),
                    if reasons.is_empty() {
                        "eligibility is true without the expected nonordinary-purpose fact"
                            .to_owned()
                    } else {
                        reasons
                    }
                ),
            ));
        }
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
