//! The ordinary Codex host: one prepared native new/resume/fork session served
//! by one harness-owned app-server in the manager's console. The native
//! frontend chooses the thread; the relay observes that exact choice before
//! releasing it, publishes the verified endpoint, and the host then drives the
//! same app-server for messaging and the existing managed-lead lifecycle.

use crate::executor_cli::{
    self, SelectionObserver, attach_observed_frontend,
    control::{
        self, BoundIdentity, ControlPaths, ControlPlan, Conversation, Endpoint, RegisteredLead,
        endpoint_process_live, publish_lead_endpoint,
    },
    end_owned_child, note_log, retire_stale_endpoint,
};
use crate::self_reflection_cli::{
    host::LeadLifecycle,
    lifecycle::{self, Preparation},
};
use crate::ordinary_recovery::RecoveryHost;
use harness_core::{
    process::{Job, Limits},
    task_arguments::{self, ManagedArguments},
};
use std::{
    env,
    ffi::OsString,
    io,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

/// A backend or selection failure gets this long for the native frontend to
/// leave the terminal by itself before this host closes the owned surface.
const FRONTEND_TAIL_GRACE: Duration = Duration::from_secs(5);
const IDLE_POLL: Duration = Duration::from_millis(50);

fn invalid(reason: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, reason.into())
}

