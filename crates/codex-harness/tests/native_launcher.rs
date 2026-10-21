//! Actual Rust launcher entry point on owned installation targets.
#![cfg(windows)]
use harness_core::{
    build_identity::{self, BINARIES, BuildRecord, INSPECTION_SCHEMA, SCHEMA},
    build_selection,
    console::{ConsoleSession, ConsoleSpec},
    process::{
        Cancellation, CommandSpec, Deadline, SHARED_CPU_PERCENT, SharedCpuBudget, StopReason,
    },
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    env,
    ffi::OsString,
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::OnceLock,
    time::{Duration, Instant},
};

/// Account override used by every fixture launch: an ordinary launch must not
/// touch the developer's real account state from a test, and the lowered test
/// ceiling stays scoped to this account directory.
const CPU_ACCOUNT_ENV: &str = "CODEX_HARNESS_CPU_ACCOUNT";
/// Machine-local ceiling override read by the launcher before admission.
const CPU_PERCENT_ENV: &str = "CODEX_HARNESS_CPU_PERCENT";
/// A relative account location is refused by the launcher, so a misconfigured
/// machine-local value must not silently create a second allowance.
const RELATIVE_ACCOUNT: &str = "relative-cpu-account";
/// Lowered rate for measurement: the spinning demand stays many times the
/// allowance, so a working cap is unambiguous and the uncapped control of the
/// surrounding developer session cannot explain the observation.
const TEST_RATE: f64 = 0.5;
/// Leading text of every fail-open notice. It is asserted on the diagnostic
/// channel and never on stdout.
const CAP_WARNING: &str = "codex-harness: shared agent CPU cap not verified";

struct Fixture {
    root: tempfile::TempDir,
    home: PathBuf,
    source: PathBuf,
    state: PathBuf,
    launcher: PathBuf,
    upstream: PathBuf,
    account: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::Builder::new()
            .prefix("native-launch проверка-")
            .tempdir()
            .unwrap();
        let home = root.path().join("home");
        let source = root.path().join("source");
        let state = root.path().join("state");
        let build = state.join("builds/fixture");
        for path in [
            home.join("harness"),
            source.join("global"),
            source.join("crates/one/src"),
            source.join("crates/harness-rtk/src"),
            source.join(INSPECTION_SCHEMA).parent().unwrap().to_owned(),
            build.clone(),
        ] {
            fs::create_dir_all(path).unwrap();
        }
        for name in [
            "Cargo.toml",
            "Cargo.lock",
            "crates/one/src/lib.rs",
            INSPECTION_SCHEMA,
        ] {
            fs::write(source.join(name), "fixture").unwrap();
        }
        fs::write(
            source.join("global/harness.config.toml"),
            "approval_policy = 'never'\n",
        )
        .unwrap();
        // Existing cases exercise the native and legacy paths; the ordinary
        // reflection cases below opt in by rewriting this file. Omitting the
        // key still defaults on, which its own case exercises.
        fs::write(
            source.join("global/orchestration.toml"),
            reflection_config(Some(false)),
        )
        .unwrap();
        fs::write(source.join("global/kit.json"), serde_json::to_vec(&json!({"schema":1,"profile_name":"harness","profile":"global/harness.config.toml","instructions":"AGENTS.md","skills":"skills","agents":"agents","hooks":"hooks.json","token_hooks":"rtk-hooks.json"})).unwrap()).unwrap();
        fs::write(state.join("owner"), "codex-harness-native-state-v1\n").unwrap();
        let launcher = build.join("codex.exe");
        let upstream = root.path().join("upstream.exe");
        fs::copy(env!("CARGO_BIN_EXE_codex"), &launcher).unwrap();
        fs::copy(env!("CARGO_BIN_EXE_harness-launch-fixture"), &upstream).unwrap();
        let mut binaries = BTreeMap::new();
        for name in BINARIES {
            let path = build.join(name);
            if *name != "codex.exe" {
                fs::write(&path, name).unwrap();
            }
            binaries.insert(name.to_string(), build_identity::hash_file(&path).unwrap());
        }
        let record = BuildRecord {
            schema: SCHEMA,
            source_root: source.clone(),
            source: build_identity::source_identity(&source).unwrap(),
            rustc: "fixture".into(),
            cargo: "fixture".into(),
            target: "x86_64-pc-windows-msvc".into(),
            profile: "release".into(),
            binaries,
        };
        fs::write(
            build.join("build.json"),
            serde_json::to_vec(&record).unwrap(),
        )
        .unwrap();
        build_selection::activate(&state, &build).unwrap();
        let account = root.path().join("cpu-account");
        let f = Self {
            root,
            home,
            source,
            state,
            launcher,
            upstream,
            account,
        };
        f.register(json!({"executable":f.upstream, "sha256":build_identity::hash_file(&f.upstream).unwrap(), "package":null}));
        // Every session launch composes a responsibility role from the shared
        // kit, so the fixture carries role sources as part of its base state:
        // each case reaches its own assertions instead of failing on missing
        // role inputs. Source freshness ignores global sources and skills.
        write_role_sources(&f);
        f
    }
    fn register(&self, upstream: Value) {
        fs::write(
            self.home.join("harness/native-launch.json"),
            serde_json::to_vec(&json!({"schema":1,"state":self.state,"upstream":upstream}))
                .unwrap(),
        )
        .unwrap();
    }
    fn register_immutable(&self) {
        fs::write(self.home.join("harness/native-launch.json"),serde_json::to_vec(&json!({
            "schema":2,"build":self.launcher.parent().unwrap(),
            "upstream":{"executable":self.upstream,"sha256":build_identity::hash_file(&self.upstream).unwrap(),"package":null}
        })).unwrap()).unwrap();
    }
    fn command(&self) -> Command {
        let mut c = Command::new(&self.launcher);
        c.current_dir(self.root.path())
            .env("CODEX_HOME", &self.home)
            .env(CPU_ACCOUNT_ENV, &self.account)
            .env_remove(CPU_PERCENT_ENV)
            .env_remove("HARNESS_EXECUTOR_SESSION")
            .env_remove("HARNESS_REFLECTION_SESSION")
            .env_remove("HARNESS_BOUNDED_LEAD_SESSION")
            .env_remove("HARNESS_LAUNCH_FIXTURE_MODE");
        c
    }
    /// Register the synthetic CPU consumer as the installed upstream payload.
    fn register_consumer(&self, consumer: &Path) {
        self.register(json!({
            "executable": consumer,
            "sha256": build_identity::hash_file(consumer).unwrap(),
            "package": null
        }));
    }
    fn set_reflection(&self, enabled: Option<bool>) {
        self.write_reflection(&reflection_config(enabled));
    }
    fn write_reflection(&self, body: &str) {
        fs::write(self.source.join("global/orchestration.toml"), body).unwrap();
    }
    /// Replace the placeholder manager with the owned native capture fixture,
    /// then re-record its digest so the immutable launch admission accepts it.
    fn install_manager(&self) {
        let build = self.launcher.parent().unwrap();
        let manager = build.join("codex-harness.exe");
        fs::copy(env!("CARGO_BIN_EXE_harness-launch-fixture"), &manager).unwrap();
        let mut record = build_identity::read_record(build).unwrap();
        record.binaries.insert(
            "codex-harness.exe".into(),
            build_identity::hash_file(&manager).unwrap(),
        );
        fs::write(
            build.join("build.json"),
            serde_json::to_vec(&record).unwrap(),
        )
        .unwrap();
        build_selection::activate(&self.state, build).unwrap();
    }
    fn console(&self, mode: &str) -> ConsoleSession {
        let mut c = CommandSpec::new(&self.launcher);
        c.current_dir = Some(self.root.path().to_owned());
        c.env.insert(
            "CODEX_HOME".into(),
            Some(self.home.clone().into_os_string()),
        );
        c.env.insert(
            CPU_ACCOUNT_ENV.into(),
            Some(self.account.clone().into_os_string()),
        );
        // Absent means "inherit the caller's ceiling override", which a test
        // must not pick up from the developer's environment.
        c.env.insert(CPU_PERCENT_ENV.into(), None);
        c.env.insert("HARNESS_EXECUTOR_SESSION".into(), None);
        c.env.insert("HARNESS_REFLECTION_SESSION".into(), None);
        c.env.insert("HARNESS_BOUNDED_LEAD_SESSION".into(), None);
        c.env
            .insert("HARNESS_LAUNCH_FIXTURE_MODE".into(), Some(mode.into()));
        ConsoleSession::spawn(ConsoleSpec::new(c)).unwrap()
    }
}

fn reflection_config(enabled: Option<bool>) -> String {
    let mut text = concat!(
        "schema = 1\n",
        "lead_profile = \"default\"\n",
        "successor_lead_profile = \"default\"\n",
        "executor_profiles = [\"default\"]\n",
        "max_concurrent_executors = 1\n",
    )
    .to_owned();
    if let Some(enabled) = enabled {
        text.push_str(&format!("self_reflection_enabled = {enabled}\n"));
    }
    text
}

/// Run one ordinary launcher invocation through its actual entry point and
/// return the captured fixture report. The fixture exits 19 after reporting,
/// which also makes the launcher's exit forwarding observable.
fn launch_report(f: &Fixture, args: &[OsString]) -> (i32, Value) {
    launch_report_with_env(f, args, &[])
}

