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