/// Runs the registered `_ordinary-lead` entry. Its arguments are exactly the
/// kit-home option, the separator and the prepared native argv.
pub(crate) fn run(args: &[OsString]) -> io::Result<i32> {
    let (home, native_args) = parse(args)?;
    let original_cwd = env::current_dir()?;
    let managed =
        task_arguments::plan_hosted(native_args, &original_cwd, &home)?.ok_or_else(|| {
            invalid("the prepared native arguments are not an ordinary hosted session")
        })?;
    let ManagedArguments {
        backend,
        tui,
        cwd: backend_cwd,
        ..
    } = managed;

    let launcher = home
        .join("harness/bin/codex.exe")
        .canonicalize()
        .map_err(|error| {
            invalid(format!(
                "the installed app-server launcher {} could not be resolved: {error}",
                home.join("harness/bin/codex.exe").display()
            ))
        })?;
    let mut plan = ControlPlan::new(
        launcher.clone(),
        &home,
        &backend_cwd,
        "ordinary lead",
        BoundIdentity {
            profile: "default".into(),
            model: None,
            model_provider: None,
            reasoning_effort: None,
        },
        ControlPaths::for_lead_host(&home, std::process::id()),
    );
    // This is the native ordinary lead, not an executor, bounded lead or
    // reflection fork. Its backend keeps the native agent capability and the
    // prepared profile/config layer travels only as app-server arguments.
    plan.executor_session = false;
    plan.bounded_lead_session = false;
    plan.reflection_session = false;
    plan.args.extend(backend);
    plan.env
        .insert("HARNESS_EXECUTOR_FIXTURE_MODE".into(), None);
    if let Some(mode) = env::var_os("HARNESS_EXECUTOR_CHILD_FIXTURE_MODE") {
        plan.env
            .insert("HARNESS_EXECUTOR_FIXTURE_MODE".into(), Some(mode));
    }

    let job = Job::new(Limits::default())?;
    let mut conversation = match Conversation::open(&job, &plan) {
        Ok(conversation) => conversation,
        Err(error) => {
            let notes = retire_transient(&plan);
            for note in notes {
                eprintln!("ordinary lead note: {note}");
            }
            return Err(invalid(format!(
                "the ordinary app-server did not start: {error}; the native frontend was not started"
            )));
        }
    };
    if let Some(identity) = conversation.process_identity() {
        note_log(
            &plan.paths.log,
            &format!(
                "ordinary lead: backend pid {} creation {}",
                identity.pid, identity.creation_time
            ),
        );
    }
    let observed = Arc::new(Mutex::new(SelectionState::default()));
    let lifecycle = Arc::new(Mutex::new(None::<LeadLifecycle>));
    if let Err(error) = conversation.endpoint().record(&plan.paths.endpoint) {
        let (_, notes) = release(
            &plan,
            job,
            None,
            &lifecycle,
            &observed,
            &conversation,
            &launcher,
        );
        for note in notes {
            eprintln!("ordinary lead note: {note}");
        }
        return Err(invalid(format!(
            "the ordinary app-server endpoint could not be recorded before attachment: {error}"
        )));
    }

    let observer = observer(
        Arc::clone(&observed),
        Arc::clone(&lifecycle),
        home.clone(),
        backend_cwd.clone(),
        conversation.endpoint().clone(),
    );
    let frontend =
        match attach_observed_frontend(&plan, &original_cwd, &tui, &conversation, observer) {
            Ok(frontend) => frontend,
            Err(error) => {
                let (_, notes) = release(
                    &plan,
                    job,
                    None,
                    &lifecycle,
                    &observed,
                    &conversation,
                    &launcher,
                );
                for note in notes {
                    eprintln!("ordinary lead note: {note}");
                }
                eprintln!("ordinary lead: {error}");
                return Ok(1);
            }
        };
    note_log(
        &plan.paths.log,
        &format!(
            "ordinary lead: app-server pid {} cwd {} frontend cwd {}",
            conversation
                .process()
                .map(|process| process.identity().pid.to_string())
                .unwrap_or_else(|| "unrecorded".to_owned()),
            backend_cwd.display(),
            original_cwd.display()
        ),
    );

    let mut adopted: Option<String> = None;
    let mut failure: Option<String> = None;
    let mut deadline: Option<Instant> = None;
    let mut recovery: Option<RecoveryHost> = None;
    loop {
        let running = match frontend.is_running() {
            Ok(running) => running,
            Err(error) => {
                failure.get_or_insert_with(|| {
                    format!("the native frontend state could not be read: {error}")
                });
                deadline.get_or_insert_with(|| Instant::now() + FRONTEND_TAIL_GRACE);
                false
            }
        };
        let (selected_now, observer_error, warnings) = {
            let mut state = observed
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let warnings = std::mem::take(&mut state.warnings);
            (
                state.selected.clone(),
                state.callback_error.clone(),
                warnings,
            )
        };
        for warning in warnings {
            note_log(
                &plan.paths.log,
                &format!("reflection unavailable: {warning}"),
            );
        }
        if failure.is_none()
            && let Some(error) = observer_error
        {
            failure = Some(format!(
                "the native frontend's thread selection was not registered: {error}"
            ));
            deadline = Some(Instant::now() + FRONTEND_TAIL_GRACE);
        }
        // Only an observation that saw the frontend still running may adopt
        // its selection or drive the reflection lifecycle. After exit the
        // host owns cleanup only; it never starts a stage for a dead surface.
        if running {
            if let Some(selected_now) = selected_now.as_ref()
                && adopted.as_deref() != Some(selected_now.thread.as_str())
            {
                match adopt_selected(selected_now, &mut conversation, &plan.paths.log) {
                    Ok(()) => {
                        adopted = Some(selected_now.thread.clone());
                        if recovery.is_none()
                            && let Some((gate, warnings)) = frontend.recovery_surface()
                        {
                            recovery = Some(RecoveryHost::new(
                                selected_now.thread.clone(),
                                gate,
                                warnings,
                                plan.paths.log.with_extension("recovery.json"),
                                plan.paths.log.clone(),
                            ));
                        }
                    }
                    Err(error) => {
                        adopted = None;
                        failure.get_or_insert_with(|| {
                            format!(
                                "the selected native thread could not be adopted by the same app-server: {error}"
                            )
                        });
                        deadline.get_or_insert_with(|| Instant::now() + FRONTEND_TAIL_GRACE);
                    }
                }
            }
            if failure.is_none() && adopted.is_some() {
                match conversation.pump() {
                    Ok(events) => {
                        if let Some(host) = recovery.as_mut() {
                            for event in events {
                                if failure.is_some() {
                                    break;
                                }
                                if let Err(error) =
                                    host.on_event(&mut conversation, &event.raw)
                                {
                                    host.observation_failed(&error.to_string());
                                }
                            }
                        }
                    }
                    Err(error) => {
                        failure = Some(format!(
                            "the ordinary app-server control connection ended while the native frontend was attached: {error}"
                        ));
                        deadline = Some(Instant::now() + FRONTEND_TAIL_GRACE);
                    }
                }
            }
            if failure.is_none()
                && adopted.is_some()
                && let Some(host) = recovery.as_mut()
            {
                host.tick(&mut conversation);
            }
            if adopted.is_some() {
                let mut owned = lifecycle
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                if let Some(started) = owned.as_mut()
                    && let Err(error) = started.tick()
                {
                    note_log(&plan.paths.log, &format!("reflection lifecycle: {error}"));
                }
            }
        }
        if !running {
            break;
        }
        if failure.is_some() && deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            break;
        }
        thread::sleep(if adopted.is_some() {
            control::DEFAULT_POLL
        } else {
            IDLE_POLL
        });
    }

    if let Some(mut host) = recovery.take() {
        host.shutdown();
    }

    // The frontend can finish while the relay is still completing its native
    // selection. Close the acceptance boundary before waiting so a callback
    // that finishes late retires what it staged instead of publishing it. A
    // late selection is never adopted, prepared or ticked.
    let (_, late_error) = begin_release(&observed);
    if failure.is_none()
        && let Some(error) = late_error
    {
        failure = Some(format!(
            "the native frontend's thread selection was not registered: {error}"
        ));
    }

    let (frontend_code, notes) = release(
        &plan,
        job,
        Some(frontend),
        &lifecycle,
        &observed,
        &conversation,
        &launcher,
    );
    for note in &notes {
        eprintln!("ordinary lead note: {note}");
    }
    if let Some(error) = failure {
        eprintln!("ordinary lead: {error}");
        return Ok(1);
    }
    if !notes.is_empty() {
        eprintln!(
            "ordinary lead: cleanup could not be confirmed; the state named by the notes above requires repair"
        );
        return Ok(1);
    }
    match frontend_code {
        Some(code) => Ok(i32::try_from(code).unwrap_or(1)),
        None => {
            eprintln!("ordinary lead: the native frontend ended without a recorded exit code");
            Ok(1)
        }
    }
}