fn launch_report_with_env(
    f: &Fixture,
    args: &[OsString],
    environment: &[(&str, &str)],
) -> (i32, Value) {
    let mut command = f.command();
    command
        .args(args)
        .env("HARNESS_LAUNCH_FIXTURE_MODE", "nonzero")
        .env("CODEX_MANAGED_BY_BUN", "1");
    for (name, value) in environment {
        command.env(name, value);
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"reflection input line\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    let report = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "launcher produced no fixture report: {error}; status {:?}; stderr {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (output.status.code().unwrap_or(-1), report)
}

fn ordinary_reflection_args(f: &Fixture) -> (Vec<OsString>, PathBuf) {
    let sub = f.root.path().join("sub");
    let added = f.root.path().join("added");
    fs::create_dir_all(&sub).unwrap();
    fs::create_dir_all(&added).unwrap();
    (
        vec![
            OsString::from("--profile"),
            OsString::from("work"),
            OsString::from("-m"),
            OsString::from("gpt-6-astra"),
            OsString::from("-C"),
            OsString::from("sub"),
            OsString::from("--add-dir"),
            added.clone().into_os_string(),
            OsString::from("hello"),
        ],
        added,
    )
}

#[test]
fn ordinary_reflection_forwards_prepared_argv_env_cwd_and_exit_to_manager() {
    let f = Fixture::new();
    let (args, added) = ordinary_reflection_args(&f);

    f.set_reflection(Some(false));
    let (native_code, native) = launch_report(&f, &args);
    assert_eq!(native_code, 19);
    let native_args = native["args"].as_array().unwrap();
    for expected in [
        "--profile",
        "work",
        "-m",
        "gpt-6-astra",
        "-C",
        "sub",
        "--add-dir",
    ] {
        assert!(
            native_args
                .iter()
                .filter_map(Value::as_str)
                .any(|arg| arg == expected),
            "the prepared native argv lost {expected}: {native_args:?}"
        );
    }
    let added = added.canonicalize().unwrap();
    let roots: Value = serde_json::from_str(
        native["environment"]["HARNESS_LSP_WORKSPACE_ROOTS"]
            .as_str()
            .expect("the prepared environment must carry the additional roots"),
    )
    .unwrap();
    assert!(
        roots
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .any(|path| Path::new(path)
                .canonicalize()
                .is_ok_and(|path| path == added)),
        "the prepared environment lost the explicit --add-dir root: {roots:?}"
    );

    f.set_reflection(Some(true));
    f.install_manager();
    let (manager_code, manager) = launch_report(&f, &args);
    assert_eq!(manager_code, 19);
    let manager_args = manager["args"].as_array().unwrap();
    assert_eq!(manager_args[0], "_ordinary-lead");
    assert_eq!(manager_args[1], "--codex-home");
    assert_eq!(
        Path::new(manager_args[2].as_str().unwrap())
            .canonicalize()
            .unwrap(),
        f.home.canonicalize().unwrap()
    );
    assert_eq!(manager_args[3], "--");
    assert_eq!(
        &manager_args[4..],
        native_args.as_slice(),
        "the manager must receive the prepared native argv unchanged"
    );
    assert_eq!(manager["cwd"], native["cwd"]);
    assert_eq!(manager["stdin"], native["stdin"]);
    assert_eq!(
        manager["environment"], native["environment"],
        "the manager must receive the prepared environment unchanged"
    );
}

#[test]
fn ordinary_reflection_false_keeps_the_native_launcher_path() {
    let f = Fixture::new();
    f.install_manager();
    f.set_reflection(Some(false));
    let (code, report) = launch_report(&f, &[OsString::from("hello")]);
    assert_eq!(code, 19);
    assert_ne!(report["args"][0], "_ordinary-lead");
}

#[test]
fn ordinary_reflection_preserves_resume_and_fork_selection() {
    let f = Fixture::new();
    f.install_manager();
    f.set_reflection(Some(true));
    let cases = [
        ("resume", vec!["resume", "--last", "continue"]),
        ("fork", vec!["fork", "9b2f", "continue"]),
    ];
    for (name, selection) in cases {
        let args = selection
            .iter()
            .copied()
            .map(OsString::from)
            .collect::<Vec<_>>();
        let (code, report) = launch_report(&f, &args);
        assert_eq!(code, 19, "{name}");
        let manager_args = report["args"].as_array().unwrap();
        assert_eq!(manager_args[0], "_ordinary-lead", "{name}");
        let native = manager_args[4..]
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        let start = native
            .iter()
            .position(|arg| *arg == name)
            .unwrap_or_else(|| panic!("{name} selection is missing: {native:?}"));
        assert_eq!(
            &native[start..],
            selection.as_slice(),
            "the manager must receive the native {name} selection unchanged"
        );
    }
}

#[test]
fn ordinary_reflection_defaults_on_when_the_permission_is_omitted_or_missing() {
    let f = Fixture::new();
    f.install_manager();
    f.set_reflection(None);
    let (code, report) = launch_report(&f, &[OsString::from("hello")]);
    assert_eq!(code, 19);
    assert_eq!(report["args"][0], "_ordinary-lead");

    fs::remove_file(f.source.join("global/orchestration.toml")).unwrap();
    let (code, report) = launch_report(&f, &[OsString::from("hello")]);
    assert_eq!(code, 19);
    assert_eq!(report["args"][0], "_ordinary-lead");
}

#[test]
fn ordinary_reflection_rejects_invalid_configuration_before_starting_a_payload() {
    let f = Fixture::new();
    f.install_manager();
    let mut body = reflection_config(None);
    body.push_str("self_reflection_enabled = 1\n");
    f.write_reflection(&body);
    let marker = f.root.path().join("invalid-configuration-started");
    let output = f
        .command()
        .args(["hello"])
        .env("HARNESS_LAUNCH_FIXTURE_STARTED", &marker)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("orchestration configuration is invalid"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !marker.exists(),
        "an invalid permission must start no payload"
    );
}

#[test]
fn ordinary_reflection_bypasses_nonordinary_and_owned_sessions() {
    let f = Fixture::new();
    f.install_manager();
    f.set_reflection(Some(true));
    let cases: Vec<(&str, Vec<&str>, Vec<(&str, &str)>)> = vec![
        (
            "explicit remote",
            vec!["--remote", "ws://127.0.0.1:1", "hello"],
            vec![],
        ),
        ("noninteractive exec", vec!["exec", "hello"], vec![]),
        ("internal app-server", vec!["app-server"], vec![]),
        ("help", vec!["--help"], vec![]),
        ("version", vec!["--version"], vec![]),
        ("short version", vec!["-V"], vec![]),
        (
            "executor session",
            vec!["hello"],
            vec![("HARNESS_EXECUTOR_SESSION", "1")],
        ),
        (
            "reflection session",
            vec!["hello"],
            vec![("HARNESS_REFLECTION_SESSION", "1")],
        ),
        (
            "bounded lead session",
            vec!["hello"],
            vec![("HARNESS_BOUNDED_LEAD_SESSION", "1")],
        ),
    ];
    for (name, raw, environment) in cases {
        let args = raw.iter().copied().map(OsString::from).collect::<Vec<_>>();
        let (code, report) = launch_report_with_env(&f, &args, &environment);
        assert_eq!(code, 19, "{name}");
        assert_ne!(
            report["args"][0], "_ordinary-lead",
            "{name} must keep its native owner"
        );
    }
}

#[test]
fn ordinary_reflection_rejects_unsupported_native_arguments_before_starting_a_payload() {
    let f = Fixture::new();
    f.install_manager();
    f.set_reflection(Some(true));
    let marker = f.root.path().join("unsupported-argument-started");
    let output = f
        .command()
        .args(["--oss", "hello"])
        .env("HARNESS_LAUNCH_FIXTURE_STARTED", &marker)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--oss"), "{stderr}");
    assert!(stderr.contains("open-source provider"), "{stderr}");
    assert!(
        !marker.exists(),
        "an unsupported argument must start no payload"
    );
}

/// Re-record the fixture build's source identity after the test edited the
/// checkout, so the recorded state matches the live source again.
fn refresh_record(f: &Fixture) {
    let build = f.launcher.parent().unwrap();
    let mut record = build_identity::read_record(build).unwrap();
    record.source = build_identity::source_identity(&f.source).unwrap();
    fs::write(
        build.join("build.json"),
        serde_json::to_vec(&record).unwrap(),
    )
    .unwrap();
    build_selection::activate(&f.state, build).unwrap();
}

/// Write the role sources the launcher's responsibility context resolves:
/// the kit manifest points at `AGENTS.md` and `skills`, and the role
/// descriptor names one instruction file per role. Fixture::new calls this so
/// every launch that reaches session composition resolves them.
fn write_role_sources(f: &Fixture) {
    fs::create_dir_all(f.source.join("skills")).unwrap();
    fs::create_dir_all(f.source.join("global/roles")).unwrap();
    fs::write(f.source.join("AGENTS.md"), "fixture common guidance").unwrap();
    fs::write(f.source.join("global/roles/lead.md"), "lead role guidance").unwrap();
    fs::write(
        f.source.join("global/roles/executor.md"),
        "executor role guidance",
    )
    .unwrap();
    fs::write(
        f.source.join("global/roles.toml"),
        concat!(
            "schema = 1\n",
            "shared_skills = []\n",
            "[roles.lead]\n",
            "instructions = \"global/roles/lead.md\"\n",
            "skills = []\n",
            "[roles.executor]\n",
            "instructions = \"global/roles/executor.md\"\n",
            "skills = []\n",
        ),
    )
    .unwrap();
}

/// Bytes the launcher process has read so far, from the kernel's own
/// per-process I/O accounting (never a file-access heuristic).
fn process_read_bytes(child: &std::process::Child) -> u64 {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::Threading::{GetProcessIoCounters, IO_COUNTERS};
    let mut counters: IO_COUNTERS = unsafe { std::mem::zeroed() };
    let ok = unsafe { GetProcessIoCounters(child.as_raw_handle(), &mut counters) };
    assert_ne!(
        ok,
        0,
        "GetProcessIoCounters: {}",
        std::io::Error::last_os_error()
    );
    counters.ReadTransferCount
}

/// One ordinary launch with an interactive upstream: wait until the payload
/// runs (launch verification is complete), sample the launcher's read bytes,
/// then let the payload finish.
fn launch_read_bytes(f: &Fixture) -> u64 {
    let mut child = f
        .command()
        .env("HARNESS_LAUNCH_FIXTURE_MODE", "interactive")
        .env("CARGO", "must-not-run")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let (ready, line) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        use std::io::BufRead;
        let mut reader = std::io::BufReader::new(stdout);
        let mut first = String::new();
        let _ = reader.read_line(&mut first);
        let _ = ready.send(first);
        let mut rest = String::new();
        while reader
            .read_line(&mut rest)
            .map(|read| read > 0)
            .unwrap_or(false)
        {
            rest.clear();
        }
    });
    let line = line
        .recv_timeout(Duration::from_secs(30))
        .unwrap_or_else(|_| {
            let _ = child.kill();
            let _ = child.wait();
            panic!("launcher never started the interactive payload");
        });
    assert!(line.contains("upstream prompt"), "{line}");
    let reads = process_read_bytes(&child);
    drop(child.stdin.take());
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    reads
}

#[test]
fn immutable_registration_is_independent_of_build_tool_selection_and_rejects_ambiguous_binding() {
    let fixture = Fixture::new();
    fixture.register_immutable();
    let original = fs::read(fixture.home.join("harness/native-launch.json")).unwrap();
    let other = fixture.state.join("builds/other");
    fs::create_dir(&other).unwrap();
    for entry in fs::read_dir(fixture.launcher.parent().unwrap()).unwrap() {
        let entry = entry.unwrap();
        fs::copy(entry.path(), other.join(entry.file_name())).unwrap();
    }
    build_selection::activate(&fixture.state, &other).unwrap();
    let accepted = fixture.command().stdin(Stdio::null()).output().unwrap();
    assert!(
        accepted.status.success(),
        "{}",
        String::from_utf8_lossy(&accepted.stderr)
    );
    assert_eq!(
        fs::read(fixture.home.join("harness/native-launch.json")).unwrap(),
        original
    );
    let mut ambiguous: Value = serde_json::from_slice(&original).unwrap();
    ambiguous["state"] = json!(fixture.state);
    fs::write(
        fixture.home.join("harness/native-launch.json"),
        serde_json::to_vec(&ambiguous).unwrap(),
    )
    .unwrap();
    assert!(
        !fixture
            .command()
            .stdin(Stdio::null())
            .output()
            .unwrap()
            .status
            .success()
    );
    ambiguous.as_object_mut().unwrap().remove("state");
    ambiguous["build"] = json!(other);
    fs::write(
        fixture.home.join("harness/native-launch.json"),
        serde_json::to_vec(&ambiguous).unwrap(),
    )
    .unwrap();
    assert!(
        !fixture
            .command()
            .stdin(Stdio::null())
            .output()
            .unwrap()
            .status
            .success()
    );
    fs::write(fixture.home.join("harness/native-launch.json"), original).unwrap();
    fs::write(
        fixture.source.join("crates/one/src/lib.rs"),
        "changed source",
    )
    .unwrap();
    let stale = fixture.command().stdin(Stdio::null()).output().unwrap();
    assert!(
        stale.status.success(),
        "{}",
        String::from_utf8_lossy(&stale.stderr)
    );
}

#[test]
fn core_check_cli_refuses_missing_legacy_and_pending_without_creating_or_rewriting_state() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("uninstalled-home");
    let user = root.path().join("user");
    let run = || {
        Command::new(env!("CARGO_BIN_EXE_codex-harness"))
            .args(["check", "--core-only", "--codex-home"])
            .arg(&home)
            .arg("--user-home")
            .arg(&user)
            .current_dir(root.path())
            .stdin(Stdio::null())
            .output()
            .unwrap()
    };
    let missing = run();
    assert!(!missing.status.success());
    assert!(String::from_utf8_lossy(&missing.stderr).contains("not connected"));
    assert!(!home.exists());
    assert!(!user.exists());
    fs::create_dir_all(home.join("harness")).unwrap();
    let metadata = home.join("harness/installation.json");
    let legacy = br#"{"schemaVersion":1}"#;
    fs::write(&metadata, legacy).unwrap();
    let old = run();
    assert!(!old.status.success());
    assert!(String::from_utf8_lossy(&old.stderr).contains("legacy"));
    assert_eq!(fs::read(&metadata).unwrap(), legacy);
    let pending = home.join("harness/pending.json");
    fs::write(&pending, b"PRIVATE_PENDING_SENTINEL").unwrap();
    let blocked = run();
    assert!(!blocked.status.success());
    let error = String::from_utf8_lossy(&blocked.stderr);
    assert!(error.contains("pending"));
    assert!(!error.contains("PRIVATE_PENDING_SENTINEL"));
    assert_eq!(fs::read(&pending).unwrap(), b"PRIVATE_PENDING_SENTINEL");
    assert_eq!(fs::read(&metadata).unwrap(), legacy);
    assert!(!user.exists());
}

#[test]
fn installed_manager_link_enforces_integrity_and_allows_source_stale_recovery() {
    use std::os::windows::fs::symlink_file;
    let fixture = Fixture::new();
    let build = fixture.launcher.parent().unwrap();
    let manager = build.join("codex-harness.exe");
    fs::copy(env!("CARGO_BIN_EXE_codex-harness"), &manager).unwrap();
    let mut record = build_identity::read_record(build).unwrap();
    record.binaries.insert(
        "codex-harness.exe".into(),
        build_identity::hash_file(&manager).unwrap(),
    );
    fs::write(
        build.join("build.json"),
        serde_json::to_vec(&record).unwrap(),
    )
    .unwrap();
    let bin = fixture.home.join("harness/bin");
    fs::create_dir(&bin).unwrap();
    let link = bin.join("codex-harness.exe");
    symlink_file(&manager, &link).unwrap();
    let target = fixture.root.path().join("uninstalled-home");
    let user = fixture.root.path().join("uninstalled-user");
    let run = |command: &Path| {
        Command::new(command)
            .args(["recover", "--core-only", "--codex-home"])
            .arg(&target)
            .arg("--user-home")
            .arg(&user)
            .current_dir(fixture.root.path())
            .stdin(Stdio::null())
            .output()
            .unwrap()
    };
    assert!(run(&link).status.success());
    fs::write(
        fixture.source.join("crates/one/src/lib.rs"),
        b"source changed",
    )
    .unwrap();
    assert!(run(&link).status.success());
    let unrecorded = build.join("unrecorded-manager.exe");
    fs::copy(&manager, &unrecorded).unwrap();
    assert!(!run(&unrecorded).status.success());
    fs::write(build.join("harness-observe.exe"), b"altered artifact").unwrap();
    assert!(run(&link).status.success());
    fs::OpenOptions::new()
        .append(true)
        .open(&manager)
        .unwrap()
        .write_all(b"altered manager overlay")
        .unwrap();
    assert!(!run(&link).status.success());
    assert!(!target.exists());
    assert!(!user.exists());
}

#[test]
fn native_argv_unicode_stdin_streams_cwd_and_nonzero_exit() {
    let f = Fixture::new();
    // Pre-establish this fixture's own account so the launch exercises joining
    // an existing group. Forwarding and exit status are the assertions here;
    // positive membership is proven by the measurement case below.
    let _budget = SharedCpuBudget::acquire(&f.account, SHARED_CPU_PERCENT).unwrap();
    let args = [
        "--harness-effort",
        "routine",
        "exec",
        "",
        "проверка \"кавычки\"",
        "trailing\\",
        "$() `literal` ; &",
    ];
    let mut child = f
        .command()
        .args(args)
        .env("HARNESS_LAUNCH_FIXTURE_MODE", "nonzero")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all("первая строка\nsecond line\n".as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(19));
    assert_eq!(
        String::from_utf8(output.stderr).unwrap().trim(),
        "upstream stderr"
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        report["args"],
        json!([
            "-c",
            "approval_policy=\"never\"",
            "-c",
            "model_reasoning_effort=\"low\"",
            "-c",
            "developer_instructions=\"lead role guidance\"",
            "-c",
            "skills.config=[]",
            "exec",
            "",
            "проверка \"кавычки\"",
            "trailing\\",
            "$() `literal` ; &"
        ])
    );
    assert_eq!(report["stdin"], "первая строка\nsecond line\n");
    assert_eq!(
        Path::new(report["cwd"].as_str().unwrap())
            .canonicalize()
            .unwrap(),
        f.root.path().canonicalize().unwrap()
    );
    // Positive admission of a piped session is proven by the dedicated
    // measurement case below; here the exact stderr above already rules out
    // every warned fallback, so forwarding and exit status stay the assertions.
}

