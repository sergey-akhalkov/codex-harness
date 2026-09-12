//! One owned native server per interactive launch. The service owns its process
//! tree independently of the TUI and records observations in private host state.
use crate::{
    broker_endpoint::random_key,
    broker_state::BrokerRoot,
    build_identity,
    process::{Cancellation, CommandSpec, Deadline},
    process_service::{self, ServiceGuard},
    registration_native::{FileGuard, StagedFile},
    task_control::ControlConnection,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, VecDeque},
    env,
    ffi::OsStr,
    fs::{self, File},
    io::{self, Read},
    net::TcpListener,
    path::Path,
    process::Command,
    time::{Duration, Instant},
};

const STARTUP: Duration = Duration::from_secs(20);
const POLL: Duration = Duration::from_millis(200);
const TOKEN_ENV: &str = "HARNESS_TASK_CONTROL_TOKEN";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Launch {
    schema: u32,
    executable: std::path::PathBuf,
    sha256: String,
    cwd: std::path::PathBuf,
    arguments: Vec<String>,
    #[serde(default)]
    new_session: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Endpoint {
    schema: u32,
    port: u16,
    token: String,
    #[serde(default)]
    thread_id: Option<String>,
}

fn unicode(value: &OsStr) -> io::Result<String> {
    value.to_str().map(str::to_owned).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "control launch requires Unicode arguments",
        )
    })
}

