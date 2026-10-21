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
        let mut spec = CommandSpec::new(MANAGER);
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

/// Reads the one retained lifecycle state the real board source produced.
fn reflection_state(fixture: &Fixture) -> Option<Value> {
    let dir = fixture.home.join("harness/delivery-reflection");
    for entry in fs::read_dir(dir).ok()? {
        let path = entry.ok()?.path();
        if path
            .extension()
            .is_some_and(|extension| extension == "json")
        {
            return serde_json::from_slice(&fs::read(path).ok()?).ok();
        }
    }
    None
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
    let native = ["--no-alt-screen", "resume", RESUME_THREAD];
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
        native,
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
