//! Bounded interactive presentation, retained complete detail and explicit
//! expiry over synthetic rollouts.
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
};

struct Fixture {
    root: tempfile::TempDir,
    home: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        Self {
            root: tempfile::tempdir().unwrap(),
            home: tempfile::tempdir().unwrap(),
        }
    }

    fn sessions(&self) -> PathBuf {
        self.root.path().join("sessions")
    }

    fn home(&self) -> PathBuf {
        self.home.path().to_path_buf()
    }

    fn rollout(&self, relative: &str, lines: &[String]) {
        let path = self.sessions().join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, lines.join("\n") + "\n").unwrap();
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_token-audit"))
            .args(args)
            .env("CODEX_HOME", self.home())
            .output()
            .unwrap()
    }

    /// One measured session per amount, named `session-<index>`.
    fn measured_sessions(&self, amounts: &[u64]) {
        for (index, amount) in amounts.iter().enumerate() {
            self.rollout(
                &format!("2026/09/20/rollout-{index:02}.jsonl"),
                &[
                    json!({"type":"session_meta","payload":{"id":format!("session-{index:02}"),
                        "session_id":format!("session-{index:02}")}})
                    .to_string(),
                    json!({"type":"turn_context","payload":{"model":"fixture-model",
                        "effort":"medium","cwd":"D:/work/fixture"}})
                    .to_string(),
                    json!({"type":"event_msg","payload":{"type":"token_count","info":{
                        "total_token_usage":{"input_tokens":amount,"cached_input_tokens":amount/2,
                        "output_tokens":100,"reasoning_output_tokens":0,
                        "total_tokens":amount+100}}}})
                    .to_string(),
                ],
            );
        }
    }

    fn report_text(&self) -> String {
        let sessions = self.sessions();
        let output = self.run(&[
            "report",
            "--sessions",
            sessions.to_str().unwrap(),
            "--format",
            "text",
        ]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }
}

/// The retained locator path printed by a bounded presentation.
fn locator(text: &str) -> PathBuf {
    let line = text
        .lines()
        .find(|line| line.starts_with("retained "))
        .unwrap_or_else(|| panic!("no retained locator in: {text}"));
    let path = line
        .strip_prefix("retained ")
        .unwrap()
        .split("  (")
        .next()
        .unwrap()
        .trim();
    PathBuf::from(path)
}

/// Session identities presented in ranked session lines.
fn presented_sessions(text: &str) -> Vec<String> {
    text.lines()
        .filter(|line| line.starts_with("session "))
        .map(|line| line.split_whitespace().nth(1).unwrap().to_owned())
        .collect()
}

#[test]
fn bounded_summary_keeps_warnings_and_names_the_retained_complete_report() {
    let fixture = Fixture::new();
    let amounts: Vec<u64> = (0..15).map(|index| 10_000 * (index + 1)).collect();
    fixture.measured_sessions(&amounts);
    // A session without recorded usage must stay visible as a warning, never
    // as zero tokens.
    fixture.rollout(
        "2026/09/20/rollout-nousage.jsonl",
        &[
            json!({"type":"session_meta","payload":{"id":"session-nousage",
                "session_id":"session-nousage"}})
            .to_string(),
            json!({"type":"turn_context","payload":{"model":"fixture-model","effort":"medium",
                "cwd":"D:/work/fixture"}})
            .to_string(),
        ],
    );
    let text = fixture.report_text();
    assert!(
        text.contains("sessions ranked by recorded total tokens, showing 12 of 16; 4 omitted"),
        "{text}"
    );
    let presented = presented_sessions(&text);
    assert_eq!(presented.len(), 12, "{presented:?}");
    assert_eq!(presented[0], "session-14", "{presented:?}");
    assert!(
        presented.contains(&"session-03".to_owned()),
        "{presented:?}"
    );
    for omitted in ["session-00", "session-01", "session-02", "session-nousage"] {
        assert!(!presented.contains(&omitted.to_owned()), "{presented:?}");
    }
    assert!(text.contains("warnings missing_session_usage=1"), "{text}");
    assert!(text.contains("coverage "), "{text}");
    assert!(text.contains("without_usage=1"), "{text}");
    assert!(text.contains("limitation "), "{text}");

    let retained = locator(&text);
    let retained_text = fs::read_to_string(&retained).unwrap();
    let complete: Value = serde_json::from_str(&retained_text).unwrap();
    assert_eq!(complete["sessions"].as_array().unwrap().len(), 16);
    assert_eq!(complete["schema_version"], 1);
    assert_eq!(complete["command"], "report");

    // A session without recorded usage stays an explicit null, not zero.
    let output = fixture.run(&[
        "detail",
        "--report",
        retained.to_str().unwrap(),
        "--session",
        "session-nousage",
    ]);
    assert!(output.status.success());
    let no_usage: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(no_usage["usage"]["input_tokens"].is_null(), "{no_usage}");

    // Explicit full JSON stays the complete machine contract and retains nothing.
    let sessions = fixture.sessions();
    let output = fixture.run(&["report", "--sessions", sessions.to_str().unwrap()]);
    assert!(output.status.success());
    let machine: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(machine["sessions"].as_array().unwrap().len(), 16);
}