fn adopt_selected(
    selected: &Selected,
    conversation: &mut Conversation,
    log: &Path,
) -> io::Result<()> {
    conversation.adopt_thread(&selected.thread)?;
    note_log(
        log,
        &format!(
            "ordinary lead: selected thread {} registry {}",
            selected.thread,
            selected.path.display()
        ),
    );
    Ok(())
}

fn parse(args: &[OsString]) -> io::Result<(PathBuf, &[OsString])> {
    if args.len() < 3 || args[0] != "--codex-home" || args[2] != "--" {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "usage: codex-harness _ordinary-lead --codex-home HOME -- PREPARED_NATIVE_ARGS",
        ));
    }
    let home = PathBuf::from(&args[1]);
    if home.as_os_str().is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the ordinary lead requires a non-empty CODEX_HOME",
        ));
    }
    Ok((home, &args[3..]))
}

#[derive(Clone)]
struct Selected {
    thread: String,
    endpoint: Endpoint,
    path: PathBuf,
}

#[derive(Default)]
struct SelectionState {
    selected: Option<Selected>,
    /// Every endpoint this host published, including a prior registration
    /// retained because reflection still owns it.
    owned: Vec<Selected>,
    /// Non-fatal reflection preparation results for the host log.
    warnings: Vec<String>,
    callback_error: Option<String>,
    /// The thread whose registration callback is still completing.
    registering: Option<String>,
    /// Release has begun; no selection may be published after this point.
    closing: bool,
    /// Cleanup gaps discovered after a selection had already been accepted.
    cleanup_errors: Vec<String>,
}

