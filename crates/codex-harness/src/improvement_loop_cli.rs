//! `codex-harness improve`: one explicitly started, recoverable improvement
//! run.
//!
//! The controller consumes the integrated owners instead of duplicating them:
//! Beads (`bd`) owns hypothesis identity and decisions, OpenSpec owns the
//! planning contracts, the orchestration configuration owns the effective
//! dispatch profile, the build-selection owner owns prepared runtime
//! selection, and the visible executor dispatch owner opens every model
//! conversation. Comparison execution and frozen runtime preparation are
//! separate owners; phases that need them stay explicitly pending here until
//! an actual effect records them.
#![cfg(windows)]

use crate::executor_cli::{
    ConversationState, VisibleAccepted, VisibleConversation, conversation_state,
    dispatch_visible_conversation, executor_title, stop_owned_run,
};
use harness_core::board_feedback;
use harness_core::board_hypothesis;
use harness_core::build_identity;
use harness_core::build_selection;
use harness_core::improvement_loop::{
    Attempt, AttemptRole, AttemptState, Cursor, DispatchFacts, DispatchGate, EffectKind,
    ObservedOutcome, Phase, RemovalGate, ResumeReport, RunMutation, RunSpec, RunStore, SPEC_FILE,
    VariantSet, declared_removal_gate, dispatch_gate, dispatch_owner, frozen_removal_digest,
    now_ms, read_json, selection_gate, settle_completed_reuse, write_json_atomic,
};
use harness_core::improvement_spec::{OpenSpec, PlanningReceipt};
use harness_core::orchestration_config;
use harness_core::outcome_qualification::Qualification;
use serde_json::json;
use std::{ffi::OsString, io, path::Path, path::PathBuf, time::Duration};

const USAGE: &str = "\
codex-harness improve start --run DIRECTORY --spec FILE
codex-harness improve status --run DIRECTORY [--json]
codex-harness improve select --run DIRECTORY --variant baseline|candidate
codex-harness improve stop --run DIRECTORY [--reason TEXT]
codex-harness improve resume --run DIRECTORY

One explicitly started, durable improvement run. Beads owns the hypothesis and
its decisions, OpenSpec owns the planning artifacts, the installed profile
binding owns the effective model, and every model conversation is opened by the
visible executor dispatch owner with its own titled surface. Comparison
execution and frozen runtime preparation stay separate owners; phases needing
them remain pending until an actual effect records them.

start validates the explicit run inputs (project, board, OpenSpec target, base
revision, writable scope, runner profile, oracle, publication scope and any
removal scope), refuses duplicate run ownership, qualifies the linked OpenSpec
change through the installed CLI, creates the private run directory and
performs model-free preparation. When the runner, visibility, qualification and
removal gates are all satisfied it dispatches the bounded investigator
conversation through the visible owner; otherwise it records the exact missing
fact as blocked and starts no model work.

status prints the recoverable phase cursor: current phase and condition, the
hypothesis card, the qualified planning change, the effective runner binding,
the dispatch gate, the removal gate, every attempt with its receipt, the
selected prepared variant and the phases still pending. It performs no model
call. --json prints the same report as JSON.

select activates an already prepared baseline/candidate runtime through the
build-selection owner, records its effective identity and performs no model
call, build or source edit. It refuses while a measured attempt is active or
unreconciled, and a candidate that is a removal treatment additionally needs
the current experimental removal authority.

stop suspends new work, preserves every attempt and marks in-flight attempts
unknown so resume never replays them. resume takes over a stopped or
interrupted run, reconciles recorded receipts (a completed arm is reused only
while its planning inputs still validate), re-resolves the current removal
authority, and reports the recovered phase and next action.

