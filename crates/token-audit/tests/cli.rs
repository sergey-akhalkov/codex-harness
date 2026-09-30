//! Skeleton CLI behavior of `token-audit`: commands, options and exit codes.
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
};

struct Fixture(tempfile::TempDir);

impl Fixture {
    fn new() -> Self {
        Self(tempfile::tempdir().unwrap())
    }

    fn root(&self) -> PathBuf {
        self.0.path().to_path_buf()
    }

    fn sessions(&self) -> PathBuf {
        self.root().join("sessions")
    }

    fn rollout(&self, relative: &str, events: &[Value]) -> PathBuf {
        let path = self.sessions().join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let text: String = events.iter().map(|event| format!("{event}\n")).collect();
        fs::write(&path, text).unwrap();
        path
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_token-audit"))
            .args(args)
            .env("CODEX_HOME", self.root().join("codex-home"))
            .output()
            .unwrap()
    }

    /// A child without CODEX_HOME must resolve the native user-profile home.
    fn run_without_codex_home(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_token-audit"))
            .args(args)
            .env_remove("CODEX_HOME")
            .env("USERPROFILE", self.root().join("user-profile"))
            .output()
            .unwrap()
    }

    fn baseline(&self, subcommand: &str, extra: &[&str]) -> Output {
        let sessions = self.sessions();
        let mut args = vec![
            "baseline",
            subcommand,
            "--sessions",
            sessions.to_str().unwrap(),
        ];
        args.extend_from_slice(extra);
        self.run(&args)
    }

    fn report(&self, extra: &[&str]) -> Output {
        let sessions = self.sessions();
        let mut args = vec!["report", "--sessions", sessions.to_str().unwrap()];
        args.extend_from_slice(extra);
        self.run(&args)
    }
}

fn meta(id: &str) -> Value {
    json!({"type":"session_meta","payload":{"id":id,"session_id":id}})
}

fn context() -> Value {
    json!({"type":"turn_context","payload":{"model":"fixture-model","effort":"high","cwd":"D:/work/project-one"}})
}

fn token_count(amount: u64) -> Value {
    json!({"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{
        "input_tokens":amount,"cached_input_tokens":amount/2,"output_tokens":amount/2,
        "reasoning_output_tokens":amount/5,"total_tokens":amount+amount/2}}}})
}

fn simple_session(id: &str) -> Vec<Value> {
    vec![meta(id), context(), token_count(100)]
}

#[test]
fn help_lists_every_command_and_option() {
    let fixture = Fixture::new();
    let output = fixture.run(&["--help"]);
    assert!(output.status.success());
    let help = String::from_utf8(output.stdout).unwrap();
    for expected in [
        "report",
        "findings",
        "baseline",
        "--sessions",
        "--days",
        "--format",
        "--private-sources",
        "Exit codes",
    ] {
        assert!(help.contains(expected), "help omits {expected}: {help}");
    }
    for command in [
        ["report", "--help"],
        ["findings", "--help"],
        ["baseline", "--help"],
    ] {
        let output = fixture.run(&command);
        assert!(output.status.success(), "{command:?}");
    }
    assert_eq!(fixture.run(&["baseline"]).status.code(), Some(2));
}

#[test]
fn report_prints_json_by_default_and_text_on_request() {
    let fixture = Fixture::new();
    fixture.rollout(
        "2026/09/20/rollout-alpha.jsonl",
        &simple_session("session_alpha"),
    );
    let output = fixture.report(&[]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["command"], "report");
    assert_eq!(report["sessions"].as_array().unwrap().len(), 1);
    assert_eq!(report["sessions"][0]["session_id"], "session_alpha");
    assert_eq!(report["coverage"]["formats"]["event_msg_token_count"], 1);
    assert_eq!(report["coverage"]["corrupt_lines"], 0);

    let output = fixture.report(&["--format", "text"]);
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("token-audit report"), "{text}");
    assert!(text.contains("session_alpha"), "{text}");
    assert!(text.contains("coverage"), "{text}");
}