pub(crate) fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> io::Result<T> {
    retry_checkpoint(|| {
        let mut bytes = Vec::new();
        File::open(path)?
            .take(1024 * 1024 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() > 1024 * 1024 {
            return Err(io::Error::other("task record exceeds its bound"));
        }
        Ok(serde_json::from_slice(&bytes)?)
    })
}

fn retry_checkpoint<T>(mut operation: impl FnMut() -> io::Result<T>) -> io::Result<T> {
    let until = Instant::now() + Duration::from_millis(500);
    loop {
        match operation() {
            Err(error)
                if matches!(error.raw_os_error(), Some(32 | 33)) && Instant::now() < until =>
            {
                // A guarded transaction excludes brief readers too. Retry only
                // sharing/lock contention; identity, content and other errors
                // retain their meaning. This never repeats a native RPC.
                std::thread::sleep(Duration::from_millis(10));
            }
            result => return result,
        }
    }
}

/// Explicit cancellation of this owned runtime, retaining its checkpoints.
pub fn request_stop(path: &Path) -> io::Result<()> {
    let root = BrokerRoot::open(path)?;
    save(
        &root.path().join("stop.json"),
        &json!({"schema":1,"stop":true}),
    )
}

/// Atomic replacement preserves the previous checkpoint if staging fails.
pub(crate) fn save(path: &Path, value: &Value) -> io::Result<()> {
    let after = serde_json::to_vec(value)?;
    if after.len() > 1024 * 1024 {
        return Err(io::Error::other("task checkpoint exceeds its bound"));
    }
    retry_checkpoint(|| match FileGuard::read_regular(path) {
        Ok((guard, before)) => {
            let identity = guard.object_identity()?;
            drop(guard);
            FileGuard::replace_regular(path, &identity, &before, &after)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            StagedFile::create(path, &after)?.commit()
        }
        Err(error) => Err(error),
    })
}

/// The caller has already verified the registered upstream and runtime. None
/// preserves the ordinary CLI path for commands outside managed interaction.
pub fn run(command: &Command, manager: &Path, home: &Path) -> io::Result<Option<i32>> {
    let arguments = command
        .get_args()
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    let plan = match crate::task_arguments::plan(&arguments, &env::current_dir()?) {
        Ok(Some(plan)) => plan,
        Ok(None) => return Ok(None),
        Err(error) => {
            eprintln!(
                "codex-harness: task control unavailable for these arguments ({error}); starting native Codex"
            );
            return Ok(None);
        }
    };
    let placements = crate::task_view::three_windows()?;
    let root = BrokerRoot::prepare()?.keep();
    let launch = Launch {
        schema: 1,
        executable: command.get_program().into(),
        sha256: build_identity::hash_file(Path::new(command.get_program()))?,
        cwd: plan.cwd,
        arguments: plan
            .backend
            .iter()
            .map(|value| unicode(value))
            .collect::<io::Result<_>>()?,
        new_session: plan.new_session,
    };
    let launch_path = root.path().join("launch.json");
    save(&launch_path, &serde_json::to_value(&launch)?)?;
    let mut environment = env::vars_os()
        .map(|(key, value)| Ok((unicode(&key)?.to_uppercase(), unicode(&value)?)))
        .collect::<io::Result<BTreeMap<_, _>>>()?;
    for (key, value) in command.get_envs() {
        let key = unicode(key)?.to_uppercase();
        if let Some(value) = value {
            environment.insert(key, unicode(value)?);
        } else {
            environment.remove(&key);
        }
    }
    environment.insert("CODEX_HOME".into(), unicode(home.as_os_str())?);
    // The service launcher carries environment through its private pipe, never
    // in command-line arguments or persisted environment snapshots.
    let service = process_service::spawn(
        manager,
        root.path(),
        vec![
            "task-control".into(),
            build_identity::hash_file(&launch_path)?,
            "{}".into(),
        ],
        environment,
        Deadline::after(STARTUP)?,
        &Cancellation::default(),
    )?;
    save(
        &root.path().join("runtime.json"),
        &json!({"schema":1,"process":{"pid":service.identity().pid,"creationTime":service.identity().creation_time},"executable":manager}),
    )?;
    let until = Instant::now() + STARTUP;
    let endpoint: Endpoint = loop {
        if root.path().join("endpoint.json").try_exists()? {
            break read_json(&root.path().join("endpoint.json"))?;
        }
        if !service.is_running()? || Instant::now() >= until {
            return Err(io::Error::other(format!(
                "task control did not become ready; private evidence: {}",
                root.path().display()
            )));
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    if endpoint.schema != 1 {
        return Err(io::Error::other("unsupported task control endpoint"));
    }
    let mut tui = CommandSpec::new(command.get_program());
    for (key, value) in command.get_envs() {
        if let Some(value) = value {
            tui.env.insert(key.to_owned(), Some(value.to_owned()));
        } else {
            tui.env.insert(key.to_owned(), None);
        }
    }
    tui.env.insert("CODEX_HOME".into(), Some(home.into()));
    tui.env
        .insert(TOKEN_ENV.into(), Some(endpoint.token.clone().into()));
    tui.args = vec![
        "--remote".into(),
        format!("ws://127.0.0.1:{}", endpoint.port).into(),
        "--remote-auth-token-env".into(),
        TOKEN_ENV.into(),
    ];
    if let Some(id) = &endpoint.thread_id {
        tui.args.extend(["resume".into(), id.into()]);
        tui.args.extend(plan.attachment);
    } else {
        tui.args.extend(plan.tui);
    }
    tui.current_dir = Some(env::current_dir()?);
    tui.new_console = Some("Codex task".into());
    eprintln!(
        "codex-harness: task control state: {}",
        root.path().display()
    );
    let view = match crate::task_view::View::spawn(&tui, placements[0], STARTUP) {
        Ok(view) => view,
        Err(error) => {
            request_stop(root.path())?;
            return Err(error);
        }
    };
    save(
        &root.path().join("view.json"),
        &json!({"schema":1,"threadId":endpoint.thread_id,"window":view.snapshot()?}),
    )?;
    let mut visible = None;
    let mut additional = BTreeMap::<String, crate::task_view::View>::new();
    let mut snapshots = BTreeMap::<String, crate::task_view::Snapshot>::new();
    loop {
        if !service.is_running()? {
            return Err(io::Error::other(
                "task controller exited while its conversation was open",
            ));
        }
        match read_json::<crate::task_view::Request>(&root.path().join("view-request.json")) {
            Ok(request) => {
                if request.schema != 1
                    || request.slot == 0
                    || request.slot >= placements.len()
                    || request.thread_id.is_empty()
                    || endpoint.thread_id.as_ref() == Some(&request.thread_id)
                {
                    request_stop(root.path())?;
                    return Err(io::Error::other(
                        "invalid additional conversation view request",
                    ));
                }
                if let std::collections::btree_map::Entry::Vacant(entry) =
                    additional.entry(request.thread_id.clone())
                {
                    let mut next = CommandSpec::new(command.get_program());
                    next.env.clone_from(&tui.env);
                    next.current_dir = Some(launch.cwd.clone());
                    next.args = vec![
                        "--remote".into(),
                        format!("ws://127.0.0.1:{}", endpoint.port).into(),
                        "--remote-auth-token-env".into(),
                        TOKEN_ENV.into(),
                        "resume".into(),
                        request.thread_id.clone().into(),
                        "--no-alt-screen".into(),
                    ];
                    next.new_console = Some(format!("Opening {}", request.title).into());
                    let next = match crate::task_view::View::spawn(
                        &next,
                        placements[request.slot],
                        STARTUP,
                    )
                    .and_then(|view| {
                        view.wait_for_title(&request.title, STARTUP)?;
                        Ok(view)
                    }) {
                        Ok(view) => view,
                        Err(error) => {
                            save(
                                &root.path().join("view-error.json"),
                                &json!({"schema":1,"threadId":request.thread_id,"error":error.to_string()}),
                            )?;
                            request_stop(root.path())?;
                            return Err(error);
                        }
                    };
                    snapshots.insert(request.thread_id.clone(), next.snapshot()?);
                    entry.insert(next);
                    save(
                        &root.path().join("additional-views.json"),
                        &json!({"schema":1,"threads":snapshots}),
                    )?;
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => (),
            Err(error) => return Err(error),
        }
        if !view.is_running()?
            && !additional
                .values()
                .map(|view| view.is_running())
                .collect::<io::Result<Vec<_>>>()?
                .contains(&true)
        {
            break;
        }
        let observed = view.snapshot().is_ok();
        if visible != Some(observed) {
            save(
                &root.path().join("view-state.json"),
                &json!({"schema":1,"visible":observed}),
            )?;
            visible = Some(observed);
        }
        std::thread::sleep(POLL);
    }
    let mut exit_code = view
        .exit_code()?
        .ok_or_else(|| io::Error::other("native TUI exit state is unknown"))?;
    for view in additional.values() {
        let code = view
            .exit_code()?
            .ok_or_else(|| io::Error::other("additional TUI exit state is unknown"))?;
        if exit_code == 0 {
            exit_code = code;
        }
    }
    save(
        &root.path().join("client-closed.json"),
        &json!({"schema":1,"exitCode":exit_code}),
    )?;
    Ok(Some(exit_code as i32))
}

pub fn serve(mut guard: ServiceGuard, expected: &str) -> io::Result<()> {
    let root = BrokerRoot::open(&env::current_dir()?)?;
    let _owner = root
        .try_instance()?
        .ok_or_else(|| io::Error::other("task control already has an owner"))?;
    let launch_path = root.path().join("launch.json");
    if build_identity::hash_file(&launch_path)? != expected {
        return Err(io::Error::other(
            "task launch changed before service startup",
        ));
    }
    let launch: Launch = read_json(&launch_path)?;
    if launch.schema != 1 || build_identity::hash_file(&launch.executable)? != launch.sha256 {
        return Err(io::Error::other("task control upstream identity changed"));
    }
    let log = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(root.path().join("service.log"))?;
    guard.redirect_standard_streams(&log)?;
    let token = random_key()?;
    StagedFile::create(&root.path().join("ws-token"), token.as_bytes())?.commit()?;
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let port = listener.local_addr()?.port();
    drop(listener);
    let mut command = CommandSpec::new(&launch.executable);
    command.current_dir = Some(launch.cwd.clone());
    command.args = vec!["app-server".into()];
    command
        .args
        .extend(launch.arguments.into_iter().map(Into::into));
    command.args.extend([
        "--listen".into(),
        format!("ws://127.0.0.1:{port}").into(),
        "--ws-auth".into(),
        "capability-token".into(),
        "--ws-token-file".into(),
        root.path().join("ws-token").into_os_string(),
    ]);
    command.stdout = Some(log.try_clone()?);
    command.stderr = Some(log.try_clone()?);
    let server = guard.job().spawn(&command)?;
    let until = Instant::now() + STARTUP;
    let mut connection = loop {
        match ControlConnection::connect(port, &token, POLL) {
            Ok(connection) => break connection,
            Err(error) if Instant::now() >= until || !server.is_running()? => return Err(error),
            Err(_) => std::thread::sleep(Duration::from_millis(50)),
        }
    };
    connection.send(&json!({"id":1,"method":"initialize","params":{"clientInfo":{"name":"harness-task-control","version":"1"},"capabilities":{"experimentalApi":true}}}), STARTUP)?;
    loop {
        if Instant::now() >= until {
            return Err(io::Error::other("task control initialization deadline"));
        }
        if let Some(value) = connection.receive(POLL)? {
            if value["id"] != 1 {
                continue;
            }
            if value.get("error").is_some() {
                return Err(io::Error::other("task control initialization rejected"));
            }
            break;
        }
    }
    connection.send(&json!({"method":"initialized"}), STARTUP)?;
    let mut initial_events = VecDeque::new();
    let thread_id = if launch.new_session {
        let started = startup_request(
            &root,
            &mut connection,
            &mut initial_events,
            "startup-thread",
            "thread/start",
            json!({"cwd":launch.cwd,"allowProviderModelFallback":false}),
        )?;
        let id = started["thread"]["id"]
            .as_str()
            .ok_or_else(|| io::Error::other("native task thread identity missing"))?
            .to_owned();
        startup_request(
            &root,
            &mut connection,
            &mut initial_events,
            "startup-name",
            "thread/name/set",
            json!({"threadId":id,"name":"Codex task"}),
        )?;
        // Materialize the named empty thread through its native ID. The TUI
        // resumes by rollout path, which otherwise has no source history yet.
        let attached = startup_request(
            &root,
            &mut connection,
            &mut initial_events,
            "startup-attach",
            "thread/resume",
            json!({"threadId":id}),
        )?;
        if attached["thread"]["id"] != id
            || attached["model"] != started["model"]
            || attached["modelProvider"] != started["modelProvider"]
        {
            return Err(io::Error::other(
                "native attachment changed the task binding",
            ));
        }
        save(
            &root.path().join("leader.json"),
            &json!({"schema":1,"threadId":id,"model":started["model"],"modelProvider":started["modelProvider"],"native":started}),
        )?;
        Some(id)
    } else {
        None
    };
    save(
        &root.path().join("endpoint.json"),
        &serde_json::to_value(Endpoint {
            schema: 1,
            port,
            token,
            thread_id,
        })?,
    )?;
    guard.mark_ready()?;
    let result =
        crate::task_observer::observe(&root, &mut connection, initial_events, &launch.executable);
    if let Err(error) = &result {
        let _ = save(
            &root.path().join("controller-error.json"),
            &json!({"schema":1,"error":error.to_string(),"reconciliationRequired":true}),
        );
    }
    // No inferred retry or replay: later recovery must reconcile the checkpoint.
    guard.exit(if result.is_ok() { 0 } else { 1 });
}

fn startup_request(
    root: &BrokerRoot,
    connection: &mut ControlConnection,
    events: &mut VecDeque<Value>,
    id: &str,
    method: &str,
    params: Value,
) -> io::Result<Value> {
    connection.send(&json!({"id":id,"method":method,"params":params}), STARTUP)?;
    let until = Instant::now() + STARTUP;
    loop {
        if Instant::now() >= until {
            return Err(io::Error::other("native task preparation deadline"));
        }
        if let Some(value) = connection.receive(POLL)? {
            if value.get("method").is_none() && value["id"] == id {
                if value.get("error").is_some() {
                    save(&root.path().join("startup-error.json"), &value)?;
                    return Err(io::Error::other(
                        "native task preparation rejected; inspect private startup error",
                    ));
                }
                return Ok(value["result"].clone());
            }
            if events.len() >= 512 {
                return Err(io::Error::other("native task startup event bound"));
            }
            events.push_back(value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checkpoint_waits_for_short_reader_without_losing_prior_state() {
        let owned = tempfile::tempdir().unwrap();
        let path = owned.path().join("state.json");
        save(&path, &json!({"generation":1})).unwrap();
        let reader = File::open(&path).unwrap();
        let release = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(60));
            drop(reader);
        });
        let result = save(&path, &json!({"generation":2}));
        release.join().unwrap();
        result.expect("brief observation must not abort checkpoint persistence");
        assert_eq!(read_json::<Value>(&path).unwrap(), json!({"generation":2}));
    }

    #[test]
    fn checkpoint_reports_persistent_sharing_failure_and_preserves_previous_state() {
        let owned = tempfile::tempdir().unwrap();
        let path = owned.path().join("state.json");
        save(&path, &json!({"generation":1})).unwrap();
        let reader = File::open(&path).unwrap();
        let started = Instant::now();
        let error = save(&path, &json!({"generation":2})).unwrap_err();
        assert!(matches!(error.raw_os_error(), Some(32 | 33)));
        assert!(started.elapsed() < Duration::from_secs(2));
        drop(reader);
        assert_eq!(read_json::<Value>(&path).unwrap(), json!({"generation":1}));
    }
}
