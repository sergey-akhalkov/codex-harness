//! Normal CLI inputs over the existing board lifecycle and native fork runner.
//!
//! Task, source-turn, result, route and return identities are resolved by their
//! owners. Operators never construct the low-level fork request themselves.

use super::*;
use harness_core::delivery_reflection::{
    ActivationRequest, BindingStatus, BudgetLimit, BudgetPause,
    DeliveryState as RecordedDeliveryState, JournalPolicy, LifecycleStatus, ReflectionController,
    ReflectionLimits, ReflectionRecord, RetainedOwnerProcess, Stage, StageRecord,
    StageState as RecordedStageState, StageWork, SuccessionRequest, UsageDelta,
};

const USAGE: &str = "codex-harness delivery-improvement enable|status|disable|process|succeed|restore-journal --project DIRECTORY [--source KIT] [--codex-home DIRECTORY]\ncodex-harness delivery-improvement recover --resolution resume-retained|rebaseline-current [--evidence TEXT] --project DIRECTORY [--source KIT] [--codex-home DIRECTORY]\ncodex-harness delivery-improvement continue --acceptance ACCEPTANCE --stage executor|lead --project DIRECTORY [--source KIT] [--codex-home DIRECTORY] [--max-attempts-per-stage N] [--max-turns-per-stage N] [--max-tokens-per-stage N] [--max-tokens-per-record N] [--max-investigations-per-stage N] [--max-investigations-per-record N] [--max-context-bytes-per-stage N] [--max-context-bytes-per-record N] [--reconcile-usage-turns N --reconcile-usage-tokens N] [--reconcile-investigations N --reconcile-context-bytes N]\nRun enable/process/succeed/continue/recover from the real managed lead conversation. Enable binds the current board journal head; process handles one pending acceptance or retained continuation; succeed transfers current ownership to the live managed lead running it after the retained owner is verified inactive; continue resumes only the exact retained budget-paused stage named by status, with every supplied allowance strictly above its current value and the paused allowance above its observed value. Reconciliation values are measured additions, never estimates, and each pair must be supplied together; unchanged input starts no model work. Status is model-free. Disable preserves results and stops new stages. After disable, restore-journal removes only a journal setting enabled by this lifecycle. Recovery never retries an unknown model outcome; rebaseline-current requires evidence acknowledging the reconciled event gap.";

const LIMIT_OPTIONS: [(&str, BudgetLimit); 8] = [
    ("--max-attempts-per-stage", BudgetLimit::StageAttempts),
    ("--max-turns-per-stage", BudgetLimit::StageTurns),
    ("--max-tokens-per-stage", BudgetLimit::StageTokens),
    ("--max-tokens-per-record", BudgetLimit::RecordTokens),
    (
        "--max-investigations-per-stage",
        BudgetLimit::StageInvestigations,
    ),
    (
        "--max-investigations-per-record",
        BudgetLimit::RecordInvestigations,
    ),
    (
        "--max-context-bytes-per-stage",
        BudgetLimit::StageContextBytes,
    ),
    (
        "--max-context-bytes-per-record",
        BudgetLimit::RecordContextBytes,
    ),
];

enum RecoveryAction {
    ResumeRetained,
    RebaselineCurrent(String),
}

struct Continuation {
    acceptance: String,
    stage: Stage,
    limits: Vec<(BudgetLimit, u64)>,
    usage: Option<UsageDelta>,
    work: Option<StageWork>,
}

struct Options {
    command: String,
    project: PathBuf,
    source: PathBuf,
    home: PathBuf,
    recovery: Option<RecoveryAction>,
    continuation: Option<Continuation>,
}

fn allowed_option(command: &str, name: &str) -> bool {
    match command {
        "continue" => {
            matches!(
                name,
                "--project"
                    | "--source"
                    | "--codex-home"
                    | "--acceptance"
                    | "--stage"
                    | "--reconcile-usage-turns"
                    | "--reconcile-usage-tokens"
                    | "--reconcile-investigations"
                    | "--reconcile-context-bytes"
            ) || LIMIT_OPTIONS.iter().any(|(flag, _)| *flag == name)
        }
        "recover" => matches!(
            name,
            "--project" | "--source" | "--codex-home" | "--resolution" | "--evidence"
        ),
        _ => matches!(name, "--project" | "--source" | "--codex-home"),
    }
}

