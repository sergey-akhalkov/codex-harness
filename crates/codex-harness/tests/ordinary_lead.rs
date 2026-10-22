#![cfg(windows)]
//! Entry-point checks for the ordinary native host. Each check runs the real
//! `_ordinary-lead` manager in an owned console, one harness-owned app-server
//! child and the owned native frontend double. The doubles speak only local
//! canned protocols; no model, installed Codex CLI or subscription is used.

use harness_core::{
    build_identity,
    console::{ConsoleOutcome, ConsoleSession, ConsoleSpec},
    native_launcher::{Registration, Upstream},
    process::{Cancellation, CommandSpec, Deadline, ProcessIdentity},
};
use serde_json::Value;
use std::{
    ffi::OsString,
    fs,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT},
    System::Threading::{
        GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
        WaitForSingleObject,
    },
};

const MANAGER: &str = env!("CARGO_BIN_EXE_codex-harness");
const BACKEND: &str = env!("CARGO_BIN_EXE_harness-executor-fixture");
const FRONTEND: &str = env!("CARGO_BIN_EXE_harness-frontend-double");
const MODEL: &str = "deepseek-v4-flash";
const PROVIDER: &str = "fixture-provider";
const EFFORT: &str = "max";
const NEW_THREAD: &str = "01a0c719-f4d4-7880-a9d2-1a96ee0f23f4";
const RESUME_THREAD: &str = "01a0c719-f4d4-7880-a9d2-1a96ee0f23f5";
const FORK_THREAD: &str = "01a0c719-f4d4-7880-a9d2-1a96ee0f23f6";
const TRANSFER_NEW_THREAD: &str = "01a0c719-f4d4-7880-a9d2-1a96ee0f23f7";
const PROMPT: &str = "ordinary native prompt";
const WAIT: Duration = Duration::from_secs(30);
const CLEANUP: Duration = Duration::from_secs(10);

struct Fixture {
    _root: tempfile::TempDir,
    home: PathBuf,
    project: PathBuf,
    record: PathBuf,
    kit: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        let project = root.path().join("project");
        fs::create_dir_all(home.join("harness/bin")).unwrap();
        fs::create_dir_all(&project).unwrap();
        fs::copy(BACKEND, home.join("harness/bin/codex.exe")).unwrap();
        let frontend = PathBuf::from(FRONTEND);
        let registration = Registration {
            schema: 2,
            state: None,
            build: None,
            task_control: false,
            goal_recovery: false,
            upstream: Upstream {
                executable: frontend.clone(),
                sha256: build_identity::hash_file(&frontend).unwrap(),
                package: None,
            },
        };
        fs::write(
            home.join("harness/native-launch.json"),
            serde_json::to_vec_pretty(&registration).unwrap(),
        )
        .unwrap();
        fs::write(
            home.join("config.toml"),
            format!(
                "model = \"{MODEL}\"\nmodel_provider = \"{PROVIDER}\"\nmodel_reasoning_effort = \"{EFFORT}\"\n"
            ),
        )
        .unwrap();
        fs::write(home.join("frontend-title.txt"), "ordinary lead fixture").unwrap();
        Self {
            record: root.path().join("backend-record.json"),
            kit: root.path().join("kit"),
            _root: root,
            home,
            project,
        }
    }

    /// Adds the board, git and kit metadata the real reflection lifecycle reads.
    /// Existing entrypoint checks keep their smaller fixture unchanged.
    fn reflection(&mut self) {
        git(&self.project, &["init", "-q", "--initial-branch=main"]);
        git(
            &self.project,
            &["config", "user.email", "fixture@example.test"],
        );
        git(&self.project, &["config", "user.name", "Fixture"]);
        fs::write(self.project.join("README.md"), "synthetic ordinary lead\n").unwrap();
        git(&self.project, &["add", "."]);
        git(&self.project, &["commit", "-qm", "seed"]);
        let bd = bd_executable();
        let init = Command::new(&bd)
            .args([
                "init",
                "--skip-agents",
                "--non-interactive",
                "--quiet",
                "--prefix",
                "bdol",
            ])
            .current_dir(&self.project)
            .env("BD_NON_INTERACTIVE", "1")
            .env("BEADS_ACTOR", "ordinary-lead-fixture")
            .output()
            .expect("bd init runs");
        assert!(
            init.status.success(),
            "bd init: {} {}",
            String::from_utf8_lossy(&init.stdout),
            String::from_utf8_lossy(&init.stderr)
        );
        let bin = self.home.join("harness/bin");
        fs::copy(&bd, bin.join("bd.exe")).unwrap();
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .unwrap();
        copy_tree(&workspace.join("global"), &self.kit.join("global"));
        copy_tree(
            &workspace.join(".agents/skills"),
            &self.kit.join(".agents/skills"),
        );
        // The workspace kit routes the executor-side reflection stage through
        // this explicit profile. The synthetic home provides the same binding
        // shape an installed profile file would.
        fs::write(
            self.home.join("ds.config.toml"),
            format!(
                "model = \"{MODEL}\"\nmodel_provider = \"{PROVIDER}\"\nmodel_reasoning_effort = \"{EFFORT}\"\n"
            ),
        )
        .unwrap();
        fs::write(
            self.home.join("harness/installation.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "settings": {"sourceRoot": self.kit}
            }))
            .unwrap(),
        )
        .unwrap();
    }

    fn command(
        &self,
        mode: &str,
        selected: &str,
        source: Option<&str>,
        native: &[&str],
    ) -> CommandSpec {
        let manager = std::env::var_os("HARNESS_ACCEPTANCE_BUILD")
            .map(|build| PathBuf::from(build).join("codex-harness.exe"))
            .unwrap_or_else(|| PathBuf::from(MANAGER));
        let mut spec = CommandSpec::new(manager);
        spec.current_dir = Some(self.project.clone());
        spec.args = vec![
            OsString::from("_ordinary-lead"),
            OsString::from("--codex-home"),
            self.home.as_os_str().to_owned(),
            OsString::from("--"),
        ];
        spec.args.extend(native.iter().map(OsString::from));
        spec.env
            .insert("CODEX_HOME".into(), Some(self.home.as_os_str().to_owned()));
        spec.env.insert(
            "HARNESS_EXECUTOR_CHILD_FIXTURE_MODE".into(),
            Some("control-app-server".into()),
        );
        spec.env.insert(
            "HARNESS_IMPROVEMENT_FIXTURE_SESSION".into(),
            Some(selected.into()),
        );
        spec.env.insert(
            "HARNESS_IMPROVEMENT_FIXTURE_SOLUTION_TEXT".into(),
            Some(String::new().into()),
        );
        spec.env
            .insert("HARNESS_FRONTEND_PROTOCOL_MODE".into(), Some(mode.into()));
        spec.env
            .insert("HARNESS_FRONTEND_THREAD".into(), source.map(OsString::from));
        spec.env.insert(
            "HARNESS_EXECUTOR_FIXTURE_RECORD".into(),
            Some(self.record.as_os_str().to_owned()),
        );
        for name in [
            "HARNESS_EXECUTOR_SESSION",
            "HARNESS_EXECUTOR_RUN",
            "HARNESS_EXECUTOR_FIXTURE_MODE",
            "HARNESS_BOUNDED_LEAD_SESSION",
            "HARNESS_REFLECTION_SESSION",
            "CODEX_SESSION_ID",
            "CODEX_THREAD_ID",
            "HARNESS_ORIGINATING_LEAD",
            "HARNESS_LEAD_THREAD",
            "HARNESS_LEAD_RECIPIENT",
            "WT_SESSION",
        ] {
            spec.env.insert(name.into(), None);
        }
        spec
    }

    fn registry(&self, thread: &str) -> PathBuf {
        self.home
            .join("harness/lead-endpoints")
            .join(format!("{thread}.json"))
    }

    fn selected(&self) -> PathBuf {
        self.home.join("frontend-selected-thread.txt")
    }

    fn readiness(&self) -> PathBuf {
        self.home.join("frontend-registry-before-turn.txt")
    }

    fn argv(&self) -> PathBuf {
        self.home.join("frontend-argv.txt")
    }

    fn release(&self) {
        fs::write(self.home.join("frontend-release"), b"release").unwrap();
    }
}

fn git(cwd: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn bd_executable() -> PathBuf {
    if let Some(value) = std::env::var_os("HARNESS_BD_EXE") {
        return PathBuf::from(value);
    }
    if let Some(home) = std::env::var_os("CODEX_HOME") {
        let candidate = PathBuf::from(home).join("harness/bin/bd.exe");
        if candidate.is_file() {
            return candidate;
        }
    }
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            let candidate = dir.join("bd.exe");
            if candidate.is_file() {
                return candidate;
            }
        }
    }
    panic!("bd is required on PATH, CODEX_HOME/harness/bin, or HARNESS_BD_EXE");
}

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn spawn(spec: CommandSpec) -> ConsoleSession {
    ConsoleSession::spawn(ConsoleSpec::new(spec)).unwrap()
}

