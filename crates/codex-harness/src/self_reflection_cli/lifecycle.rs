//! Normal CLI inputs over the existing board lifecycle and native fork runner.
//!
//! Task, source-turn, result, route and return identities are resolved by their
//! owners. Operators never construct the low-level fork request themselves.

use super::*;
use harness_core::delivery_reflection::{
    ActivationRequest, JournalPolicy, LifecycleStatus, ReflectionController, ReflectionRecord,
};

const USAGE: &str = "codex-harness delivery-improvement enable|status|disable|process --project DIRECTORY [--source KIT] [--codex-home DIRECTORY]\nRun enable/process from the real managed lead conversation. Enable binds the current board journal head; process handles one pending acceptance. Status is model-free. Disable preserves results and stops new stages.";

struct Options {
    command: String,
    project: PathBuf,
    source: PathBuf,
    home: PathBuf,
}

impl Options {
    fn parse(args: &[OsString]) -> io::Result<Self> {
        let command = args
            .first()
            .and_then(|arg| arg.to_str())
            .unwrap_or_default();
        if !matches!(command, "enable" | "status" | "disable" | "process") {
            return Err(invalid(USAGE));
        }
        let mut values = BTreeMap::new();
        let mut pairs = args[1..].chunks_exact(2);
        for pair in &mut pairs {
            let name = pair[0].to_str().unwrap_or_default();
            if !matches!(name, "--project" | "--source" | "--codex-home")
                || values.insert(name, PathBuf::from(&pair[1])).is_some()
            {
                return Err(invalid(format!(
                    "unknown or repeated option {name}; {USAGE}"
                )));
            }
        }
        if !pairs.remainder().is_empty() {
            return Err(invalid(format!("an option has no value; {USAGE}")));
        }
        let project = values
            .remove("--project")
            .unwrap_or(std::env::current_dir()?);
        let home = values
            .remove("--codex-home")
            .map_or_else(harness_core::native_launcher::codex_home, Ok)?;
        let source = values
            .remove("--source")
            .or_else(|| crate::feedback_cli::installed_kit_source_in(&home))
            .unwrap_or_else(|| project.clone());
        Ok(Self {
            command: command.to_owned(),
            project: canonical_dir(&project, "consumer project")?,
            source: canonical_dir(&source, "kit source")?,
            home: canonical_dir(&home, "CODEX_HOME")?,
        })
    }

    fn controller(&self) -> io::Result<ReflectionController> {
        Ok(ReflectionController::new(
            crate::feedback_cli::discover_bd()?,
            &self.project,
            &self.home,
        ))
    }

    fn lead(&self) -> io::Result<String> {
        let thread = std::env::var("CODEX_THREAD_ID").ok()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| invalid("CODEX_THREAD_ID is missing; run this command from the managed lead conversation"))?;
        match control::registered_lead(&self.home, &thread) {
            control::RegisteredLead::Live(_) => Ok(thread),
            control::RegisteredLead::Unavailable(reason) => Err(invalid(format!(
                "the real lead is unavailable: {reason}; start or resume it with `codex-harness lead start --session THREAD`"
            ))),
        }
    }
}

pub(crate) fn run(args: &[OsString]) -> io::Result<i32> {
    if args.is_empty() || args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!("{USAGE}");
        return Ok(0);
    }
    let options = Options::parse(args)?;
    let controller = options.controller()?;
    match options.command.as_str() {
        "status" => status(&controller),
        "enable" => {
            let lead = options.lead()?;
            // Resolve both roles and configured routes before changing journal
            // configuration. Actual stage startup revalidates these identities.
            let config = orchestration_config::load(&options.source)?;
            declarations(&options, &config)?;
            let launcher = options.home.join("harness/bin/codex.exe");
            if !launcher.is_file() {
                return Err(invalid(
                    "the installed Codex launcher is missing; repair the installation before enabling reflection",
                ));
            }
            if let Some(state) = controller.stored_state()?
                && state.binding.lead_session != lead
            {
                return Err(invalid(
                    "reflection belongs to another lead session; reconcile the retained lifecycle before transferring it",
                ));
            }
            let receipt = controller.activate(&ActivationRequest {
                lead_session: lead,
                run: None,
                boundary: None,
                journal: JournalPolicy::EnableIfMissing,
                limits: config.reflection_limits,
            })?;
            println!("{}", serde_json::to_string(&receipt)?);
            Ok(0)
        }
        "disable" => {
            // Explicit stopping is also available when the lead has exited.
            // It cannot start any model work or discard retained outcomes.
            if controller.stored_state()?.is_none() {
                println!("{}", json!({"lifecycle": "inactive", "changed": false}));
            } else {
                let stopped = controller.stop()?;
                println!("{}", json!({"stopped": stopped}));
            }
            Ok(0)
        }
        "process" => process(&options, &controller),
        _ => unreachable!(),
    }
}