fn continuation_options(values: &mut BTreeMap<&str, OsString>) -> io::Result<Continuation> {
    let acceptance = take_option_text(
        values,
        "--acceptance",
        "continue requires --acceptance ACCEPTANCE naming the exact retained pause",
    )?;
    if acceptance.len() > harness_core::delivery_reflection::MAX_TEXT_BYTES {
        return Err(invalid("--acceptance exceeds the retained text bound"));
    }
    let stage = match take_option_text(
        values,
        "--stage",
        "continue requires --stage executor or --stage lead naming the exact paused stage",
    )?
    .as_str()
    {
        "executor" => Stage::Executor,
        "lead" => Stage::Lead,
        other => {
            return Err(invalid(format!(
                "--stage must be executor or lead, not '{other}'; {USAGE}"
            )));
        }
    };
    let mut limits = Vec::new();
    for (flag, limit) in LIMIT_OPTIONS {
        if let Some(value) = take_option_number(values, flag)? {
            limits.push((limit, value));
        }
    }
    let usage_turns = take_option_number(values, "--reconcile-usage-turns")?;
    let usage_tokens = take_option_number(values, "--reconcile-usage-tokens")?;
    let usage = match (usage_turns, usage_tokens) {
        (None, None) => None,
        (Some(turns), Some(tokens)) => Some(UsageDelta { turns, tokens }),
        _ => {
            return Err(invalid(
                "--reconcile-usage-turns and --reconcile-usage-tokens must be supplied together as measured additions from the retained native turn; one-sided reconciliation is refused",
            ));
        }
    };
    let investigations = take_option_number(values, "--reconcile-investigations")?;
    let context_bytes = take_option_number(values, "--reconcile-context-bytes")?;
    let work = match (investigations, context_bytes) {
        (None, None) => None,
        (Some(investigations), Some(context_bytes)) => Some(StageWork {
            investigations,
            context_bytes,
        }),
        _ => {
            return Err(invalid(
                "--reconcile-investigations and --reconcile-context-bytes must be supplied together as measured additions from the retained native stage; one-sided reconciliation is refused",
            ));
        }
    };
    Ok(Continuation {
        acceptance,
        stage,
        limits,
        usage,
        work,
    })
}

fn take_option_text(
    values: &mut BTreeMap<&str, OsString>,
    name: &str,
    missing: &str,
) -> io::Result<String> {
    let raw = values.remove(name).ok_or_else(|| invalid(missing))?;
    let text = raw
        .into_string()
        .map_err(|_| invalid(format!("{name} requires UTF-8 text")))?;
    if text.trim().is_empty() {
        return Err(invalid(missing));
    }
    Ok(text)
}

fn take_option_number(
    values: &mut BTreeMap<&str, OsString>,
    name: &str,
) -> io::Result<Option<u64>> {
    let Some(raw) = values.remove(name) else {
        return Ok(None);
    };
    let text = raw
        .to_str()
        .ok_or_else(|| invalid(format!("{name} requires a UTF-8 non-negative integer")))?;
    text.parse::<u64>().map(Some).map_err(|_| {
        invalid(format!(
            "{name} requires a non-negative integer, not '{text}'"
        ))
    })
}

