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
        if !matches!(
            command,
            "enable" | "status" | "disable" | "process" | "_serve"
        ) {
            return Err(invalid(USAGE));
        }
        let mut values = BTreeMap::new();
        let (pairs, remainder) = args[1..].as_chunks::<2>();
        for pair in pairs {
            let name = pair[0].to_str().unwrap_or_default();
            if !matches!(name, "--project" | "--source" | "--codex-home")
                || values.insert(name, PathBuf::from(&pair[1])).is_some()
            {
                return Err(invalid(format!(
                    "unknown or repeated option {name}; {USAGE}"
                )));
            }
        }
        if !remainder.is_empty() {
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
            crate::feedback_cli::discover_bd_in(&self.home)?,
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
    if options.command == "_serve" {
        return serve(&options);
    }
    let controller = options.controller()?;
    match options.command.as_str() {
        "status" => status(&controller, &options.home),
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
        "process" => process(&options, &controller, false),
        _ => unreachable!(),
    }
}

fn status(controller: &ReflectionController, home: &Path) -> io::Result<i32> {
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
            "lastPoll": state.last_poll,
            "stateFile": controller.state_file()?,
            "host": host::status(home, &state.binding.lead_session)?,
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

fn process(options: &Options, controller: &ReflectionController, quiet: bool) -> io::Result<i32> {
    let Some(state) = controller.stored_state()? else {
        return status(controller, &options.home);
    };
    if state.lifecycle != LifecycleStatus::Active {
        return status(controller, &options.home);
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
        if !quiet {
            println!(
                "{}",
                json!({"lifecycle": "active", "pending": 0, "modelCalls": 0})
            );
        }
        return Ok(0);
    };
    let record = controller
        .record(&job.acceptance_id)?
        .ok_or_else(|| invalid("the queued acceptance disappeared"))?;
    let request = request(options, controller, &record)?;
    accepted::execute(request, &options.project)
}

fn serve(options: &Options) -> io::Result<i32> {
    let lead = options.lead()?;
    if std::env::var("HARNESS_REFLECTION_HOST").as_deref() != Ok(lead.as_str()) {
        return Err(invalid(
            "the journal consumer must be started by its owning managed lead host",
        ));
    }
    let lease = options
        .home
        .join("harness/delivery-reflection/hosts")
        .join(format!("{}.lock", digest(&lead)));
    fs::create_dir_all(lease.parent().unwrap())?;
    let _lease =
        harness_core::process::ExclusiveFileLock::try_acquire(&lease)?.ok_or_else(|| {
            invalid("this lead already owns a journal consumer; no second consumer started")
        })?;
    loop {
        // Reading endpoint state and native journal events is model-free.
        // Every model stage still needs the shared run admission and a ready
        // visible frontend. A lost lead cannot start another stage.
        options.lead()?;
        let boards = host::active_boards(&options.home, &lead)?;
        if boards.is_empty() {
            return Ok(0);
        }
        for board in boards {
            let options = Options {
                command: "process".to_owned(),
                project: canonical_dir(&board.project, "consumer project")?,
                source: options.source.clone(),
                home: options.home.clone(),
            };
            let controller = options.controller()?;
            if controller.state_file()? != board.state_file {
                return Err(invalid(
                    "the retained board lifecycle no longer matches its native board source; reconcile the source before continuation",
                ));
            }
            match process(&options, &controller, true) {
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                Err(error) => return Err(error),
            }
        }
        thread::sleep(host::POLL_INTERVAL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_default_provider_does_not_require_editing_the_ordinary_lead_profile() {
        let home = tempfile::tempdir().unwrap();
        fs::write(
            home.path().join("config.toml"),
            "model = \"gpt-6-astra\"\nmodel_reasoning_effort = \"xhigh\"\n",
        )
        .unwrap();
        let binding = reflection_binding(home.path(), "default").unwrap();
        let mut declaration = StageDeclaration {
            role: LEAD_STAGE.to_owned(),
            revision: "fixture-role-revision".to_owned(),
            profile: "default".to_owned(),
            model: "gpt-6-astra".to_owned(),
            provider: "openai".to_owned(),
            effort: "xhigh".to_owned(),
            checkout: home.path().to_owned(),
            title: "fixture lead reflection".to_owned(),
        };
        require_route(&binding, &declaration, Role::LeadSelfReflection).unwrap();
        declaration.provider = "different-provider".to_owned();
        assert!(require_route(&binding, &declaration, Role::LeadSelfReflection).is_err());
        fs::write(home.path().join("ds.config.toml"),
            "model = \"deepseek-flash\"\nmodel_provider = \"deepseek\"\nmodel_reasoning_effort = \"max\"\n").unwrap();
        let executor = reflection_binding(home.path(), "ds").unwrap();
        assert_eq!(executor.model_provider.as_deref(), Some("deepseek"));
        assert_eq!(executor.model.as_deref(), Some("deepseek-flash"));
    }
}