fn observer(
    state: Arc<Mutex<SelectionState>>,
    owned_lifecycle: Arc<Mutex<Option<LeadLifecycle>>>,
    home: PathBuf,
    project: PathBuf,
    base: Endpoint,
) -> SelectionObserver {
    Arc::new(move |thread| {
        {
            let mut state = state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if state
                .selected
                .as_ref()
                .is_some_and(|selected| selected.thread == thread)
            {
                return Ok(());
            }
            if let Some(registering) = state.registering.as_deref() {
                if registering == thread {
                    return Ok(());
                }
                let error = invalid(format!(
                    "the native frontend selected thread {thread} while thread {registering} was still being registered on the same app-server"
                ));
                state.callback_error = Some(error.to_string());
                return Err(error);
            }
            state.registering = Some(thread.to_owned());
        }
        let result = select_thread(&state, &owned_lifecycle, &home, &project, &base, thread);
        let mut state = state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.registering = None;
        match result {
            Ok(selected) => {
                if state.closing {
                    // The host began release while this callback owned the
                    // staged registration. Never publish it afterwards; retire
                    // only what this callback staged and drop any lifecycle
                    // object created for it before it could ever tick.
                    if let Err(cleanup) = retire_owned_registry(&selected) {
                        state.cleanup_errors.push(cleanup.to_string());
                    }
                    let owned = owned_lifecycle
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .take();
                    if let Some(mut lifecycle) = owned
                        && let Err(cleanup) = lifecycle.stop()
                    {
                        state
                            .cleanup_errors
                            .push(format!("reflection lifecycle cleanup: {cleanup}"));
                    }
                    return Ok(());
                }
                state.owned.retain(|owned| owned.path != selected.path);
                state.owned.push(selected.clone());
                state.selected = Some(selected);
                Ok(())
            }
            Err(error) => {
                state.callback_error = Some(error.to_string());
                Err(error)
            }
        }
    })
}

/// Resolves one observed thread to the exact registry record this host owns.
/// The first selection installs the host's lifecycle object; a different
/// selection on the same app-server performs the verified current-lifetime
/// transfer before the relay releases the native response.
fn select_thread(
    state: &Arc<Mutex<SelectionState>>,
    owned_lifecycle: &Arc<Mutex<Option<LeadLifecycle>>>,
    home: &Path,
    project: &Path,
    base: &Endpoint,
    thread: &str,
) -> io::Result<Selected> {
    let (previous, closing) = {
        let state = state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        (state.selected.clone(), state.closing)
    };
    let staged = register_selection(home, base, thread)?;
    if closing || is_closing(state) {
        return Ok(staged);
    }
    let result = match previous {
        None => match selection_gate() {
            // The gate can outlive the native frontend. Re-read the release
            // boundary before creating any lifecycle object for this thread.
            Ok(()) if !is_closing(state) => {
                *owned_lifecycle
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()) =
                    Some(LeadLifecycle::new(home, project, &staged.thread));
                Ok(())
            }
            Ok(()) => Ok(()),
            Err(error) => Err(error),
        },
        Some(previous) => {
            transfer_selection(state, owned_lifecycle, home, project, &previous, &staged)
        }
    };
    if let Err(error) = result {
        // A failed transfer or first registration retires only the record this
        // callback staged; the prior registry and retained work stay intact.
        if let Err(cleanup) = retire_owned_registry(&staged) {
            state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .cleanup_errors
                .push(cleanup.to_string());
        }
        return Err(error);
    }
    Ok(staged)
}