#[test]
fn explicit_native_precedence_and_package_manager_metadata() {
    let f = Fixture::new();
    let package = f.root.path().join("package");
    fs::create_dir(&package).unwrap();
    fs::write(
        package.join("package.json"),
        "{\"name\":\"@openai/codex\",\"version\":\"0.153.4\"}",
    )
    .unwrap();
    f.register(json!({"executable":f.upstream,"sha256":build_identity::hash_file(&f.upstream).unwrap(),"package":{"root":package,"manifest_sha256":build_identity::hash_file(&package.join("package.json")).unwrap(),"manager":"npm"}}));
    let out = f
        .command()
        .args([
            "--harness-effort=routine",
            "--profile",
            "user",
            "exec",
            "--",
            "-cmodel_reasoning_effort=max",
        ])
        .env("CODEX_MANAGED_BY_BUN", "1")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        report["args"],
        json!([
            "-c",
            "developer_instructions=\"lead role guidance\"",
            "-c",
            "features.worktrees=false",
            "-c",
            "skills.config=[]",
            "--profile",
            "user",
            "exec",
            "--",
            "-cmodel_reasoning_effort=max"
        ])
    );
    assert_eq!(report["environment"]["CODEX_MANAGED_BY_NPM"], "1");
    assert!(report["environment"]["CODEX_MANAGED_BY_BUN"].is_null());
    assert_eq!(
        Path::new(
            report["environment"]["CODEX_MANAGED_PACKAGE_ROOT"]
                .as_str()
                .unwrap()
        )
        .canonicalize()
        .unwrap(),
        package.canonicalize().unwrap()
    );
}

#[test]
fn saved_native_effort_survives_per_model_default_injection() {
    let f = Fixture::new();
    // A saved native model/effort pair is the effective session choice: the
    // launcher must not promote the per-model max default over the lighter
    // saved effort as a CLI override.
    fs::write(
        f.home.join("config.toml"),
        "model = 'zai/glm-5.3'\nmodel_reasoning_effort = 'low'\n",
    )
    .unwrap();
    let out = f.command().args(["exec", "hello"]).output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        report["args"],
        json!([
            "-c",
            "approval_policy=\"never\"",
            "-c",
            "developer_instructions=\"lead role guidance\"",
            "-c",
            "skills.config=[]",
            "exec",
            "hello"
        ])
    );
    // A plain TUI session start resolves the same effective configuration.
    let out = f.command().output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: Value = serde_json::from_slice(&out.stdout).unwrap();
    // The daemon opt-out is unrelated to effort; no effort override appears.
    assert_eq!(
        report["args"],
        json!([
            "-c",
            "approval_policy=\"never\"",
            "-c",
            "features.daemon_auto_start=false",
            "-c",
            "developer_instructions=\"lead role guidance\"",
            "-c",
            "skills.config=[]"
        ])
    );
}

#[test]
fn trusted_project_preferences_win_over_shared_defaults() {
    let f = Fixture::new();
    // The shared portable defaults carry both keys; an applicable trusted
    // project file is the native choice and shields both promotions.
    fs::write(
        f.source.join("global/harness.config.toml"),
        "model = 'gpt-6-astra'\nmodel_reasoning_effort = 'xhigh'\n",
    )
    .unwrap();
    let project = f.root.path().join("project");
    fs::create_dir_all(project.join(".codex")).unwrap();
    fs::write(
        f.home.join("config.toml"),
        format!(
            "[projects.'{}']\ntrust_level = 'trusted'\n",
            project.display()
        ),
    )
    .unwrap();
    fs::write(
        project.join(".codex/config.toml"),
        "model = 'zai/glm-5.3'\nmodel_reasoning_effort = 'medium'\n",
    )
    .unwrap();
    let out = f
        .command()
        .current_dir(&project)
        .args(["exec", "hello"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        report["args"],
        json!([
            "-c",
            "developer_instructions=\"lead role guidance\"",
            "-c",
            "skills.config=[]",
            "exec",
            "hello"
        ])
    );
}

#[test]
fn per_model_default_applies_without_any_native_effort() {
    let f = Fixture::new();
    // A saved model without any applicable effort keeps the fallback path:
    // the mapped default is injected at its documented precedence.
    fs::write(f.home.join("config.toml"), "model = 'zai/glm-5.3'\n").unwrap();
    let out = f.command().args(["exec", "hello"]).output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        report["args"],
        json!([
            "-c",
            "approval_policy=\"never\"",
            "-c",
            "model_reasoning_effort=\"max\"",
            "-c",
            "developer_instructions=\"lead role guidance\"",
            "-c",
            "skills.config=[]",
            "exec",
            "hello"
        ])
    );
}

#[test]
fn unresolved_native_configuration_blocks_default_injection() {
    let f = Fixture::new();
    // An unreadable native configuration is an unknown choice: no fallback is
    // promoted over it, and the launch-availability contract is preserved.
    fs::write(f.home.join("config.toml"), "model = [\n").unwrap();
    let out = f
        .command()
        .args(["-m", "zai/glm-5.3", "exec", "hello"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains(
            "codex-harness: shared harness unavailable; launching registered Codex without harness overrides"
        ),
        "{stderr}"
    );
    let report: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        report["args"],
        json!(["-m", "zai/glm-5.3", "exec", "hello"])
    );
}

#[test]
fn embedded_tui_sessions_suppress_the_daemon_fallback_warning() {
    let f = Fixture::new();
    let out = f.command().args(["--profile", "user"]).output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        report["args"],
        json!([
            "-c",
            "features.daemon_auto_start=false",
            "-c",
            "developer_instructions=\"lead role guidance\"",
            "-c",
            "features.worktrees=false",
            "-c",
            "skills.config=[]",
            "--profile",
            "user"
        ])
    );

    let out = f.command().arg("hello").output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        report["args"],
        json!([
            "-c",
            "approval_policy=\"never\"",
            "-c",
            "features.daemon_auto_start=false",
            "-c",
            "developer_instructions=\"lead role guidance\"",
            "-c",
            "skills.config=[]",
            "hello"
        ])
    );

    // Non-TUI commands keep codex-cli's own startup path and arguments.
    let out = f.command().args(["exec", "hello"]).output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        report["args"],
        json!([
            "-c",
            "approval_policy=\"never\"",
            "-c",
            "developer_instructions=\"lead role guidance\"",
            "-c",
            "skills.config=[]",
            "exec",
            "hello"
        ])
    );
}

#[test]
fn managed_session_starts_forward_the_worktree_tool_suppression_leaf() {
    let f = Fixture::new();
    // The kit's shared defaults own the managed suppression: the native
    // worktree tools would compete with the common harness pool, so a
    // session start forwards the feature leaf as a native CLI override.
    fs::write(
        f.source.join("global/harness.config.toml"),
        "approval_policy = 'never'\n[features]\nworktrees = false\n",
    )
    .unwrap();
    let out = f.command().arg("hello").output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        report["args"],
        json!([
            "-c",
            "approval_policy=\"never\"",
            "-c",
            "features.worktrees=false",
            "-c",
            "features.daemon_auto_start=false",
            "-c",
            "developer_instructions=\"lead role guidance\"",
            "-c",
            "skills.config=[]",
            "hello"
        ])
    );

    // Non-TUI commands share the same defaults layer and carry the leaf too;
    // the daemon opt-out stays TUI-only.
    let out = f.command().args(["exec", "hello"]).output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        report["args"],
        json!([
            "-c",
            "approval_policy=\"never\"",
            "-c",
            "features.worktrees=false",
            "-c",
            "developer_instructions=\"lead role guidance\"",
            "-c",
            "skills.config=[]",
            "exec",
            "hello"
        ])
    );

    // The executor marker composes the executor role instead of the lead role
    // and keeps the same shared suppression leaf alongside the single-agent
    // switch.
    let out = f
        .command()
        .env("HARNESS_EXECUTOR_SESSION", "1")
        .arg("hello")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        report["args"],
        json!([
            "-c",
            "approval_policy=\"never\"",
            "-c",
            "features.worktrees=false",
            "-c",
            "features.daemon_auto_start=false",
            "-c",
            "agents.enabled=false",
            "-c",
            "developer_instructions=\"executor role guidance\"",
            "-c",
            "skills.config=[]",
            "hello"
        ])
    );

    // A machine-local leaf that agrees with the shared suppression is not a
    // conflict: the shared leaf stays skipped and no re-enable is reported.
    fs::write(
        f.home.join("config.toml"),
        "[features]\nworktrees = false\n",
    )
    .unwrap();
    let out = f.command().arg("hello").output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        report["args"],
        json!([
            "-c",
            "approval_policy=\"never\"",
            "-c",
            "features.daemon_auto_start=false",
            "-c",
            "developer_instructions=\"lead role guidance\"",
            "-c",
            "skills.config=[]",
            "hello"
        ])
    );
    assert!(
        !String::from_utf8_lossy(&out.stderr).contains("features.worktrees"),
        "an agreeing machine-local leaf is not a worktree-tool conflict"
    );
}

