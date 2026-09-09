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