fn transfer_selection(
    state: &Arc<Mutex<SelectionState>>,
    owned_lifecycle: &Arc<Mutex<Option<LeadLifecycle>>>,
    home: &Path,
    project: &Path,
    previous: &Selected,
    staged: &Selected,
) -> io::Result<()> {
    if is_closing(state) {
        return Ok(());
    }
    // Quiesce and drop the old owned lifecycle before asking the core to move
    // current control. The object is taken under the lock, then stopped
    // outside it, so it can never tick again and the observer is not blocked.
    let previous_lifecycle = owned_lifecycle
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .take();
    if let Some(mut lifecycle) = previous_lifecycle
        && let Err(error) = lifecycle.stop()
    {
        return Err(error);
    }
    // The frontend may have ended while the old lifecycle was quiescing.
    // Leave the retained binding untouched rather than moving current control
    // for a surface that is already gone.
    if is_closing(state) {
        return Ok(());
    }
    let retained = match lifecycle::retained_thread_owner(home, project) {
        Ok(owner) => owner,
        Err(error) => {
            state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .warnings
                .push(format!(
                    "the retained reflection owner could not be read: {error}; native thread '{}' remains selected without reflection",
                    staged.thread
                ));
            *owned_lifecycle
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) =
                Some(LeadLifecycle::new(home, project, &staged.thread));
            return Ok(());
        }
    };
    let source = match kit_source(home, project) {
        Ok(source) => source,
        Err(error) => {
            state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .warnings
                .push(format!(
                    "reflection preparation is unavailable: {error}; native thread '{}' remains selected",
                    staged.thread
                ));
            *owned_lifecycle
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) =
                Some(LeadLifecycle::new(home, project, &staged.thread));
            return Ok(());
        }
    };
    // The first local selection is not a core owner. Never pass it to the
    // core as the expected owner merely because this host selected it.
    if retained.as_deref() != Some(previous.thread.as_str()) {
        if let Err(error) = retire_owned_registry(previous) {
            state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .cleanup_errors
                .push(error.to_string());
        }
        *owned_lifecycle
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) =
            Some(LeadLifecycle::new(home, project, &staged.thread));
        state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .warnings
            .push(format!(
                "reflection remains owned by '{}'; native thread '{}' is selected without reflection",
                retained.as_deref().unwrap_or("no retained lead"),
                staged.thread
            ));
        return Ok(());
    }
    let preparation = lifecycle::prepare_selected_thread(
        home,
        &source,
        project,
        &previous.thread,
        &staged.thread,
        true,
    );
    let preparation = match preparation {
        Ok(preparation) => preparation,
        Err(error) => {
            // Keep the prior registration available for reflection recovery;
            // the new native selection remains independently usable.
            *owned_lifecycle
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) =
                Some(LeadLifecycle::new(home, project, &staged.thread));
            state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .warnings
                .push(format!(
                    "reflection selection for '{}' could not be prepared: {error}; the prior registration '{}' was retained for recovery",
                    staged.thread, previous.thread
                ));
            return Ok(());
        }
    };
    match preparation {
        // An administrative stop stays stopped until a genuinely fresh host
        // starts. A transferred selection never resumes it implicitly.
        Preparation::Stopped => {}
        Preparation::Active | Preparation::Disabled => {
            *owned_lifecycle
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) =
                Some(LeadLifecycle::new(home, project, &staged.thread));
        }
    }
    // The core transition has succeeded, so the prior record is now inert.
    // A retirement failure is a cleanup gap, not a reason to refuse the
    // already-transferred native selection.
    if let Err(error) = retire_owned_registry(previous) {
        state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .cleanup_errors
            .push(error.to_string());
    }
    Ok(())
}

/// The managed host resolves the kit source exactly as its lifecycle owner
/// does: a checkout containing `global/roles.toml`, otherwise the installed
/// kit source recorded for this CODEX_HOME.
fn kit_source(home: &Path, candidate: &Path) -> io::Result<PathBuf> {
    if candidate.join("global/roles.toml").is_file() {
        return Ok(candidate.to_owned());
    }
    crate::feedback_cli::installed_kit_source_in(home)
        .ok_or_else(|| invalid("the managed lead has no installed kit source"))
}

/// Fixture synchronization for the exit-boundary check. Unset in ordinary
/// use; when set, the callback publishes its registry record and then waits
/// so a check can make the frontend exit before the selection becomes current.
fn selection_gate() -> io::Result<()> {
    let Some(path) = env::var_os("HARNESS_ORDINARY_LEAD_FIXTURE_SELECTION_GATE").map(PathBuf::from)
    else {
        return Ok(());
    };
    let until = Instant::now() + Duration::from_secs(30);
    while Instant::now() < until {
        if path.is_file() {
            return Ok(());
        }
        thread::sleep(IDLE_POLL);
    }
    Err(invalid(format!(
        "the fixture selection gate {} was not released",
        path.display()
    )))
}

/// Waits, bounded, for an in-flight selection registration and returns the
/// settled observation. The run loop uses this after exit so a callback that
/// was already completing is cleaned up rather than activated.
fn settled_selection(state: &Arc<Mutex<SelectionState>>) -> (Option<Selected>, Option<String>) {
    let until = Instant::now() + FRONTEND_TAIL_GRACE;
    loop {
        let (selected, error, registering) = {
            let state = state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            (
                state.selected.clone(),
                state.callback_error.clone(),
                state.registering.is_some(),
            )
        };
        if !registering {
            return (selected, error);
        }
        if Instant::now() >= until {
            return (
                None,
                Some(
                    "the native frontend's thread selection was still being registered when its process ended; the staged record requires repair"
                        .to_owned(),
                ),
            );
        }
        thread::sleep(IDLE_POLL);
    }
}