The run inputs are strict schema 1 JSON: {\"schema\":1,\"run\":\"ID\",
\"project\":\"DIRECTORY\",\"codex_home\":\"DIRECTORY\",
\"board\":{\"bd\":\"FILE\",\"project\":\"DIRECTORY\"},
\"specification\":{\"project\":\"DIRECTORY\",\"change\":\"NAME\",\"store\":null,
\"planning_root\":\"DIRECTORY\"},\"hypothesis_item\":\"BD-ID\",
\"experiment\":{...ExperimentContract...},\"base_revision\":\"REV\",
\"writable_scope\":[\"relative/path\"],\"runner\":{\"profile\":\"NAME\",
\"model\":null,\"model_provider\":null,\"reasoning_effort\":null},
\"local_runner\":null,\"qualification\":null,
\"publication_scope\":[\"experiment\"],\"oracle\":\"REFERENCE\",
\"removal\":null}. Private run data stays outside tracked source: the run
directory, the spec file and every retained receipt are local inputs.
";

pub fn run(args: &[OsString]) -> io::Result<i32> {
    if args.first().is_some_and(|arg| arg == "--help") {
        println!("{USAGE}");
        return Ok(0);
    }
    match args.first().and_then(|arg| arg.to_str()) {
        Some("start") => start(&args[1..]),
        Some("status") => status(&args[1..]),
        Some("select") => select(&args[1..]),
        Some("stop") => stop(&args[1..]),
        Some("resume") => resume(&args[1..]),
        _ => Err(invalid(
            "invalid improve options; use codex-harness improve --help",
        )),
    }
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

/// `--name value` options plus the declared boolean flags, each at most once.
struct Options {
    values: Vec<(String, String)>,
    flags: Vec<String>,
}

impl Options {
    fn parse(
        args: &[OsString],
        values_allowed: &[&str],
        flags_allowed: &[&str],
    ) -> io::Result<Self> {
        let mut values: Vec<(String, String)> = Vec::new();
        let mut flags: Vec<String> = Vec::new();
        let mut iter = args.iter();
        while let Some(arg) = iter.next() {
            let key = arg
                .to_str()
                .ok_or_else(|| invalid("improve option names must be UTF-8"))?;
            if !key.starts_with("--") {
                return Err(invalid(format!(
                    "unexpected improve argument {key}; options are --name VALUE"
                )));
            }
            if flags_allowed.contains(&key) {
                if flags.iter().any(|existing| existing == key) {
                    return Err(invalid(format!("{key} is repeated")));
                }
                flags.push(key.to_owned());
                continue;
            }
            if !values_allowed.contains(&key) {
                return Err(invalid(format!("unknown improve option {key}")));
            }
            let value = iter
                .next()
                .ok_or_else(|| invalid(format!("{key} requires a value")))?
                .to_str()
                .ok_or_else(|| invalid(format!("{key} value must be UTF-8")))?;
            if values.iter().any(|(existing, _)| existing == key) {
                return Err(invalid(format!("{key} is repeated")));
            }
            values.push((key.to_owned(), value.to_owned()));
        }
        Ok(Self { values, flags })
    }

    fn get(&self, key: &str) -> Option<&str> {
        self.values
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.as_str())
    }

    fn required(&self, key: &str) -> io::Result<&str> {
        self.get(key)
            .ok_or_else(|| invalid(format!("{key} is required")))
    }

    fn has(&self, key: &str) -> bool {
        self.flags.iter().any(|flag| flag == key)
    }
}

struct Run {
    store: RunStore,
    spec: RunSpec,
    cursor: Cursor,
    /// The exclusive run-mutation guard. It is held for the whole mutating
    /// command and `None` only for the read-only `status` view.
    guard: Option<RunMutation>,
}

fn read_run(store: RunStore) -> io::Result<Run> {
    let spec = store.spec()?;
    let cursor = store.cursor()?;
    if cursor.run != spec.run {
        return Err(invalid(format!(
            "run state belongs to {} instead of {}",
            cursor.run, spec.run
        )));
    }
    if cursor.spec_digest != spec.digest()? {
        return Err(invalid(
            "the run inputs changed since start; dependent effects require a new run decision",
        ));
    }
    Ok(Run {
        store,
        spec,
        cursor,
        guard: None,
    })
}

/// Read-only view. `status` stays cheap and takes no mutation guard.
fn open_run(dir: &Path) -> io::Result<Run> {
    read_run(RunStore::open(dir)?)
}

/// Mutating view: the exclusive run-mutation guard is acquired *before* the
/// spec and cursor are read, so the caller's gates, effects and persisted
/// state all observe one fresh state under the guard. A concurrent mutation
/// waits bounded and then reports the busy owner instead of acting on stale
/// state.
fn open_run_locked(dir: &Path) -> io::Result<Run> {
    let (store, guard) = RunStore::open_locked(dir)?;
    let mut run = read_run(store)?;
    run.guard = Some(guard);
    Ok(run)
}

/// Records this command as the run's owner under the held mutation guard and
/// journals a takeover note when a previously recorded owner was replaced.
fn claim_ownership(run: &mut Run) -> io::Result<()> {
    let ownership = run.store.claim_ownership(&run.spec.run)?;
    if let Some(note) = ownership.takeover_note() {
        run.cursor.effect(EffectKind::OwnershipTaken, note);
    }
    Ok(())
}

fn comments(spec: &RunSpec) -> io::Result<Vec<String>> {
    board_feedback::list_comments(&spec.board.bd, &spec.board.project, &spec.hypothesis_item)
}

fn removal_state(spec: &RunSpec, cursor: &Cursor, comments: &[String]) -> Option<RemovalGate> {
    declared_removal_gate(
        spec,
        cursor,
        comments,
        board_hypothesis::RemovalAction::Experiment,
    )
}

/// The exact effective binding of the configured dispatch profile, plus the
/// launcher the titled surface needs.
struct Surface {
    launcher: PathBuf,
    binding_error: Option<String>,
    binding: Option<orchestration_config::ProfileBinding>,
}

fn surface(spec: &RunSpec) -> Surface {
    let launcher = spec.codex_home.join("harness/bin/codex.exe");
    let Some(runner) = &spec.runner else {
        return Surface {
            launcher,
            binding_error: None,
            binding: None,
        };
    };
    match orchestration_config::binding(&spec.codex_home, &runner.profile) {
        Err(error) => Surface {
            launcher,
            binding_error: Some(format!(
                "profile {} is not installed in {}: {error}",
                runner.profile,
                spec.codex_home.display()
            )),
            binding: None,
        },
        Ok(binding) => {
            let compare = |name: &str,
                           declared: &Option<String>,
                           actual: &Option<String>|
             -> Option<String> {
                match (declared, actual) {
                    (Some(declared), Some(actual)) if declared != actual => Some(format!(
                        "the declared {name} {declared} does not match the installed profile binding {actual}"
                    )),
                    (Some(declared), None) => Some(format!(
                        "the run declares {name} {declared} but the installed profile binds none"
                    )),
                    _ => None,
                }
            };
            let mut error = compare("model", &runner.model, &binding.model)
                .or_else(|| compare("provider", &runner.model_provider, &binding.model_provider));
            if let Some(local) = &spec.local_runner
                && binding.model.as_deref() != Some(local.model.as_str())
            {
                error = error.or_else(|| {
                    Some(format!(
                        "the dispatch profile binds model {} but the declared local runner serves {}",
                        binding.model.as_deref().unwrap_or("none"),
                        local.model
                    ))
                });
            }
            Surface {
                launcher,
                binding_error: error,
                binding: Some(binding),
            }
        }
    }
}

/// The qualification owner's refusal, when strict comparison inputs are
/// missing or unqualified. It blocks measured arms, not bounded research.
fn qualification_block(spec: &RunSpec) -> io::Result<Option<String>> {
    if spec.local_runner.is_none() {
        return Ok(None);
    }
    let Some(path) = &spec.qualification else {
        return Ok(Some(
            "no local qualification record path is declared".to_owned(),
        ));
    };
    if !path.is_file() {
        return Ok(Some(format!(
            "the local qualification record is not available at {}",
            path.display()
        )));
    }
    let qualification: Qualification = match read_json(path, 256 * 1024) {
        Ok(qualification) => qualification,
        Err(error) => {
            return Ok(Some(format!(
                "the local qualification record is unreadable: {error}"
            )));
        }
    };
    if qualification.qualified() {
        return Ok(None);
    }
    let mut parts = Vec::new();
    if !qualification.missing_identity.is_empty() {
        parts.push(format!(
            "missing material identity: {}",
            qualification.missing_identity.join(", ")
        ));
    }
    if !qualification.unfinished_attempts.is_empty() {
        parts.push(format!(
            "unfinished attempts: {}",
            qualification.unfinished_attempts.join(", ")
        ));
    }
    if !qualification.unverified_attempts.is_empty() {
        parts.push(format!(
            "unverified model metadata: {}",
            qualification.unverified_attempts.join(", ")
        ));
    }
    if !qualification.tool_exchange_missing.is_empty() {
        parts.push(format!(
            "missing tool exchange: {}",
            qualification.tool_exchange_missing.join(", ")
        ));
    }
    if !qualification.runner_mismatch.is_empty() {
        parts.push("the recorded runner identity drifted across attempts".to_owned());
    }
    if !qualification.missing_outputs.is_empty() {
        parts.push(format!(
            "missing required outputs: {}",
            qualification.missing_outputs.join(", ")
        ));
    }
    if !qualification.divergent_outputs.is_empty() {
        parts.push(format!(
            "divergent outputs: {}",
            qualification.divergent_outputs.join(", ")
        ));
    }
    if parts.is_empty() {
        parts.push("the qualification is blocked".to_owned());
    }
    Ok(Some(format!(
        "{} ({} of {} repeats observed)",
        parts.join("; "),
        qualification.observed_repeats,
        qualification.required_repeats
    )))
}

/// A surface loss recorded without a later explicit stop suspends new model
/// work until the run is stopped again or the owning dispatcher reconciles the
/// attempt.
fn surface_loss(cursor: &Cursor) -> Option<String> {
    let last_stop = cursor
        .effects
        .iter()
        .filter(|effect| effect.kind == EffectKind::StopRequested)
        .map(|effect| effect.at_ms)
        .max();
    for attempt in &cursor.attempts {
        if attempt.state != AttemptState::Interrupted {
            continue;
        }
        if last_stop.is_some_and(|stop| stop >= attempt.updated_ms) {
            continue;
        }
        return Some(
            attempt
                .reason
                .clone()
                .unwrap_or_else(|| "an attempt was interrupted".to_owned()),
        );
    }
    None
}

fn dispatch_facts(
    spec: &RunSpec,
    cursor: &Cursor,
    comments: &[String],
) -> io::Result<DispatchFacts> {
    let surface = surface(spec);
    Ok(DispatchFacts {
        runner_declared: spec.runner.is_some(),
        launcher: surface.launcher.is_file().then_some(surface.launcher),
        binding_error: surface.binding_error,
        qualification_block: qualification_block(spec)?,
        removal: removal_state(spec, cursor, comments),
        surface_loss: surface_loss(cursor),
    })
}

fn pending_phases() -> &'static str {
    "candidate-ready -> baseline-attempt -> candidate-attempt -> acceptance -> decision-recorded -> activation-confirmed (comparison execution and frozen runtime preparation are separate owners; this controller slice records them as pending until an actual effect exists)"
}

