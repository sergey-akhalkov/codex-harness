//! Actual Rust launcher entry point on owned installation targets.
#![cfg(windows)]
use harness_core::{
    build_identity::{self, BINARIES, BuildRecord, INSPECTION_SCHEMA, SCHEMA},
    build_selection,
    console::{ConsoleSession, ConsoleSpec},
    process::{Cancellation, CommandSpec, Deadline, StopReason},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

struct Fixture {
    root: tempfile::TempDir,
    home: PathBuf,
    source: PathBuf,
    state: PathBuf,
    launcher: PathBuf,
    upstream: PathBuf,
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
        let f = Self {
            root,
            home,
            source,
            state,
            launcher,
            upstream,
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
            .env_remove("HARNESS_LAUNCH_FIXTURE_MODE");
        c
    }
    fn console(&self, mode: &str) -> ConsoleSession {
        let mut c = CommandSpec::new(&self.launcher);
        c.current_dir = Some(self.root.path().to_owned());
        c.env.insert(
            "CODEX_HOME".into(),
            Some(self.home.clone().into_os_string()),
        );
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
    assert!(!stale.status.success());
    assert!(String::from_utf8_lossy(&stale.stderr).contains("registered native build is stale"));
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
            "--profile",
            "harness",
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
    for mode in [
        "source",
        "binary",
        "registration",
        "journal",
        "upstream",
        "recursion",
    ] {
        let f = Fixture::new();
        let before = fs::read(f.state.join("active-build.json")).unwrap();
        match mode {
            "source" => fs::write(f.source.join("crates/one/src/lib.rs"),"changed").unwrap(),
            "binary" => fs::write(f.launcher.parent().unwrap().join("harness-rtk.exe"),"changed").unwrap(),
            "registration" => fs::remove_file(f.home.join("harness/native-launch.json")).unwrap(),
            "journal" => fs::write(f.state.join("build-selection-journal.json"),"interrupted").unwrap(),
            "upstream" => { let mut file=fs::OpenOptions::new().append(true).open(&f.upstream).unwrap(); file.write_all(b"changed").unwrap(); },
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