impl Options {
    fn parse(args: &[OsString]) -> io::Result<Self> {
        let command = args
            .first()
            .and_then(|arg| arg.to_str())
            .unwrap_or_default();
        if !matches!(
            command,
            "enable"
                | "status"
                | "disable"
                | "process"
                | "succeed"
                | "recover"
                | "restore-journal"
                | "continue"
                | "_serve"
        ) {
            return Err(invalid(USAGE));
        }
        let mut values = BTreeMap::new();
        let (pairs, remainder) = args[1..].as_chunks::<2>();
        for pair in pairs {
            let name = pair[0].to_str().unwrap_or_default();
            if !allowed_option(command, name) || values.insert(name, pair[1].clone()).is_some() {
                return Err(invalid(format!(
                    "unknown or repeated option {name}; {USAGE}"
                )));
            }
        }
        if !remainder.is_empty() {
            return Err(invalid(format!("an option has no value; {USAGE}")));
        }
        let continuation = if command == "continue" {
            Some(continuation_options(&mut values)?)
        } else {
            None
        };
        let resolution = values.remove("--resolution");
        let evidence = values.remove("--evidence");
        let recovery = if command == "recover" {
            match resolution.as_deref().and_then(|value| value.to_str()) {
                Some("resume-retained") if evidence.is_none() => {
                    Some(RecoveryAction::ResumeRetained)
                }
                Some("rebaseline-current") => {
                    let evidence = evidence.and_then(|value| value.into_string().ok())
                        .filter(|value| !value.trim().is_empty())
                        .ok_or_else(|| invalid("rebaseline-current requires nonempty UTF-8 --evidence describing the reconciled event gap"))?;
                    if evidence.len() > harness_core::delivery_reflection::MAX_TEXT_BYTES {
                        return Err(invalid(
                            "recovery evidence exceeds the existing board evidence limit",
                        ));
                    }
                    Some(RecoveryAction::RebaselineCurrent(evidence))
                }
                _ => {
                    return Err(invalid(
                        "recover requires --resolution resume-retained (without --evidence) or rebaseline-current --evidence TEXT",
                    ));
                }
            }
        } else {
            None
        };
        let project = values
            .remove("--project")
            .map(PathBuf::from)
            .unwrap_or(std::env::current_dir()?);
        let home = values
            .remove("--codex-home")
            .map(PathBuf::from)
            .map_or_else(harness_core::native_launcher::codex_home, Ok)?;
        let source = values
            .remove("--source")
            .map(PathBuf::from)
            .or_else(|| crate::feedback_cli::installed_kit_source_in(&home))
            .unwrap_or_else(|| project.clone());
        Ok(Self {
            command: command.to_owned(),
            project: canonical_dir(&project, "consumer project")?,
            source: canonical_dir(&source, "kit source")?,
            home: canonical_dir(&home, "CODEX_HOME")?,
            recovery,
            continuation,
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
        self.managed_lead().map(|(lead, _)| lead)
    }

    /// Resolves the current managed lead and the exact live app-server process
    /// registered for it. The process identity is lifecycle ownership proof,
    /// not a caller-authored recipient or acceptance identity.
    fn managed_lead(&self) -> io::Result<(String, RetainedOwnerProcess)> {
        let thread = std::env::var("CODEX_THREAD_ID").ok()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| invalid("CODEX_THREAD_ID is missing; run this command from the managed lead conversation"))?;
        match control::registered_lead(&self.home, &thread) {
            control::RegisteredLead::Live(endpoint) => {
                let process = endpoint.process.as_ref().ok_or_else(|| invalid(format!(
                    "the real lead '{thread}' is registered without an app-server process identity; start or resume it with `codex-harness lead start --session THREAD`"
                )))?;
                Ok((
                    thread,
                    RetainedOwnerProcess {
                        identity: harness_core::process::ProcessIdentity {
                            pid: process.pid,
                            creation_time: process.creation_time,
                        },
                        program: process.program.clone(),
                    },
                ))
            }
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
        "status" => status(
            &controller,
            &options.home,
            &gate::ReflectionGate::load(&options.source)?,
        ),
        "enable" => {
            // Resolve both roles and configured routes before changing journal
            // configuration. Actual stage startup revalidates these identities.
            let config = orchestration_config::load(&options.source)?;
            let gate = gate::ReflectionGate::from_config(&options.source, &config);
            gate.ensure_enabled()?;
            declarations(&options, &config)?;
            let (lead, owner_process) = options.managed_lead()?;
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
                    "reflection belongs to another lead session; after that owner exits, run `codex-harness delivery-improvement succeed --project DIRECTORY` from the successor to transfer current ownership without rewriting retained work",
                ));
            }
            let receipt = controller.activate_with_owner_process(
                &ActivationRequest {
                    lead_session: lead,
                    run: None,
                    boundary: None,
                    journal: JournalPolicy::EnableIfMissing,
                    limits: config.reflection_limits,
                },
                &owner_process,
            )?;
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
        "succeed" => succession(&options, &controller),
        "continue" => {
            let (lead, owner_process) = options.managed_lead()?;
            continuation(&options, &controller, &lead, &owner_process)
        }
        "restore-journal" => {
            let receipt = controller.restore_owned_journal()?;
            println!("{}", serde_json::to_string(&receipt)?);
            Ok(0)
        }
        "recover" => {
            let (lead, owner_process) = options.managed_lead()?;
            let state = controller.stored_state()?.ok_or_else(|| {
                invalid("reflection is inactive; there is no recovery to resolve")
            })?;
            if state.lifecycle != LifecycleStatus::Active || state.binding.lead_session != lead {
                return Err(invalid(
                    "recovery requires the active lifecycle's own managed lead; retained work was not changed",
                ));
            }
            controller.retain_owner_process(&lead, &owner_process)?;
            let receipt = match options
                .recovery
                .as_ref()
                .expect("validated recovery action")
            {
                RecoveryAction::ResumeRetained => controller.resume_from_floor()?,
                RecoveryAction::RebaselineCurrent(evidence) => {
                    controller.rebaseline_current(evidence)?
                }
            };
            println!("{}", serde_json::to_string(&receipt)?);
            Ok(0)
        }
        _ => unreachable!(),
    }
}

const MAX_STATUS_RECORDS: usize = 64;