#[test]
fn small_reports_present_every_record_without_an_omitted_count() {
    let fixture = Fixture::new();
    fixture.measured_sessions(&[10_000]);
    let text = fixture.report_text();
    assert!(
        text.contains("sessions ranked by recorded total tokens, all 1 presented"),
        "{text}"
    );
    assert!(!text.contains("omitted"), "{text}");
}

#[test]
fn retention_falls_back_to_the_codex_home_under_the_user_profile() {
    let fixture = Fixture::new();
    fixture.measured_sessions(&[10_000]);
    let profile = fixture.home();
    let sessions = fixture.sessions();
    // An isolated child without CODEX_HOME must use the same
    // USERPROFILE/.codex convention as the session root.
    let output = Command::new(env!("CARGO_BIN_EXE_token-audit"))
        .args([
            "report",
            "--sessions",
            sessions.to_str().unwrap(),
            "--format",
            "text",
        ])
        .env_remove("CODEX_HOME")
        .env("USERPROFILE", &profile)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    let retained = locator(&text);
    let expected = profile
        .join(".codex")
        .join("harness")
        .join("token-audit")
        .join("reports");
    assert!(
        retained.starts_with(&expected),
        "retained {} must sit under {}",
        retained.display(),
        expected.display()
    );
    let complete: Value = serde_json::from_str(&fs::read_to_string(&retained).unwrap()).unwrap();
    assert_eq!(complete["sessions"].as_array().unwrap().len(), 1);
}

#[test]
fn successive_scans_keep_distinct_locators_and_their_own_records() {
    let fixture = Fixture::new();
    fixture.measured_sessions(&[10_000]);
    let first = locator(&fixture.report_text());
    // The recorded session grows and a second session appears in the next
    // scan. Each run keeps its own create-new locator regardless of how close
    // the two recorded timestamps are.
    fixture.measured_sessions(&[20_000, 30_000]);
    let second = locator(&fixture.report_text());
    assert_ne!(first, second, "each retained run needs its own locator");
    let first_record: Value = serde_json::from_str(&fs::read_to_string(&first).unwrap()).unwrap();
    let second_record: Value = serde_json::from_str(&fs::read_to_string(&second).unwrap()).unwrap();
    assert_eq!(first_record["sessions"].as_array().unwrap().len(), 1);
    assert_eq!(second_record["sessions"].as_array().unwrap().len(), 2);
    assert_eq!(first_record["sessions"][0]["usage"]["input_tokens"], 10_000);
    assert_eq!(
        second_record["sessions"][0]["usage"]["input_tokens"],
        20_000
    );
}