/// Ends the window in which a relay callback may publish a selection, then
/// waits for any in-flight registration. A callback that observes this close
/// retires only the record it staged. The host never adopts that late record.
fn begin_release(state: &Arc<Mutex<SelectionState>>) -> (Option<Selected>, Option<String>) {
    state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .closing = true;
    settled_selection(state)
}

fn is_closing(state: &Arc<Mutex<SelectionState>>) -> bool {
    state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .closing
}

fn register_selection(home: &Path, base: &Endpoint, thread: &str) -> io::Result<Selected> {
    let endpoint = base.for_thread(thread)?;
    let process = endpoint.process.as_ref().ok_or_else(|| {
        invalid(format!(
            "the app-server serving selected thread {thread} carries no process proof"
        ))
    })?;
    if !endpoint_process_live(process) {
        return Err(invalid(format!(
            "the app-server process recorded for selected thread {thread} is no longer live"
        )));
    }
    match control::registered_lead(home, thread) {
        RegisteredLead::Live(existing) if existing.as_ref() != &endpoint => {
            return Err(invalid(format!(
                "thread {thread} already has a different live lead endpoint; this host will not replace another owner's registration"
            )));
        }
        RegisteredLead::Live(_) | RegisteredLead::Unavailable(_) => {}
    }
    let path = publish_lead_endpoint(home, &endpoint).map_err(|error| {
        io::Error::other(format!(
            "the selected thread {thread} could not be registered before its first turn: {error}"
        ))
    })?;
    Ok(Selected {
        thread: thread.to_owned(),
        endpoint,
        path,
    })
}

fn retire_transient(plan: &ControlPlan) -> Vec<String> {
    let mut notes = Vec::new();
    for path in [&plan.paths.endpoint, &plan.paths.token] {
        if let Err(error) = retire_stale_endpoint(path) {
            notes.push(error.to_string());
        }
    }
    notes
}

fn release(
    plan: &ControlPlan,
    job: Job,
    frontend: Option<executor_cli::OwnedFrontend>,
    lifecycle: &Arc<Mutex<Option<LeadLifecycle>>>,
    observed: &Arc<Mutex<SelectionState>>,
    conversation: &Conversation,
    launcher: &Path,
) -> (Option<u32>, Vec<String>) {
    let mut notes = Vec::new();
    let (registrations, cleanup_errors) = {
        let mut state = observed
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.closing = true;
        (
            std::mem::take(&mut state.owned),
            std::mem::take(&mut state.cleanup_errors),
        )
    };
    notes.extend(cleanup_errors);
    let frontend_code = match frontend.as_ref() {
        Some(frontend) => match frontend.exit_code() {
            Ok(code) => code,
            Err(error) => {
                notes.push(format!(
                    "the native frontend exit code could not be read: {error}"
                ));
                None
            }
        },
        None => None,
    };
    let owned = lifecycle
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .take();
    if let Some(mut lifecycle) = owned
        && let Err(error) = lifecycle.stop()
    {
        notes.push(format!("reflection lifecycle cleanup: {error}"));
    }
    if let Some(frontend) = frontend
        && let Err(error) = frontend.close()
    {
        notes.push(format!("the native frontend could not be ended: {error}"));
    }
    for selection in registrations {
        if let Err(error) = retire_owned_registry(&selection) {
            notes.push(error.to_string());
        }
    }
    notes.extend(retire_transient(plan));
    if let Err(error) = end_owned_child(job, conversation, launcher) {
        notes.push(error.to_string());
    }
    (frontend_code, notes)
}

fn retire_owned_registry(selection: &Selected) -> io::Result<()> {
    match Endpoint::read(&selection.path) {
        Ok(current) if current == selection.endpoint => retire_stale_endpoint(&selection.path),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io::Error::other(format!(
            "the selected thread registry {} could not be read for ownership-safe retirement: {error}; it was left unchanged",
            selection.path.display()
        ))),
    }
}