fn status(controller: &ReflectionController) -> io::Result<i32> {
    let Some(state) = controller.stored_state()? else {
        println!("{}", json!({"lifecycle": "inactive", "pending": 0}));
        return Ok(0);
    };
    println!(
        "{}",
        json!({
            "lifecycle": state.lifecycle,
            "binding": state.binding,
            "checkpoint": state.checkpoint,
            "journal": state.journal,
            "retained": state.records.len(),
            "pending": controller.pending_jobs()?.len(),
            "recovery": state.recovery,
            "stateFile": controller.state_file()?,
        })
    );
    Ok(0)
}

fn declarations(
    options: &Options,
    config: &orchestration_config::Orchestration,
) -> io::Result<(StageDeclaration, StageDeclaration)> {
    let declaration = |role: Role, profile: &str| -> io::Result<StageDeclaration> {
        let resolved = role_context::resolve(&options.source, role)?;
        let binding = reflection_binding(&options.home, profile)?;
        canonical_stage(
            &StageDeclaration {
                role: role.label().to_owned(),
                revision: resolved.revision,
                profile: binding.profile,
                model: binding.model.unwrap_or_default(),
                provider: binding.model_provider.unwrap_or_default(),
                effort: binding.reasoning_effort.unwrap_or_default(),
                checkout: options.project.clone(),
                title: format!("{} accepted work", role.label()),
            },
            role.label(),
        )
    };
    Ok((
        declaration(
            Role::ExecutorSelfReflection,
            &orchestration_config::executor_profile(config, None)?,
        )?,
        declaration(Role::LeadSelfReflection, &config.lead_profile)?,
    ))
}

fn request(
    options: &Options,
    controller: &ReflectionController,
    record: &ReflectionRecord,
) -> io::Result<Request> {
    let config = orchestration_config::load(&options.source)?;
    let (executor_stage, lead_stage) = declarations(options, &config)?;
    let binding = &record.binding;
    let required = |value: &Option<String>, name: &str| -> io::Result<String> {
        value.clone().filter(|value| !value.trim().is_empty())
            .ok_or_else(|| invalid(format!("accepted work has no {name}; retain the exact native acceptance identity before reflection")))
    };
    let state_file = controller.state_file()?;
    let state_dir = state_file.with_extension("").join(&record.acceptance_id);
    Ok(Request {
        schema: SCHEMA,
        task: binding.task.clone(),
        run: binding.run.clone(),
        acceptance: record.acceptance_id.clone(),
        source: options.source.clone(),
        codex_home: options.home.clone(),
        launcher: options.home.join("harness/bin/codex.exe"),
        state_dir,
        executor_source: Boundary {
            thread: required(&binding.executor_session, "executor session")?,
            last_turn: required(&binding.executor_turn, "completed executor turn")?,
        },
        lead_source: Boundary {
            thread: binding.lead_session.clone(),
            last_turn: required(&binding.lead_turn, "accepting lead turn")?,
        },
        executor_stage,
        lead_stage,
        lead_return: LeadReturn {
            endpoint: control::lead_endpoint_path(&options.home, &binding.lead_session)?,
            thread: binding.lead_session.clone(),
        },
        bound_seconds: DEFAULT_BOUND_SECONDS,
    })
}

fn process(options: &Options, controller: &ReflectionController) -> io::Result<i32> {
    let Some(state) = controller.stored_state()? else {
        return status(controller);
    };
    if state.lifecycle != LifecycleStatus::Active {
        return status(controller);
    }
    let lead = options.lead()?;
    let checked = controller.check_active_binding(&lead, None)?;
    if !checked.is_active() {
        return Err(invalid(checked.detail));
    }
    let polled = controller.poll()?;
    if let Some(recovery) = polled.recovery {
        return Err(invalid(format!(
            "the board needs reconciliation: {}",
            recovery.detail
        )));
    }
    let Some(job) = polled.jobs.first() else {
        println!(
            "{}",
            json!({"lifecycle": "active", "pending": 0, "modelCalls": 0})
        );
        return Ok(0);
    };
    let record = controller
        .record(&job.acceptance_id)?
        .ok_or_else(|| invalid("the queued acceptance disappeared"))?;
    let request = request(options, controller, &record)?;
    accepted::execute(request, &options.project)
}
