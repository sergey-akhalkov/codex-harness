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
use crate::self_reflection_cli::host::LeadLifecycle;
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
    if let Err(error) = conversation.endpoint().record(&plan.paths.endpoint) {
        let (_, notes) = release(&plan, job, None, None, &conversation, None, &launcher);
        for note in notes {
            eprintln!("ordinary lead note: {note}");
        }
        return Err(invalid(format!(
            "the ordinary app-server endpoint could not be recorded before attachment: {error}"
        )));
    }

    let observed = Arc::new(Mutex::new(SelectionState::default()));
    let observer = observer(
        Arc::clone(&observed),
        home.clone(),
        conversation.endpoint().clone(),
    );
    let frontend =
        match attach_observed_frontend(&plan, &original_cwd, &tui, &conversation, observer) {
            Ok(frontend) => frontend,
            Err(error) => {
                let (_, notes) = release(&plan, job, None, None, &conversation, None, &launcher);
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

    let mut selection: Option<Selected> = None;
    let mut lifecycle: Option<LeadLifecycle> = None;
    let mut failure: Option<String> = None;
    let mut deadline: Option<Instant> = None;
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
        let (selected_now, observer_error) = {
            let state = observed
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            (state.selected.clone(), state.callback_error.clone())
        };
        if failure.is_none()
            && let Some(error) = observer_error
        {
            failure = Some(format!(
                "the native frontend's thread selection was not registered: {error}"
            ));
            deadline = Some(Instant::now() + FRONTEND_TAIL_GRACE);
        }
        if selection.is_none()
            && let Some(selected_now) = selected_now
        {
            selection = Some(selected_now.clone());
            match start_selected(
                &selected_now,
                &mut conversation,
                &home,
                &backend_cwd,
                &plan.paths.log,
            ) {
                Ok(started) => lifecycle = Some(started),
                Err(error) => {
                    failure.get_or_insert_with(|| {
                        format!(
                            "the selected native thread could not be adopted by the same app-server: {error}"
                        )
                    });
                    deadline.get_or_insert_with(|| Instant::now() + FRONTEND_TAIL_GRACE);
                }
            }
        }
        if lifecycle.is_some() {
            if failure.is_none()
                && running
                && let Err(error) = conversation.pump()
            {
                failure = Some(format!(
                    "the ordinary app-server control connection ended while the native frontend was attached: {error}"
                ));
                deadline = Some(Instant::now() + FRONTEND_TAIL_GRACE);
            }
            if let Some(started) = lifecycle.as_mut()
                && let Err(error) = started.tick()
            {
                note_log(&plan.paths.log, &format!("reflection lifecycle: {error}"));
            }
        }
        if !running {
            break;
        }
        if failure.is_some() && deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            break;
        }
        thread::sleep(if selection.is_some() {
            control::DEFAULT_POLL
        } else {
            IDLE_POLL
        });
    }

    // The frontend can finish immediately after the relay accepted its native
    // selection. Re-read the callback state after process exit so that a
    // selection observed at that boundary is still adopted, prepared and
    // cleaned up rather than left registered.
    let (late_selected, late_error) = {
        let state = observed
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        (state.selected.clone(), state.callback_error.clone())
    };
    if failure.is_none()
        && let Some(error) = late_error
    {
        failure = Some(format!(
            "the native frontend's thread selection was not registered: {error}"
        ));
    }
    if selection.is_none()
        && let Some(selected_now) = late_selected
    {
        selection = Some(selected_now.clone());
        match start_selected(
            &selected_now,
            &mut conversation,
            &home,
            &backend_cwd,
            &plan.paths.log,
        ) {
            Ok(started) => lifecycle = Some(started),
            Err(error) => {
                failure.get_or_insert_with(|| {
                    format!(
                        "the selected native thread could not be adopted by the same app-server: {error}"
                    )
                });
            }
        }
    }

    let (frontend_code, notes) = release(
        &plan,
        job,
        Some(frontend),
        lifecycle.as_mut(),
        &conversation,
        selection.as_ref(),
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

fn start_selected(
    selected: &Selected,
    conversation: &mut Conversation,
    home: &Path,
    backend_cwd: &Path,
    log: &Path,
) -> io::Result<LeadLifecycle> {
    conversation.adopt_thread(&selected.thread)?;
    note_log(
        log,
        &format!(
            "ordinary lead: selected thread {} registry {}",
            selected.thread,
            selected.path.display()
        ),
    );
    let mut lifecycle = LeadLifecycle::new(home, backend_cwd, &selected.thread);
    if let Err(error) = lifecycle.tick() {
        note_log(log, &format!("reflection lifecycle: {error}"));
    }
    Ok(lifecycle)
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
    callback_error: Option<String>,
}

fn observer(state: Arc<Mutex<SelectionState>>, home: PathBuf, base: Endpoint) -> SelectionObserver {
    Arc::new(move |thread| {
        let mut state = state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(selected) = state.selected.as_ref() {
            if selected.thread == thread {
                return Ok(());
            }
            let error = invalid(format!(
                "the native frontend selected thread {thread} after thread {} on the same app-server; same-backend native selection switching requires a verified current-lifetime transfer and is an unfinished dependency",
                selected.thread
            ));
            state.callback_error = Some(error.to_string());
            return Err(error);
        }
        match register_selection(&home, &base, thread) {
            Ok(selected) => {
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
    lifecycle: Option<&mut LeadLifecycle>,
    conversation: &Conversation,
    selection: Option<&Selected>,
    launcher: &Path,
) -> (Option<u32>, Vec<String>) {
    let mut notes = Vec::new();
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
    if let Some(lifecycle) = lifecycle
        && let Err(error) = lifecycle.stop()
    {
        notes.push(format!("reflection lifecycle cleanup: {error}"));
    }
    if let Some(frontend) = frontend
        && let Err(error) = frontend.close()
    {
        notes.push(format!("the native frontend could not be ended: {error}"));
    }
    if let Some(selection) = selection
        && let Err(error) = retire_owned_registry(selection)
    {
        notes.push(error.to_string());
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