#[test]
fn more_than_twenty_runs_keep_their_own_locators_and_expire_old_ones() {
    let fixture = Fixture::new();
    let mut written: Vec<(PathBuf, u64)> = Vec::new();
    for index in 0..token_audit::RETENTION_LIMIT + 1 {
        let amount = 10_000 * (index as u64 + 1);
        fixture.measured_sessions(&[amount]);
        let retained = locator(&fixture.report_text());
        assert!(
            !written.iter().any(|(path, _)| path == &retained),
            "locator reuse: {}",
            retained.display()
        );
        // Each new locator reads its own exact scan immediately, even though
        // runs within one second share the recorded timestamp.
        let complete: Value =
            serde_json::from_str(&fs::read_to_string(&retained).unwrap()).unwrap();
        assert_eq!(complete["sessions"].as_array().unwrap().len(), 1);
        assert_eq!(complete["sessions"][0]["usage"]["input_tokens"], amount);
        written.push((retained, amount));
    }

    // The oldest run is evicted for good, and its detail stays missing instead
    // of resolving to a later scan through a reused name.
    let (evicted, _) = &written[0];
    assert!(!evicted.exists(), "{}", evicted.display());
    let output = fixture.run(&[
        "detail",
        "--report",
        evicted.to_str().unwrap(),
        "--session",
        "session-00",
    ]);
    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("retained detail not found"),
        "an evicted locator is an explicit error"
    );

    // The newest run still reads back its own scan.
    let (newest, amount) = &written[written.len() - 1];
    let output = fixture.run(&[
        "detail",
        "--report",
        newest.to_str().unwrap(),
        "--session",
        "session-00",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let record: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(record["usage"]["input_tokens"], *amount);
}

#[test]
fn detail_recalls_an_omitted_session_without_rescanning_sessions() {
    let fixture = Fixture::new();
    let amounts: Vec<u64> = (0..15).map(|index| 10_000 * (index + 1)).collect();
    fixture.measured_sessions(&amounts);
    let retained = locator(&fixture.report_text());

    // The scanned sessions disappear: a detail read must still return the
    // recorded session from the retained complete report alone.
    fs::remove_dir_all(fixture.sessions()).unwrap();
    let output = fixture.run(&[
        "detail",
        "--report",
        retained.to_str().unwrap(),
        "--session",
        "session-00",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let record: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(record["session_id"], "session-00");
    assert_eq!(record["usage"]["input_tokens"], 10_000);

    let missing = fixture.run(&[
        "detail",
        "--report",
        retained.to_str().unwrap(),
        "--session",
        "session-absent",
    ]);
    assert_eq!(missing.status.code(), Some(2));
    assert!(missing.stdout.is_empty());
    let error = String::from_utf8(missing.stderr).unwrap();
    assert!(error.contains("session-absent"), "{error}");
    assert!(error.contains("nothing was rescanned"), "{error}");

    // A mixed selector is a usage error, never a silent guess.
    let mixed = fixture.run(&[
        "detail",
        "--report",
        retained.to_str().unwrap(),
        "--finding",
        "low-worth-session:session-00",
    ]);
    assert_eq!(mixed.status.code(), Some(2));
    assert!(
        String::from_utf8(mixed.stderr)
            .unwrap()
            .contains("--report PATH with --session ID"),
        "a mixed selector is an explicit usage error"
    );

    // A retained session report is not a findings report: the mismatch names
    // the missing array instead of scanning anything again.
    let wrong_kind = fixture.run(&[
        "detail",
        "--findings",
        retained.to_str().unwrap(),
        "--finding",
        "low-worth-session:session-00",
    ]);
    assert_eq!(wrong_kind.status.code(), Some(2));
    assert!(
        String::from_utf8(wrong_kind.stderr)
            .unwrap()
            .contains("no `findings` array"),
        "a mismatched retained kind is an explicit error"
    );
}

#[test]
fn expired_retained_detail_is_an_explicit_error() {
    let fixture = Fixture::new();
    fixture.measured_sessions(&[10_000]);
    let retained = locator(&fixture.report_text());
    assert!(retained.is_file());

    // Eviction or manual cleanup leaves the record unavailable, never a rescan.
    fs::remove_file(&retained).unwrap();
    let output = fixture.run(&[
        "detail",
        "--report",
        retained.to_str().unwrap(),
        "--session",
        "session-00",
    ]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let error = String::from_utf8(output.stderr).unwrap();
    assert!(error.contains("retained detail not found"), "{error}");
    assert!(error.contains("--format text"), "{error}");
}

#[test]
fn findings_summary_is_bounded_and_retains_its_complete_contract() {
    let fixture = Fixture::new();
    for index in 0..13 {
        fixture.rollout(
            &format!("2026/09/20/rollout-low-{index:02}.jsonl"),
            &[
                json!({"type":"session_meta","payload":{"id":format!("session-{index:02}"),
                    "session_id":format!("session-{index:02}")}})
                .to_string(),
                json!({"type":"turn_context","payload":{"model":"fixture-model",
                    "effort":"medium","cwd":"D:/work/fixture"}})
                .to_string(),
                json!({"type":"event_msg","payload":{"type":"token_count","info":{
                    "total_token_usage":{"input_tokens":200_000 + index * 1_000,
                    "cached_input_tokens":0,"output_tokens":10,
                    "reasoning_output_tokens":0,
                    "total_tokens":200_010 + index * 1_000}}}})
                .to_string(),
            ],
        );
    }
    let sessions = fixture.sessions();
    let output = fixture.run(&[
        "findings",
        "--sessions",
        sessions.to_str().unwrap(),
        "--format",
        "text",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(
        text.contains("findings ranked by measured mass, showing 12 of 13; 1 omitted"),
        "{text}"
    );
    assert!(!text.contains("low-worth-session:session-00"), "{text}");
    assert!(text.contains("basis=measured"), "{text}");
    assert!(text.contains("limitation "), "{text}");

    let retained = locator(&text);
    let complete: Value = serde_json::from_str(&fs::read_to_string(&retained).unwrap()).unwrap();
    assert_eq!(complete["findings"].as_array().unwrap().len(), 13);
    let owner = complete["findings"][0]["owner"].as_str().unwrap();
    assert!(
        token_audit::TOKEN_AUDIT_OWNERS.contains(&owner),
        "owner {owner} is not a declared route"
    );

    let output = fixture.run(&[
        "detail",
        "--findings",
        retained.to_str().unwrap(),
        "--finding",
        "low-worth-session:session-00",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let record: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(record["id"], "low-worth-session:session-00");
    assert_eq!(record["basis"], "measured");
    assert_eq!(
        record["validation"]["command"],
        "token-audit report --format json"
    );
}

#[test]
fn every_declared_owner_route_resolves_in_current_source() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..");
    assert!(!token_audit::TOKEN_AUDIT_OWNERS.is_empty());
    for owner in token_audit::TOKEN_AUDIT_OWNERS {
        let path = root.join(owner);
        assert!(
            path.is_file(),
            "declared report owner {owner} does not resolve at {}",
            path.display()
        );
    }
}