fn status(
    controller: &ReflectionController,
    home: &Path,
    gate: &gate::ReflectionGate,
) -> io::Result<i32> {
    let Some(state) = controller.stored_state()? else {
        let (model_work, reason) = gate.model_work(None);
        println!(
            "{}",
            json!({
                "lifecycle": "inactive",
                "pending": 0,
                "selfReflectionEnabled": gate.enabled,
                "configPath": gate.config_path,
                "modelWork": model_work,
                "modelWorkReason": reason,
            })
        );
        return Ok(0);
    };
    let (model_work, model_work_reason) = gate.model_work(Some(&state));
    let records: Vec<Value> = state
        .records
        .values()
        .take(MAX_STATUS_RECORDS)
        .map(record_status)
        .collect();
    let omitted = state.records.len().saturating_sub(records.len());
    println!(
        "{}",
        json!({
            "lifecycle": state.lifecycle,
            "binding": state.binding,
            "checkpoint": state.checkpoint,
            "journal": state.journal,
            "limits": state.limits,
            "retained": state.records.len(),
            "pending": controller.pending_jobs()?.len(),
            "unfinished": state.records.values().filter(|record| record.delivery != RecordedDeliveryState::Consumed).count(),
            "undelivered": state.records.values().filter(|record| retained_delivery(record)).count(),
            "recovery": state.recovery,
            "lastPoll": state.last_poll,
            "stateFile": controller.state_file()?,
            "selfReflectionEnabled": gate.enabled,
            "configPath": gate.config_path,
            "modelWork": model_work,
            "modelWorkReason": model_work_reason,
            "host": host::status(home, &state.binding.lead_session)?,
            "records": records,
            "recordsOmitted": omitted,
        })
    );
    Ok(0)
}

/// Model-free visibility for the exact retained pause. `continuation` is the
/// same installed command the native pause recorded; reconciliation flags are
/// named separately because their values are measured additions the operator
/// must supply, never estimates or assumed zero.
fn record_status(record: &ReflectionRecord) -> Value {
    let slot = |stage: Stage, value: &StageRecord| {
        let mut missing = Vec::new();
        if !value.usage_reconciled {
            missing.push("--reconcile-usage-turns N --reconcile-usage-tokens N");
        }
        if !value.work_reconciled {
            missing.push("--reconcile-investigations N --reconcile-context-bytes N");
        }
        let next_action = value.pause.as_ref().map(|_| {
            if missing.is_empty() {
                format!(
                    "run the continuation for --stage {} to resume only this retained pause; otherwise leave the stage retained as inconclusive without another analysis",
                    stage_arg(stage)
                )
            } else {
                format!(
                    "measure the retained native turn/stage and supply {} together with the continuation for --stage {}; never assume zero; otherwise leave the stage retained as inconclusive",
                    missing.join(" and "),
                    stage_arg(stage)
                )
            }
        });
        json!({
            "state": value.state,
            "usage": value.usage,
            "usageReconciled": value.usage_reconciled,
            "workReconciled": value.work_reconciled,
            "pause": value.pause,
            "missingReconciliation": missing,
            "continuation": value.pause.as_ref().map(|pause| pause.continuation_choice.clone()),
            "nextAction": next_action,
        })
    };
    json!({
        "acceptance": record.acceptance_id,
        "executor": slot(Stage::Executor, &record.executor),
        "lead": slot(Stage::Lead, &record.lead),
        "delivery": record.delivery,
        "deliveryReceipt": record.delivery_receipt,
    })
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
    current_lead: &str,
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
            endpoint: control::lead_endpoint_path(&options.home, current_lead)?,
            thread: current_lead.to_owned(),
        },
        bound_seconds: DEFAULT_BOUND_SECONDS,
    })
}

/// What the managed-lead registry and the retained lifecycle proof establish
/// about the retained owner's exact app-server process. Succession requires a
/// positively verified exit. A missing registration is not an exit by itself:
/// normal managed-lead shutdown removes it, so the exact process identity
/// retained with lifecycle ownership is inspected as the fallback. Live,
/// malformed, mismatched, process-less or unverifiable evidence refuses.
enum RetainedOwner {
    Inactive,
    Live,
    Unverifiable(String),
}