fn wait_for(path: &Path, what: &str, session: &ConsoleSession) {
    let until = Instant::now() + WAIT;
    while Instant::now() < until {
        if path.is_file() {
            return;
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!(
        "{what} did not appear at {}: {}",
        path.display(),
        session.transcript()
    );
}

fn wait_session(session: ConsoleSession) -> ConsoleOutcome {
    session
        .wait(
            Deadline::after(Duration::from_secs(60)).unwrap(),
            &Cancellation::default(),
            CLEANUP,
        )
        .unwrap()
}

fn text(outcome: &ConsoleOutcome) -> String {
    outcome.transcript.replace("\r\n", "\n")
}

fn lines(path: &Path) -> Vec<String> {
    fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect()
}

fn config_values(argv: &[String]) -> Vec<&str> {
    argv.windows(2)
        .filter(|pair| pair[0] == "-c" || pair[0] == "--config")
        .map(|pair| pair[1].as_str())
        .collect()
}

fn assert_cleanup(fixture: &Fixture, backend: ProcessIdentity, thread: &str) {
    assert!(
        !fixture.registry(thread).exists(),
        "the selected thread registry survived its owner"
    );
    let host = fixture.home.join("harness/lead-endpoints/host");
    for entry in fs::read_dir(&host).unwrap() {
        let path = entry.unwrap().path();
        assert!(
            path.extension().is_some_and(|extension| extension == "log"),
            "ordinary host transient state survived cleanup: {}",
            path.display()
        );
    }
    assert_process_gone(backend);
}

fn backend_identity(home: &Path, transcript: &str) -> ProcessIdentity {
    const PREFIX: &str = "ordinary lead: backend pid ";
    let until = Instant::now() + WAIT;
    loop {
        if let Ok(entries) = fs::read_dir(home.join("harness/lead-endpoints/host")) {
            for entry in entries.filter_map(Result::ok) {
                let path = entry.path();
                if !path.extension().is_some_and(|extension| extension == "log") {
                    continue;
                }
                let Ok(text) = fs::read_to_string(&path) else {
                    continue;
                };
                for line in text.lines() {
                    let Some(identity) = line.strip_prefix(PREFIX) else {
                        continue;
                    };
                    let mut fields = identity.split_whitespace();
                    let Some(pid) = fields.next().and_then(|value| value.parse().ok()) else {
                        continue;
                    };
                    if fields.next() != Some("creation") {
                        continue;
                    }
                    let Some(creation_time) = fields.next().and_then(|value| value.parse().ok())
                    else {
                        continue;
                    };
                    return ProcessIdentity { pid, creation_time };
                }
            }
        }
        if Instant::now() >= until {
            let logs = fs::read_dir(home.join("harness/lead-endpoints/host"))
                .into_iter()
                .flatten()
                .filter_map(Result::ok)
                .filter(|entry| {
                    entry
                        .path()
                        .extension()
                        .is_some_and(|extension| extension == "log")
                })
                .filter_map(|entry| {
                    fs::read_to_string(entry.path())
                        .ok()
                        .map(|text| format!("{}:\n{text}", entry.path().display()))
                })
                .collect::<Vec<_>>()
                .join("\n");
            panic!(
                "the ordinary host did not record its backend identity: {transcript}\nhost logs:\n{logs}"
            );
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn assert_process_gone(identity: ProcessIdentity) {
    let until = Instant::now() + CLEANUP;
    while process_alive(identity) && Instant::now() < until {
        thread::sleep(Duration::from_millis(20));
    }
    assert!(
        !process_alive(identity),
        "the owned app-server survived cleanup: pid {} creation {}",
        identity.pid,
        identity.creation_time
    );
}

fn process_alive(identity: ProcessIdentity) -> bool {
    let handle = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
            0,
            identity.pid,
        )
    };
    if handle.is_null() {
        return false;
    }
    let handle = unsafe { OwnedHandle::from_raw_handle(handle) };
    let (mut creation, mut exit, mut kernel, mut user) = unsafe {
        (
            std::mem::zeroed(),
            std::mem::zeroed(),
            std::mem::zeroed(),
            std::mem::zeroed(),
        )
    };
    if unsafe {
        GetProcessTimes(
            handle.as_raw_handle(),
            &mut creation,
            &mut exit,
            &mut kernel,
            &mut user,
        )
    } == 0
    {
        return false;
    }
    let creation_time = ((creation.dwHighDateTime as u64) << 32) | creation.dwLowDateTime as u64;
    if creation_time != identity.creation_time {
        return false;
    }
    let result = unsafe { WaitForSingleObject(handle.as_raw_handle(), 0) };
    assert!(result == WAIT_OBJECT_0 || result == WAIT_TIMEOUT);
    result == WAIT_TIMEOUT
}

fn wait_for_content(path: &Path, expected: &str, what: &str, session: &ConsoleSession) {
    let until = Instant::now() + WAIT;
    while Instant::now() < until {
        if fs::read_to_string(path).ok().as_deref() == Some(expected) {
            return;
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!(
        "{what} did not become {expected:?} at {}: {}",
        path.display(),
        session.transcript()
    );
}

fn reflection_state_path(fixture: &Fixture) -> Option<PathBuf> {
    let dir = fixture.home.join("harness/delivery-reflection");
    for entry in fs::read_dir(dir).ok()? {
        let path = entry.ok()?.path();
        if path
            .extension()
            .is_some_and(|extension| extension == "json")
        {
            return Some(path);
        }
    }
    None
}

/// Reads the one retained lifecycle state the real board source produced.
fn reflection_state(fixture: &Fixture) -> Option<Value> {
    serde_json::from_slice(&fs::read(reflection_state_path(fixture)?).ok()?).ok()
}

fn wait_reflection_binding(fixture: &Fixture, expected: &str, session: &ConsoleSession) -> Value {
    let until = Instant::now() + WAIT;
    while Instant::now() < until {
        if let Some(state) = reflection_state(fixture)
            && state["binding"]["lead_session"].as_str() == Some(expected)
        {
            return state;
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!(
        "reflection state never bound {expected}: {}",
        session.transcript()
    );
}

fn host_logs(fixture: &Fixture) -> String {
    fs::read_dir(fixture.home.join("harness/lead-endpoints/host"))
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|entry| fs::read_to_string(entry.path()).ok())
        .collect()
}

#[test]
fn new_selection_is_registered_before_the_frontend_turn_and_retired_after_exit() {
    let fixture = Fixture::new();
    let unrelated = fixture
        .home
        .join("harness/lead-endpoints/unrelated-registration.json");
    fs::create_dir_all(unrelated.parent().unwrap()).unwrap();
    fs::write(&unrelated, b"inherited record").unwrap();
    let native = [
        "--no-alt-screen",
        "-m",
        MODEL,
        "-c",
        "model_provider=\"fixture-provider\"",
        "-c",
        "model_reasoning_effort=\"max\"",
        "--",
        PROMPT,
    ];
    let session = spawn(fixture.command("new", NEW_THREAD, None, &native));
    wait_for(
        &fixture.selected(),
        "the frontend-selected thread",
        &session,
    );
    wait_for(
        &fixture.readiness(),
        "the pre-turn registry check",
        &session,
    );
    wait_for(&fixture.record, "the backend invocation record", &session);
    let backend = backend_identity(&fixture.home, &session.transcript());

    assert_eq!(fs::read_to_string(fixture.selected()).unwrap(), NEW_THREAD);
    assert_eq!(fs::read_to_string(fixture.readiness()).unwrap(), "true");
    let record: Value = serde_json::from_slice(&fs::read(&fixture.record).unwrap()).unwrap();
    assert_eq!(record["executorSession"], Value::Null);
    assert_eq!(record["boundedLeadSession"], Value::Null);
    assert_eq!(
        Path::new(record["currentDir"].as_str().unwrap())
            .canonicalize()
            .unwrap(),
        fixture.project.canonicalize().unwrap()
    );
    let backend_argv = record["argv"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let config = config_values(&backend_argv);
    assert!(
        config.contains(&"model=\"deepseek-v4-flash\""),
        "{backend_argv:?}"
    );
    assert!(
        config.contains(&"model_provider=\"fixture-provider\""),
        "{backend_argv:?}"
    );
    assert!(
        config.contains(&"model_reasoning_effort=\"max\""),
        "{backend_argv:?}"
    );
    assert!(
        !backend_argv.iter().any(|arg| arg == "-m"),
        "{backend_argv:?}"
    );
    assert!(
        !backend_argv.join(" ").contains("base_url"),
        "{backend_argv:?}"
    );

    let frontend_argv = lines(&fixture.argv());
    assert_eq!(frontend_argv[0], "--remote");
    assert_eq!(frontend_argv[2], "--remote-auth-token-env");
    assert_eq!(frontend_argv[3], "HARNESS_EXECUTOR_FRONTEND_TOKEN");
    assert_eq!(
        &frontend_argv[4..],
        native,
        "the native preference or prompt changed"
    );
    assert!(
        !frontend_argv.iter().any(|arg| arg.contains("provider-url")),
        "{frontend_argv:?}"
    );

    let registered: Value =
        serde_json::from_slice(&fs::read(fixture.registry(NEW_THREAD)).unwrap()).unwrap();
    assert_eq!(registered["threadId"], NEW_THREAD);
    fixture.release();
    let finished = wait_session(session);
    let transcript = text(&finished);
    assert_eq!(finished.outcome.exit_code, 0, "{transcript}");
    assert!(
        unrelated.is_file(),
        "cleanup removed an unrelated registration"
    );
    let logs = fs::read_dir(fixture.home.join("harness/lead-endpoints/host"))
        .unwrap()
        .filter_map(Result::ok)
        .filter_map(|entry| fs::read_to_string(entry.path()).ok())
        .collect::<String>();
    assert!(
        logs.contains(NEW_THREAD),
        "the selected lifecycle path was not entered: {logs}"
    );
    assert_cleanup(&fixture, backend, NEW_THREAD);
}

#[test]
fn resume_keeps_the_native_source_and_registers_the_returned_selection() {
    let fixture = Fixture::new();
    let native = [
        "-c",
        "approval_policy=\"never\"",
        "--config=sandbox_mode=\"read-only\"",
        "--no-alt-screen",
        "resume",
        RESUME_THREAD,
    ];
    let session = spawn(fixture.command("resume", RESUME_THREAD, Some(RESUME_THREAD), &native));
    wait_for(&fixture.selected(), "the resumed thread identity", &session);
    wait_for(
        &fixture.readiness(),
        "the resumed pre-turn registry check",
        &session,
    );
    let backend = backend_identity(&fixture.home, &session.transcript());
    assert_eq!(
        fs::read_to_string(fixture.selected()).unwrap(),
        RESUME_THREAD
    );
    assert_eq!(fs::read_to_string(fixture.readiness()).unwrap(), "true");
    let frontend_argv = lines(&fixture.argv());
    assert_eq!(
        &frontend_argv[4..],
        ["--no-alt-screen", "resume", RESUME_THREAD],
        "the explicit native resume selection changed"
    );
    fixture.release();
    let finished = wait_session(session);
    let transcript = text(&finished);
    assert_eq!(finished.outcome.exit_code, 0, "{transcript}");
    assert_cleanup(&fixture, backend, RESUME_THREAD);
}

#[test]
fn fork_selection_registers_the_returned_thread_before_its_turn() {
    let fixture = Fixture::new();
    let mut spec = fixture.command("fork", FORK_THREAD, None, &["--no-alt-screen"]);
    spec.env
        .insert("HARNESS_FRONTEND_THREAD".into(), Some(NEW_THREAD.into()));
    spec.env.insert(
        "HARNESS_IMPROVEMENT_FIXTURE_FORK_SESSION".into(),
        Some(FORK_THREAD.into()),
    );
    let session = spawn(spec);
    wait_for_content(
        &fixture.selected(),
        FORK_THREAD,
        "the fork-selected thread",
        &session,
    );
    wait_for(
        &fixture.readiness(),
        "the forked pre-turn registry check",
        &session,
    );
    assert_eq!(fs::read_to_string(fixture.readiness()).unwrap(), "true");
    let registered: Value =
        serde_json::from_slice(&fs::read(fixture.registry(FORK_THREAD)).unwrap()).unwrap();
    assert_eq!(registered["threadId"], FORK_THREAD);
    assert!(
        !fixture.registry(NEW_THREAD).exists(),
        "the fork source was registered instead of its returned thread"
    );
    let backend = backend_identity(&fixture.home, &session.transcript());
    fixture.release();
    let finished = wait_session(session);
    let transcript = text(&finished);
    assert_eq!(finished.outcome.exit_code, 0, "{transcript}");
    assert_cleanup(&fixture, backend, FORK_THREAD);
}

#[test]
fn current_selection_transfer_completes_before_the_new_thread_turn() {
    let mut fixture = Fixture::new();
    fixture.reflection();
    let mut spec = fixture.command("transfer", NEW_THREAD, None, &["--no-alt-screen"]);
    spec.env
        .insert("HARNESS_FRONTEND_THREAD".into(), Some(NEW_THREAD.into()));
    spec.env.insert(
        "HARNESS_FRONTEND_FORK_THREAD".into(),
        Some(TRANSFER_NEW_THREAD.into()),
    );
    spec.env.insert(
        "HARNESS_IMPROVEMENT_FIXTURE_FORK_SESSION".into(),
        Some(TRANSFER_NEW_THREAD.into()),
    );
    let session = spawn(spec);
    wait_for(
        &fixture.home.join("frontend-transfer-repeat.txt"),
        "the repeated current selection",
        &session,
    );
    let before = wait_reflection_binding(&fixture, NEW_THREAD, &session);
    let initial_revision = before["binding"]["revision"].as_u64().unwrap();
    assert!(fixture.registry(NEW_THREAD).is_file());

    fs::write(fixture.home.join("frontend-transfer-before-fork"), b"fork").unwrap();
    wait_for_content(
        &fixture.selected(),
        TRANSFER_NEW_THREAD,
        "the transferred thread selection",
        &session,
    );
    let after = wait_reflection_binding(&fixture, TRANSFER_NEW_THREAD, &session);

    // The real selection operation ran inside the relay callback, so both its
    // state and ownership retirement are complete before the frontend receives
    // the fork result and may submit the next turn.
    assert_eq!(
        after["binding"]["revision"].as_u64(),
        Some(initial_revision + 1),
        "the repeat selection was not idempotent or the transfer did not run once: {after}"
    );
    assert!(
        after["retired_leads"]
            .as_array()
            .unwrap()
            .iter()
            .any(|retired| retired.as_str() == Some(NEW_THREAD)),
        "{after}"
    );
    assert!(
        !fixture.registry(NEW_THREAD).exists(),
        "the transferred source registry survived the completed transfer"
    );
    assert!(fixture.registry(TRANSFER_NEW_THREAD).is_file());

    fs::write(fixture.home.join("frontend-transfer-turn-gate"), b"turn").unwrap();
    wait_for(
        &fixture.home.join("frontend-transfer-turn-started.txt"),
        "the transferred thread's first turn",
        &session,
    );
    assert_eq!(
        fs::read_to_string(fixture.home.join("frontend-transfer-turn-started.txt")).unwrap(),
        TRANSFER_NEW_THREAD
    );
    assert!(fixture.registry(TRANSFER_NEW_THREAD).is_file());

    let backend = backend_identity(&fixture.home, &session.transcript());
    fixture.release();
    let finished = wait_session(session);
    let transcript = text(&finished);
    assert_eq!(finished.outcome.exit_code, 0, "{transcript}");
    assert_cleanup(&fixture, backend, TRANSFER_NEW_THREAD);
}

#[test]
fn live_reflection_owner_does_not_make_a_second_host_selection_fatal() {
    let mut fixture = Fixture::new();
    fixture.reflection();
    let owner_exit = fixture.home.join("owner-exit");
    let mut owner_spec = fixture.command("new", NEW_THREAD, None, &["--no-alt-screen"]);
    owner_spec.env.insert(
        "HARNESS_FRONTEND_EXIT_ON_FILE".into(),
        Some(owner_exit.as_os_str().to_owned()),
    );
    let owner = spawn(owner_spec);
    wait_for(
        &fixture.selected(),
        "the retained owner's selection",
        &owner,
    );
    wait_reflection_binding(&fixture, NEW_THREAD, &owner);
    let owner_process = owner.identity();
    assert!(
        process_alive(owner_process),
        "the retained owner exited early"
    );
    let state_path = reflection_state_path(&fixture).expect("the owner published reflection state");
    let owner_state: Value = serde_json::from_slice(&fs::read(&state_path).unwrap()).unwrap();
    let owner_registry = fs::read(fixture.registry(NEW_THREAD)).unwrap();

    let newcomer_exit = fixture.home.join("newcomer-exit");
    let mut spec = fixture.command(
        "transfer",
        RESUME_THREAD,
        Some(RESUME_THREAD),
        &["--profile", "ds", "--no-alt-screen"],
    );
    spec.env.insert(
        "HARNESS_FRONTEND_FORK_THREAD".into(),
        Some(TRANSFER_NEW_THREAD.into()),
    );
    spec.env.insert(
        "HARNESS_IMPROVEMENT_FIXTURE_FORK_SESSION".into(),
        Some(TRANSFER_NEW_THREAD.into()),
    );
    spec.env.insert(
        "HARNESS_FRONTEND_EXIT_ON_FILE".into(),
        Some(newcomer_exit.as_os_str().to_owned()),
    );
    let newcomer = spawn(spec);
    wait_for(
        &fixture.home.join("frontend-transfer-repeat.txt"),
        "the non-owning host's first selection",
        &newcomer,
    );
    assert_eq!(
        fs::read_to_string(fixture.selected()).unwrap(),
        RESUME_THREAD
    );
    assert!(fixture.registry(RESUME_THREAD).is_file());
    assert_eq!(
        fs::read(fixture.registry(NEW_THREAD)).unwrap(),
        owner_registry
    );

    fs::write(fixture.home.join("frontend-transfer-before-fork"), b"fork").unwrap();
    wait_for_content(
        &fixture.selected(),
        TRANSFER_NEW_THREAD,
        "the non-owning host's second native selection",
        &newcomer,
    );
    assert!(fixture.registry(TRANSFER_NEW_THREAD).is_file());
    assert!(fixture.registry(NEW_THREAD).is_file());
    assert!(
        !fixture.registry(RESUME_THREAD).exists(),
        "the prior locally selected registration survived the native switch"
    );
    assert_eq!(
        fs::read(fixture.registry(NEW_THREAD)).unwrap(),
        owner_registry
    );
    let after_selection: Value = serde_json::from_slice(&fs::read(&state_path).unwrap()).unwrap();
    for key in [
        "binding",
        "source",
        "journal",
        "checkpoint",
        "current_session_floor",
        "records",
        "recovery",
        "refusals",
        "retired_leads",
        "stopped_at",
        "owner_process",
    ] {
        assert_eq!(
            after_selection[key], owner_state[key],
            "the non-owning host changed {key}"
        );
    }
    assert!(process_alive(owner_process));

    fs::write(fixture.home.join("frontend-transfer-turn-gate"), b"turn").unwrap();
    wait_for(
        &fixture.home.join("frontend-transfer-turn-started.txt"),
        "the non-owning host's real turn",
        &newcomer,
    );
    assert_eq!(
        fs::read_to_string(fixture.home.join("frontend-transfer-turn-started.txt")).unwrap(),
        TRANSFER_NEW_THREAD
    );

    fs::write(newcomer_exit, b"exit").unwrap();
    let finished = wait_session(newcomer);
    let transcript = text(&finished);
    assert_eq!(finished.outcome.exit_code, 0, "{transcript}");
    assert!(
        !transcript.contains("reflection ownership changed"),
        "{transcript}"
    );
    assert!(
        !transcript.contains("thread selection was not registered"),
        "{transcript}"
    );
    assert!(
        host_logs(&fixture).contains("reflection remains owned by"),
        "{}",
        host_logs(&fixture)
    );
    assert!(
        !host_logs(&fixture).contains("reflection ownership changed"),
        "{}",
        host_logs(&fixture)
    );
    assert!(!fixture.registry(RESUME_THREAD).exists());
    assert!(!fixture.registry(TRANSFER_NEW_THREAD).exists());
    assert_eq!(
        fs::read(fixture.registry(NEW_THREAD)).unwrap(),
        owner_registry
    );
    let after_exit: Value = serde_json::from_slice(&fs::read(&state_path).unwrap()).unwrap();
    for key in [
        "binding",
        "source",
        "journal",
        "checkpoint",
        "current_session_floor",
        "records",
        "recovery",
        "refusals",
        "retired_leads",
        "stopped_at",
        "owner_process",
    ] {
        assert_eq!(
            after_exit[key], owner_state[key],
            "the non-owning host changed {key} on exit"
        );
    }
    assert!(process_alive(owner_process));

    fs::write(owner_exit, b"exit").unwrap();
    let owner_finished = wait_session(owner);
    assert_eq!(
        owner_finished.outcome.exit_code,
        0,
        "{}",
        text(&owner_finished)
    );
    assert!(!fixture.registry(NEW_THREAD).exists());
}

#[test]
fn frontend_exit_before_selection_publication_cleans_up_without_reflection() {
    let mut fixture = Fixture::new();
    fixture.reflection();
    let release = fixture.home.join("frontend-selection-release");
    let exit = fixture.home.join("frontend-exit-trigger");
    let mut spec = fixture.command("new", NEW_THREAD, None, &["--no-alt-screen"]);
    spec.env.insert(
        "HARNESS_ORDINARY_LEAD_FIXTURE_SELECTION_GATE".into(),
        Some(release.as_os_str().to_owned()),
    );
    spec.env.insert(
        "HARNESS_FRONTEND_EXIT_ON_FILE".into(),
        Some(exit.as_os_str().to_owned()),
    );
    let session = spawn(spec);
    wait_for(
        &fixture.registry(NEW_THREAD),
        "the staged selection registry",
        &session,
    );
    fs::write(&exit, b"exit").unwrap();
    wait_for(
        &fixture.home.join("frontend-exit-triggered.txt"),
        "the frontend exit trigger",
        &session,
    );
    // Let the host observe the native process exit while its relay callback is
    // still staged at the selection gate; only then release that callback.
    thread::sleep(Duration::from_millis(300));
    fs::write(&release, b"release").unwrap();

    let finished = wait_session(session);
    let transcript = text(&finished);
    assert_eq!(finished.outcome.exit_code, 0, "{transcript}");
    assert!(
        !fixture.registry(NEW_THREAD).exists(),
        "the late staged registry was not retired"
    );
    assert!(
        reflection_state(&fixture).is_none(),
        "a reflection lifecycle was prepared after the frontend had exited"
    );
    assert!(
        !host_logs(&fixture).contains("reflection lifecycle:"),
        "the host ticked a reflection lifecycle after frontend exit"
    );
    let backend = backend_identity(&fixture.home, &finished.transcript);
    assert_cleanup(&fixture, backend, NEW_THREAD);
}

#[test]
fn the_frontend_exit_code_survives_cleanup() {
    let fixture = Fixture::new();
    let session = spawn(fixture.command("unsupported", NEW_THREAD, None, &["--no-alt-screen"]));
    let finished = wait_session(session);
    let transcript = text(&finished);
    assert_eq!(
        finished.outcome.exit_code, 4,
        "the frontend's own failure code was replaced: {transcript}"
    );
    assert!(
        transcript.contains("unsupported HARNESS_FRONTEND_PROTOCOL_MODE"),
        "{transcript}"
    );
    let backend = backend_identity(&fixture.home, &finished.transcript);
    assert!(!fixture.registry(NEW_THREAD).exists());
    assert_cleanup(&fixture, backend, NEW_THREAD);
}

#[test]
fn a_failed_frontend_attachment_retires_only_the_owned_state() {
    let fixture = Fixture::new();
    fs::remove_file(fixture.home.join("harness/native-launch.json")).unwrap();
    let unrelated = fixture
        .home
        .join("harness/lead-endpoints/unrelated-registration.json");
    fs::create_dir_all(unrelated.parent().unwrap()).unwrap();
    fs::write(&unrelated, b"inherited record").unwrap();
    let session = spawn(fixture.command("new", NEW_THREAD, None, &["--no-alt-screen"]));
    let finished = wait_session(session);
    let transcript = text(&finished);
    assert_eq!(finished.outcome.exit_code, 1, "{transcript}");
    assert!(
        transcript.contains("native frontend attachment is unavailable"),
        "{transcript}"
    );
    assert!(
        unrelated.is_file(),
        "attachment cleanup removed an unrelated registration"
    );
    assert!(!fixture.registry(NEW_THREAD).exists());
    let backend = backend_identity(&fixture.home, &finished.transcript);
    assert_cleanup(&fixture, backend, NEW_THREAD);
}

// The installed ordinary-entry check uses the immutable candidate launcher and
// the real native CLI. The owned provider answers every model request; the
// test sets no executor, bounded-lead or reflection marker and never runs the
// explicit enable command. Root runs this after integrating the final host.
const INSTALLED_FINAL: &str = "ORDINARY_INSTALLED_CANNED_RESULT";
const INSTALLED_PROMPT: &str = "Print the installed ordinary canned result.";
const INSTALLED_MODEL: &str = "gpt-6-astra";
const INSTALLED_PROVIDER: &str = "control_fixture";

#[path = "fixtures/control_responses.rs"]
mod installed_responses;
#[path = "fixtures/synthetic_role_build.rs"]
#[allow(dead_code)]
mod installed_role_build;

#[test]
#[ignore = "requires HARNESS_ACCEPTANCE_BUILD (immutable candidate) and HARNESS_CONTROL_CODEX_EXE; real native frontend/app-server with owned canned Responses; no model request"]
fn installed_ordinary_entry_registers_and_activates_reflection_without_enable() {
    let build = installed_input("HARNESS_ACCEPTANCE_BUILD");
    assert!(
        build.is_dir(),
        "HARNESS_ACCEPTANCE_BUILD must name the immutable build directory: {}",
        build.display()
    );
    let native = installed_input("HARNESS_CONTROL_CODEX_EXE");
    assert!(
        native.is_file(),
        "HARNESS_CONTROL_CODEX_EXE must name the real native codex.exe: {}",
        native.display()
    );
    let record = build_identity::verify_record_integrity(&build).unwrap();
    let source = harness_core::orchestration_config::load(&record.source_root)
        .expect("the candidate's recorded kit source must carry orchestration configuration");
    assert!(
        source.self_reflection_enabled,
        "the candidate was built from a kit source with self_reflection_enabled off"
    );
    if let Some(expected) = std::env::var_os("HARNESS_ACCEPTANCE_SOURCE_IDENTITY") {
        assert_eq!(
            record.source.sha256,
            expected.to_string_lossy().into_owned(),
            "the candidate source identity is not the pinned acceptance input"
        );
    }
    eprintln!(
        "ordinary installed candidate source {} from {}",
        record.source.sha256,
        record.source_root.display()
    );

    let root = tempfile::Builder::new()
        .prefix("ordinary-installed-")
        .tempdir()
        .unwrap()
        .keep();
    eprintln!("ordinary installed evidence {}", root.display());
    let home = root.join("home");
    let evidence = root.join("evidence");
    fs::create_dir_all(home.join("harness/bin")).unwrap();
    fs::create_dir_all(&evidence).unwrap();

    let installed_build = home.join("harness/bin");
    fs::copy(build.join("build.json"), installed_build.join("build.json")).unwrap();
    for name in build_identity::BINARIES {
        fs::copy(build.join(name), installed_build.join(name)).unwrap();
    }
    build_identity::verify_record_integrity(&installed_build)
        .expect("the isolated candidate copy must retain its recorded integrity");
    let launcher = installed_build.join("codex.exe");

    let registration = Registration {
        schema: 2,
        state: None,
        build: Some(installed_build.clone()),
        task_control: false,
        goal_recovery: false,
        upstream: Upstream {
            executable: native.clone(),
            sha256: build_identity::hash_file(&native).unwrap(),
            package: None,
        },
    };
    fs::write(
        home.join("harness/native-launch.json"),
        serde_json::to_vec_pretty(&registration).unwrap(),
    )
    .unwrap();

    let project = installed_role_build::kit_source(&root);
    installed_write_roles(&project);
    let bd = installed_bd();
    installed_git(&project, &["init", "-q", "--initial-branch=main"]);
    installed_git(&project, &["config", "user.email", "fixture@example.test"]);
    installed_git(&project, &["config", "user.name", "Fixture"]);
    fs::write(
        project.join("README.md"),
        "ordinary installed reflection fixture\n",
    )
    .unwrap();
    installed_git(&project, &["add", "."]);
    installed_git(&project, &["commit", "-qm", "seed"]);
    let init = std::process::Command::new(&bd)
        .args(["init", "--skip-agents", "--non-interactive", "--quiet"])
        .current_dir(&project)
        .env("BD_NON_INTERACTIVE", "1")
        .env("BEADS_ACTOR", "ordinary-installed")
        .output()
        .unwrap();
    assert!(
        init.status.success(),
        "bd init: {}\n{}",
        String::from_utf8_lossy(&init.stdout),
        String::from_utf8_lossy(&init.stderr)
    );
    assert!(
        project.join(".beads/config.yaml").is_file(),
        "bd init did not create the project board"
    );

    const SIDE_PROMPT: &str = "Explain the side question fixture.";
    const SIDE_FINAL: &str = "SIDE_QUESTION_CANNED_RESULT";
    const RETURN_PROMPT: &str = "Continue the main conversation fixture.";
    const RETURN_FINAL: &str = "RETURNED_MAIN_CANNED_RESULT";
    let responses = installed_responses::Responses::routed(evidence.clone(), |request| {
        let request = request.to_string();
        let result = if request.contains(RETURN_PROMPT) {
            RETURN_FINAL
        } else if request.contains(SIDE_PROMPT) {
            SIDE_FINAL
        } else {
            INSTALLED_FINAL
        };
        serde_json::json!({
            "type": "message",
            "id": "msg-installed-final",
            "role": "assistant",
            "content": [{"type": "output_text", "text": result}]
        })
    });
    installed_write_home(&home, &project, responses.port);
    fs::write(
        home.join("ds.config.toml"),
        "model = \"deepseek-v4-flash\"\nmodel_provider = \"control_fixture\"\nmodel_reasoning_effort = \"max\"\n",
    )
    .unwrap();

    let mut spec = CommandSpec::new(&launcher);
    spec.current_dir = Some(project.clone());
    spec.args = vec![OsString::from("--no-alt-screen")];
    spec.env
        .insert("CODEX_HOME".into(), Some(home.as_os_str().to_owned()));
    spec.env.insert(
        "HARNESS_CONTROL_FIXTURE_KEY".into(),
        Some("harness-installed-ordinary".into()),
    );
    spec.env
        .insert("PATH".into(), Some(installed_child_path(&bd)));
    for name in [
        "HARNESS_EXECUTOR_SESSION",
        "HARNESS_EXECUTOR_RUN",
        "HARNESS_REFLECTION_SESSION",
        "HARNESS_BOUNDED_LEAD_SESSION",
        "CODEX_SESSION_ID",
        "CODEX_THREAD_ID",
        "HARNESS_ORIGINATING_LEAD",
        "HARNESS_LEAD_THREAD",
        "HARNESS_LEAD_RECIPIENT",
        "WT_SESSION",
        "OPENAI_API_KEY",
        "CODEX_API_KEY",
    ] {
        spec.env.insert(name.into(), None);
    }
    let mut console = ConsoleSpec::new(spec);
    console.limits.memory_bytes = Some(512 * 1024 * 1024);
    console.limits.cpu_percent = Some(50.0);
    let session = ConsoleSession::spawn(console).unwrap();

    let ready = installed_wait(Duration::from_secs(60), || {
        let transcript = session.transcript();
        transcript.contains("Codex") || transcript.contains(INSTALLED_MODEL)
    });
    assert!(
        ready,
        "the real native frontend did not become ready: {}",
        session.transcript()
    );
    thread::sleep(Duration::from_secs(2));

    session.send(INSTALLED_PROMPT).unwrap();
    thread::sleep(Duration::from_millis(250));
    session.send("\r").unwrap();
    let mut requested = installed_wait(Duration::from_secs(20), || {
        installed_provider_thread(&evidence, INSTALLED_PROMPT).is_some()
    });
    let mut retries = 0;
    while !requested && retries < 2 {
        let _ = session.send("\r");
        retries += 1;
        requested = installed_wait(Duration::from_secs(20), || {
            installed_provider_thread(&evidence, INSTALLED_PROMPT).is_some()
        });
    }
    assert!(
        requested,
        "the owned provider recorded no request for the first ordinary turn: {}\n{}",
        session.transcript(),
        installed_logs(&home)
    );

    let thread =
        installed_wait_value(WAIT, || installed_selected_thread(&home)).unwrap_or_else(|| {
            panic!(
                "the launcher selected no registered ordinary thread: {}\n{}",
                session.transcript(),
                installed_logs(&home)
            )
        });
    let lifecycle = installed_wait_value(WAIT, || installed_active_lifecycle(&home, &thread))
        .unwrap_or_else(|| {
            panic!(
                "the automatic reflection lifecycle did not become active for {thread}: {}\n{}",
                session.transcript(),
                installed_logs(&home)
            )
        });
    assert_eq!(lifecycle["binding"]["lead_session"], thread.as_str());
    assert_eq!(lifecycle["lifecycle"], "active");
    assert!(
        installed_wait(Duration::from_secs(60), || {
            installed_marker_visible(&session, INSTALLED_FINAL)
        }),
        "the canned ordinary response was not visible: {}\n{}",
        session.transcript(),
        installed_logs(&home)
    );
    let backend = backend_identity(&home, &session.transcript());

    // Seed the reported retained recovery under the controller's own lock.
    // The embedded board fixture cannot perform the raw/sync writes that caused it.
    let state: harness_core::delivery_reflection::ReflectionState =
        serde_json::from_value(lifecycle.clone()).unwrap();
    let state_file = home
        .join("harness/delivery-reflection")
        .join(format!("{}.json", state.source.stable_key()));
    let lock = installed_wait_value(WAIT, || {
        harness_core::process::ExclusiveFileLock::try_acquire(
            &state_file.with_extension("json.lock"),
        )
        .unwrap()
    })
    .expect("the fixture must acquire the reflection state lock");
    let mut state: harness_core::delivery_reflection::ReflectionState =
        serde_json::from_slice(&fs::read(&state_file).unwrap()).unwrap();
    state.recovery = Some(harness_core::delivery_reflection::RecoveryState {
        kind: harness_core::delivery_reflection::RecoveryKind::ReplicaChanged,
        detail: "synthetic board commit changed without a journal record".to_owned(),
        observed_source: Some(state.source.clone()),
        floor: None,
        head: None,
        records: Vec::new(),
        recorded_at: state.binding.activated_at.clone(),
    });
    let recovering = serde_json::to_value(&state).unwrap();
    let staged = state_file.with_extension("json.tmp");
    fs::write(&staged, serde_json::to_vec_pretty(&state).unwrap()).unwrap();
    fs::rename(staged, &state_file).unwrap();
    drop(lock);
    for (prompt, result) in [
        (format!("/btw {SIDE_PROMPT}"), SIDE_FINAL),
        (RETURN_PROMPT.to_owned(), RETURN_FINAL),
    ] {
        session.send(&prompt).unwrap();
        thread::sleep(Duration::from_millis(250));
        session.send("\r").unwrap();
        assert!(
            installed_wait(WAIT, || installed_marker_visible(&session, result)),
            "the native side/return turn did not complete: {}\n{}",
            session.transcript(),
            installed_logs(&home)
        );
        assert_eq!(
            installed_selected_thread(&home).as_deref(),
            Some(thread.as_str())
        );
        let retained = installed_active_lifecycle(&home, &thread).unwrap();
        assert_eq!(retained["binding"], recovering["binding"]);
        assert_eq!(retained["recovery"], recovering["recovery"]);
        assert_eq!(retained["records"], recovering["records"]);
        if result == SIDE_FINAL {
            // The Windows frontend enables Win32 input mode: Ctrl+/ key-down/up.
            session
                .send("\u{1b}[191;53;47;1;8;1_\u{1b}[191;53;47;0;8;1_")
                .unwrap();
            thread::sleep(Duration::from_millis(300));
        } else {
            assert_eq!(
                installed_provider_thread(&evidence, RETURN_PROMPT).as_deref(),
                Some(thread.as_str()),
                "the return prompt must reach the main conversation"
            );
        }
    }

    let _ = session.send("/quit");
    thread::sleep(Duration::from_millis(300));
    let _ = session.send("\r");
    let finished = wait_session(session);
    let transcript = text(&finished);
    assert_eq!(finished.outcome.exit_code, 0, "{transcript}");
    assert!(
        !home
            .join("harness/lead-endpoints")
            .join(format!("{thread}.json"))
            .exists(),
        "the selected ordinary registration survived its owner"
    );
    installed_assert_cleanup(&home, backend, &thread);
    assert!(
        installed_host_log_contains(&home, &thread),
        "the ordinary host log did not name the selected thread: {}",
        installed_logs(&home)
    );
    let host_status = installed_host_status(&home, &thread);
    let retained = installed_active_lifecycle(&home, &thread).unwrap_or_else(|| {
        panic!(
            "the automatic reflection lifecycle was not retained as active for {thread} after the ordinary host exited: {}\nhost status: {host_status:?}",
            installed_logs(&home)
        )
    });
    let owned = retained["binding"]["lead_session"] == thread.as_str();
    let retired = retained["retired_leads"].as_array().is_some_and(|retired| {
        retired
            .iter()
            .any(|lead| lead.as_str() == Some(thread.as_str()))
    });
    assert!(
        owned || retired,
        "the retained reflection lifecycle no longer identifies {thread}: {retained}\nhost status: {host_status:?}\n{}",
        installed_logs(&home)
    );
    assert_eq!(retained["lifecycle"], "active", "{retained}");
    assert!(
        retained["stopped_at"].is_null(),
        "the retained reflection lifecycle recorded a stop: {retained}"
    );
    let host = host_status.unwrap_or_else(|| {
        panic!(
            "the ordinary host retained no exact stopped status for {thread}: {}",
            installed_logs(&home)
        )
    });
    assert_eq!(host["lead"], thread.as_str(), "{host}");
    assert_eq!(host["state"], "stopped", "{host}");
    assert_eq!(
        host["detail"],
        "the owning lead host ended; retained results and pending work are preserved",
        "{host}"
    );
}

fn installed_input(name: &str) -> PathBuf {
    let path = std::env::var_os(name)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("{name} must name an absolute path"));
    assert!(
        path.is_absolute(),
        "{name} must be absolute: {}",
        path.display()
    );
    assert!(
        path.exists(),
        "{name} must name an existing path: {}",
        path.display()
    );
    path
}

fn installed_bd() -> PathBuf {
    if let Some(value) = std::env::var_os("HARNESS_BD_EXE") {
        let path = PathBuf::from(value);
        assert!(
            path.is_absolute() && path.is_file(),
            "HARNESS_BD_EXE must name an existing absolute bd.exe: {}",
            path.display()
        );
        return path;
    }
    if let Some(home) = std::env::var_os("CODEX_HOME").map(PathBuf::from) {
        let candidate = home.join("harness/bin/bd.exe");
        if candidate.is_file() {
            return candidate;
        }
    }
    if let Some(user) = std::env::var_os("USERPROFILE") {
        let candidate = PathBuf::from(user).join(".codex/harness/bin/bd.exe");
        if candidate.is_file() {
            return candidate;
        }
    }
    if let Some(path) = std::env::var_os("PATH") {
        for directory in std::env::split_paths(&path) {
            let candidate = directory.join("bd.exe");
            if candidate.is_file() {
                return candidate;
            }
        }
    }
    panic!("bd.exe is required at HARNESS_BD_EXE, CODEX_HOME/harness/bin, or PATH");
}

fn installed_child_path(bd: &Path) -> OsString {
    let mut entries = vec![bd.parent().unwrap().to_path_buf()];
    if let Some(path) = std::env::var_os("PATH") {
        entries.extend(std::env::split_paths(&path).filter(|entry| {
            !entry.components().any(|part| {
                part.as_os_str()
                    .to_string_lossy()
                    .eq_ignore_ascii_case("WindowsApps")
            })
        }));
    }
    std::env::join_paths(entries).unwrap()
}

fn installed_write_roles(project: &Path) {
    for (path, body) in [
        (
            "global/executor-self-reflection.md",
            "Fixture executor reflection responsibility",
        ),
        (
            "global/lead-self-reflection.md",
            "Fixture lead reflection responsibility",
        ),
    ] {
        fs::write(project.join(path), body).unwrap();
    }
    fs::write(
        project.join("global/roles.toml"),
        r#"schema = 1
shared_skills = ["shared"]

[roles.lead]
instructions = "global/lead.md"
skills = ["coordinate"]

[roles.executor]
instructions = "global/executor.md"
skills = []

[roles.executor-self-reflection]
instructions = "global/executor-self-reflection.md"
skills = []

[roles.lead-self-reflection]
instructions = "global/lead-self-reflection.md"
skills = []
"#,
    )
    .unwrap();
    fs::write(
        project.join("global/orchestration.toml"),
        "schema = 1\nlead_profile = \"default\"\nsuccessor_lead_profile = \"default\"\nexecutor_profiles = [\"ds\"]\nmax_concurrent_executors = 1\nself_reflection_enabled = true\n",
    )
    .unwrap();
}

fn installed_write_home(home: &Path, project: &Path, provider_port: u16) {
    let trusted = serde_json::to_string(&project.to_string_lossy().to_lowercase()).unwrap();
    fs::write(
        home.join("config.toml"),
        format!(
            r#"
model = "{INSTALLED_MODEL}"
model_provider = "{INSTALLED_PROVIDER}"
model_reasoning_effort = "low"
approval_policy = "never"
sandbox_mode = "danger-full-access"
web_search = "disabled"
cli_auth_credentials_store = "file"
[model_providers.{INSTALLED_PROVIDER}]
name = "Owned ordinary reflection fixture"
base_url = "http://127.0.0.1:{provider_port}/v1"
wire_api = "responses"
env_key = "HARNESS_CONTROL_FIXTURE_KEY"
requires_openai_auth = false
request_max_retries = 0
stream_max_retries = 0
supports_websockets = false
[windows]
sandbox = "unelevated"
[features]
hooks = false
apps = false
multi_agent = false
multi_agent_v2 = false
memories = false
goals = false
plugins = false
skill_search = false
skip_host_skill_discovery = true
[analytics]
enabled = false
[tui]
screen_reader_detection_done = true
[projects.{trusted}]
trust_level = "trusted"
"#
        ),
    )
    .unwrap();
    fs::write(
        home.join("auth.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "auth_mode": "apikey",
            "OPENAI_API_KEY": "harness-installed-ordinary"
        }))
        .unwrap(),
    )
    .unwrap();
}

fn installed_git(project: &Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(project)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn installed_wait(timeout: Duration, mut predicate: impl FnMut() -> bool) -> bool {
    let until = Instant::now() + timeout;
    loop {
        if predicate() {
            return true;
        }
        if Instant::now() >= until {
            return false;
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn installed_wait_value<T>(timeout: Duration, mut probe: impl FnMut() -> Option<T>) -> Option<T> {
    let until = Instant::now() + timeout;
    loop {
        if let Some(value) = probe() {
            return Some(value);
        }
        if Instant::now() >= until {
            return None;
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn installed_selected_thread(home: &Path) -> Option<String> {
    let entries = fs::read_dir(home.join("harness/lead-endpoints")).ok()?;
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.extension().is_none_or(|extension| extension != "json") {
            continue;
        }
        let Ok(bytes) = fs::read(&path) else {
            continue;
        };
        let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
            continue;
        };
        if let Some(thread) = value["threadId"].as_str() {
            return Some(thread.to_owned());
        }
    }
    None
}

fn installed_active_lifecycle(home: &Path, thread: &str) -> Option<Value> {
    let entries = fs::read_dir(home.join("harness/delivery-reflection")).ok()?;
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.extension().is_none_or(|extension| extension != "json") {
            continue;
        }
        let Ok(bytes) = fs::read(&path) else {
            continue;
        };
        let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
            continue;
        };
        let owned = value["binding"]["lead_session"] == thread;
        let retired = value["retired_leads"]
            .as_array()
            .is_some_and(|retired| retired.iter().any(|lead| lead.as_str() == Some(thread)));
        if value["lifecycle"] == "active" && (owned || retired) {
            return Some(value);
        }
    }
    None
}

fn installed_host_status(home: &Path, thread: &str) -> Option<Value> {
    let entries = fs::read_dir(home.join("harness/delivery-reflection/hosts")).ok()?;
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.extension().is_none_or(|extension| extension != "json") {
            continue;
        }
        let Ok(bytes) = fs::read(&path) else {
            continue;
        };
        let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
            continue;
        };
        if value["lead"] == thread {
            return Some(value);
        }
    }
    None
}

fn installed_provider_thread(evidence: &Path, prompt: &str) -> Option<String> {
    let entries = fs::read_dir(evidence).ok()?;
    for entry in entries.filter_map(Result::ok) {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with("provider-") || !name.ends_with(".json") {
            continue;
        }
        let Ok(bytes) = fs::read(entry.path()) else {
            continue;
        };
        if String::from_utf8_lossy(&bytes).contains(prompt)
            && let Ok(request) = serde_json::from_slice::<Value>(&bytes)
            && let Some(thread) = request["client_metadata"]["thread_id"].as_str()
        {
            return Some(thread.to_owned());
        }
    }
    None
}

fn installed_marker_visible(session: &ConsoleSession, marker: &str) -> bool {
    session.transcript().contains(marker)
}

fn installed_host_log_contains(home: &Path, needle: &str) -> bool {
    let Ok(entries) = fs::read_dir(home.join("harness/lead-endpoints/host")) else {
        return false;
    };
    entries.filter_map(Result::ok).any(|entry| {
        let path = entry.path();
        path.extension().is_some_and(|extension| extension == "log")
            && fs::read_to_string(path).is_ok_and(|text| text.contains(needle))
    })
}

fn installed_assert_cleanup(home: &Path, backend: ProcessIdentity, thread: &str) {
    assert!(
        !home
            .join("harness/lead-endpoints")
            .join(format!("{thread}.json"))
            .exists(),
        "the selected thread registry survived its owner"
    );
    let host = home.join("harness/lead-endpoints/host");
    if let Ok(entries) = fs::read_dir(&host) {
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            assert!(
                path.extension().is_some_and(|extension| extension == "log"),
                "ordinary host transient state survived cleanup: {}",
                path.display()
            );
        }
    }
    assert_process_gone(backend);
}

fn installed_logs(home: &Path) -> String {
    let mut output = String::new();
    for directory in [
        home.join("harness/delivery-reflection"),
        home.join("harness/lead-endpoints/host"),
    ] {
        let Ok(entries) = fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.is_dir() {
                continue;
            }
            output.push_str(&format!("{}:\n", path.display()));
            output.push_str(&fs::read_to_string(&path).unwrap_or_default());
            output.push('\n');
        }
    }
    output
}

const RECOVERY_WAIT: Duration = Duration::from_secs(25);
const UNCERTAIN_WAIT: Duration = Duration::from_secs(40);
const RECOVERY_SETTLE: Duration = Duration::from_secs(5);

fn recovery_native() -> [&'static str; 9] {
    [
        "--no-alt-screen",
        "-m",
        MODEL,
        "-c",
        "model_provider=\"fixture-provider\"",
        "-c",
        "model_reasoning_effort=\"max\"",
        "--",
        PROMPT,
    ]
}

fn recovery_record(fixture: &Fixture) -> PathBuf {
    fixture.home.join("fixture/recovery.jsonl")
}

fn recovery_session(
    fixture: &Fixture,
    drop_ack: bool,
    pause_gate: bool,
    observation_failure: bool,
) -> ConsoleSession {
    let native = recovery_native();
    let mut spec = fixture.command("new", NEW_THREAD, None, &native);
    spec.env.insert(
        "HARNESS_IMPROVEMENT_FIXTURE_CONTROL".into(),
        Some("recovery".into()),
    );
    spec.env.insert(
        "HARNESS_IMPROVEMENT_FIXTURE_RECOVERY_RECORD".into(),
        Some(recovery_record(fixture).as_os_str().to_owned()),
    );
    if drop_ack {
        spec.env.insert(
            "HARNESS_IMPROVEMENT_FIXTURE_RECOVERY_ACK".into(),
            Some("drop-first".into()),
        );
    }
    if observation_failure {
        spec.env.insert(
            "HARNESS_IMPROVEMENT_FIXTURE_RECOVERY_OBSERVE".into(),
            Some("fail".into()),
        );
    }
    if pause_gate {
        spec.env.insert(
            "HARNESS_FRONTEND_GOAL_PAUSE_ON_FILE".into(),
            Some(
                fixture
                    .home
                    .join("frontend-goal-pause-gate")
                    .as_os_str()
                    .to_owned(),
            ),
        );
    }
    spawn(spec)
}

fn wait_transcript(session: &ConsoleSession, needle: &str, what: &str, timeout: Duration) {
    let until = Instant::now() + timeout;
    while Instant::now() < until {
        if session.transcript().contains(needle) {
            return;
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!(
        "{what} was not shown within {timeout:?}; expected {needle:?}:\n{}",
        session.transcript()
    );
}

fn recovery_entries(fixture: &Fixture) -> Vec<Value> {
    fs::read_to_string(recovery_record(fixture))
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

fn wait_recovery(
    session: &ConsoleSession,
    fixture: &Fixture,
    what: &str,
    timeout: Duration,
    predicate: impl Fn(&[Value]) -> bool,
) -> Vec<Value> {
    let until = Instant::now() + timeout;
    while Instant::now() < until {
        let entries = recovery_entries(fixture);
        if predicate(&entries) {
            return entries;
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!(
        "{what} was not observed within {timeout:?}; fixture record:\n{}\ntranscript:\n{}",
        serde_json::to_string_pretty(&recovery_entries(fixture)).unwrap_or_default(),
        session.transcript()
    );
}

fn recovery_requests(entries: &[Value], method: &str) -> Vec<Value> {
    entries
        .iter()
        .filter(|entry| entry["kind"] == "request" && entry["method"] == method)
        .cloned()
        .collect()
}

fn automatic_goal_sets(entries: &[Value]) -> Vec<Value> {
    recovery_requests(entries, "thread/goal/set")
        .into_iter()
        .filter(|entry| entry["params"]["origin"] == "automatic")
        .collect()
}

fn recovery_goal(entries: &[Value], status: &str) -> Option<Value> {
    entries
        .iter()
        .filter_map(|entry| {
            if entry["kind"] == "goal" {
                Some(entry["goal"].clone())
            } else if entry["kind"] == "notification"
                && entry["notification"]["method"] == "thread/goal/updated"
            {
                Some(entry["notification"]["params"]["goal"].clone())
            } else {
                None
            }
        })
        .filter(|goal| goal["status"] == status)
        .next_back()
}

fn recovery_checkpoint(fixture: &Fixture) -> Option<Value> {
    let host = fixture.home.join("harness/lead-endpoints/host");
    let path = fs::read_dir(host)
        .ok()?
        .filter_map(Result::ok)
        .find_map(|entry| {
            let path = entry.path();
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().ends_with(".recovery.json"))
                .then_some(path)
        })?;
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}

fn wait_checkpoint(
    session: &ConsoleSession,
    fixture: &Fixture,
    what: &str,
    predicate: impl Fn(&Value) -> bool,
) -> Value {
    let until = Instant::now() + RECOVERY_WAIT;
    while Instant::now() < until {
        if let Some(checkpoint) = recovery_checkpoint(fixture)
            && predicate(&checkpoint)
        {
            return checkpoint;
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!(
        "{what} was not recorded; checkpoint {:?}; transcript:\n{}",
        recovery_checkpoint(fixture),
        session.transcript()
    );
}

fn release_and_assert_cleanup(
    fixture: &Fixture,
    session: ConsoleSession,
    backend: ProcessIdentity,
) {
    fixture.release();
    let finished = wait_session(session);
    let transcript = text(&finished);
    assert_eq!(finished.outcome.exit_code, 0, "{transcript}");
    assert_cleanup(fixture, backend, NEW_THREAD);
}

#[test]
fn ordinary_host_recovers_blocked_goal_after_overload() {
    let fixture = Fixture::new();
    let session = recovery_session(&fixture, false, false, false);
    wait_for(
        &fixture.selected(),
        "the frontend-selected thread",
        &session,
    );
    wait_for(
        &fixture.readiness(),
        "the pre-turn registry check",
        &session,
    );
    let entries = wait_recovery(
        &session,
        &fixture,
        "the first failed turn and blocked goal",
        RECOVERY_WAIT,
        |entries| {
            entries.iter().any(|entry| {
                entry["kind"] == "notification"
                    && entry["notification"]["method"] == "turn/completed"
                    && entry["notification"]["params"]["turn"]["status"] == "failed"
            }) && recovery_goal(entries, "blocked").is_some()
        },
    );
    wait_transcript(
        &session,
        "Automatic recovery attempt 1/5 is scheduled",
        "the scheduled-attempt warning",
        RECOVERY_WAIT,
    );
    assert!(
        automatic_goal_sets(&entries).is_empty(),
        "the automatic set bypassed the scheduled delay"
    );
    let backend = backend_identity(&fixture.home, &session.transcript());
    assert_eq!(
        recovery_requests(&entries, "turn/start").len(),
        1,
        "the frontend's submitted turn was resent"
    );

    let entries = wait_recovery(
        &session,
        &fixture,
        "the accepted recovery and completed goal",
        RECOVERY_WAIT,
        |entries| {
            recovery_goal(entries, "complete").is_some()
                && entries
                    .iter()
                    .any(|entry| entry["kind"] == "automatic-set" && entry["acknowledged"] == true)
        },
    );
    wait_transcript(
        &session,
        "Automatic recovery attempt 1/5 was accepted",
        "the accepted-attempt warning",
        RECOVERY_WAIT,
    );
    wait_transcript(
        &session,
        "Goal completed; automatic recovery is no longer needed.",
        "the successful-completion notice",
        RECOVERY_WAIT,
    );

    let automatic = automatic_goal_sets(&entries);
    assert_eq!(automatic.len(), 1, "exactly one activation is allowed");
    let params = &automatic[0]["params"];
    assert_eq!(params["threadId"], NEW_THREAD);
    assert_eq!(params["status"], "active");
    assert_eq!(params["origin"], "automatic");
    for field in ["objective", "tokenBudget", "model", "provider"] {
        assert!(
            params.get(field).is_none(),
            "automatic activation carried {field}: {params}"
        );
    }
    assert_eq!(
        recovery_requests(&entries, "turn/start").len(),
        1,
        "recovery resent the prompt or turn"
    );

    let blocked = recovery_goal(&entries, "blocked").expect("blocked goal");
    let active = recovery_goal(&entries, "active").expect("active goal");
    let complete = recovery_goal(&entries, "complete").expect("complete goal");
    for goal in [&blocked, &active, &complete] {
        assert_eq!(goal["threadId"], NEW_THREAD);
        assert_eq!(goal["createdAt"], blocked["createdAt"]);
        assert_eq!(goal["objective"], blocked["objective"]);
        assert_eq!(goal["tokenBudget"], blocked["tokenBudget"]);
    }
    assert_eq!(blocked["tokensUsed"], 20);
    assert_eq!(complete["tokensUsed"], 40);

    let recovered = entries
        .iter()
        .filter(|entry| {
            entry["kind"] == "notification"
                && entry["notification"]["method"] == "turn/completed"
                && entry["notification"]["params"]["turn"]["id"] == "fixture-turn-recovered"
                && entry["notification"]["params"]["turn"]["status"] == "completed"
        })
        .count();
    assert_eq!(
        recovered, 1,
        "the continuation did not complete exactly once"
    );
    let active_position = entries
        .iter()
        .position(|entry| {
            entry["kind"] == "notification"
                && entry["notification"]["method"] == "thread/goal/updated"
                && entry["notification"]["params"]["goal"]["status"] == "active"
        })
        .expect("the native active update");
    let completed_position = entries
        .iter()
        .position(|entry| {
            entry["kind"] == "notification"
                && entry["notification"]["method"] == "turn/completed"
                && entry["notification"]["params"]["turn"]["id"] == "fixture-turn-recovered"
                && entry["notification"]["params"]["turn"]["status"] == "completed"
        })
        .expect("the recovered turn completion");
    assert!(
        active_position < completed_position,
        "the active update did not precede the recovered turn"
    );

    let checkpoint = wait_checkpoint(&session, &fixture, "the completed recovery", |value| {
        value["status"] == "complete"
    });
    assert_eq!(checkpoint["threadId"], NEW_THREAD);

    let transcript = session.transcript();
    assert!(
        !transcript.contains("Automatic recovery cancelled"),
        "the successful goal showed a generic cancellation:\n{transcript}"
    );
    release_and_assert_cleanup(&fixture, session, backend);
}

#[test]
fn ordinary_host_user_pause_wins_before_dispatch() {
    let fixture = Fixture::new();
    let session = recovery_session(&fixture, false, true, false);
    wait_for(
        &fixture.selected(),
        "the frontend-selected thread",
        &session,
    );
    wait_transcript(
        &session,
        "Automatic recovery attempt 1/5 is scheduled",
        "the scheduled-attempt warning",
        RECOVERY_WAIT,
    );
    let backend = backend_identity(&fixture.home, &session.transcript());

    fs::write(fixture.home.join("frontend-goal-pause-gate"), b"pause").unwrap();
    wait_transcript(
        &session,
        "Automatic recovery cancelled (a user action)",
        "the cancellation notice",
        RECOVERY_WAIT,
    );
    let entries = wait_recovery(
        &session,
        &fixture,
        "the user's paused goal",
        RECOVERY_WAIT,
        |entries| {
            entries.iter().any(|entry| {
                entry["kind"] == "request"
                    && entry["method"] == "thread/goal/set"
                    && entry["params"]["origin"] == "user"
                    && entry["params"]["status"] == "paused"
            }) && recovery_goal(entries, "paused").is_some()
        },
    );
    assert!(
        automatic_goal_sets(&entries).is_empty(),
        "the host dispatched after the user pause"
    );

    thread::sleep(RECOVERY_SETTLE);
    let later = recovery_entries(&fixture);
    assert!(
        automatic_goal_sets(&later).is_empty(),
        "a later read resumed the paused goal"
    );
    assert_eq!(
        recovery_requests(&later, "turn/start").len(),
        1,
        "the pause caused a prompt or turn resend"
    );
    assert_eq!(
        recovery_goal(&later, "paused").expect("paused goal")["status"],
        "paused"
    );
    assert!(
        recovery_goal(&later, "active").is_none(),
        "the paused goal was activated after cancellation"
    );
    release_and_assert_cleanup(&fixture, session, backend);
}

#[test]
fn ordinary_host_closes_without_reopening_a_blocked_goal() {
    let fixture = Fixture::new();
    let session = recovery_session(&fixture, false, false, false);
    wait_for(
        &fixture.selected(),
        "the frontend-selected thread",
        &session,
    );
    wait_transcript(
        &session,
        "Automatic recovery attempt 1/5 is scheduled",
        "the scheduled-attempt warning",
        RECOVERY_WAIT,
    );
    let backend = backend_identity(&fixture.home, &session.transcript());
    fixture.release();
    let finished = wait_session(session);
    let transcript = text(&finished);
    assert_eq!(finished.outcome.exit_code, 0, "{transcript}");
    assert!(
        automatic_goal_sets(&recovery_entries(&fixture)).is_empty(),
        "a delayed activation arrived before the host closed"
    );

    thread::sleep(RECOVERY_SETTLE);
    let later = recovery_entries(&fixture);
    assert!(
        automatic_goal_sets(&later).is_empty(),
        "the closed host left a delayed activation"
    );
    assert_eq!(
        recovery_requests(&later, "turn/start").len(),
        1,
        "closing the frontend resent the prompt or turn"
    );
    assert_cleanup(&fixture, backend, NEW_THREAD);
}

#[test]
fn ordinary_host_reconciles_an_unconfirmed_ack_without_a_duplicate() {
    let fixture = Fixture::new();
    let session = recovery_session(&fixture, true, false, false);
    wait_for(
        &fixture.selected(),
        "the frontend-selected thread",
        &session,
    );
    wait_transcript(
        &session,
        "Automatic recovery attempt 1/5 is scheduled",
        "the scheduled-attempt warning",
        RECOVERY_WAIT,
    );
    let entries = wait_recovery(
        &session,
        &fixture,
        "the first unacknowledged activation",
        RECOVERY_WAIT,
        |entries| {
            entries
                .iter()
                .any(|entry| entry["kind"] == "automatic-set" && entry["acknowledged"] == false)
        },
    );
    let backend = backend_identity(&fixture.home, &session.transcript());
    assert_eq!(automatic_goal_sets(&entries).len(), 1);

    wait_transcript(
        &session,
        "Automatic recovery stopped after an unconfirmed acknowledgement; the goal is still blocked. Use /goal resume to continue.",
        "the unconfirmed-acknowledgement notice",
        UNCERTAIN_WAIT,
    );
    let entries = wait_recovery(
        &session,
        &fixture,
        "the reconciled blocked goal",
        RECOVERY_WAIT,
        |entries| {
            let set = entries
                .iter()
                .position(|entry| entry["kind"] == "automatic-set");
            let read = entries.iter().rposition(|entry| {
                entry["kind"] == "goal"
                    && entry["origin"] == "read"
                    && entry["goal"]["status"] == "blocked"
            });
            set.zip(read).is_some_and(|(set, read)| read > set)
        },
    );
    assert_eq!(
        automatic_goal_sets(&entries).len(),
        1,
        "an unconfirmed activation was sent twice"
    );
    assert_eq!(
        recovery_goal(&entries, "blocked").expect("blocked goal")["status"],
        "blocked"
    );
    assert!(
        recovery_goal(&entries, "complete").is_none(),
        "the unanswered activation was reported as recovered"
    );
    let checkpoint = wait_checkpoint(&session, &fixture, "the unconfirmed outcome", |value| {
        value["status"] == "unconfirmed"
    });
    assert_eq!(checkpoint["attempt"], 1, "{checkpoint}");
    assert_eq!(checkpoint["dispatch"]["attempt"], 1, "{checkpoint}");
    release_and_assert_cleanup(&fixture, session, backend);
}

#[test]
fn ordinary_host_reports_goal_observation_failure_without_relaunching() {
    let fixture = Fixture::new();
    let session = recovery_session(&fixture, false, false, true);
    wait_for(
        &fixture.selected(),
        "the frontend-selected thread",
        &session,
    );
    wait_transcript(
        &session,
        "Automatic recovery stopped because the native goal could not be observed",
        "the observation-failure notice",
        RECOVERY_WAIT,
    );
    wait_transcript(
        &session,
        "use /goal resume to continue manually",
        "the manual recovery action",
        RECOVERY_WAIT,
    );
    let entries = wait_recovery(
        &session,
        &fixture,
        "the failed native goal observation",
        RECOVERY_WAIT,
        |entries| {
            entries
                .iter()
                .any(|entry| entry["kind"] == "observation-failure")
        },
    );
    let backend = backend_identity(&fixture.home, &session.transcript());
    assert!(
        process_alive(backend),
        "the owned app-server was replaced after the observation fault"
    );
    assert!(
        automatic_goal_sets(&entries).is_empty(),
        "the observation fault still dispatched an automatic activation"
    );
    assert_eq!(
        recovery_requests(&entries, "turn/start").len(),
        1,
        "the observation fault resent the prompt or turn"
    );
    let backend_launches = entries
        .iter()
        .filter(|entry| entry["kind"] == "fixture-start")
        .collect::<Vec<_>>();
    assert_eq!(
        backend_launches.len(),
        1,
        "the host launched a second app-server"
    );
    assert_eq!(
        backend_launches[0]["pid"], backend.pid,
        "the surviving session is not the original app-server"
    );
    assert_eq!(
        entries
            .iter()
            .filter(|entry| entry["kind"] == "frontend-start")
            .count(),
        1,
        "the host launched a second frontend"
    );
    let checkpoint = wait_checkpoint(&session, &fixture, "the observation failure", |value| {
        value["status"] == "observationFailed"
    });
    assert_eq!(checkpoint["threadId"], NEW_THREAD);
    assert!(
        recovery_goal(&entries, "complete").is_none(),
        "the observation fault was reported as recovered"
    );

    fixture.release();
    let finished = wait_session(session);
    let transcript = text(&finished);
    assert_eq!(finished.outcome.exit_code, 0, "{transcript}");
    thread::sleep(RECOVERY_SETTLE);
    let later = recovery_entries(&fixture);
    assert!(
        automatic_goal_sets(&later).is_empty(),
        "a delayed activation survived the observation fault"
    );
    assert_eq!(
        later
            .iter()
            .filter(|entry| entry["kind"] == "fixture-start")
            .count(),
        1,
        "cleanup launched another app-server"
    );
    assert_eq!(
        later
            .iter()
            .filter(|entry| entry["kind"] == "frontend-start")
            .count(),
        1,
        "cleanup launched another frontend"
    );
    assert_cleanup(&fixture, backend, NEW_THREAD);
}