fn next_action(cursor: &Cursor, gate: &DispatchGate, removal: &Option<RemovalGate>) -> String {
    if cursor.phase == Phase::Stopped {
        return "the run is stopped; `improve resume` restores the suspended phase".to_owned();
    }
    let base = match gate {
        DispatchGate::Blocked { reason } => format!("resolve before dispatch: {reason}"),
        DispatchGate::Ready => format!(
            "dispatch may proceed through the visible owner for the next ready role; pending phases: {}",
            pending_phases()
        ),
    };
    if removal.is_some() && !matches!(removal, Some(RemovalGate::Authorized { .. })) {
        return format!("{base} (the declared removal keeps its own gate)");
    }
    base
}

fn print_attempts(cursor: &Cursor) -> String {
    if cursor.attempts.is_empty() {
        return "  (none recorded)".to_owned();
    }
    cursor
        .attempts
        .iter()
        .map(|attempt| {
            let mut line = format!(
                "  - id={} role={} state={} observed={} owner={} title=\"{}\" profile={}",
                attempt.id,
                attempt.role.as_str(),
                attempt.state.as_str(),
                observed_state(attempt),
                attempt.owner,
                attempt.title,
                attempt.profile
            );
            if let Some(receipt) = &attempt.receipt {
                line.push_str(&format!(" receipt={}", receipt.display()));
            }
            if let Some(reason) = &attempt.reason {
                line.push_str(&format!(" reason=\"{reason}\""));
            }
            if let Some(reuse) = &attempt.reuse_refused {
                line.push_str(&format!(" reuse-refused=\"{reuse}\""));
            }
            line
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The read-only receipt observation of one attempt. Status reports the
/// durable state and this authoritative observation side by side, so a
/// retained live conversation is never conflated with a truly unknown effect.
fn observed_state(attempt: &Attempt) -> String {
    let Some(receipt) = &attempt.receipt else {
        return "no receipt".to_owned();
    };
    match conversation_state(receipt) {
        Ok(ConversationState::Completed) => "completed".to_owned(),
        Ok(ConversationState::Failed(cause)) => named_observation("failed", &cause),
        Ok(ConversationState::Interrupted(cause)) => named_observation("interrupted", &cause),
        Ok(ConversationState::Stopped(cause)) => named_observation("stopped", &cause),
        Ok(ConversationState::Active) => "active(host live)".to_owned(),
        Ok(ConversationState::Unknown(reason)) => format!("unknown({reason})"),
        Ok(ConversationState::Missing) => "missing".to_owned(),
        Err(error) => format!("unreadable({error})"),
    }
}

fn named_observation(name: &str, cause: &str) -> String {
    if cause.is_empty() {
        name.to_owned()
    } else {
        format!("{name}({cause})")
    }
}

fn removal_text(gate: &Option<RemovalGate>) -> String {
    match gate {
        None => "none declared".to_owned(),
        Some(RemovalGate::Authorized { reviewed }) => {
            format!("authorized (experiment; reviewed digest {reviewed})")
        }
        Some(RemovalGate::Refused { .. }) => "refused by the user; not repeated".to_owned(),
        Some(RemovalGate::Withdrawn { .. }) => "approval withdrawn".to_owned(),
        Some(RemovalGate::Pending { reason }) => format!("pending: {reason}"),
    }
}

fn run_report(run: &Run) -> io::Result<serde_json::Value> {
    let board_comments = comments(&run.spec)?;
    let facts = dispatch_facts(&run.spec, &run.cursor, &board_comments)?;
    let gate = dispatch_gate(&run.cursor, AttemptRole::Investigator, &facts);
    let removal = removal_state(&run.spec, &run.cursor, &board_comments);
    let planning = run.store.planning()?;
    let variants = run.store.variants_path();
    Ok(json!({
        "schema": 1,
        "run": run.spec.run,
        "directory": run.store.root().display().to_string(),
        "phase": run.cursor.phase.as_str(),
        "condition": run.cursor.condition,
        "hypothesis_item": run.spec.hypothesis_item,
        "experiment": run.cursor.experiment,
        "planning": {
            "change": run.spec.specification.change,
            "change_root": planning.change_root.display().to_string(),
            "schema": planning.schema,
            "implementation_state": planning.implementation_state,
            "artifacts": planning.artifacts.len(),
            "contract_digest": planning.contract_digest,
        },
        "runner": run.spec.runner.as_ref().map(|runner| json!({
            "profile": runner.profile,
            "model": runner.model,
            "model_provider": runner.model_provider,
            "reasoning_effort": runner.reasoning_effort,
        })),
        "dispatch": match &gate {
            DispatchGate::Ready => json!({"state": "ready"}),
            DispatchGate::Blocked { reason } => json!({"state": "blocked", "reason": reason}),
        },
        "removal": match &removal {
            None => json!({"declared": false}),
            Some(gate) => json!({"declared": true, "gate": removal_text(&Some(gate.clone()))}),
        },
        "attempts": run.cursor.attempts.iter().map(|attempt| json!({
            "id": attempt.id,
            "role": attempt.role.as_str(),
            "state": attempt.state.as_str(),
            "observed": observed_state(attempt),
            "owner": attempt.owner,
            "title": attempt.title,
            "receipt": attempt.receipt.as_ref().map(|path| path.display().to_string()),
            "reason": attempt.reason,
            "reuse_refused": attempt.reuse_refused,
        })).collect::<Vec<_>>(),
        "variants": if variants.is_file() { "prepared" } else { "pending" },
        "selected_variant": run.cursor.selected_variant,
        "selected_runtime": run.cursor.selected_runtime.as_ref().map(|path| path.display().to_string()),
        "selected_identity": run.cursor.selected_identity,
        "pending_phases": pending_phases(),
        "next": next_action(&run.cursor, &gate, &removal),
    }))
}

fn print_report(run: &Run) -> io::Result<()> {
    println!(
        "improve run: {} at {}",
        run.spec.run,
        run.store.root().display()
    );
    println!(
        "phase: {}{}",
        run.cursor.phase.as_str(),
        run.cursor
            .condition
            .as_deref()
            .map(|condition| format!(" ({condition})"))
            .unwrap_or_default()
    );
    println!(
        "hypothesis: {} (card spec reference validated against change {})",
        run.spec.hypothesis_item, run.spec.specification.change
    );
    let planning = run.store.planning()?;
    println!(
        "planning: change={} root={} schema={} state={} artifacts={} contract={}",
        run.spec.specification.change,
        planning.change_root.display(),
        planning.schema,
        planning.implementation_state,
        planning.artifacts.len(),
        &planning.contract_digest[..16.min(planning.contract_digest.len())]
    );
    match &run.spec.runner {
        Some(runner) => {
            let surface = surface(&run.spec);
            match (&surface.binding, &surface.binding_error) {
                (Some(binding), _) => println!(
                    "runner: profile={} model={} provider={} effort={}",
                    binding.profile,
                    binding.model.as_deref().unwrap_or("unknown"),
                    binding.model_provider.as_deref().unwrap_or("unknown"),
                    binding.reasoning_effort.as_deref().unwrap_or("default")
                ),
                (None, Some(error)) => {
                    println!("runner: profile={} unusable: {error}", runner.profile)
                }
                _ => println!("runner: profile={}", runner.profile),
            }
        }
        None => {
            println!("runner: pending (no runner profile declared; model-free preparation only)")
        }
    }
    let board_comments = comments(&run.spec)?;
    let facts = dispatch_facts(&run.spec, &run.cursor, &board_comments)?;
    let gate = dispatch_gate(&run.cursor, AttemptRole::Investigator, &facts);
    match &gate {
        DispatchGate::Ready => println!("dispatch: ready through the visible owner"),
        DispatchGate::Blocked { reason } => println!("dispatch: blocked ({reason})"),
    }
    let removal = removal_state(&run.spec, &run.cursor, &board_comments);
    println!("removal: {}", removal_text(&removal));
    println!("attempts:\n{}", print_attempts(&run.cursor));
    println!(
        "variants: {}",
        if run.store.variants_path().is_file() {
            format!("prepared at {}", run.store.variants_path().display())
        } else {
            "pending (variants.json is written by the runtime-preparation owner)".to_owned()
        }
    );
    match (&run.cursor.selected_variant, &run.cursor.selected_runtime) {
        (Some(variant), Some(runtime)) => println!(
            "selected: variant={variant} runtime={} identity={}",
            runtime.display(),
            run.cursor.selected_identity.as_deref().unwrap_or("unknown")
        ),
        _ => println!("selected: none"),
    }
    println!("pending phases: {}", pending_phases());
    println!("next: {}", next_action(&run.cursor, &gate, &removal));
    Ok(())
}

fn start(args: &[OsString]) -> io::Result<i32> {
    let options = Options::parse(args, &["--run", "--spec"], &[])?;
    let run_dir = PathBuf::from(options.required("--run")?);
    let spec_path = PathBuf::from(options.required("--spec")?);
    if !run_dir.is_absolute() || !spec_path.is_absolute() {
        return Err(invalid("--run and --spec must be absolute paths"));
    }
    let spec = RunSpec::load(&spec_path)?;
    for (name, protected) in [
        ("the run project", &spec.project),
        ("the planning root", &spec.specification.planning_root),
    ] {
        if run_dir.starts_with(protected) {
            return Err(invalid(format!(
                "the run state directory {} is inside {name} ({}); private run records stay outside tracked source",
                run_dir.display(),
                protected.display()
            )));
        }
    }
    if run_dir.join(SPEC_FILE).exists() {
        return Err(invalid(format!(
            "run state already exists at {}; a second start would duplicate run ownership - use `improve resume` to take over the interrupted or stopped run",
            run_dir.display()
        )));
    }

    // The board is the hypothesis owner; the card must be an admitted
    // hypothesis whose declared spec reference is this run's change.
    let snapshot =
        board_hypothesis::load_card(&spec.board.bd, &spec.board.project, &spec.hypothesis_item)?;
    if !snapshot.labels.iter().any(|label| label == "hypothesis") {
        return Err(invalid(format!(
            "board item {} is not a hypothesis card: the hypothesis label is missing",
            spec.hypothesis_item
        )));
    }
    if matches!(snapshot.status.as_str(), "closed" | "deferred") {
        return Err(invalid(format!(
            "hypothesis card {} is {}; record a reconsideration basis before starting dependent implementation work",
            spec.hypothesis_item, snapshot.status
        )));
    }
    let Some(admission) = board_hypothesis::parse_admission(&snapshot.description) else {
        return Err(invalid(format!(
            "hypothesis card {} carries no recognized admission record; admit the hypothesis before starting a run",
            spec.hypothesis_item
        )));
    };
    let declared = admission.spec.unwrap_or_default().replace('\\', "/");
    if !declared.ends_with(&spec.specification.change) {
        return Err(invalid(format!(
            "hypothesis card {} references spec '{declared}' instead of the run's change '{}'; every implementation task needs its own complete OpenSpec change",
            spec.hypothesis_item, spec.specification.change
        )));
    }

    // The planning prerequisite is qualified through the installed OpenSpec
    // CLI before any dependent dispatch; an incomplete change starts nothing.
    let openspec = OpenSpec::default();
    let receipt = openspec.qualify(&spec.specification, &spec.experiment)?;
    spec.supervisor_gate(&run_dir, &receipt.change_root, &spec.oracle)?;

    let board_comments =
        board_feedback::list_comments(&spec.board.bd, &spec.board.project, &spec.hypothesis_item)?;
    let frozen = frozen_removal_digest(&spec, &board_comments);
    // The exclusive run-mutation guard is acquired before the run exists and
    // stays held across creation, ownership claiming, the gates, any dispatch
    // and the persisted state, so two concurrent starts cannot both create or
    // dispatch into this run.
    let (store, guard) = RunStore::lock_new(&run_dir)?;
    store.create_locked(&spec, &spec.digest()?, &receipt.change_root)?;
    store.save_planning(&receipt)?;
    let mut cursor = store.cursor()?;
    cursor.removal_frozen = frozen;
    cursor.effect(
        EffectKind::PlanningQualified,
        format!(
            "{} artifacts qualified; implementation state {}",
            receipt.artifacts.len(),
            receipt.implementation_state
        ),
    );
    let mut run = Run {
        store,
        spec,
        cursor,
        guard: Some(guard),
    };
    claim_ownership(&mut run)?;

    let facts = dispatch_facts(&run.spec, &run.cursor, &board_comments)?;
    let gate = dispatch_gate(&run.cursor, AttemptRole::Investigator, &facts);
    match gate {
        DispatchGate::Ready => {
            let dispatched = dispatch_investigator(&run, &receipt)?;
            run.cursor = run.store.cursor()?;
            print_report(&run)?;
            if dispatched {
                println!(
                    "dispatched: the bounded investigator conversation was accepted through the visible owner"
                );
            }
        }
        DispatchGate::Blocked { reason } => {
            run.cursor.effect(EffectKind::DispatchRefused, &reason);
            run.cursor.block(reason);
            run.store.save_cursor(&run.cursor)?;
            print_report(&run)?;
        }
    }
    Ok(0)
}

/// Dispatches the bounded investigator conversation through the visible
/// executor owner. Returns whether a conversation was accepted. A refusal
/// records the exact cause and never substitutes another route.
fn dispatch_investigator(run: &Run, receipt: &PlanningReceipt) -> io::Result<bool> {
    let mut cursor = run.cursor.clone();
    let surface = surface(&run.spec);
    let Some(binding) = surface.binding.clone() else {
        cursor.effect(
            EffectKind::DispatchRefused,
            "the profile binding is unavailable; no dispatch was attempted",
        );
        cursor.block("the dispatch profile binding is unavailable");
        run.store.save_cursor(&cursor)?;
        return Ok(false);
    };
    let runner = run
        .spec
        .runner
        .as_ref()
        .expect("ready gate requires a declared runner");
    let mut inputs = Vec::new();
    for path in receipt.artifacts.keys() {
        let Ok(relative) = path.strip_prefix(&run.spec.project) else {
            let reason = format!(
                "planning artifact {} lives outside the project checkout; the bounded assignment cannot reference it by relative path, so implementation dispatch stays pending",
                path.display()
            );
            cursor.effect(EffectKind::DispatchRefused, &reason);
            cursor.block(reason);
            run.store.save_cursor(&cursor)?;
            return Ok(false);
        };
        inputs.push(relative.to_string_lossy().replace('\\', "/"));
    }
    inputs.sort();
    inputs.dedup();
    let ordinal = cursor
        .attempts
        .iter()
        .filter(|attempt| attempt.role == AttemptRole::Investigator)
        .count() as u32
        + 1;
    let owner = dispatch_owner(&run.spec.run, AttemptRole::Investigator, ordinal);
    let title = executor_title(&runner.profile, &owner);
    let attempt_id = format!("investigator-{ordinal}");
    let assignment_path = run
        .store
        .assignments_dir()
        .join(format!("{attempt_id}.json"));
    let assignment = json!({
        "schema": 1,
        "objective": format!(
            "Produce a bounded implementation plan for hypothesis {} against the qualified planning change {}: name the exact writable-scope changes, the independent acceptance path, the existing owners to reuse and concrete risks or counterexamples. Do not edit source; report the plan and any missing input.",
            run.spec.hypothesis_item, run.spec.specification.change
        ),
        "inputs": inputs,
        "outputs": run.spec.writable_scope,
        "invariants": [
            "keep every change inside the declared writable scope",
            "the planning artifacts, the independent oracle and the run state stay read-only",
        ],
        "acceptance": [run.spec.experiment.independent_acceptance],
        "consumer": "the improvement controller (codex-harness improve)",
        "escalate": [],
    });
    write_json_atomic(&assignment_path, &assignment)?;
    let attempt = Attempt {
        id: attempt_id.clone(),
        role: AttemptRole::Investigator,
        owner: owner.clone(),
        title: title.clone(),
        profile: binding.profile.clone(),
        model: binding.model.clone(),
        model_provider: binding.model_provider.clone(),
        reasoning_effort: binding.reasoning_effort.clone(),
        checkout: None,
        assignment: Some(assignment_path.clone()),
        receipt: None,
        result: None,
        detail: None,
        state: AttemptState::Requested,
        reason: None,
        reuse_refused: None,
        started_ms: now_ms(),
        updated_ms: now_ms(),
    };
    if let Err(error) = cursor.push_attempt(attempt) {
        cursor.effect(EffectKind::DispatchRefused, error.to_string());
        cursor.block(error.to_string());
        run.store.save_cursor(&cursor)?;
        return Ok(false);
    }
    cursor.effect(
        EffectKind::DispatchPrepared,
        format!(
            "owner={owner} title=\"{title}\" assignment={}",
            assignment_path.display()
        ),
    );
    run.store.save_cursor(&cursor)?;

    match dispatch_visible_conversation(&VisibleConversation {
        codex_home: run.spec.codex_home.clone(),
        source: run.spec.project.clone(),
        owner: owner.clone(),
        profile: binding.profile.clone(),
        base: Some(run.spec.base_revision.clone()),
        assignment: assignment_path,
    }) {
        Ok(accepted) => {
            record_accepted(&mut cursor, &attempt_id, &accepted);
            run.store.save_cursor(&cursor)?;
            Ok(true)
        }
        Err(error) => {
            let reason = format!(
                "dispatch refused before submission: {error}; no fallback was attempted and no model request was made"
            );
            if let Some(attempt) = cursor
                .attempts
                .iter_mut()
                .find(|attempt| attempt.id == attempt_id)
            {
                attempt.state = AttemptState::Failed;
                attempt.reason = Some(reason.clone());
                attempt.updated_ms = now_ms();
            }
            cursor.effect(EffectKind::DispatchRefused, &reason);
            cursor.block(reason);
            run.store.save_cursor(&cursor)?;
            Ok(false)
        }
    }
}

fn record_accepted(cursor: &mut Cursor, attempt_id: &str, accepted: &VisibleAccepted) {
    if let Some(attempt) = cursor
        .attempts
        .iter_mut()
        .find(|attempt| attempt.id == attempt_id)
    {
        attempt.state = AttemptState::Started;
        attempt.title = accepted.title.clone();
        attempt.checkout = Some(accepted.checkout.clone());
        attempt.receipt = Some(accepted.receipt.clone());
        attempt.result = Some(accepted.result.clone());
        attempt.detail = Some(accepted.detail.clone());
        attempt.model = accepted.model.clone();
        attempt.model_provider = accepted.model_provider.clone();
        attempt.reasoning_effort = accepted.reasoning_effort.clone();
        attempt.updated_ms = now_ms();
    }
    cursor.effect(
        EffectKind::DispatchAccepted,
        format!(
            "attempt={attempt_id} owner={} title=\"{}\" receipt={} coverage=native",
            accepted.owner,
            accepted.title,
            accepted.receipt.display()
        ),
    );
}

fn status(args: &[OsString]) -> io::Result<i32> {
    let options = Options::parse(args, &["--run"], &["--json"])?;
    let run = open_run(&PathBuf::from(options.required("--run")?))?;
    if options.has("--json") {
        println!("{}", serde_json::to_string_pretty(&run_report(&run)?)?);
        return Ok(0);
    }
    print_report(&run)?;
    Ok(0)
}

fn select(args: &[OsString]) -> io::Result<i32> {
    let options = Options::parse(args, &["--run", "--variant"], &[])?;
    let mut run = open_run_locked(&PathBuf::from(options.required("--run")?))?;
    let variant = options.required("--variant")?.to_owned();
    if !matches!(variant.as_str(), "baseline" | "candidate") {
        return Err(invalid("--variant is baseline or candidate"));
    }
    claim_ownership(&mut run)?;
    let board_comments = comments(&run.spec)?;
    let removal = removal_state(&run.spec, &run.cursor, &board_comments);
    selection_gate(&run.cursor, &variant, removal.as_ref()).map_err(invalid)?;

    let variants_path = run.store.variants_path();
    if !variants_path.is_file() {
        return Err(invalid(format!(
            "prepared runtimes are not available at {}; runtime preparation is a separate owner and stays pending - nothing was selected",
            variants_path.display()
        )));
    }
    let set = VariantSet::load(&variants_path)?;
    let entry = set
        .named(&variant)
        .cloned()
        .ok_or_else(|| invalid(format!("prepared variant {variant} is absent")))?;
    let record = build_identity::verify_record_integrity(&entry.build).map_err(|error| {
        invalid(format!(
            "the prepared {variant} runtime at {} is missing or stale: {error}; prepare it through the build lifecycle - nothing was selected",
            entry.build.display()
        ))
    })?;
    let identity = format!(
        "sha256:{}",
        &record.source.sha256[..16.min(record.source.sha256.len())]
    );
    if let Some(declared) = &entry.identity
        && declared != &identity
    {
        return Err(invalid(format!(
            "the prepared {variant} runtime identity {declared} does not match its build record {identity}; preparation must be refreshed - nothing was selected"
        )));
    }
    let selection = build_selection::activate(&entry.state, &entry.build).map_err(|error| {
        invalid(format!(
            "the runtime-selection owner refused variant {variant}: {error}"
        ))
    })?;
    let (effective, artifacts) = build_selection::selected(&entry.state)?;
    if !artifacts.check().runtime_allowed {
        return Err(invalid(format!(
            "the selected {variant} runtime at {} is not runtime-allowed: {}",
            effective.display(),
            artifacts.check().action
        )));
    }

    let mut cursor = run.cursor.clone();
    let changed = selection.changed
        || cursor.selected_variant.as_deref() != Some(variant.as_str())
        || cursor.selected_runtime.as_deref() != Some(effective.as_path());
    cursor.selected_variant = Some(variant.clone());
    cursor.selected_runtime = Some(effective.clone());
    cursor.selected_identity = Some(identity.clone());
    if changed {
        cursor.effect(
            EffectKind::VariantSelected,
            format!(
                "variant={variant} runtime={} identity={identity} (no model call, no build, no source edit)",
                effective.display()
            ),
        );
    }
    run.store.save_cursor(&cursor)?;
    println!(
        "improve select: variant={variant} runtime={} identity={identity} changed={changed} (no model call, no build, no source edit)",
        effective.display()
    );
    Ok(0)
}

fn timeout_option(value: Option<&str>) -> io::Result<Duration> {
    match value {
        None => Ok(Duration::from_secs(30)),
        Some(text) => {
            let seconds: u64 = text
                .parse()
                .map_err(|_| invalid("--timeout must be a whole number of seconds"))?;
            if !(1..=600).contains(&seconds) {
                return Err(invalid("--timeout must be between 1 and 600 seconds"));
            }
            Ok(Duration::from_secs(seconds))
        }
    }
}

/// The pooled slot an attempt's dispatch receipt records for the exact owner.
/// `None` means this attempt's effect cannot be located as one owned pooled
/// run, so no process may be terminated on this evidence.
fn pooled_slot(receipt: &Path, owner: &str) -> Option<u32> {
    let value: serde_json::Value = serde_json::from_slice(&std::fs::read(receipt).ok()?).ok()?;
    let slot = value.get("slot")?;
    let index = slot.get("index")?.as_u64()?;
    let recorded_owner = slot.get("owner").and_then(|value| value.as_str())?;
    (recorded_owner == owner && index > 0 && index <= u32::MAX as u64).then_some(index as u32)
}

fn terminal_outcome(state: &ConversationState) -> Option<ObservedOutcome> {
    match state {
        ConversationState::Completed => Some(ObservedOutcome::Completed),
        ConversationState::Failed(_) => Some(ObservedOutcome::Failed),
        ConversationState::Interrupted(_) => Some(ObservedOutcome::Interrupted),
        ConversationState::Stopped(_) => Some(ObservedOutcome::Stopped),
        ConversationState::Active | ConversationState::Unknown(_) | ConversationState::Missing => {
            None
        }
    }
}

fn state_reason(state: &ConversationState) -> Option<String> {
    match state {
        ConversationState::Completed => Some("the recorded run completed".to_owned()),
        ConversationState::Failed(cause) => Some(if cause.is_empty() {
            "the recorded run failed".to_owned()
        } else {
            cause.clone()
        }),
        ConversationState::Interrupted(cause) => Some(if cause.is_empty() {
            "the recorded run was interrupted".to_owned()
        } else {
            cause.clone()
        }),
        ConversationState::Stopped(cause) => Some(if cause.is_empty() {
            "the recorded run is stopped".to_owned()
        } else {
            cause.clone()
        }),
        ConversationState::Active | ConversationState::Unknown(_) | ConversationState::Missing => {
            None
        }
    }
}

/// Verified owned cleanup of every attempt whose effect may still exist. Each
/// known owned attempt is stopped through the exact-identity executor stop
/// owner, which verifies the slot binding and the recorded host identity and
/// terminates only that run's recorded process tree. Where cleanup cannot be
/// established - no receipt, no pooled address, a refused or partial stop, or
/// a receipt that records no terminal state - the effect stays explicitly
/// retained as unknown and is never replayed. Attempts that are already
/// terminal are left untouched.
fn cleanup_owned_attempts(run: &mut Run, timeout: Duration) -> Vec<(String, String)> {
    let ids: Vec<String> = run
        .cursor
        .attempts
        .iter()
        .filter(|attempt| attempt.state.is_in_flight() || attempt.state == AttemptState::Unknown)
        .map(|attempt| attempt.id.clone())
        .collect();
    let at = now_ms();
    let mut notes = Vec::new();
    for id in ids {
        let Some(index) = run
            .cursor
            .attempts
            .iter()
            .position(|attempt| attempt.id == id)
        else {
            continue;
        };
        let (receipt, owner) = {
            let attempt = &run.cursor.attempts[index];
            (attempt.receipt.clone(), attempt.owner.clone())
        };
        let Some(receipt) = receipt else {
            notes.push((
                id.clone(),
                "no dispatch receipt is recorded, so no owned effect can be located; the effect stays retained as unknown and is never replayed"
                    .to_owned(),
            ));
            run.cursor.effect(
                EffectKind::OwnedCleanup,
                format!("attempt={id}: no dispatch receipt; retained unknown"),
            );
            continue;
        };
        if !receipt.is_file() {
            notes.push((
                id.clone(),
                "the recorded dispatch receipt is missing, so no owned effect can be located; the effect stays retained as unknown and is never replayed"
                    .to_owned(),
            ));
            run.cursor.effect(
                EffectKind::OwnedCleanup,
                format!("attempt={id}: dispatch receipt missing; retained unknown"),
            );
            continue;
        }
        let Some(slot) = pooled_slot(&receipt, &owner) else {
            notes.push((
                id.clone(),
                "the dispatch receipt does not name this attempt's pooled slot and owner, so cleanup could not be established; the effect stays retained as unknown"
                    .to_owned(),
            ));
            run.cursor.effect(
                EffectKind::OwnedCleanup,
                format!("attempt={id}: no pooled address; retained unknown"),
            );
            continue;
        };
        let exit = stop_owned_run(
            &run.spec.project,
            &run.spec.codex_home,
            slot,
            &owner,
            timeout,
        );
        let observed = conversation_state(&receipt);
        let (outcome, note) = match (&exit, &observed) {
            (Err(error), _) => (
                None,
                format!(
                    "cleanup could not be established: {error}; the effect stays retained as unknown"
                ),
            ),
            (Ok(code), Ok(state)) => match terminal_outcome(state) {
                Some(outcome) => (
                    Some(outcome),
                    format!(
                        "owned stop of slot {slot} (exit {code}) verified: {}",
                        state_reason(state)
                            .unwrap_or_else(|| "settled from the receipt".to_owned())
                    ),
                ),
                None => {
                    let detail = match state {
                        ConversationState::Active => {
                            "the receipt still reports a live host".to_owned()
                        }
                        ConversationState::Unknown(reason) => reason.clone(),
                        ConversationState::Missing => "the receipt disappeared".to_owned(),
                        _ => "the receipt records no terminal state".to_owned(),
                    };
                    (
                        None,
                        format!(
                            "the owned stop of slot {slot} exited {code} but {detail}; the effect stays retained as unknown"
                        ),
                    )
                }
            },
            (Ok(code), Err(error)) => (
                None,
                format!(
                    "the owned stop of slot {slot} exited {code} and the receipt is unreadable: {error}; the effect stays retained as unknown"
                ),
            ),
        };
        {
            let attempt = &mut run.cursor.attempts[index];
            match outcome {
                Some(outcome) => {
                    attempt.settle(outcome, at);
                    attempt.reason = Some(note.clone());
                }
                None => {
                    if attempt.state != AttemptState::Unknown {
                        attempt.settle(ObservedOutcome::Unknown, at);
                    }
                    attempt.reason = Some(note.clone());
                }
            }
        }
        run.cursor.effect(
            EffectKind::OwnedCleanup,
            format!("attempt={id} owner={owner} slot={slot}: {note}"),
        );
        notes.push((id, note));
    }
    notes
}

fn stop(args: &[OsString]) -> io::Result<i32> {
    let options = Options::parse(args, &["--run", "--reason", "--timeout"], &[])?;
    let mut run = open_run_locked(&PathBuf::from(options.required("--run")?))?;
    claim_ownership(&mut run)?;
    let reason = options
        .get("--reason")
        .map(str::to_owned)
        .unwrap_or_else(|| "stopped through `improve stop`".to_owned());
    let timeout = timeout_option(options.get("--timeout"))?;

    // Every known owned effect is resolved through the exact-identity executor
    // stop owner; anything that cannot be verified stays retained as unknown.
    let cleanup = cleanup_owned_attempts(&mut run, timeout);

    let already_stopped = run.cursor.phase == Phase::Stopped;
    if !already_stopped {
        run.cursor.stop(&reason);
    }
    run.store.save_cursor(&run.cursor)?;

    if already_stopped {
        println!(
            "improve stop: run {} is already stopped; owned cleanup ran again for {} retained effect(s)",
            run.spec.run,
            cleanup.len()
        );
    } else {
        println!("improve stop: run {} stopped ({reason})", run.spec.run);
    }
    for (id, note) in &cleanup {
        println!("improve stop: attempt {id}: {note}");
    }
    let retained = run
        .cursor
        .attempts
        .iter()
        .filter(|attempt| attempt.state == AttemptState::Unknown)
        .count();
    if retained > 0 {
        println!(
            "improve stop: {retained} attempt(s) stay retained as unknown (their effect could not be verified); they are never replayed - reconcile them through `executor watch`/`executor stop` or resume after their receipt becomes terminal"
        );
    }
    Ok(0)
}

fn reconcile(run: &Run) -> io::Result<(Cursor, ResumeReport, Vec<String>)> {
    let mut cursor = run.cursor.clone();
    let mut report = ResumeReport::default();
    let mut notes = Vec::new();
    let planning_ok = OpenSpec::default()
        .revalidate(&run.store.planning()?)
        .map_err(|error| error.to_string());
    if let Err(reason) = &planning_ok {
        notes.push(format!(
            "planning revalidation failed: {reason}; dependent comparison requires preparation again"
        ));
    }
    let stale_planning = || "planning inputs changed since the attempt".to_owned();
    let at = now_ms();
    // In-flight attempts and attempts retained as unknown both reconcile from
    // their exact receipts: an authoritative terminal outcome settles them
    // once, a live host stays retained live, and a truly unobserved effect
    // stays unknown without ever being replayed.
    let reconcile_ids: Vec<String> = cursor
        .attempts_requiring_reconciliation()
        .iter()
        .map(|attempt| attempt.id.clone())
        .collect();
    for id in reconcile_ids {
        let Some(index) = cursor.attempts.iter().position(|attempt| attempt.id == id) else {
            continue;
        };
        let attempt = &mut cursor.attempts[index];
        let previous = attempt.state;
        let Some(receipt) = attempt.receipt.clone() else {
            if previous != AttemptState::Unknown {
                attempt.settle(ObservedOutcome::Unknown, at);
                attempt.reason = Some(
                    "no dispatch receipt was recorded; the outcome is unknown and the attempt is never resubmitted"
                        .to_owned(),
                );
            }
            report.unknown.push(attempt.id.clone());
            continue;
        };
        let state = conversation_state(&receipt)?;
        if let Some(outcome) = terminal_outcome(&state) {
            attempt.settle(outcome, at);
            attempt.reason = state_reason(&state);
            if outcome == ObservedOutcome::Completed
                && !settle_completed_reuse(
                    attempt,
                    planning_ok.clone().map_err(|_| stale_planning()),
                )
            {
                report.remeasure.push((
                    attempt.id.clone(),
                    attempt.reuse_refused.clone().unwrap_or_default(),
                ));
            }
            report.settled.push(attempt.id.clone());
            continue;
        }
        match state {
            ConversationState::Active => {
                if previous == AttemptState::Unknown {
                    attempt.reason = Some(
                        "retained live: the recorded host still runs; the outcome stays unknown until its receipt is terminal"
                            .to_owned(),
                    );
                    report.retained_live.push(attempt.id.clone());
                } else {
                    report.active.push(attempt.id.clone());
                }
            }
            ConversationState::Unknown(reason) => {
                if previous != AttemptState::Unknown {
                    attempt.settle(ObservedOutcome::Unknown, at);
                }
                attempt.reason = Some(reason);
                report.unknown.push(attempt.id.clone());
            }
            ConversationState::Missing => {
                if previous != AttemptState::Unknown {
                    attempt.settle(ObservedOutcome::Unknown, at);
                }
                attempt.reason = Some(
                    "the recorded dispatch receipt is missing; the outcome is unknown and the attempt is never resubmitted"
                        .to_owned(),
                );
                report.unknown.push(attempt.id.clone());
            }
            ConversationState::Completed
            | ConversationState::Failed(_)
            | ConversationState::Interrupted(_)
            | ConversationState::Stopped(_) => unreachable!("terminal states settled above"),
        }
    }
    // Completed attempts settle reuse against the current planning inputs.
    for attempt in &mut cursor.attempts {
        if attempt.state == AttemptState::Completed {
            let already = attempt.reuse_refused.is_some();
            if !settle_completed_reuse(attempt, planning_ok.clone().map_err(|_| stale_planning()))
                && !already
            {
                report.remeasure.push((
                    attempt.id.clone(),
                    attempt.reuse_refused.clone().unwrap_or_default(),
                ));
            }
        }
    }
    Ok((cursor, report, notes))
}

fn resume(args: &[OsString]) -> io::Result<i32> {
    let options = Options::parse(args, &["--run"], &[])?;
    let mut run = open_run_locked(&PathBuf::from(options.required("--run")?))?;
    claim_ownership(&mut run)?;
    let (mut cursor, report, notes) = reconcile(&run)?;
    cursor.resume_phase();
    cursor.effect(
        EffectKind::OwnershipTaken,
        format!("resume recovered phase {}", cursor.phase.as_str()),
    );
    for id in &report.settled {
        cursor.effect(
            EffectKind::Reconciled,
            format!("attempt {id} settled from its receipt"),
        );
    }
    for (id, why) in &report.remeasure {
        cursor.effect(
            EffectKind::RemeasurementRequired,
            format!("attempt {id} is not reusable: {why}"),
        );
    }

    // Removal authority is resolved again after resume, before any removal
    // effect; a pending, changed, refused or withdrawn decision blocks only
    // the dependent removal work.
    let board_comments = comments(&run.spec)?;
    let removal = removal_state(&run.spec, &cursor, &board_comments);
    if let Some(gate) = &removal {
        cursor.effect(
            EffectKind::RemovalChecked,
            removal_text(&Some(gate.clone())),
        );
    }

    let mut condition = report.condition();
    if notes
        .iter()
        .any(|note| note.starts_with("planning revalidation"))
    {
        let planning = notes.join("; ");
        condition = Some(match condition {
            Some(existing) => format!("{existing}; {planning}"),
            None => planning,
        });
    }
    let removal_blocks = matches!(
        &removal,
        Some(RemovalGate::Pending { .. })
            | Some(RemovalGate::Refused { .. })
            | Some(RemovalGate::Withdrawn { .. })
    );
    if removal_blocks {
        let removal_condition = format!("removal authority: {}", removal_text(&removal));
        condition = Some(match condition {
            Some(existing) => format!("{existing}; {removal_condition}"),
            None => removal_condition,
        });
    }
    match condition {
        Some(condition) => cursor.block(condition),
        None => cursor.clear_blocked(),
    }
    run.cursor = cursor;
    run.store.save_cursor(&run.cursor)?;

    print_report(&run)?;
    if !report.settled.is_empty() {
        println!(
            "resume: settled attempt(s) {} from their receipts; no model attempt was replayed",
            report.settled.join(", ")
        );
    }
    if !report.active.is_empty() {
        println!(
            "resume: attempt(s) {} are still active; their frozen runtime is kept",
            report.active.join(", ")
        );
    }
    if !report.retained_live.is_empty() {
        println!(
            "resume: attempt(s) {} are retained live: the recorded host still runs, the outcome stays unknown and the frozen runtime is kept",
            report.retained_live.join(", ")
        );
    }
    if !report.unknown.is_empty() {
        println!(
            "resume: attempt(s) {} have unknown outcomes; reconcile them through the owning dispatcher and never resubmit them",
            report.unknown.join(", ")
        );
    }
    if !report.remeasure.is_empty() {
        println!(
            "resume: changed conditions require remeasurement for attempt(s) {}",
            report
                .remeasure
                .iter()
                .map(|(id, _)| id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    for note in &notes {
        println!("resume: {note}");
    }
    Ok(0)
}
