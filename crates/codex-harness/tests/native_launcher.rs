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
    env, fs,
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
            source.join("tools/rtk-adapter/src"),
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
        c.env
            .insert("HARNESS_LAUNCH_FIXTURE_MODE".into(), Some(mode.into()));
        ConsoleSession::spawn(ConsoleSpec::new(c)).unwrap()
    }
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
    assert_eq!(stderr.trim(), "upstream stderr");
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
    assert_eq!(stderr.trim(), "upstream stderr");
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