#[test]
fn missing_sessions_directory_names_the_cause_and_next_action() {
    let fixture = Fixture::new();
    let absent = fixture.root().join("absent");
    let output = fixture.run(&["report", "--sessions", absent.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let error = String::from_utf8(output.stderr).unwrap();
    assert!(error.contains("not found"), "{error}");
    assert!(error.contains("--sessions"), "{error}");
}

#[test]
fn a_single_rollout_file_is_a_valid_scan_root() {
    let fixture = Fixture::new();
    let rollout = fixture.rollout(
        "2026/09/20/rollout-alpha.jsonl",
        &simple_session("session_alpha"),
    );
    let output = fixture.run(&["report", "--sessions", rollout.to_str().unwrap()]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["files_discovered"], 1);
    assert_eq!(report["coverage"]["sessions"], 1);
    assert_eq!(report["sessions"][0]["session_id"], "session_alpha");
}

#[test]
fn invalid_options_fail_without_output() {
    let fixture = Fixture::new();
    fixture.rollout(
        "2026/09/20/rollout-alpha.jsonl",
        &simple_session("session_alpha"),
    );
    for extra in [
        vec!["--format", "yaml"],
        vec!["--days", "0"],
        vec!["--days", "soon"],
        vec!["--days"],
        vec!["--sessions"],
        vec!["--unknown"],
        vec!["extra"],
    ] {
        let output = fixture.report(&extra);
        assert_eq!(output.status.code(), Some(2), "{extra:?}");
        assert!(output.stdout.is_empty(), "{extra:?}");
        assert!(!output.stderr.is_empty(), "{extra:?}");
    }
    for args in [Vec::new(), vec!["nonsense"], vec!["baseline", "prune"]] {
        let output = fixture.run(&args);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert!(output.stdout.is_empty(), "{args:?}");
    }
}

#[test]
fn default_sessions_root_follows_codex_home() {
    let fixture = Fixture::new();
    fixture.rollout(
        "2026/09/20/rollout-alpha.jsonl",
        &simple_session("session_alpha"),
    );
    let output = Command::new(env!("CARGO_BIN_EXE_token-audit"))
        .arg("report")
        .env("CODEX_HOME", fixture.root())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["sessions"][0]["session_id"], "session_alpha");
}

#[test]
fn private_sources_destination_is_protected() {
    let fixture = Fixture::new();
    let rollout = fixture.rollout(
        "2026/09/20/rollout-alpha.jsonl",
        &simple_session("session_alpha"),
    );
    let original = fs::read(&rollout).unwrap();
    let output = fixture.report(&["--private-sources", rollout.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert_eq!(fs::read(&rollout).unwrap(), original);

    let output = fixture.report(&["--private-sources", fixture.root().to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(2));
    assert!(!output.stderr.is_empty());
}

#[test]
fn baseline_save_name_works_unchanged_and_as_a_basename() {
    let fixture = Fixture::new();
    fixture.rollout(
        "2026/09/20/rollout-alpha.jsonl",
        &simple_session("session_alpha"),
    );
    let sessions = fixture.sessions();
    let saved = fixture.baseline("save", &[]);
    assert!(
        saved.status.success(),
        "{}",
        String::from_utf8_lossy(&saved.stderr)
    );
    let receipt: Value = serde_json::from_slice(&saved.stdout).unwrap();
    let name = receipt["baseline"].as_str().unwrap().to_owned();
    assert!(
        name.starts_with("baseline-") && name.ends_with(".json"),
        "{name}"
    );
    let basename = name.strip_suffix(".json").unwrap();
    for requested in [name.as_str(), basename, "latest"] {
        let output = fixture.baseline("diff", &["--baseline", requested]);
        assert!(
            output.status.success(),
            "{requested}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let diff: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(diff["snapshot_status"], "valid", "{requested}");
        assert_eq!(diff["compatible"], true, "{requested}");
        assert_eq!(diff["comparable"], true, "{requested}");
        assert_eq!(diff["baseline"], name.as_str(), "{requested}");
        assert_eq!(diff["sessions"][0]["session_id"], "session_alpha");
    }
    assert!(sessions.is_dir());
}

#[test]
fn baseline_defaults_use_the_native_codex_home_under_the_user_profile() {
    let fixture = Fixture::new();
    fixture.rollout(
        "2026/09/20/rollout-alpha.jsonl",
        &simple_session("session_alpha"),
    );
    let sessions = fixture.sessions();
    let saved = fixture.run_without_codex_home(&[
        "baseline",
        "save",
        "--sessions",
        sessions.to_str().unwrap(),
    ]);
    assert!(
        saved.status.success(),
        "{}",
        String::from_utf8_lossy(&saved.stderr)
    );
    let receipt: Value = serde_json::from_slice(&saved.stdout).unwrap();
    let directory = PathBuf::from(receipt["directory"].as_str().unwrap());
    let expected = fixture
        .root()
        .join("user-profile")
        .join(".codex")
        .join("harness")
        .join("token-audit")
        .join("baselines");
    assert_eq!(
        directory, expected,
        "baseline state must resolve the native Codex home under the profile"
    );
    let output = fixture.run_without_codex_home(&[
        "baseline",
        "diff",
        "--sessions",
        sessions.to_str().unwrap(),
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let diff: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(diff["sessions"][0]["session_id"], "session_alpha");
    assert_eq!(diff["compatible"], true);
}

#[test]
fn escaping_baseline_names_and_pointers_are_rejected() {
    let fixture = Fixture::new();
    fixture.rollout(
        "2026/09/20/rollout-alpha.jsonl",
        &simple_session("session_alpha"),
    );
    let saved = fixture.baseline("save", &[]);
    assert!(saved.status.success());
    let directory = fixture
        .root()
        .join("codex-home")
        .join("harness")
        .join("token-audit")
        .join("baselines");
    // An unrelated local file outside the baseline directory stays unread.
    fs::write(fixture.root().join("outside.json"), "{}").unwrap();
    for requested in [
        "../outside",
        "..\\outside",
        "C:/outside",
        "D:\\outside",
        "sub/outside",
        "",
    ] {
        let output = fixture.baseline("diff", &["--baseline", requested]);
        assert_eq!(output.status.code(), Some(2), "{requested:?}");
        assert!(output.stdout.is_empty(), "{requested:?}");
        assert!(!output.stderr.is_empty(), "{requested:?}");
    }
    // Pointer contents are validated the same way, before any file is
    // interpreted as an owned baseline.
    for pointer in ["../outside.json", "..\\outside.json", "C:/Windows/win.ini"] {
        fs::write(directory.join("latest"), pointer).unwrap();
        let output = fixture.baseline("diff", &[]);
        assert_eq!(output.status.code(), Some(2), "{pointer:?}");
        assert!(output.stdout.is_empty(), "{pointer:?}");
        let error = String::from_utf8(output.stderr).unwrap();
        assert!(
            error.contains("does not name an owned baseline"),
            "{pointer:?}: {error}"
        );
    }
    // A missing pointer is an explicit error as well.
    fs::remove_file(directory.join("latest")).unwrap();
    let output = fixture.baseline("diff", &[]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
}