#[test]
fn explicit_native_worktree_feature_choice_shields_the_managed_suppression_leaf() {
    let f = Fixture::new();
    fs::write(
        f.source.join("global/harness.config.toml"),
        "approval_policy = 'never'\n[features]\nworktrees = false\n",
    )
    .unwrap();
    // A machine-local leaf is the explicit native choice: the shared default
    // is skipped instead of fought, so the session keeps the native worktree
    // tools and the re-exposed surface is reported as a configuration conflict
    // for managed-pool delivery instead of a silent suppression claim.
    fs::write(
        f.home.join("config.toml"),
        "model = \"gpt-6.1-sol\"\n[features]\nworktrees = true\n[mcp_servers.demo]\ncommand = \"demo-server\"\n",
    )
    .unwrap();
    let before = fs::read(f.home.join("config.toml")).unwrap();
    let out = f.command().arg("hello").output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        report["args"],
        json!([
            "-c",
            "approval_policy=\"never\"",
            "-c",
            "features.daemon_auto_start=false",
            "-c",
            "developer_instructions=\"lead role guidance\"",
            "-c",
            "skills.config=[]",
            "hello"
        ])
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("the user configuration sets features.worktrees=true")
            && stderr.contains("compete with the shared worktree pool"),
        "the re-enabled native worktree tools must be reported instead of lost silently: {stderr}"
    );
    // The explicit machine-local choice and its unrelated model and MCP
    // settings survive the launch untouched.
    assert_eq!(before, fs::read(f.home.join("config.toml")).unwrap());

    // An explicit profile replaces the kit's shared layer but selects no
    // worktree value, so the base user configuration's `true` is the winning
    // native choice: the session keeps the competing tools and the ineffective
    // suppression is reported instead of claimed.
    let out = f.command().args(["--profile", "user"]).output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        report["args"],
        json!([
            "-c",
            "features.daemon_auto_start=false",
            "-c",
            "developer_instructions=\"lead role guidance\"",
            "-c",
            "skills.config=[]",
            "--profile",
            "user"
        ])
    );
    assert!(
        String::from_utf8_lossy(&out.stderr)
            .contains("the user configuration sets features.worktrees=true"),
        "an explicit profile must report the re-exposed native tools: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn native_worktree_choice_precedence_reports_only_unsuppressed_sessions() {
    let f = Fixture::new();
    fs::write(
        f.source.join("global/harness.config.toml"),
        "approval_policy = 'never'\n[features]\nworktrees = false\n",
    )
    .unwrap();
    let run = |fixture: &Fixture, args: &[&str]| {
        let out = fixture.command().args(args).output().unwrap();
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        (
            serde_json::from_slice::<Value>(&out.stdout).unwrap(),
            String::from_utf8(out.stderr).unwrap(),
        )
    };
    // The installed native precedence for an explicit feature choice:
    // repeated `-c` leaves keep the last value, `--enable`/`--disable` win
    // over `-c` regardless of encounter order, and a disable wins over an
    // enable. Only a winning `false` establishes the managed suppression; any
    // other winning choice is an explicit override that re-exposes the
    // competing tools or leaves the suppression unestablishable, reported as a
    // configuration conflict instead of a successful managed-pool delivery.
    let cases: &[(&str, &[&str], Option<&str>)] = &[
        (
            "explicit true",
            &["-c", "features.worktrees=true", "hello"],
            Some("the command-line configuration sets features.worktrees=true"),
        ),
        (
            "explicit false",
            &["-c", "features.worktrees=false", "hello"],
            None,
        ),
        (
            "enable flag",
            &["--enable", "worktrees", "hello"],
            Some("the command-line configuration sets features.worktrees=true"),
        ),
        (
            "enable flag beats config",
            &[
                "--enable=worktrees",
                "-c",
                "features.worktrees=false",
                "hello",
            ],
            Some("the command-line configuration sets features.worktrees=true"),
        ),
        ("disable flag", &["--disable", "worktrees", "hello"], None),
        (
            "disable flag beats config",
            &[
                "-c",
                "features.worktrees=true",
                "--disable=worktrees",
                "hello",
            ],
            None,
        ),
        (
            "last leaf wins false",
            &[
                "-c",
                "features.worktrees=true",
                "-c",
                "features.worktrees=false",
                "hello",
            ],
            None,
        ),
        (
            "last leaf wins true",
            &[
                "-c",
                "features.worktrees=false",
                "-c",
                "features.worktrees=true",
                "hello",
            ],
            Some("the command-line configuration sets features.worktrees=true"),
        ),
        (
            "non-boolean value",
            &["-c", "features.worktrees=maybe", "hello"],
            Some("which is not a boolean"),
        ),
    ];
    for (label, args, expected) in cases {
        let (report, stderr) = run(&f, args);
        match expected {
            Some(fragment) => assert!(stderr.contains(*fragment), "{label}: {stderr}"),
            None => assert!(
                !stderr.contains("features.worktrees"),
                "{label}: an established or unneeded suppression must stay silent: {stderr}"
            ),
        }
        if *label == "disable flag" {
            let composed = report["args"].as_array().unwrap();
            assert!(
                !composed
                    .iter()
                    .any(|value| value == "features.worktrees=false"),
                "{label}: the shared leaf is skipped, not fought: {composed:?}"
            );
            assert!(
                composed.iter().any(|value| value == "--disable"),
                "{label}: the explicit flag is forwarded: {composed:?}"
            );
        }
    }
    // A machine-local `true` is the earlier native choice; the command line
    // still wins with an explicit `false`.
    let user = Fixture::new();
    fs::write(
        user.source.join("global/harness.config.toml"),
        "approval_policy = 'never'\n[features]\nworktrees = false\n",
    )
    .unwrap();
    fs::write(
        user.home.join("config.toml"),
        "[features]\nworktrees = true\n",
    )
    .unwrap();
    let (_, stderr) = run(&user, &["hello"]);
    assert!(
        stderr.contains("the user configuration sets features.worktrees=true"),
        "{stderr}"
    );
    let (_, stderr) = run(&user, &["-c", "features.worktrees=false", "hello"]);
    assert!(
        !stderr.contains("features.worktrees"),
        "command-line false must override the earlier user true: {stderr}"
    );
    // Selecting a profile is not itself a worktree choice: a managed session
    // whose profile selects no features.worktrees leaf still receives the
    // suppression default with the profile's other settings untouched, while
    // an actual winning choice in any layer is reported instead of fought.
    let profiles = Fixture::new();
    fs::write(
        profiles.source.join("global/harness.config.toml"),
        "approval_policy = 'never'\n[features]\nworktrees = false\n",
    )
    .unwrap();
    fs::write(
        profiles.home.join("user.config.toml"),
        "model = \"gpt-6.1-sol\"\nmodel_reasoning_effort = \"low\"\n[mcp_servers.demo]\ncommand = \"demo-server\"\n",
    )
    .unwrap();
    let before = fs::read(profiles.home.join("user.config.toml")).unwrap();
    let (report, stderr) = run(&profiles, &["--profile", "user", "hello"]);
    assert!(
        !stderr.contains("features.worktrees"),
        "a profile with no worktree choice must receive the suppression default silently: {stderr}"
    );
    let composed = report["args"].as_array().unwrap();
    assert!(
        composed
            .iter()
            .any(|value| value == "features.worktrees=false"),
        "a profile with no worktree choice must still carry the managed suppression leaf: {composed:?}"
    );
    assert!(
        composed.iter().any(|value| value == "--profile")
            && composed.iter().any(|value| value == "user"),
        "the selected profile stays part of the composed session: {composed:?}"
    );
    assert!(
        !composed.iter().any(|value| {
            let value = value.as_str().unwrap_or("");
            value.starts_with("model=") || value.starts_with("mcp_servers")
        }),
        "the profile's own model and MCP settings are not replaced by launch overrides: {composed:?}"
    );
    assert_eq!(
        before,
        fs::read(profiles.home.join("user.config.toml")).unwrap(),
        "the profile layer is never rewritten by a launch"
    );

    // The same profile session under the executor marker keeps the managed
    // suppression leaf beside the single-agent switch.
    let out = profiles
        .command()
        .env("HARNESS_EXECUTOR_SESSION", "1")
        .args(["--profile", "user", "hello"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: Value = serde_json::from_slice(&out.stdout).unwrap();
    let composed = report["args"].as_array().unwrap();
    assert!(
        composed
            .iter()
            .any(|value| value == "features.worktrees=false")
            && composed.iter().any(|value| value == "agents.enabled=false"),
        "an executor profile session carries the suppression leaf and stays single-agent: {composed:?}"
    );
    assert!(
        !String::from_utf8_lossy(&out.stderr).contains("features.worktrees"),
        "an executor profile session with no worktree choice is not a conflict: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    // Only an actual profile choice is a conflict, and it keeps the tools.
    fs::write(
        profiles.home.join("user.config.toml"),
        "model = \"gpt-6.1-sol\"\n[features]\nworktrees = true\n",
    )
    .unwrap();
    let (report, stderr) = run(&profiles, &["--profile", "user", "hello"]);
    assert!(
        stderr.contains("the explicit profile 'user' configuration sets features.worktrees=true"),
        "{stderr}"
    );
    let composed = report["args"].as_array().unwrap();
    assert!(
        !composed
            .iter()
            .any(|value| value == "features.worktrees=false"),
        "an explicit profile choice is honored instead of fought: {composed:?}"
    );

    // A trusted project file still wins over the profile, so a winning false
    // needs no injected leaf and stays silent.
    let project = profiles.root.path().join("project");
    fs::create_dir_all(project.join(".codex")).unwrap();
    fs::write(
        project.join(".codex/config.toml"),
        "[features]\nworktrees = false\n",
    )
    .unwrap();
    fs::write(
        profiles.home.join("config.toml"),
        format!(
            "[projects.'{}']\ntrust_level = 'trusted'\n",
            project.display()
        ),
    )
    .unwrap();
    let out = profiles
        .command()
        .current_dir(&project)
        .args(["--profile", "user", "hello"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        !String::from_utf8_lossy(&out.stderr).contains("features.worktrees"),
        "the trusted project choice wins over the profile: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn stale_missing_altered_and_interrupted_installations_do_not_launch_or_build() {
    for mode in ["registration", "journal", "recursion"] {
        let f = Fixture::new();
        let before = fs::read(f.state.join("active-build.json")).unwrap();
        match mode {
            "registration" => fs::remove_file(f.home.join("harness/native-launch.json")).unwrap(),
            "journal" => fs::write(f.state.join("build-selection-journal.json"),"interrupted").unwrap(),
            "recursion" => f.register(json!({"executable":f.launcher,"sha256":build_identity::hash_file(&f.launcher).unwrap(),"package":null})),
            _ => unreachable!(),
        }
        let out = f
            .command()
            .arg("--version")
            .env("CARGO", "must-not-run")
            .output()
            .unwrap();
        assert!(!out.status.success(), "{mode}");
        assert!(out.stdout.is_empty(), "{mode}");
        assert_eq!(
            fs::read(f.state.join("active-build.json")).unwrap(),
            before,
            "{mode}"
        );
        assert!(
            !f.state.join("staging").exists(),
            "ordinary launch compiled: {mode}"
        );
    }
}

#[test]
fn ordinary_launch_reads_do_not_scale_with_compiled_sources() {
    for immutable in [false, true] {
        let f = Fixture::new();
        if immutable {
            f.register_immutable();
        }
        let small = launch_read_bytes(&f);
        // A checkout-sized compiled input is recorded, then the same launch is
        // measured again: any compiled-source freshness traversal would read
        // every byte of it before starting the payload.
        let payload = f.source.join("crates/one/src/payload.bin");
        fs::write(&payload, vec![0x5a; 64 * 1024 * 1024]).unwrap();
        refresh_record(&f);
        let large = launch_read_bytes(&f);
        let delta = large.saturating_sub(small);
        println!("immutable={immutable} launch reads small={small} large={large} delta={delta}");
        assert!(
            delta < 4 * 1024 * 1024,
            "launch read {delta} extra bytes of the compiled payload (small={small}, large={large})"
        );
    }
}

#[test]
fn every_required_binary_is_reverified_before_launch() {
    for name in BINARIES.iter().filter(|name| **name != "codex.exe") {
        let f = Fixture::new();
        fs::write(f.launcher.parent().unwrap().join(name), b"changed").unwrap();
        let out = f
            .command()
            .args(["exec", "hello"])
            .env("CARGO", "must-not-run")
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains("launching registered Codex without harness overrides"),
            "{name}: {stderr}"
        );
        let report: Value = serde_json::from_slice(&out.stdout).unwrap();
        assert!(report["args"].is_array(), "{name}: {report}");
    }
}

#[test]
fn altered_bytes_with_a_restored_timestamp_are_not_trusted_at_launch() {
    let f = Fixture::new();
    let path = f.launcher.parent().unwrap().join("harness-rtk.exe");
    let original = fs::read(&path).unwrap();
    let timestamp = fs::metadata(&path).unwrap().modified().unwrap();
    let mut altered = original.clone();
    altered[0] ^= 0xff;
    fs::write(&path, &altered).unwrap();
    fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(timestamp)
        .unwrap();
    let out = f.command().stdin(Stdio::null()).output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out.stderr)
            .contains("launching registered Codex without harness overrides"),
        "a same-size mutation with a restored timestamp must not reuse verification"
    );
    // Restoring the bytes restores admission: each launch verifies again.
    fs::write(&path, &original).unwrap();
    let out = f.command().stdin(Stdio::null()).output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        !String::from_utf8_lossy(&out.stderr)
            .contains("launching registered Codex without harness overrides")
    );
}

#[test]
fn explicit_check_reports_fresh_stale_unavailable_and_altered_sources() {
    let run = |f: &Fixture| {
        let out = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
            .arg("check")
            .arg("--build")
            .arg(f.launcher.parent().unwrap())
            .current_dir(f.root.path())
            .stdin(Stdio::null())
            .output()
            .unwrap();
        let report: Value = serde_json::from_slice(&out.stdout).unwrap();
        (out.status.code(), report)
    };
    let f = Fixture::new();
    let (code, report) = run(&f);
    assert_eq!(
        (code, report["status"].as_str()),
        (Some(0), Some("healthy")),
        "{report}"
    );
    fs::write(f.source.join("crates/one/src/lib.rs"), "changed").unwrap();
    let (code, report) = run(&f);
    assert_eq!(
        (code, report["status"].as_str()),
        (Some(1), Some("source-stale")),
        "{report}"
    );
    assert_eq!(report["runtime_allowed"], json!(true));
    assert_eq!(report["management_allowed"], json!(true));
    fs::rename(&f.source, f.root.path().join("moved-source")).unwrap();
    let (code, report) = run(&f);
    assert_eq!(
        (code, report["status"].as_str()),
        (Some(1), Some("source-unavailable")),
        "{report}"
    );
    assert_eq!(report["runtime_allowed"], json!(true));
    // An altered dependent binary is damage, separate from source freshness.
    fs::rename(f.root.path().join("moved-source"), &f.source).unwrap();
    fs::write(
        f.launcher.parent().unwrap().join("harness-rtk.exe"),
        "changed",
    )
    .unwrap();
    let (code, report) = run(&f);
    assert_eq!(
        (code, report["status"].as_str()),
        (Some(1), Some("altered")),
        "{report}"
    );
    assert_eq!(report["runtime_allowed"], json!(false));
    assert_eq!(report["management_allowed"], json!(true));
}

#[test]
fn changed_upstream_digest_still_launches_without_a_compatibility_warning() {
    let f = Fixture::new();
    f.register(json!({
        "executable": f.upstream,
        "sha256": "0".repeat(64),
        "package": null
    }));
    let out = f
        .command()
        .args(["--profile", "user", "exec"])
        .env("CARGO", "must-not-run")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8(out.stderr).unwrap();
    // The explicit profile carries no managed suppression, so the launch
    // reports that conflict; nothing else is added to the upstream's line.
    assert!(stderr.contains("upstream stderr"), "{stderr}");
    assert!(
        stderr.lines().all(|line| line == "upstream stderr"
            || line.starts_with("codex-harness: the explicit profile")),
        "{stderr}"
    );
    assert!(!stderr.contains("explicit update"), "{stderr}");
    assert!(!f.state.join("staging").exists());

    let f = Fixture::new();
    let package = f.root.path().join("package");
    fs::create_dir(&package).unwrap();
    fs::write(
        package.join("package.json"),
        "{\"name\":\"@openai/codex\",\"version\":\"0.155.0\"}",
    )
    .unwrap();
    f.register(json!({
        "executable": f.upstream,
        "sha256": build_identity::hash_file(&f.upstream).unwrap(),
        "package": {
            "root": package,
            "manifest_sha256": "0".repeat(64),
            "manager": "npm"
        }
    }));
    let out = f
        .command()
        .args(["--profile", "user", "exec"])
        .env("CARGO", "must-not-run")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8(out.stderr.clone()).unwrap();
    assert!(stderr.contains("upstream stderr"), "{stderr}");
    assert!(
        stderr.lines().all(|line| line == "upstream stderr"
            || line.starts_with("codex-harness: the explicit profile")),
        "{stderr}"
    );
    let report: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["environment"]["CODEX_MANAGED_BY_NPM"], "1");
}

#[test]
fn missing_stale_source_and_shared_toml_fail_open_to_verified_upstream() {
    let args = [
        "--harness-effort",
        "routine",
        "exec",
        "",
        "проверка \"кавычки\"",
        "trailing\\",
        "$() `literal` ; &",
    ];
    let expected_args = json!([
        "-c",
        "model_reasoning_effort=\"low\"",
        "exec",
        "",
        "проверка \"кавычки\"",
        "trailing\\",
        "$() `literal` ; &"
    ]);
    for mode in [
        "missing-source",
        "shared-toml",
        "companion-altered",
        "companion-missing",
    ] {
        let f = Fixture::new();
        if mode == "schema2-stale" {
            f.register_immutable();
        }
        let before = fs::read(f.state.join("active-build.json")).unwrap();
        match mode {
            "missing-source" => fs::remove_dir_all(&f.source).unwrap(),
            "shared-toml" => fs::write(
                f.source.join("global/harness.config.toml"),
                "approval_policy = [\n",
            )
            .unwrap(),
            "companion-altered" => fs::write(
                f.launcher.parent().unwrap().join("harness-rtk.exe"),
                "changed",
            )
            .unwrap(),
            "companion-missing" => {
                fs::remove_file(f.launcher.parent().unwrap().join("harness-rtk.exe")).unwrap()
            }
            _ => unreachable!(),
        }
        let mut child = f
            .command()
            .args(args)
            .env("CARGO", "must-not-run")
            .env("HARNESS_LAUNCH_FIXTURE_MODE", "nonzero")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all("первая строка\nsecond line\n".as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert_eq!(output.status.code(), Some(19), "{mode}");
        let stderr = String::from_utf8(output.stderr.clone()).unwrap();
        assert!(
            stderr.contains(
                "codex-harness: shared harness unavailable; launching registered Codex without harness overrides"
            ),
            "{mode}: {stderr}"
        );
        assert!(stderr.contains("upstream stderr"), "{mode}: {stderr}");
        let report: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["args"], expected_args, "{mode}");
        assert_eq!(report["stdin"], "первая строка\nsecond line\n", "{mode}");
        assert_eq!(
            Path::new(report["cwd"].as_str().unwrap())
                .canonicalize()
                .unwrap(),
            f.root.path().canonicalize().unwrap(),
            "{mode}"
        );
        assert_eq!(
            fs::read(f.state.join("active-build.json")).unwrap(),
            before,
            "{mode}"
        );
        assert!(
            !f.state.join("staging").exists(),
            "ordinary launch compiled: {mode}"
        );
    }
}