fn retained_owner(
    options: &Options,
    thread: &str,
    retained: Option<&RetainedOwnerProcess>,
) -> io::Result<RetainedOwner> {
    let path = control::lead_endpoint_path(&options.home, thread).map_err(|error| {
        invalid(format!(
            "the retained lead identity '{thread}' cannot name a managed registry record: {error}; verify the managed-lead registry and repeat `codex-harness delivery-improvement succeed --project DIRECTORY` from the successor; no ownership or lifecycle state was changed"
        ))
    })?;
    match Endpoint::read(&path) {
        Ok(endpoint) => {
            if endpoint.thread_id.as_deref() != Some(thread.trim()) {
                return Ok(RetainedOwner::Unverifiable(format!(
                    "the registered lead endpoint record at {} belongs to another conversation",
                    path.display()
                )));
            }
            let Some(process) = endpoint.process.as_ref() else {
                return Ok(RetainedOwner::Unverifiable(format!(
                    "the registered lead endpoint record at {} records no app-server process",
                    path.display()
                )));
            };
            let user = harness_core::process_service::current_user().map_err(|error| {
                invalid(format!(
                    "the registered lead endpoint process identity for '{thread}' cannot be inspected: {error}; no ownership or lifecycle state was changed"
                ))
            })?;
            match harness_core::process_service::ServiceProcess::inspect_endpoint(
                harness_core::process::ProcessIdentity {
                    pid: process.pid,
                    creation_time: process.creation_time,
                },
                &process.program,
                &user,
            ) {
                Ok(Some(_)) => return Ok(RetainedOwner::Live),
                // The registration positively names an exited or reused
                // process, so fall through to the retained exact identity.
                Ok(None) => {}
                Err(error) => {
                    return Ok(RetainedOwner::Unverifiable(format!(
                        "the registered lead endpoint process identity at {} cannot be verified: {error}",
                        path.display()
                    )));
                }
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(_) => {
            return Ok(RetainedOwner::Unverifiable(format!(
                "the registered lead endpoint record at {} is unreadable or malformed",
                path.display()
            )));
        }
    }
    let Some(retained) = retained else {
        return Ok(RetainedOwner::Unverifiable(format!(
            "the retained lifecycle has no exact managed-lead process identity for '{thread}', so its exit cannot be established from the absent or already-exited registration"
        )));
    };
    match retained.inspect() {
        Ok(Some(_)) => Ok(RetainedOwner::Live),
        Ok(None) => Ok(RetainedOwner::Inactive),
        Err(error) => Ok(RetainedOwner::Unverifiable(format!(
            "the exact managed-lead process identity retained for '{thread}' cannot be verified: {error}"
        ))),
    }
}

/// Transfers current control and delivery ownership to the live managed lead
/// running this command. The retained owner is derived from the lifecycle, not
/// supplied by the caller, and must be verifiably inactive before the atomic
/// lifecycle write. A repeat by the current owner is an idempotent no-op.
fn succession(options: &Options, controller: &ReflectionController) -> io::Result<i32> {
    let (successor, successor_process) = options.managed_lead()?;
    let state = controller.stored_state()?.ok_or_else(|| {
        invalid(
            "delivery reflection is inactive; enable it under the retained lead before succession; nothing was changed",
        )
    })?;
    if state.lifecycle != LifecycleStatus::Active {
        return Err(invalid(
            "delivery reflection is stopped; enable the retained lifecycle before succession; nothing was changed",
        ));
    }
    if state
        .retired_leads
        .iter()
        .any(|retired| retired.trim() == successor)
    {
        return Err(invalid(format!(
            "lead session '{successor}' was superseded by succession and cannot reclaim this reflection lifecycle; inspect retained work under '{}' or start a fresh lifecycle for new work; no ownership or lifecycle state was changed",
            state.binding.lead_session
        )));
    }
    let previous = state.binding.lead_session.clone();
    if successor != previous {
        match retained_owner(options, &previous, state.owner_process.as_ref())? {
            RetainedOwner::Live => {
                return Err(invalid(format!(
                    "the retained lead '{previous}' is still live; stop or exit that managed lead, verify the process has ended, then repeat `codex-harness delivery-improvement succeed --project DIRECTORY` from the successor; no ownership or lifecycle state was changed"
                )));
            }
            RetainedOwner::Unverifiable(reason) => {
                return Err(invalid(format!(
                    "{reason}; start or resume that exact managed lead with `codex-harness lead start --session {previous}` so its own owner rewrites the registration, run `codex-harness delivery-improvement enable --project DIRECTORY` from it while registered to refresh the exact process proof, exit it normally, then repeat succession from the successor; do not fabricate or delete identity evidence; no ownership or lifecycle state was changed"
                )));
            }
            RetainedOwner::Inactive => {}
        }
    }
    let receipt = controller.succeed(&SuccessionRequest {
        expected_lead_session: previous,
        successor_lead: successor,
        successor_process,
    })?;
    println!("{}", serde_json::to_string(&receipt)?);
    Ok(0)
}

fn process(options: &Options, controller: &ReflectionController, quiet: bool) -> io::Result<i32> {
    let gate = gate::ReflectionGate::load(&options.source)?;
    let idle = || {
        if quiet {
            Ok(0)
        } else {
            status(controller, &options.home, &gate)
        }
    };
    let Some(state) = controller.stored_state()? else {
        return idle();
    };
    if state.lifecycle != LifecycleStatus::Active {
        return idle();
    }
    if !gate.enabled {
        // Keep the retained event boundary current without admitting closures
        // from the disabled interval. No model, stage admission or lifecycle
        // mutation is involved; re-enabling therefore sees only work retained
        // before the gate closed.
        controller.poll_without_admission()?;
        return idle();
    }
    let (lead, owner_process) = options.managed_lead()?;
    let checked = controller.check_active_binding(&lead, None)?;
    if !checked.is_active() {
        if matches!(
            checked.status,
            BindingStatus::Inactive | BindingStatus::Stopped
        ) {
            return idle();
        }
        return Err(invalid(checked.detail));
    }
    // This is the ordinary current-owner route. Refresh the exact process
    // proof while the registration is available so a later retirement of the
    // registry record can still distinguish exit from an unreadable absence.
    controller.retain_owner_process(&lead, &owner_process)?;
    let Some(polled) = controller.poll_if_active()? else {
        return idle();
    };
    if let Some(recovery) = polled.recovery {
        return Err(invalid(format!(
            "the board needs reconciliation: {}",
            recovery.detail
        )));
    }
    let record = if let Some(job) = polled.jobs.first() {
        Some(
            controller
                .record(&job.acceptance_id)?
                .ok_or_else(|| invalid("the queued acceptance disappeared"))?,
        )
    } else {
        // New-stage admission and retained delivery are different work. A
        // restart must still observe an already submitted turn or synthesis;
        // absence of a new-stage job never proves that delivery finished.
        controller.stored_state()?.and_then(|state| {
            state.records.into_values().find(|record| {
                record.executor.state == RecordedStageState::InFlight
                    || record.lead.state == RecordedStageState::InFlight
                    || retained_delivery(record)
            })
        })
    };
    let Some(record) = record else {
        if !quiet {
            return status(controller, &options.home, &gate);
        }
        return Ok(0);
    };
    let request = request(options, controller, &record, &lead)?;
    let delivery_path = request.state_dir.join("lead-delivery.json");
    let exit = accepted::execute(request, &options.project)?;
    if quiet && exit != 0 {
        let delivery: DeliveryState = read_json(&delivery_path, MAX_REQUEST_BYTES)?;
        if !matches!(delivery.state.as_str(), "queued" | "pending") {
            return Err(invalid(format!(
                "retained lead delivery is {}; preserve {} and run delivery-improvement process after resolving its cause: {}",
                delivery.state,
                delivery_path.display(),
                delivery
                    .detail
                    .as_deref()
                    .unwrap_or("delivery is not established")
            )));
        }
    }
    Ok(exit)
}

fn retained_delivery(record: &ReflectionRecord) -> bool {
    record.executor.state == RecordedStageState::Retained
        && record.lead.state == RecordedStageState::Retained
        && record.delivery != RecordedDeliveryState::Consumed
}

fn stage_slot(record: &ReflectionRecord, stage: Stage) -> &StageRecord {
    match stage {
        Stage::Executor => &record.executor,
        Stage::Lead => &record.lead,
    }
}

/// Resumes the exact retained budget pause through the installed interface.
/// Every refusal happens before a state write or model turn: the lead binding,
/// board recovery, exact acceptance/stage, reconciliation inventory, raised
/// allowances and projected admission are all checked first.
fn continuation(
    options: &Options,
    controller: &ReflectionController,
    lead: &str,
    owner_process: &RetainedOwnerProcess,
) -> io::Result<i32> {
    gate::ReflectionGate::load(&options.source)?.ensure_enabled()?;
    let continuation = options.continuation.as_ref().ok_or_else(|| {
        invalid("continue requires --acceptance and --stage; nothing was changed")
    })?;
    let acceptance = continuation.acceptance.as_str();
    let stage = continuation.stage;
    let stage_name = stage_arg(stage);
    let state = controller.stored_state()?.ok_or_else(|| {
        invalid(
            "delivery reflection is inactive; retained pauses require the active lifecycle and its managed lead; nothing was changed",
        )
    })?;
    if state.lifecycle != LifecycleStatus::Active {
        return Err(invalid(
            "delivery reflection is stopped; retained results are preserved, but continuing a paused stage requires the active lifecycle; nothing was changed",
        ));
    }
    let checked = controller.check_active_binding(lead, Some(acceptance))?;
    if !checked.is_active() {
        return Err(invalid(format!(
            "the retained pause cannot be continued: {}; nothing was changed",
            checked.detail
        )));
    }
    if checked.matched_acceptance_id.as_deref() != Some(acceptance) {
        return Err(invalid(format!(
            "acceptance '{acceptance}' is not the current accepted work for this binding (matched {:?}); status names the exact retained pause; nothing was changed",
            checked.matched_acceptance_id
        )));
    }
    let polled = controller.poll()?;
    if let Some(recovery) = polled.recovery {
        return Err(invalid(format!(
            "the board needs reconciliation before any continuation: {}; resolve it with `codex-harness delivery-improvement recover`; no model work was started",
            recovery.detail
        )));
    }
    let state = controller.stored_state()?.ok_or_else(|| {
        invalid("the reflection lifecycle disappeared during continuation; nothing was changed")
    })?;
    if let Some((other, other_stage)) = state.records.iter().find_map(|(id, record)| {
        [Stage::Executor, Stage::Lead]
            .into_iter()
            .find_map(|stage| {
                matches!(
                    stage_slot(record, stage).state,
                    RecordedStageState::InFlight | RecordedStageState::Unknown
                )
                .then(|| (id.clone(), stage_arg(stage)))
            })
    }) {
        return Err(invalid(format!(
            "acceptance '{other}' has a {other_stage} stage in flight or unknown; reconcile that stage before changing limits or continuing '{acceptance}'; nothing was changed"
        )));
    }
    let record = controller.record(acceptance)?.ok_or_else(|| {
        invalid(format!(
            "acceptance '{acceptance}' is not retained by this lifecycle; run delivery-improvement status; nothing was changed"
        ))
    })?;
    if record.delivery == RecordedDeliveryState::Consumed {
        return Err(invalid(format!(
            "acceptance '{acceptance}' delivery is already consumed; no stage may run again; nothing was changed"
        )));
    }
    let slot = stage_slot(&record, stage);
    if slot.state != RecordedStageState::Blocked {
        return Err(invalid(format!(
            "{stage_name} reflection for '{acceptance}' is {:?}, not a retained budget pause; continue only the exact paused stage named by status; nothing was changed",
            slot.state
        )));
    }
    let pause = slot.pause.as_ref().ok_or_else(|| {
        invalid(format!(
            "{stage_name} reflection for '{acceptance}' is blocked without a retained budget pause; reconcile its cause through status and recovery; nothing was changed"
        ))
    })?;
    validate_reconciliation(continuation, stage_name, slot)?;
    let limits = raised_limits(continuation, &state.limits, pause, stage_name)?;
    validate_projected_admission(&record, slot, &limits, continuation)?;
    // Stage routing and the exact accepted boundaries are validated before any
    // retained state changes. The continuation then executes only this
    // acceptance through the same accepted-stage controller as `process`.
    let request = request(options, controller, &record, lead)?;
    controller.retain_owner_process(lead, owner_process)?;

    if let Some(usage) = continuation.usage {
        controller.record_unknown_usage(acceptance, stage, usage)?;
    }
    if let Some(work) = continuation.work {
        controller.record_observed_work(acceptance, stage, work)?;
    }
    if !continuation.limits.is_empty() {
        controller.configure_limits(&limits)?;
    }
    controller.retry_stage(acceptance, stage)?;
    accepted::execute(request, &options.project)
}

fn validate_reconciliation(
    continuation: &Continuation,
    stage_name: &str,
    slot: &StageRecord,
) -> io::Result<()> {
    match (continuation.usage.is_some(), slot.usage_reconciled) {
        (true, true) => {
            return Err(invalid(format!(
                "{stage_name} reflection usage is already reconciled; omit --reconcile-usage-turns/--reconcile-usage-tokens rather than charging again; nothing was changed"
            )));
        }
        (false, false) => {
            return Err(invalid(format!(
                "{stage_name} reflection usage is unreconciled; continue requires measured additions with --reconcile-usage-turns N --reconcile-usage-tokens N; never assume zero; nothing was changed"
            )));
        }
        _ => {}
    }
    match (continuation.work.is_some(), slot.work_reconciled) {
        (true, true) => Err(invalid(format!(
            "{stage_name} reflection observed work is already reconciled; omit --reconcile-investigations/--reconcile-context-bytes rather than charging again; nothing was changed"
        ))),
        (false, false) => Err(invalid(format!(
            "{stage_name} reflection observed work is unreconciled; continue requires measured additions with --reconcile-investigations N --reconcile-context-bytes N; never assume zero; nothing was changed"
        ))),
        _ => Ok(()),
    }
}

fn raised_limits(
    continuation: &Continuation,
    current: &ReflectionLimits,
    pause: &BudgetPause,
    stage_name: &str,
) -> io::Result<ReflectionLimits> {
    let mut limits = current.clone();
    for (limit, value) in &continuation.limits {
        let configured = limit.configured(current);
        if *value <= configured {
            return Err(invalid(format!(
                "--{} must be strictly above the current {} ({configured}); unchanged allowances cannot continue a paused stage; nothing was changed",
                limit_flag(*limit),
                limit.as_str()
            )));
        }
        match limit {
            BudgetLimit::StageAttempts => {
                limits.max_attempts_per_stage = u32::try_from(*value).map_err(|_| {
                    invalid(format!(
                        "--{} must be at most {}",
                        limit_flag(*limit),
                        u32::MAX
                    ))
                })?;
            }
            BudgetLimit::StageTurns => limits.max_turns_per_stage = *value,
            BudgetLimit::StageTokens => limits.max_tokens_per_stage = *value,
            BudgetLimit::RecordTokens => limits.max_tokens_per_record = *value,
            BudgetLimit::StageInvestigations => limits.max_investigations_per_stage = *value,
            BudgetLimit::RecordInvestigations => limits.max_investigations_per_record = *value,
            BudgetLimit::StageContextBytes => limits.max_context_bytes_per_stage = *value,
            BudgetLimit::RecordContextBytes => limits.max_context_bytes_per_record = *value,
        }
    }
    limits.validate().map_err(|error| {
        invalid(format!(
            "the requested allowances are not usable: {error}; nothing was changed"
        ))
    })?;
    if pause.limit.configured(&limits) <= pause.observed {
        return Err(invalid(format!(
            "{stage_name} reflection remains paused at {} observed {}; continue requires --{} above {}; nothing was changed",
            pause.limit.as_str(),
            pause.observed,
            limit_flag(pause.limit),
            pause.observed
        )));
    }
    Ok(limits)
}

/// Mirrors the cumulative admission `begin_stage` applies, including the
/// measured reconciliation additions, so a continuation that cannot admit the
/// next attempt is refused before limits or usage are written.
fn validate_projected_admission(
    record: &ReflectionRecord,
    slot: &StageRecord,
    limits: &ReflectionLimits,
    continuation: &Continuation,
) -> io::Result<()> {
    let usage = continuation.usage.unwrap_or_default();
    let work = continuation.work.unwrap_or_default();
    let turns = slot.usage.turns.saturating_add(usage.turns);
    let tokens = slot.usage.tokens.saturating_add(usage.tokens);
    let investigations = slot
        .usage
        .investigations
        .saturating_add(work.investigations);
    let context_bytes = slot.usage.context_bytes.saturating_add(work.context_bytes);
    let totals = record.usage_totals();
    let record_tokens = totals.tokens.saturating_add(usage.tokens);
    let record_investigations = totals.investigations.saturating_add(work.investigations);
    let record_context_bytes = totals.context_bytes.saturating_add(work.context_bytes);
    let stage_name = stage_arg(continuation.stage);
    let refusal = |flag: &str, observed: u64, scope: &str| {
        invalid(format!(
            "continuing {stage_name} reflection would admit no new attempt: {scope} observed {observed} leaves no allowance; set --{flag} above {observed} (at least {}) and keep the record floor at or above the stage value; nothing was changed",
            observed.saturating_add(1)
        ))
    };
    if u64::from(slot.usage.attempts) >= u64::from(limits.max_attempts_per_stage) {
        return Err(refusal(
            "max-attempts-per-stage",
            u64::from(slot.usage.attempts),
            "stage attempts",
        ));
    }
    if turns >= limits.max_turns_per_stage {
        return Err(refusal("max-turns-per-stage", turns, "stage turns"));
    }
    if tokens >= limits.max_tokens_per_stage {
        return Err(refusal("max-tokens-per-stage", tokens, "stage tokens"));
    }
    if record_tokens >= limits.max_tokens_per_record {
        return Err(refusal(
            "max-tokens-per-record",
            record_tokens,
            "record tokens",
        ));
    }
    if investigations >= limits.max_investigations_per_stage {
        return Err(refusal(
            "max-investigations-per-stage",
            investigations,
            "stage investigations",
        ));
    }
    if record_investigations >= limits.max_investigations_per_record {
        return Err(refusal(
            "max-investigations-per-record",
            record_investigations,
            "record investigations",
        ));
    }
    if context_bytes >= limits.max_context_bytes_per_stage {
        return Err(refusal(
            "max-context-bytes-per-stage",
            context_bytes,
            "stage supplied-context bytes",
        ));
    }
    if record_context_bytes >= limits.max_context_bytes_per_record {
        return Err(refusal(
            "max-context-bytes-per-record",
            record_context_bytes,
            "record supplied-context bytes",
        ));
    }
    Ok(())
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
                recovery: None,
                continuation: None,
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