#[test]
fn stale_source_keeps_delivered_overrides_instead_of_degrading() {
    let args = ["--harness-effort", "routine", "exec", "", "still shared"];
    for mode in ["stale-source", "schema2-stale"] {
        let f = Fixture::new();
        if mode == "schema2-stale" {
            f.register_immutable();
        }
        fs::write(f.source.join("crates/one/src/lib.rs"), "changed").unwrap();
        let mut child = f
            .command()
            .args(args)
            .env("CARGO", "must-not-run")
            .env("HARNESS_LAUNCH_FIXTURE_MODE", "nonzero")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all("первая строка\nsecond line\n".as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert_eq!(output.status.code(), Some(19), "{mode}");
        let stderr = String::from_utf8(output.stderr.clone()).unwrap();
        assert!(
            !stderr.contains("launching registered Codex without harness overrides"),
            "{mode}: {stderr}"
        );
    }
}

#[test]
fn upstream_background_lifetime_survives_ordinary_wrapper_exit() {
    let f = Fixture::new();
    let marker = f.root.path().join("background.txt");
    let out = f
        .command()
        .env("HARNESS_LAUNCH_FIXTURE_MODE", "background")
        .env("HARNESS_LAUNCH_FIXTURE_MARKER", &marker)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let end = Instant::now() + Duration::from_secs(5);
    while !marker.is_file() && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(fs::read_to_string(marker).unwrap(), "background completed");
}

#[test]
fn real_console_interaction_and_ctrl_c_return_upstream_exit() {
    let f = Fixture::new();
    let session = f.console("interactive");
    wait_for(&session, "upstream prompt console=true");
    // The account is observed while the payload runs: this interactive route
    // must either admit the payload or report the shared cap as unverified.
    let budget = SharedCpuBudget::acquire(&f.account, SHARED_CPU_PERCENT).unwrap();
    assert_eq!(
        budget.snapshot().unwrap().cpu_rate,
        cpu_rate_units(SHARED_CPU_PERCENT)
    );
    assert_cpu_attempt_visible(&budget, &session);
    session.send("console input\r\n").unwrap();
    wait_for(&session, "upstream echo:console input");
    let result = session
        .wait(
            Deadline::after(Duration::from_secs(5)).unwrap(),
            &Cancellation::default(),
            Duration::from_secs(2),
        )
        .unwrap();
    assert_eq!(result.outcome.reason, StopReason::Exited);
    assert_eq!(result.outcome.exit_code, 0);
    let session = f.console("ctrl-c");
    wait_for(&session, "upstream ready");
    assert_cpu_attempt_visible(&budget, &session);
    session.send("\u{3}").unwrap();
    let result = session
        .wait(
            Deadline::after(Duration::from_secs(5)).unwrap(),
            &Cancellation::default(),
            Duration::from_secs(2),
        )
        .unwrap();
    assert_eq!(result.outcome.reason, StopReason::Exited);
    assert_eq!(result.outcome.exit_code, 0xc000013a);
}

fn wait_for(session: &ConsoleSession, text: &str) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !session.transcript().contains(text) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        session.transcript().contains(text),
        "{}",
        session.transcript()
    );
}

fn host_logical_processors() -> u32 {
    std::thread::available_parallelism()
        .map(|count| count.get() as u32)
        .unwrap_or(1)
}

/// Spinning threads per process. The two spinning processes must demand
/// several times the lowered allowance, or the measurement proves nothing; the
/// count grows with the host only when a fixed count would not.
fn spinner_threads(cpus: u32, percent: f64) -> u64 {
    (percent / 100.0 * f64::from(cpus) * 2.0).ceil().max(2.0) as u64
}

/// Kernel rate units of the shared budget: 0.01% steps, like `Limits`.
fn cpu_rate_units(percent: f64) -> u32 {
    (percent * 100.0).floor() as u32
}

fn receipt(path: &Path) -> Value {
    let bytes =
        fs::read(path).unwrap_or_else(|error| panic!("{} is missing: {error}", path.display()));
    serde_json::from_slice(&bytes).unwrap()
}

fn wait_for_path(path: &Path, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while !path.is_file() {
        assert!(
            Instant::now() < deadline,
            "{} did not appear",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Bounded wait for kernel membership accounting: admission is observed from
/// the object itself, not from payload cooperation.
fn wait_for_members(budget: &SharedCpuBudget, expected: u32, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if budget.snapshot().unwrap().active_processes >= expected {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

/// A dispatching job hierarchy either hosts the ordered pair or the kernel
/// refuses the placement; what must never happen is a silent skip. The session
/// is therefore either an admitted member of the account group or it carries
/// the visible placement notice.
fn assert_cpu_attempt_visible(budget: &SharedCpuBudget, session: &ConsoleSession) {
    let transcript = session.transcript();
    let admitted = wait_for_members(budget, 1, Duration::from_secs(3));
    assert!(
        admitted || transcript.contains(CAP_WARNING),
        "the session neither joined the account group nor reported the shared cap as unverified: {transcript}"
    );
    if !admitted {
        assert!(
            transcript.contains("failed stage: session placement"),
            "{transcript}"
        );
    }
}

/// Bounded wait for a drained group, so a session's exit cannot leave its tree
/// behind in the shared accounting object.
fn wait_for_empty(budget: &SharedCpuBudget) -> bool {
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        if budget.snapshot().unwrap().active_processes == 0 {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

/// Observable account state: names for a directory, bytes for a plain file,
/// nothing when absent. Deliberately format-agnostic: the point is that a
/// degraded launch persists and changes no machine-local budget state.
fn account_state(account: &Path) -> String {
    match fs::metadata(account) {
        Err(_) => "absent".into(),
        Ok(metadata) if metadata.is_dir() => {
            let mut names: Vec<_> = fs::read_dir(account)
                .unwrap()
                .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();
            format!("dir {names:?}")
        }
        Ok(_) => format!(
            "file {:?}",
            String::from_utf8_lossy(&fs::read(account).unwrap())
        ),
    }
}

fn rustc() -> PathBuf {
    let output = Command::new("where.exe")
        .arg("rustc.exe")
        .output()
        .expect("where.exe runs");
    let found = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(PathBuf::from)
        .find(|path| path.is_file());
    found.unwrap_or_else(|| {
        let home = env::var_os("USERPROFILE").expect("USERPROFILE");
        PathBuf::from(home).join(".cargo/bin/rustc.exe")
    })
}

/// The synthetic direct executable used by the CPU-budget acceptance: compiled
/// once per test binary with the toolchain that already builds this test, so no
/// product entry point and no external dependency is involved.
fn consumer() -> &'static Path {
    static CONSUMER: OnceLock<PathBuf> = OnceLock::new();
    CONSUMER.get_or_init(|| {
        let root = tempfile::Builder::new()
            .prefix("cpu-budget-consumer-")
            .tempdir()
            .unwrap()
            .keep();
        let source = root.join("cpu_budget_consumer.rs");
        fs::write(&source, include_str!("fixtures/cpu_budget_consumer.rs")).unwrap();
        let executable = root.join("cpu-budget-consumer.exe");
        let compiled = Command::new(rustc())
            .arg(&source)
            .arg("--edition=2024")
            .arg("-o")
            .arg(&executable)
            .current_dir(&root)
            .stdin(Stdio::null())
            .output()
            .expect("rustc runs");
        assert!(
            compiled.status.success(),
            "cpu budget consumer fixture did not compile: {}",
            String::from_utf8_lossy(&compiled.stderr)
        );
        assert!(executable.is_file(), "{}", executable.display());
        executable
    })
}

/// One synthetic consumer launch: artifact directory, start log and the
/// `HARNESS_CPU_FIXTURE_*` environment the fixture reads instead of arguments.
struct Consumer {
    directory: PathBuf,
    starts: PathBuf,
}

impl Consumer {
    fn new(root: &Path, name: &str) -> Self {
        let directory = root.join(format!("consumer-{name}"));
        fs::create_dir_all(&directory).unwrap();
        Self {
            starts: directory.join("starts.txt"),
            directory,
        }
    }

    fn configure(
        &self,
        command: &mut Command,
        job: &str,
        threads: u64,
        spin_ms: u64,
        leaf: bool,
        exit: i32,
    ) {
        command
            .env("HARNESS_CPU_FIXTURE_DIR", &self.directory)
            .env("HARNESS_CPU_FIXTURE_JOB", job)
            .env("HARNESS_CPU_FIXTURE_THREADS", threads.to_string())
            .env("HARNESS_CPU_FIXTURE_SPIN_MS", spin_ms.to_string())
            .env("HARNESS_CPU_FIXTURE_LEAF", if leaf { "1" } else { "0" })
            .env("HARNESS_CPU_FIXTURE_EXIT", exit.to_string())
            .env("HARNESS_CPU_FIXTURE_STARTS", &self.starts);
    }

    fn tree(&self) -> Value {
        receipt(&self.directory.join("tree.json"))
    }

    fn leaf(&self) -> Value {
        receipt(&self.directory.join("leaf.json"))
    }

    fn starts(&self) -> Vec<String> {
        fs::read_to_string(&self.starts)
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }
}

#[test]
fn admitted_session_covers_payload_and_grandchildren_under_a_lowered_test_rate() {
    let f = Fixture::new();
    let cpus = host_logical_processors();
    let threads = spinner_threads(cpus, TEST_RATE);
    let spin_ms = 3000;
    // One explicit isolated account directory, established at a lowered rate
    // before the session starts: the launcher must join this exact object.
    let budget = SharedCpuBudget::acquire(&f.account, TEST_RATE).unwrap();
    let name = budget.name().to_owned();
    assert_eq!(
        budget.snapshot().unwrap().cpu_rate,
        cpu_rate_units(TEST_RATE)
    );
    f.register_consumer(consumer());
    let payload = Consumer::new(f.root.path(), "admitted");
    let mut command = f.command();
    command.env(CPU_PERCENT_ENV, TEST_RATE.to_string());
    payload.configure(&mut command, &name, threads, spin_ms, true, 21);
    let before = budget.snapshot().unwrap();
    let started = Instant::now();
    let output = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    let elapsed = started.elapsed();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert_eq!(
        output.status.code(),
        Some(21),
        "exit status must survive the ordered spawn: {stderr}"
    );
    assert!(
        output.stdout.is_empty(),
        "the account budget must not write to stdout: {:?}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        !stderr.contains(CAP_WARNING),
        "an admitted launch must not warn: {stderr}"
    );
    // The direct executable and its grandchild each read their own kernel
    // membership in the named account object at the start of their execution.
    let tree = payload.tree();
    let leaf = payload.leaf();
    assert_eq!(tree["role"], json!("tree"), "{tree}");
    assert_eq!(leaf["role"], json!("leaf"), "{leaf}");
    assert_eq!(tree["in_shared"], json!(true), "{tree}");
    assert_eq!(leaf["in_shared"], json!(true), "{leaf}");
    assert_eq!(tree["in_any_job"], json!(true), "{tree}");
    assert_eq!(leaf["in_any_job"], json!(true), "{leaf}");
    assert_eq!(tree["child"], leaf["pid"], "{tree} {leaf}");
    let starts = payload.starts();
    assert_eq!(starts.len(), 2, "one start per payload process: {starts:?}");
    assert!(starts[0].starts_with("tree "), "{starts:?}");
    assert!(starts[1].starts_with("leaf "), "{starts:?}");
    // Declared bound: the group may consume the lowered share of total host CPU
    // capacity over the measured wall clock (rate * logical processors *
    // elapsed), with 25% slack for the kernel rate cycle, timer granularity and
    // sampling. Demand is the two single-threaded spinners' unthrottled CPU
    // seconds, which must stay far above the allowance for this to prove
    // anything at all.
    let snapshot = budget.snapshot().unwrap();
    let measured = snapshot
        .cpu_time
        .saturating_sub(before.cpu_time)
        .as_secs_f64();
    let allowance = TEST_RATE / 100.0 * f64::from(cpus) * elapsed.as_secs_f64();
    let demand = 2.0 * threads as f64 * (spin_ms as f64 / 1000.0);
    // Measurement evidence for the run log: rate, aggregate consumption,
    // declared allowance and unthrottled demand over the observed wall clock.
    eprintln!(
        "cpu budget evidence: rate={TEST_RATE}% cpus={cpus} threads_per_process={threads} elapsed={:.2}s measured_cpu={measured:.3}s allowance={allowance:.3}s demand={demand:.3}s",
        elapsed.as_secs_f64()
    );
    assert!(
        demand > allowance * 2.0,
        "workload demand {demand}s must exceed the allowance {allowance}s"
    );
    assert!(
        measured <= allowance * 1.25 + 0.05,
        "measured aggregate {measured}s exceeded the lowered ceiling: allowance {allowance}s over {elapsed:?}"
    );
    assert!(
        measured < demand * 0.5,
        "measured aggregate {measured}s shows no throttling against demand {demand}s"
    );
    assert_eq!(snapshot.cpu_rate, cpu_rate_units(TEST_RATE));
    assert!(snapshot.cpu_hard_cap);
    assert!(
        wait_for_empty(&budget),
        "the isolated account still holds members after the session exited"
    );
}

/// One ordinary native session launch of the registered upstream fixture.
/// `failure` selects the CPU-admission fault the test injects.
fn launch_session(f: &Fixture, args: &[&str], failure: Option<&str>) -> (i32, Value, String) {
    let mut command = f.command();
    match failure {
        None => {}
        Some("ceiling") => {
            command.env(CPU_PERCENT_ENV, "eighty");
        }
        Some("storage") => {
            command.env(CPU_ACCOUNT_ENV, RELATIVE_ACCOUNT);
        }
        Some("account") => {}
        Some(other) => panic!("unknown failure mode {other}"),
    }
    let mut child = command
        .args(args)
        .env("HARNESS_LAUNCH_FIXTURE_MODE", "nonzero")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all("первая строка\nsecond line\n".as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let report = serde_json::from_str(&stdout).unwrap_or_else(|error| {
        panic!("stdout is not the payload's own report ({error}): {stdout:?}")
    });
    (
        output.status.code().unwrap_or(-1),
        report,
        String::from_utf8(output.stderr).unwrap(),
    )
}

#[test]
fn cpu_admission_failure_warns_once_and_preserves_native_inputs_and_exit() {
    let args = [
        "exec",
        "",
        "проверка \"кавычки\"",
        "trailing\\",
        "$() `literal` ; &",
    ];
    for mode in ["ceiling", "storage", "account"] {
        let f = Fixture::new();
        let (code, admitted, stderr) = launch_session(&f, &args, None);
        assert_eq!(code, 19, "{mode}: {stderr}");
        assert!(
            !stderr.contains(CAP_WARNING),
            "{mode}: the admitted reference launch warned: {stderr}"
        );
        match mode {
            // An unusable requested ceiling must not silently become another
            // policy, an unusable account location stays unusable, and an
            // unusable account path stays unusable.
            "ceiling" => {}
            "storage" => {}
            "account" => {
                fs::remove_dir_all(&f.account).unwrap();
                fs::write(&f.account, b"not a directory").unwrap();
            }
            _ => unreachable!(),
        }
        let account_before = account_state(&f.account);
        let (code, failed_open, stderr) = launch_session(&f, &args, Some(mode));
        assert_eq!(code, 19, "{mode}: {stderr}");
        assert_eq!(
            failed_open, admitted,
            "{mode}: the degraded launch changed the payload's arguments, cwd, streams or environment"
        );
        // One visible warning on the diagnostic channel, naming the requested
        // ceiling, the failed stage, the cause, the scope and the recovery.
        assert_eq!(
            stderr.matches(CAP_WARNING).count(),
            1,
            "{mode}: exactly one fallback notice: {stderr}"
        );
        let requested = match mode {
            "ceiling" => "requested ceiling eighty% of host CPU",
            "storage" | "account" => "requested ceiling 75% of host CPU",
            _ => unreachable!(),
        };
        assert!(stderr.contains(requested), "{mode}: {stderr}");
        let stage = match mode {
            "ceiling" => "failed stage: ceiling configuration",
            "storage" => "failed stage: account storage",
            "account" => "failed stage: budget admission",
            _ => unreachable!(),
        };
        for marker in [stage, "cause: ", "scope: ", "recovery: ", "upstream stderr"] {
            assert!(
                stderr.contains(marker),
                "{mode}: missing {marker:?}: {stderr}"
            );
        }
        assert!(
            !stderr.contains("codex-harness: shared harness unavailable"),
            "{mode}: the shared checkout is intact here: {stderr}"
        );
        // Fail-open persists nothing: no disabled default, no replaced account
        // path, and no change to the state a later launch reads.
        assert_eq!(
            account_state(&f.account),
            account_before,
            "{mode}: the degraded launch changed machine-local budget state"
        );
        assert!(
            !f.root.path().join(RELATIVE_ACCOUNT).exists(),
            "{mode}: a relative account location was created"
        );
    }
}

#[test]
fn cpu_budget_conflict_fails_open_for_one_session_and_keeps_the_peer_allowance() {
    let f = Fixture::new();
    let cpus = host_logical_processors();
    let threads = spinner_threads(cpus, TEST_RATE);
    let budget = SharedCpuBudget::acquire(&f.account, TEST_RATE).unwrap();
    let name = budget.name().to_owned();
    let account_before = account_state(&f.account);
    f.register_consumer(consumer());
    // Peer session: admitted into the same lowered account allowance.
    let peer = Consumer::new(f.root.path(), "peer");
    let mut peer_command = f.command();
    peer.configure(&mut peer_command, &name, threads, 30_000, true, 0);
    peer_command.env(CPU_PERCENT_ENV, TEST_RATE.to_string());
    let mut peer_child = peer_command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let peer_deadline = Duration::from_secs(30);
    wait_for_path(&peer.directory.join("tree.json"), peer_deadline);
    wait_for_path(&peer.directory.join("leaf.json"), peer_deadline);
    assert_eq!(peer.tree()["in_shared"], json!(true), "{:?}", peer.tree());
    assert_eq!(peer.leaf()["in_shared"], json!(true), "{:?}", peer.leaf());
    // A second session asks for the installed default ceiling while the account
    // already carries the lowered one. The account budget is preserved, so this
    // launch must warn and start outside the group.
    let failing = Consumer::new(f.root.path(), "conflict");
    let mut failing_command = f.command();
    failing.configure(&mut failing_command, &name, 1, 0, false, 23);
    let output = failing_command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert_eq!(output.status.code(), Some(23), "{stderr}");
    assert!(
        output.stdout.is_empty(),
        "the fallback notice must not reach stdout: {:?}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(stderr.matches(CAP_WARNING).count(), 1, "{stderr}");
    assert!(
        stderr.contains("requested ceiling 75% of host CPU"),
        "{stderr}"
    );
    for marker in [
        "failed stage: budget admission",
        "cause: ",
        "scope: ",
        "recovery: ",
    ] {
        assert!(stderr.contains(marker), "missing {marker:?}: {stderr}");
    }
    // Exactly one payload start, outside the account group but still inside its
    // own session Job: fallback changes CPU admission, not containment.
    let starts = failing.starts();
    assert_eq!(starts.len(), 1, "one launch, one start: {starts:?}");
    let tree = failing.tree();
    assert_eq!(tree["in_shared"], json!(false), "{tree}");
    assert_eq!(tree["in_any_job"], json!(true), "{tree}");
    // The peer keeps its membership, ceiling and settings, and the account
    // state a later launch reads is unchanged.
    let snapshot = budget.snapshot().unwrap();
    assert_eq!(
        snapshot.cpu_rate,
        cpu_rate_units(TEST_RATE),
        "the degraded launch changed the peer ceiling"
    );
    assert!(snapshot.cpu_hard_cap);
    assert!(
        wait_for_members(&budget, 2, Duration::from_secs(15)),
        "peer members left the group: {snapshot:?}"
    );
    assert!(
        peer_child.try_wait().unwrap().is_none(),
        "the peer session ended before the assertions"
    );
    assert_eq!(account_state(&f.account), account_before);
    // Abnormal peer loss still reaps its own tree through its lifecycle Job.
    peer_child.kill().unwrap();
    let _ = peer_child.wait_with_output().unwrap();
    assert!(
        wait_for_empty(&budget),
        "the peer session tree outlived its launcher"
    );
}

#[test]
fn unavailable_checkout_still_admits_the_payload_into_the_account_budget() {
    let f = Fixture::new();
    let budget = SharedCpuBudget::acquire(&f.account, TEST_RATE).unwrap();
    let name = budget.name().to_owned();
    f.register_consumer(consumer());
    // The registered build stays launchable without its shared checkout, and
    // the CPU cap must still be attempted for that launch.
    fs::remove_dir_all(&f.source).unwrap();
    let payload = Consumer::new(f.root.path(), "degraded");
    let mut command = f.command();
    command.env(CPU_PERCENT_ENV, TEST_RATE.to_string());
    payload.configure(&mut command, &name, 2, 300, true, 21);
    let output = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert_eq!(output.status.code(), Some(21), "{stderr}");
    assert!(
        stderr.contains(
            "shared harness unavailable; launching registered Codex without harness overrides"
        ),
        "{stderr}"
    );
    assert!(
        !stderr.contains(CAP_WARNING),
        "the CPU cap is independent of checkout availability: {stderr}"
    );
    assert_eq!(
        payload.tree()["in_shared"],
        json!(true),
        "{:?}",
        payload.tree()
    );
    assert_eq!(
        payload.leaf()["in_shared"],
        json!(true),
        "{:?}",
        payload.leaf()
    );
    let starts = payload.starts();
    assert_eq!(starts.len(), 2, "{starts:?}");
}

/// Re-exec entry for a caller that must already be inside the shared allowance.
/// A normal suite run has no spec and returns immediately.
#[test]
fn uncapped_session_caller() {
    let Ok(spec_path) = env::var("HARNESS_UNCAPPED_SPEC") else {
        return;
    };
    let spec: Value = serde_json::from_slice(&fs::read(&spec_path).unwrap()).unwrap();
    let job = spec["job"].as_str().unwrap();
    assert!(
        join_named_job(job),
        "the uncapped caller is not a kernel member of {job}"
    );
    let mut command = Command::new(spec["program"].as_str().unwrap());
    command
        .args(
            spec["args"]
                .as_array()
                .unwrap()
                .iter()
                .map(|arg| arg.as_str().unwrap()),
        )
        .current_dir(spec["cwd"].as_str().unwrap())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (name, value) in spec["env"].as_object().unwrap() {
        match value {
            Value::Null => {
                command.env_remove(name);
            }
            Value::String(text) => {
                command.env(name, text);
            }
            other => panic!("unsupported env value {other}"),
        }
    }
    let mut child = command.spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(spec["stdin"].as_str().unwrap_or("").as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    let result = json!({
        "code": output.status.code(),
        "stdout": String::from_utf8_lossy(&output.stdout),
        "stderr": String::from_utf8_lossy(&output.stderr),
        "caller_in_job": true,
    });
    fs::write(
        spec["result"].as_str().unwrap(),
        serde_json::to_vec(&result).unwrap(),
    )
    .unwrap();
}

fn join_named_job(name: &str) -> bool {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn OpenJobObjectW(access: u32, inherit: i32, name: *const u16) -> *mut std::ffi::c_void;
        fn AssignProcessToJobObject(
            job: *mut std::ffi::c_void,
            process: *mut std::ffi::c_void,
        ) -> i32;
        fn IsProcessInJob(
            process: *mut std::ffi::c_void,
            job: *mut std::ffi::c_void,
            result: *mut i32,
        ) -> i32;
        fn GetCurrentProcess() -> *mut std::ffi::c_void;
        fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
    }
    let mut wide: Vec<u16> = name.encode_utf16().collect();
    wide.push(0);
    unsafe {
        let job = OpenJobObjectW(0x0005, 0, wide.as_ptr());
        if job.is_null() {
            return false;
        }
        let assigned = AssignProcessToJobObject(job, GetCurrentProcess());
        let mut member = 0;
        let queried = IsProcessInJob(GetCurrentProcess(), job, &mut member);
        CloseHandle(job);
        assigned != 0 && queried != 0 && member != 0
    }
}

fn run_capped_caller(spec: &Value) -> Value {
    let spec_path = spec["spec_path"].as_str().unwrap();
    fs::write(spec_path, serde_json::to_vec(spec).unwrap()).unwrap();
    let output = Command::new(env::current_exe().unwrap())
        .args(["--exact", "uncapped_session_caller", "--test-threads=1"])
        .env("HARNESS_UNCAPPED_SPEC", spec_path)
        .env_remove(CPU_PERCENT_ENV)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "capped caller failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&fs::read(spec["result"].as_str().unwrap()).unwrap()).unwrap()
}

#[test]
fn uncapped_session_escapes_a_capped_caller_without_lifting_peers() {
    use harness_core::process_service::{self, SharedCpuCoverage};
    use std::collections::BTreeMap;
    let f = Fixture::new();
    let budget = SharedCpuBudget::acquire(&f.account, SHARED_CPU_PERCENT).unwrap();
    let job_name = budget.name().to_owned();
    let rate = budget.snapshot().unwrap().cpu_rate;
    f.register_consumer(consumer());
    let peer = Consumer::new(f.root.path(), "peer");
    let mut peer_command = f.command();
    peer_command.env(CPU_PERCENT_ENV, SHARED_CPU_PERCENT.to_string());
    peer.configure(&mut peer_command, &job_name, 1, 20_000, true, 0);
    let mut peer_child = peer_command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    wait_for_path(&peer.directory.join("tree.json"), Duration::from_secs(30));
    let peer_tree = peer.tree();
    assert_eq!(peer_tree["in_shared"], json!(true), "{peer_tree}");
    let peer_pid = peer_tree["pid"].as_u64().unwrap() as u32;

    let service_root = f.root.path().join("service");
    fs::create_dir_all(&service_root).unwrap();
    let mut environment = BTreeMap::new();
    environment.insert("SystemRoot".into(), env::var("SystemRoot").unwrap());
    environment.insert(CPU_ACCOUNT_ENV.into(), f.account.display().to_string());
    // spawn's coverage check reads this process environment. Point it at the
    // fixture so the check does not query the machine account.
    unsafe { env::set_var(CPU_ACCOUNT_ENV, &f.account) };
    let service = process_service::spawn(
        Path::new(env!("CARGO_BIN_EXE_harness-service-fixture")),
        &service_root,
        vec!["serve".into()],
        environment,
        Deadline::after(Duration::from_secs(30)).unwrap(),
        &Cancellation::default(),
    )
    .unwrap();
    unsafe { env::remove_var(CPU_ACCOUNT_ENV) };
    let coverage = process_service::shared_cpu_coverage(&service, Some(&f.account));
    let service_log = fs::read_to_string(service_root.join("service.log")).unwrap_or_default();
    assert!(
        matches!(coverage, SharedCpuCoverage::Covered { .. }),
        "shared service was not admitted before the exception: {coverage:?}\n{service_log}"
    );

    let exception = Consumer::new(f.root.path(), "exception");
    let spec_path = f.root.path().join("caller-spec.json");
    let result_path = f.root.path().join("caller-result.json");
    let mut env = serde_json::Map::new();
    env.insert("CODEX_HOME".into(), f.home.display().to_string().into());
    env.insert(
        CPU_ACCOUNT_ENV.into(),
        f.account.display().to_string().into(),
    );
    env.insert(
        CPU_PERCENT_ENV.into(),
        SHARED_CPU_PERCENT.to_string().into(),
    );
    env.insert(
        "HARNESS_CPU_FIXTURE_DIR".into(),
        exception.directory.display().to_string().into(),
    );
    env.insert("HARNESS_CPU_FIXTURE_JOB".into(), job_name.clone().into());
    env.insert("HARNESS_CPU_FIXTURE_THREADS".into(), "1".into());
    env.insert("HARNESS_CPU_FIXTURE_SPIN_MS".into(), "200".into());
    env.insert("HARNESS_CPU_FIXTURE_LEAF".into(), "1".into());
    env.insert("HARNESS_CPU_FIXTURE_EXIT".into(), "21".into());
    env.insert(
        "HARNESS_CPU_FIXTURE_STARTS".into(),
        exception.starts.display().to_string().into(),
    );
    let result = run_capped_caller(&json!({
        "spec_path": spec_path,
        "result": result_path,
        "job": job_name,
        "program": f.launcher,
        "args": ["--harness-cpu", "uncapped"],
        "cwd": f.root.path(),
        "env": env,
        "stdin": "",
    }));
    let stderr = result["stderr"].as_str().unwrap();
    assert_eq!(result["code"], json!(21), "{stderr}");
    assert!(
        stderr.contains("explicit uncapped invocation")
            && stderr.contains("can exceed")
            && stderr.contains("75%"),
        "{stderr}"
    );
    assert!(
        !stderr.contains(CAP_WARNING),
        "an explicit exception must not be reported as a failed cap: {stderr}"
    );
    let tree = exception.tree();
    assert_eq!(tree["in_shared"], json!(false), "{tree}");
    assert_eq!(tree["in_any_job"], json!(true), "{tree}");
    let leaf = exception.leaf();
    assert_eq!(leaf["in_shared"], json!(false), "{leaf}");
    assert!(
        process_in_named_job(peer_pid, &job_name),
        "the peer session lost the shared allowance"
    );
    assert!(
        service.in_shared_cpu_budget(&budget).unwrap(),
        "the shared service left the allowance while an uncapped session ran"
    );
    assert_eq!(budget.snapshot().unwrap().cpu_rate, rate);

    f.register(json!({
        "executable": f.upstream,
        "sha256": build_identity::hash_file(&f.upstream).unwrap(),
        "package": null
    }));
    let stream_spec = f.root.path().join("stream-spec.json");
    let stream_result = f.root.path().join("stream-result.json");
    let stream = run_capped_caller(&json!({
        "spec_path": stream_spec,
        "result": stream_result,
        "job": job_name,
        "program": f.launcher,
        "args": ["--harness-cpu", "uncapped", "exec", "", "проверка \"кавычки\"", "trailing\\"],
        "cwd": f.root.path(),
        "env": {
            "CODEX_HOME": f.home.display().to_string(),
            CPU_ACCOUNT_ENV: f.account.display().to_string(),
            CPU_PERCENT_ENV: SHARED_CPU_PERCENT.to_string(),
            "HARNESS_LAUNCH_FIXTURE_MODE": "nonzero",
        },
        "stdin": "первая строка\nsecond line\n",
    }));
    let stream_stderr = stream["stderr"].as_str().unwrap();
    assert_eq!(stream["code"], json!(19), "{stream_stderr}");
    assert!(
        stream_stderr.contains("explicit uncapped invocation"),
        "{stream_stderr}"
    );
    let report: Value = serde_json::from_str(stream["stdout"].as_str().unwrap())
        .unwrap_or_else(|error| panic!("{error}: {stream}"));
    let args = report["args"].as_array().unwrap();
    assert!(
        args.ends_with(&[
            json!("exec"),
            json!(""),
            json!("проверка \"кавычки\""),
            json!("trailing\\"),
        ]),
        "{args:?}"
    );
    assert_eq!(report["stdin"], json!("первая строка\nsecond line\n"));
    assert!(
        report["cwd"]
            .as_str()
            .is_some_and(|cwd| cwd.contains("native-launch")),
        "{report}"
    );

    f.register_consumer(consumer());
    let again = Consumer::new(f.root.path(), "default-after");
    let mut again_command = f.command();
    again_command.env(CPU_PERCENT_ENV, SHARED_CPU_PERCENT.to_string());
    again.configure(&mut again_command, &job_name, 1, 200, true, 0);
    let again_output = again_command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    let again_stderr = String::from_utf8_lossy(&again_output.stderr);
    assert!(
        again_output.status.success(),
        "the next default session failed: {again_stderr}"
    );
    assert!(
        !again_stderr.contains("explicit uncapped invocation"),
        "the exception persisted: {again_stderr}"
    );
    assert_eq!(again.tree()["in_shared"], json!(true), "{:?}", again.tree());
    let _ = service.terminate(0);
    let _ = peer_child.kill();
    let _ = peer_child.wait();
}

fn process_in_named_job(pid: u32, name: &str) -> bool {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut std::ffi::c_void;
        fn OpenJobObjectW(access: u32, inherit: i32, name: *const u16) -> *mut std::ffi::c_void;
        fn IsProcessInJob(
            process: *mut std::ffi::c_void,
            job: *mut std::ffi::c_void,
            result: *mut i32,
        ) -> i32;
        fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
    }
    let mut wide: Vec<u16> = name.encode_utf16().collect();
    wide.push(0);
    unsafe {
        let process = OpenProcess(0x1000, 0, pid);
        let job = OpenJobObjectW(0x0004, 0, wide.as_ptr());
        if process.is_null() || job.is_null() {
            if !process.is_null() {
                CloseHandle(process);
            }
            if !job.is_null() {
                CloseHandle(job);
            }
            return false;
        }
        let mut member = 0;
        let queried = IsProcessInJob(process, job, &mut member);
        CloseHandle(process);
        CloseHandle(job);
        queried != 0 && member != 0
    }
}

/// Whether the pid names a process that is still running.
///
/// A payload terminated with its session Job can leave a process object that
/// remains referenced: its exit is already final and process listings no
/// longer show it, while the pid stays openable. Bare openability is therefore
/// not liveness, and the reap contract is checked on the final exit code.
fn process_alive(pid: u32) -> bool {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut std::ffi::c_void;
        fn GetExitCodeProcess(process: *mut std::ffi::c_void, code: *mut u32) -> i32;
        fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
    }
    const STILL_ACTIVE: u32 = 259;
    unsafe {
        let process = OpenProcess(0x1000, 0, pid);
        if process.is_null() {
            return false;
        }
        let mut code = 0u32;
        let read = GetExitCodeProcess(process, &mut code);
        CloseHandle(process);
        read != 0 && code == STILL_ACTIVE
    }
}

fn spawn_holding_session(
    f: &Fixture,
    payload: &Consumer,
    job: &str,
    spin_ms: u64,
) -> std::process::Child {
    let mut command = f.command();
    command.env(CPU_PERCENT_ENV, TEST_RATE.to_string());
    payload.configure(&mut command, job, 1, spin_ms, false, 0);
    let stderr = fs::File::create(payload.directory.join("launcher.stderr")).unwrap();
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(stderr)
        .spawn()
        .unwrap()
}

fn wait_admitted(payload: &Consumer) -> Value {
    let path = payload.directory.join("tree.json");
    let deadline = Instant::now() + Duration::from_secs(20);
    while !path.is_file() {
        let stderr =
            fs::read_to_string(payload.directory.join("launcher.stderr")).unwrap_or_default();
        assert!(
            Instant::now() < deadline,
            "session did not report membership: {stderr}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    payload.tree()
}

fn write_shared_cpu_policy(account: &Path, body: &[u8]) {
    fs::create_dir_all(account).unwrap();
    fs::write(account.join("shared-cpu-policy.json"), body).unwrap();
}

/// Hold one admitted bootstrap payload long enough to read the kernel rate the
/// launcher actually established, then stop it. The ownership record is checked
/// before this process joins, so the assertion cannot be satisfied by creating
/// the group here.
fn assert_bootstrap_rate(f: &Fixture, expected: f64, percent_env: Option<&str>) {
    let started = f.root.path().join(format!("policy-started-{expected}"));
    let mut command = f.command();
    if let Some(value) = percent_env {
        command.env(CPU_PERCENT_ENV, value);
    }
    let child = command
        .env("HARNESS_LAUNCH_FIXTURE_MODE", "heavy-hold")
        .env("HARNESS_HEAVY_FIXTURE_MS", "30000")
        .env("HARNESS_HEAVY_FIXTURE_STARTED", &started)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let _child = KillOnDrop(child);
    wait_for_path(&started, Duration::from_secs(30));
    let record_path = f.account.join("cpu-budget.json");
    wait_for_path(&record_path, Duration::from_secs(5));
    let record: Value = serde_json::from_slice(&fs::read(&record_path).unwrap()).unwrap();
    assert_eq!(
        record["cpu_rate"],
        json!(cpu_rate_units(expected)),
        "launcher ownership record {record}"
    );
    let budget = SharedCpuBudget::acquire(&f.account, expected).expect("launcher rate must match");
    let snapshot = budget.snapshot().unwrap();
    assert_eq!(snapshot.cpu_rate, cpu_rate_units(expected));
    assert!(snapshot.cpu_hard_cap);
    assert!(
        snapshot.active_processes >= 1,
        "the launcher was not holding the group it established: {snapshot:?}"
    );
}

struct KillOnDrop(std::process::Child);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn preserved_policy_edit_changes_bootstrap_admission() {
    let absent = Fixture::new();
    assert_bootstrap_rate(&absent, SHARED_CPU_PERCENT, None);

    let edited = Fixture::new();
    write_shared_cpu_policy(
        &edited.account,
        br#"{"schema":1,"ceiling_percent":40.0,"escape_hatch":"CODEX_HARNESS_CPU_PERCENT"}"#,
    );
    assert_bootstrap_rate(&edited, 40.0, None);

    let overridden = Fixture::new();
    write_shared_cpu_policy(
        &overridden.account,
        br#"{"schema":1,"ceiling_percent":40.0}"#,
    );
    assert_bootstrap_rate(&overridden, 20.0, Some("20"));
}

#[test]
fn malformed_policy_warns_without_substituting_bootstrap_ceiling() {
    let f = Fixture::new();
    let body = b"{\"schema\":1}";
    write_shared_cpu_policy(&f.account, body);
    let (code, report, stderr) = launch_session(&f, &["exec"], None);
    assert_eq!(code, 19, "{stderr}");
    assert!(report.get("args").is_some(), "{report}");
    assert_eq!(stderr.matches(CAP_WARNING).count(), 1, "{stderr}");
    assert!(stderr.contains("not a substitute ceiling"), "{stderr}");
    assert!(
        stderr.contains("failed stage: ceiling configuration"),
        "{stderr}"
    );
    assert!(stderr.contains("cause: "), "{stderr}");
    assert!(stderr.contains("recovery: "), "{stderr}");
    assert!(
        !stderr.contains("requested ceiling 75% of host CPU"),
        "a malformed record must not be reported as the installed default: {stderr}"
    );
    assert!(
        !f.account.join("cpu-budget.json").exists(),
        "fail-open must not establish a substitute group: {}",
        account_state(&f.account)
    );
    assert_eq!(
        fs::read(f.account.join("shared-cpu-policy.json")).unwrap(),
        body
    );
}

fn wait_child_status(child: &mut std::process::Child, stderr: &Path) -> std::process::ExitStatus {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status;
        }
        assert!(
            Instant::now() < deadline,
            "session did not exit: {}",
            fs::read_to_string(stderr).unwrap_or_default()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn session_exit_death_and_reconnect_keep_one_peer_allowance() {
    let f = Fixture::new();
    f.register_consumer(consumer());
    let budget = SharedCpuBudget::acquire(&f.account, TEST_RATE).unwrap();
    let name = budget.name().to_owned();
    let rate = cpu_rate_units(TEST_RATE);
    assert_eq!(budget.snapshot().unwrap().cpu_rate, rate);
    let peer_payload = Consumer::new(f.root.path(), "peer");
    let peer = KillOnDrop(spawn_holding_session(&f, &peer_payload, &name, 60_000));
    let peer_tree = wait_admitted(&peer_payload);
    assert_eq!(peer_tree["in_shared"], json!(true), "{peer_tree}");
    let peer_pid = peer_tree["pid"].as_u64().unwrap() as u32;
    assert!(process_in_named_job(peer_pid, &name));

    let exiting = Consumer::new(f.root.path(), "normal-exit");
    let mut exiting_child = spawn_holding_session(&f, &exiting, &name, 200);
    let exited = wait_admitted(&exiting);
    assert_eq!(exited["in_shared"], json!(true), "{exited}");
    let status = wait_child_status(
        &mut exiting_child,
        &exiting.directory.join("launcher.stderr"),
    );
    assert!(
        status.success(),
        "normal session exit changed status: {status}; {}",
        fs::read_to_string(exiting.directory.join("launcher.stderr")).unwrap_or_default()
    );
    assert!(process_in_named_job(peer_pid, &name));
    assert!(process_alive(peer_pid));
    assert_eq!(budget.snapshot().unwrap().cpu_rate, rate);
    assert_eq!(
        SharedCpuBudget::acquire(&f.account, TEST_RATE)
            .unwrap()
            .name(),
        name,
        "normal session exit created a second group"
    );

    let owner_payload = Consumer::new(f.root.path(), "control-owner");
    let mut owner = KillOnDrop(spawn_holding_session(&f, &owner_payload, &name, 60_000));
    let owner_tree = wait_admitted(&owner_payload);
    assert_eq!(owner_tree["in_shared"], json!(true), "{owner_tree}");
    let owner_pid = owner_tree["pid"].as_u64().unwrap() as u32;
    assert!(process_in_named_job(owner_pid, &name));
    // TerminateProcess on the launcher: its Drop and session cleanup do not run.
    owner.0.kill().unwrap();
    let killed = wait_child_status(
        &mut owner.0,
        &owner_payload.directory.join("launcher.stderr"),
    );
    assert!(
        !killed.success(),
        "abrupt launcher death looked like a clean exit: {killed}"
    );
    let reap_deadline = Instant::now() + Duration::from_secs(5);
    while process_alive(owner_pid) && Instant::now() < reap_deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        !process_alive(owner_pid),
        "abrupt launcher death must reap that session's payload"
    );
    assert!(
        process_in_named_job(peer_pid, &name) && process_alive(peer_pid),
        "peer session lost its allowance when the other control owner died"
    );
    assert_eq!(budget.snapshot().unwrap().cpu_rate, rate);
    assert!(budget.snapshot().unwrap().cpu_hard_cap);
    assert!(!budget.snapshot().unwrap().kill_on_close);
    let rejoined = SharedCpuBudget::acquire(&f.account, TEST_RATE).unwrap();
    assert_eq!(rejoined.name(), name);
    let wrong = SharedCpuBudget::acquire(&f.account, 50.0).unwrap_err();
    assert!(wrong.to_string().contains("already established"), "{wrong}");
    assert_eq!(budget.snapshot().unwrap().cpu_rate, rate);

    let again = Consumer::new(f.root.path(), "reconnect");
    let reconnect = KillOnDrop(spawn_holding_session(&f, &again, &name, 60_000));
    let again_tree = wait_admitted(&again);
    assert_eq!(again_tree["in_shared"], json!(true), "{again_tree}");
    let again_pid = again_tree["pid"].as_u64().unwrap() as u32;
    assert!(process_in_named_job(again_pid, &name));
    assert!(process_in_named_job(peer_pid, &name));
    assert_eq!(budget.snapshot().unwrap().cpu_rate, rate);
    assert_eq!(
        SharedCpuBudget::acquire(&f.account, TEST_RATE)
            .unwrap()
            .name(),
        name
    );
    drop(reconnect);
    drop(peer);
}

struct RestoredAccountEnv(Option<std::ffi::OsString>);
impl RestoredAccountEnv {
    fn set(account: &Path) -> Self {
        let previous = env::var_os(CPU_ACCOUNT_ENV);
        unsafe { env::set_var(CPU_ACCOUNT_ENV, account) };
        Self(previous)
    }
}
impl Drop for RestoredAccountEnv {
    fn drop(&mut self) {
        unsafe {
            match self.0.take() {
                Some(value) => env::set_var(CPU_ACCOUNT_ENV, value),
                None => env::remove_var(CPU_ACCOUNT_ENV),
            }
        }
    }
}

#[test]
fn incomplete_activation_names_outside_session_route_and_keeps_peer() {
    let f = Fixture::new();
    f.register_consumer(consumer());
    let budget = SharedCpuBudget::acquire(&f.account, TEST_RATE).unwrap();
    let name = budget.name().to_owned();
    let rate = cpu_rate_units(TEST_RATE);
    let policy =
        br#"{"schema":1,"ceiling_percent":75.0,"escape_hatch":"CODEX_HARNESS_CPU_PERCENT"}"#;
    write_shared_cpu_policy(&f.account, policy);
    let _env = RestoredAccountEnv::set(&f.account);
    let peer_payload = Consumer::new(f.root.path(), "covered-peer");
    let peer = KillOnDrop(spawn_holding_session(&f, &peer_payload, &name, 60_000));
    let peer_tree = wait_admitted(&peer_payload);
    let peer_pid = peer_tree["pid"].as_u64().unwrap() as u32;
    assert!(process_in_named_job(peer_pid, &name));
    let outside_dir = f.root.path().join("outside-route");
    fs::create_dir_all(&outside_dir).unwrap();
    let mut outside = Command::new(consumer())
        .env("HARNESS_CPU_FIXTURE_DIR", &outside_dir)
        .env("HARNESS_CPU_FIXTURE_ROLE", "leaf")
        .env("HARNESS_CPU_FIXTURE_LEAF", "0")
        .env("HARNESS_CPU_FIXTURE_THREADS", "1")
        .env("HARNESS_CPU_FIXTURE_SPIN_MS", "60000")
        .env("HARNESS_CPU_FIXTURE_EXIT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    wait_for_path(&outside_dir.join("leaf.json"), Duration::from_secs(20));
    let outside_tree = receipt(&outside_dir.join("leaf.json"));
    let outside_pid = outside_tree["pid"].as_u64().unwrap();
    let report = harness_core::core_install::inspect_cpu_policy(&[consumer().to_path_buf()]);
    assert_eq!(report.activation, "incomplete", "{report:?}");
    assert_eq!(report.action, "inspected");
    assert!(!report.wrote_policy);
    assert_eq!(report.model_calls, 0);
    assert_eq!(report.measured_consumption, "not-sampled");
    assert!(
        report
            .restart_boundary
            .contains(&format!("pid {outside_pid}")),
        "{}",
        report.restart_boundary
    );
    assert!(
        report.restart_boundary.contains("does not terminate"),
        "{}",
        report.restart_boundary
    );
    assert!(
        !report.restart_boundary.contains(&format!("pid {peer_pid}")),
        "covered session was reported uncovered: {}",
        report.restart_boundary
    );
    assert!(
        report.kernel_configuration.contains(&name)
            && report
                .kernel_configuration
                .contains(&format!("cpu_rate {rate}")),
        "{}",
        report.kernel_configuration
    );
    assert!(process_in_named_job(peer_pid, &name) && process_alive(peer_pid));
    assert_eq!(budget.snapshot().unwrap().cpu_rate, rate);
    assert!(outside.try_wait().unwrap().is_none());
    assert_eq!(
        fs::read(f.account.join("shared-cpu-policy.json")).unwrap(),
        policy
    );
    assert_eq!(
        SharedCpuBudget::acquire(&f.account, TEST_RATE)
            .unwrap()
            .name(),
        name
    );
    let _ = outside.kill();
    let _ = outside.wait();
    drop(peer);
}

fn compile_rust(dir: &Path, name: &str, source: &str) -> PathBuf {
    fs::create_dir_all(dir).unwrap();
    let input = dir.join(format!("{name}.rs"));
    let output = dir.join(format!("{name}.exe"));
    fs::write(&input, source).unwrap();
    let log = dir.join(format!("{name}.err"));
    let status = Command::new(rustc())
        .arg(&input)
        .args(["--edition=2024", "-o"])
        .arg(&output)
        .stderr(fs::File::create(&log).unwrap())
        .stdout(Stdio::null())
        .status()
        .unwrap();
    assert!(
        status.success(),
        "{name} compile failed: {}",
        fs::read_to_string(&log).unwrap_or_default()
    );
    output
}

fn rollback_upstream(version: &str) -> String {
    format!(
        r#"fn main() {{
    let args: Vec<String> = std::env::args().skip(1).collect();
    let view: Vec<&str> = args.iter().map(String::as_str).collect();
    match view.as_slice() {{
        ["--version"] => println!("codex-cli {version}"),
        ["--help"] => println!("usage --profile <name>.config.toml"),
        ["features", "disable", name] => {{
            let home = std::path::PathBuf::from(std::env::var_os("CODEX_HOME").expect("home"));
            let path = home.join("config.toml");
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            let mut lines: Vec<String> = text
                .lines()
                .filter(|line| !line.trim().starts_with(&format!("{{name}} ")))
                .map(str::to_owned)
                .collect();
            lines.push(format!("{{name}} = false"));
            std::fs::write(&path, lines.join("\n") + "\n").unwrap();
        }}
        ["features", "list"] => {{
            println!("hooks stable false");
            println!("code_mode stable true");
        }}
        _ => std::process::exit(1),
    }}
}}
"#
    )
}

const ROLLBACK_LAUNCHER: &str = r#"fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let view: Vec<&str> = args.iter().map(String::as_str).collect();
    match view.as_slice() {
        ["--retained-session"] => std::thread::sleep(std::time::Duration::from_secs(180)),
        ["debug", "prompt-input"] => {
            let home = std::path::PathBuf::from(std::env::var_os("CODEX_HOME").expect("home"));
            let instructions = std::fs::read_to_string(home.join("AGENTS.md")).unwrap_or_default();
            let permissions = "Filesystem sandboxing defines which files can be read or written. sandbox_mode is danger-full-access. Approval policy is currently never.";
            println!(
                "[{{\"type\":\"message\",\"text\":\"{}\"}},{{\"type\":\"message\",\"text\":\"{}\"}}]",
                escape(&instructions),
                escape(permissions)
            );
        }
        _ => std::process::exit(1),
    }
}
fn escape(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}
"#;

fn rollback_cli(cpu: &Path, heavy: &Path, args: &[std::ffi::OsString]) -> std::process::Output {
    let output = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .args(args)
        .env(CPU_ACCOUNT_ENV, cpu)
        .env("CODEX_HARNESS_HEAVY_ACCOUNT", heavy)
        .env_remove(CPU_PERCENT_ENV)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{} {}\n{}",
        args.first()
            .map(|arg| arg.to_string_lossy())
            .unwrap_or_default(),
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

#[test]
fn rollback_preserves_live_peer_and_reports_coverage_loss() {
    let root = tempfile::Builder::new()
        .prefix("cpu-rollback-Юникод-")
        .tempdir()
        .unwrap();
    // Anchor on the ordinary long form: build agents hand out a short 8.3
    // temp prefix while installation records canonical ownership strings.
    let base = root
        .path()
        .canonicalize()
        .unwrap()
        .to_string_lossy()
        .strip_prefix(r"\\?\")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.path().canonicalize().unwrap());
    let source = base.join("source");
    let build = base.join("build");
    let home = base.join("home");
    let user = base.join("user");
    let cpu = base.join("cpu-account");
    let heavy = base.join("heavy-account");
    for dir in [
        source.join("global/agents"),
        source.join("skills/one"),
        source.join("crates/one/src"),
        source.join("crates/harness-rtk/src"),
        build.clone(),
        home.clone(),
        user.clone(),
    ] {
        fs::create_dir_all(dir).unwrap();
    }
    for file in ["Cargo.toml", "Cargo.lock", "crates/one/src/lib.rs"] {
        fs::write(source.join(file), b"fixture\n").unwrap();
    }
    fs::write(source.join("crates/harness-rtk/src/lib.rs"), b"fixture\n").unwrap();
    fs::write(
        source.join("global/profile.toml"),
        "approval_policy = 'never'\nsandbox_mode = 'danger-full-access'\nmodel = 'gpt-6-astra'\n",
    )
    .unwrap();
    fs::write(
        source.join("global/instructions.md"),
        "Owned native core acceptance. Preserve foreign data.\n",
    )
    .unwrap();
    for file in ["global/hooks.json", "global/token-hooks.json"] {
        fs::write(source.join(file), b"{}\n").unwrap();
    }
    fs::write(
        source.join("skills/one/SKILL.md"),
        "---\nname: one\ndescription: Owned acceptance skill.\n---\nPreserve foreign data.\n",
    )
    .unwrap();
    fs::write(
        source.join("global/kit.json"),
        serde_json::to_vec(&json!({
            "schema": 1,
            "profile_name": "harness",
            "profile": "global/profile.toml",
            "instructions": "global/instructions.md",
            "skills": "skills",
            "agents": "global/agents",
            "hooks": "global/hooks.json",
            "token_hooks": "global/token-hooks.json"
        }))
        .unwrap(),
    )
    .unwrap();
    let compile_root = root.path().join("compile");
    let launcher = compile_rust(&compile_root, "launcher", ROLLBACK_LAUNCHER);
    fs::copy(&launcher, build.join("codex.exe")).unwrap();
    for name in BINARIES {
        let path = build.join(name);
        if !path.exists() {
            fs::write(&path, name.as_bytes()).unwrap();
        }
    }
    let record = BuildRecord {
        schema: SCHEMA,
        source_root: source.clone(),
        source: build_identity::source_identity(&source).unwrap(),
        rustc: "fixture".into(),
        cargo: "fixture".into(),
        target: "x86_64-pc-windows-msvc".into(),
        profile: "release".into(),
        binaries: BINARIES
            .iter()
            .map(|name| {
                (
                    (*name).to_owned(),
                    build_identity::hash_file(&build.join(name)).unwrap(),
                )
            })
            .collect(),
    };
    fs::write(
        build.join("build.json"),
        serde_json::to_vec(&record).unwrap(),
    )
    .unwrap();
    let upstream = compile_rust(&compile_root, "upstream", &rollback_upstream("0.153.4"));
    let install = vec![
        std::ffi::OsString::from("install"),
        "--core-only".into(),
        "--source".into(),
        source.clone().into(),
        "--build".into(),
        build.clone().into(),
        "--codex-home".into(),
        home.clone().into(),
        "--user-home".into(),
        user.clone().into(),
        "--dependency-user-home".into(),
        user.clone().into(),
        "--upstream".into(),
        upstream.clone().into(),
        "--path-scope".into(),
        "process".into(),
        "--timeout-seconds".into(),
        "90".into(),
    ];
    let installed = rollback_cli(&cpu, &heavy, &install);
    let installed: Value = serde_json::from_slice(&installed.stdout).unwrap();
    assert_eq!(installed["status"], "connected", "{installed}");
    let edited =
        br#"{"schema":1,"ceiling_percent":40.0,"escape_hatch":"CODEX_HARNESS_CPU_PERCENT"}"#;
    fs::create_dir_all(&cpu).unwrap();
    fs::write(cpu.join("shared-cpu-policy.json"), edited).unwrap();
    let budget = SharedCpuBudget::acquire(&cpu, SHARED_CPU_PERCENT).unwrap();
    let name = budget.name().to_owned();
    assert_eq!(budget.snapshot().unwrap().cpu_rate, 7500);
    let peer_marker = root.path().join("peer.json");
    let peer_job =
        harness_core::process::Job::new(harness_core::process::Limits::default()).unwrap();
    let mut peer_spec = CommandSpec::new(env!("CARGO_BIN_EXE_harness-process-fixture"));
    peer_spec.args = vec!["hold".into(), peer_marker.clone().into()];
    let peer = budget.spawn(&peer_job, &peer_spec).unwrap();
    wait_for_path(&peer_marker, Duration::from_secs(10));
    assert!(budget.contains(&peer).unwrap());
    let mut sleeper = Command::new(build.join("codex.exe"))
        .arg("--retained-session")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let sleeper_pid = sleeper.id();
    let disconnect_args = [
        std::ffi::OsString::from("disconnect"),
        "--core-only".into(),
        "--codex-home".into(),
        home.clone().into(),
        "--user-home".into(),
        user.clone().into(),
        "--dependency-user-home".into(),
        user.into(),
    ];
    let mut preview_args = disconnect_args.to_vec();
    preview_args.push("--preview".into());
    let preview = rollback_cli(&cpu, &heavy, &preview_args);
    assert!(
        !String::from_utf8_lossy(&preview.stderr).contains("no longer provided"),
        "{}",
        String::from_utf8_lossy(&preview.stderr)
    );
    assert!(budget.contains(&peer).unwrap());
    assert_eq!(budget.snapshot().unwrap().cpu_rate, 7500);
    assert!(sleeper.try_wait().unwrap().is_none());
    let disconnected = rollback_cli(&cpu, &heavy, &disconnect_args);
    let stderr = String::from_utf8_lossy(&disconnected.stderr);
    assert!(
        stderr.contains("default coverage is no longer provided"),
        "{stderr}"
    );
    assert!(stderr.contains("not-sampled"), "{stderr}");
    assert!(
        stderr.contains(&sleeper_pid.to_string()),
        "coverage-loss report omitted the live route: {stderr}"
    );
    assert!(
        stderr.contains("cpu_rate 7500"),
        "coverage-loss report omitted the live kernel rate: {stderr}"
    );
    assert_eq!(
        fs::read(cpu.join("shared-cpu-policy.json")).unwrap(),
        edited
    );
    assert!(budget.contains(&peer).unwrap());
    assert_eq!(budget.snapshot().unwrap().cpu_rate, 7500);
    assert!(budget.snapshot().unwrap().cpu_hard_cap && !budget.snapshot().unwrap().kill_on_close);
    assert_eq!(
        SharedCpuBudget::acquire(&cpu, SHARED_CPU_PERCENT)
            .unwrap()
            .name(),
        name,
        "rollback created a second allowance"
    );
    assert!(peer.is_running().unwrap());
    assert!(
        sleeper.try_wait().unwrap().is_none(),
        "rollback stopped live work"
    );
    let _ = sleeper.kill();
    let _ = sleeper.wait();
    peer_job.terminate(0, Duration::from_secs(3)).unwrap();
}
