//! Analyzer fixtures: golden aggregates, coverage warnings, redaction and
//! context economics over synthetic mixed-format rollout files.
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
};

struct Fixture {
    root: tempfile::TempDir,
    /// Isolated CODEX_HOME so retained detail never touches real local state.
    home: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        Self {
            root: tempfile::tempdir().unwrap(),
            home: tempfile::tempdir().unwrap(),
        }
    }

    fn root(&self) -> PathBuf {
        self.root.path().to_path_buf()
    }

    fn home(&self) -> PathBuf {
        self.home.path().to_path_buf()
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
            .env("CODEX_HOME", self.home())
            .output()
            .unwrap()
    }

    fn report(&self, extra: &[&str]) -> Output {
        let sessions = self.sessions();
        let mut args = vec!["report", "--sessions", sessions.to_str().unwrap()];
        args.extend_from_slice(extra);
        self.run(&args)
    }

    fn json(&self, extra: &[&str]) -> Value {
        let output = self.report(extra);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn text(&self, extra: &[&str]) -> String {
        let mut args = vec!["--format", "text"];
        args.extend_from_slice(extra);
        let output = self.report(&args);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }
}

fn meta(id: &str, base: &str, stamp: &str) -> Value {
    json!({"type":"session_meta","timestamp":stamp,"payload":{"id":id,"session_id":id,"base_instructions":base}})
}

fn undated_meta(id: &str, base: &str) -> Value {
    json!({"type":"session_meta","payload":{"id":id,"session_id":id,"base_instructions":base}})
}

fn context(model: &str, effort: &str, cwd: &str) -> Value {
    json!({"type":"turn_context","payload":{"model":model,"effort":effort,"cwd":cwd}})
}

fn counts(amount: u64) -> Value {
    json!({"input_tokens":amount,"cached_input_tokens":amount/2,"output_tokens":amount/2,
        "reasoning_output_tokens":amount/5,"total_tokens":amount+amount/2})
}

fn token_count(amount: u64) -> Value {
    json!({"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":counts(amount)}}})
}

fn usage_record(turn: &str, response: &str, delta: u64, cumulative: u64, stamp: &str) -> Value {
    json!({"type":"token_usage_record","timestamp":stamp,"payload":{"turn_id":turn,"response_id":response,
        "usage":counts(delta),"turn_token_usage":counts(cumulative),"thread_token_usage":counts(cumulative)}})
}

fn delta_record(turn: &str, response: &str, delta: u64, stamp: &str) -> Value {
    json!({"type":"token_usage_record","timestamp":stamp,"payload":{"turn_id":turn,"response_id":response,"usage":counts(delta)}})
}

fn developer_message(text: &str) -> Value {
    json!({"type":"response_item","payload":{"type":"message","role":"developer","content":[{"type":"input_text","text":text}]}})
}

fn user_message(text: &str) -> Value {
    json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":text}]}})
}

fn workspace(label: &str) -> String {
    format!("D:/work/{label}")
}

fn days_ago(days: i64) -> String {
    (token_audit::now() - chrono::Duration::days(days))
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

fn token_count_at(amount: u64, stamp: &str) -> Value {
    json!({"type":"event_msg","timestamp":stamp,"payload":{"type":"token_count","info":{"total_token_usage":counts(amount)}}})
}

fn response_record(turn: &str, response: &str, delta: u64, cumulative: u64, stamp: &str) -> Value {
    json!({"type":"token_usage_record","timestamp":stamp,"payload":{"turn_id":turn,"response_id":response,
        "usage":counts(delta),"turn_token_usage":counts(cumulative),"thread_token_usage":counts(cumulative)}})
}

fn turn_context(turn: &str, model: &str, effort: &str) -> Value {
    json!({"type":"turn_context","payload":{"turn_id":turn,"model":model,"effort":effort,"cwd":workspace("interval")}})
}

/// Report content that must match between a full scan and a reused scan; the
/// operational I/O and timing counters are the only documented exclusion.
fn comparable(report: &Value) -> Value {
    let mut comparable = report.clone();
    comparable["generated_at"] = json!("<normalized>");
    comparable.as_object_mut().unwrap().remove("incremental");
    comparable
}

fn checkpoints(fixture: &Fixture) -> Vec<PathBuf> {
    let directory = fixture
        .home()
        .join("harness")
        .join("token-audit")
        .join("checkpoints");
    let Ok(entries) = fs::read_dir(&directory) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .collect();
    files.sort();
    files
}

fn append_line(path: &std::path::Path, text: &str) {
    use std::io::Write;
    let mut file = fs::OpenOptions::new().append(true).open(path).unwrap();
    file.write_all(text.as_bytes()).unwrap();
    drop(file);
}

fn session<'a>(report: &'a Value, id: &str) -> &'a Value {
    report["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["session_id"] == id)
        .unwrap_or_else(|| panic!("report has no session {id}"))
}

fn projects(report: &Value) -> Vec<&str> {
    report["by_project"]
        .as_array()
        .unwrap()
        .iter()
        .map(|bucket| bucket["key"].as_str().unwrap())
        .collect()
}

fn mixed_corpus(fixture: &Fixture) {
    fixture.rollout(
        "2026/09/20/rollout-2026-09-20T10-00-00-older.jsonl",
        &[
            meta(
                "session-older",
                "base prompt for audit",
                "2026-09-20T10:00:00Z",
            ),
            context("fixture-model", "high", &workspace("alpha-workspace")),
            token_count(100),
            token_count(200),
            developer_message("developer block"),
        ],
    );
    fixture.rollout(
        "2026/09/20/rollout-2026-09-20T11-00-00-newer.jsonl",
        &[
            meta("session-newer", "short base", "2026-09-20T11:00:00Z"),
            context("fixture-model", "low", &workspace("alpha-workspace")),
            usage_record("turn_one", "resp_one", 100, 100, "2026-09-20T11:01:00Z"),
            usage_record("turn_one", "resp_one", 100, 100, "2026-09-20T11:01:00Z"),
            usage_record("turn_two", "resp_two", 200, 300, "2026-09-20T11:02:00Z"),
        ],
    );
    fixture.rollout(
        "2026/09/21/rollout-2026-09-21T09-00-00-plain.jsonl",
        &[
            meta("session-plain", "", "2026-09-21T09:00:00Z"),
            context("fixture-model", "medium", &workspace("beta-workspace")),
            delta_record("turn_a", "resp_a", 10, "2026-09-21T09:01:00Z"),
            delta_record("turn_b", "resp_b", 30, "2026-09-21T09:02:00Z"),
        ],
    );
}

#[test]
fn mixed_formats_contribute_once_and_report_per_format_coverage() {
    let fixture = Fixture::new();
    mixed_corpus(&fixture);
    let report = fixture.json(&[]);

    assert_eq!(report["coverage"]["formats"]["event_msg_token_count"], 1);
    assert_eq!(report["coverage"]["formats"]["token_usage_record"], 2);
    assert_eq!(report["coverage"]["usage_basis"]["thread_cumulative"], 2);
    assert_eq!(report["coverage"]["usage_basis"]["response_sum"], 1);
    assert_eq!(report["coverage"]["mixed_usage_basis"], true);
    assert_eq!(report["coverage"]["sessions"], 3);
    assert_eq!(report["coverage"]["warning_counts"]["mixed_usage_basis"], 1);

    // The duplicate response counts once; both formats land in the aggregates.
    let totals = &report["totals"];
    assert_eq!(totals["sessions"], 3);
    assert_eq!(totals["responses"], 4);
    assert_eq!(totals["usage"]["input_tokens"], 540);
    assert_eq!(totals["usage"]["cached_input_tokens"], 270);
    assert_eq!(totals["usage"]["output_tokens"], 270);
    assert_eq!(totals["usage"]["reasoning_output_tokens"], 108);
    assert_eq!(totals["usage"]["total_tokens"], 810);
    assert_eq!(totals["missing_usage_sessions"], 0);
    assert_eq!(
        totals["context"]["cached_input_ratio"],
        json!(270.0 / 540.0)
    );
    assert_eq!(
        totals["context"]["repayment_multiplier"],
        json!(340.0 / 230.0)
    );
    assert_eq!(totals["context"]["summed_turn_input_tokens"], 340);
    assert_eq!(totals["context"]["final_turn_input_tokens"], 230);
    assert_eq!(totals["context"]["sessions_with_turn_metrics"], 2);
    assert_eq!(totals["context"]["sessions_without_turn_metrics"], 1);
    assert_eq!(totals["context"]["instruction_base_bytes"], 31);
    assert_eq!(totals["context"]["instruction_developer_bytes"], 15);
    assert_eq!(totals["context"]["sessions_with_instruction_bytes"], 2);

    let older = session(&report, "session-older");
    assert_eq!(older["usage_basis"], "thread_cumulative");
    assert_eq!(older["usage"]["total_tokens"], 300);
    assert_eq!(older["usage"]["input_tokens"], 200);
    assert_eq!(older["response_count"], 0);
    assert_eq!(older["formats"], json!(["event_msg_token_count"]));
    assert_eq!(older["day"], "2026-09-20");
    assert_eq!(
        older["context"]["turn_metrics_unavailable"],
        "no_turn_records"
    );
    assert_eq!(older["context"]["repayment_multiplier"], Value::Null);
    assert_eq!(older["context"]["instruction_base_bytes"], 21);
    assert_eq!(older["context"]["instruction_developer_bytes"], 15);

    let newer = session(&report, "session-newer");
    assert_eq!(newer["usage_basis"], "thread_cumulative");
    assert_eq!(newer["usage"]["total_tokens"], 450);
    assert_eq!(newer["response_count"], 2);
    assert_eq!(newer["formats"], json!(["token_usage_record"]));
    assert_eq!(newer["context"]["summed_turn_input_tokens"], 300);
    assert_eq!(newer["context"]["final_turn_input_tokens"], 200);
    assert_eq!(newer["context"]["repayment_multiplier"], json!(1.5));
    assert_eq!(newer["context"]["cached_input_ratio"], json!(0.5));
    assert_eq!(newer["context"]["turn_metrics_unavailable"], Value::Null);
    assert!(
        newer["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|code| code == "missing_usage")
    );

    let plain = session(&report, "session-plain");
    assert_eq!(plain["usage_basis"], "response_sum");
    assert_eq!(plain["usage"]["input_tokens"], 40);
    assert_eq!(plain["usage"]["cached_input_tokens"], 20);
    assert_eq!(plain["usage"]["total_tokens"], 60);
    assert_eq!(plain["context"]["repayment_multiplier"], json!(40.0 / 30.0));

    assert_eq!(projects(&report).len(), 2);
    assert_eq!(report["by_model"][0]["key"], "fixture-model");
    assert_eq!(report["by_model"][0]["sessions"], 3);
    assert_eq!(report["by_effort"].as_array().unwrap().len(), 3);
    assert_eq!(report["by_day"].as_array().unwrap().len(), 2);
    assert_eq!(report["by_day"][0]["key"], "2026-09-20");
    assert_eq!(report["by_day"][0]["sessions"], 2);
}

#[test]
fn project_identities_are_stable_hashes_per_workspace() {
    let fixture = Fixture::new();
    fixture.rollout(
        "2026/09/20/rollout-one.jsonl",
        &[
            meta("session_one", "", "2026-09-20T10:00:00Z"),
            context("fixture-model", "high", "C:/first/alpha-workspace"),
            token_count(100),
        ],
    );
    fixture.rollout(
        "2026/09/20/rollout-two.jsonl",
        &[
            meta("session_two", "", "2026-09-20T11:00:00Z"),
            context("fixture-model", "high", "D:/second/alpha-workspace"),
            token_count(100),
        ],
    );
    fixture.rollout(
        "2026/09/20/rollout-three.jsonl",
        &[
            meta("session_three", "", "2026-09-20T12:00:00Z"),
            context("fixture-model", "high", "D:/second/beta-workspace"),
            token_count(100),
        ],
    );
    let report = fixture.json(&[]);
    let identities = projects(&report);
    assert_eq!(identities.len(), 2);
    for identity in &identities {
        let hex = identity.strip_prefix("sha256:").unwrap();
        assert_eq!(hex.len(), 64);
        assert!(hex.chars().all(|digit| digit.is_ascii_hexdigit()));
    }
    // Same workspace directory name, different parents: one stable identity.
    assert_eq!(
        session(&report, "session_one")["project"],
        session(&report, "session_two")["project"]
    );
    assert_ne!(
        session(&report, "session_two")["project"],
        session(&report, "session_three")["project"]
    );
}

#[test]
fn malformed_and_unknown_records_stay_counted_coverage() {
    let fixture = Fixture::new();
    let path = fixture.sessions().join("2026/09/20/rollout-coverage.jsonl");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let events = [
        meta("session_coverage", "", "2026-09-20T10:00:00Z").to_string(),
        context("fixture-model", "high", &workspace("alpha-workspace")).to_string(),
        token_count(100).to_string(),
        json!({"type":"world_state","payload":{"full":true}}).to_string(),
        "not json".to_owned(),
        json!([]).to_string(),
    ];
    fs::write(&path, format!("{}\n", events.join("\n"))).unwrap();

    let report = fixture.json(&[]);
    let coverage = &report["coverage"];
    assert_eq!(coverage["lines"], 6);
    assert_eq!(coverage["events"], 5);
    assert_eq!(coverage["recognized_events"], 3);
    assert_eq!(coverage["unrecognized_events"], 2);
    assert_eq!(coverage["corrupt_lines"], 1);
    assert_eq!(coverage["oversized_lines"], 0);
    assert_eq!(coverage["partial"], true);
    assert_eq!(coverage["warning_counts"]["corrupt_jsonl"], 1);
    // The rest of the scan survives.
    assert_eq!(report["totals"]["usage"]["total_tokens"], 150);
}

#[test]
fn days_window_bounds_the_scan_with_explicit_exclusions() {
    let fixture = Fixture::new();
    fixture.rollout(
        "2026/01/01/rollout-recent.jsonl",
        &[
            meta("session-recent", "", &days_ago(1)),
            context("fixture-model", "high", &workspace("alpha-workspace")),
            token_count(100),
        ],
    );
    fixture.rollout(
        "2026/01/01/rollout-stale.jsonl",
        &[
            meta("session-stale", "", &days_ago(40)),
            context("fixture-model", "high", &workspace("alpha-workspace")),
            token_count(100),
        ],
    );
    fixture.rollout(
        "1999/01/02/rollout-1999-01-02T00-00-00-undated.jsonl",
        &[
            undated_meta("session-undated", ""),
            context("fixture-model", "high", &workspace("alpha-workspace")),
            token_count(50),
        ],
    );

    let unbounded = fixture.json(&[]);
    assert_eq!(unbounded["coverage"]["sessions"], 3);
    assert_eq!(unbounded["by_day"].as_array().unwrap().len(), 3);

    let bounded = fixture.json(&["--days", "7"]);
    assert_eq!(bounded["window_days"], 7);
    assert_eq!(bounded["coverage"]["sessions"], 2);
    assert_eq!(bounded["coverage"]["sessions_excluded_by_window"], 1);
    assert_eq!(
        bounded["coverage"]["warning_counts"]["session_timestamp_missing"],
        1
    );
    for row in bounded["sessions"].as_array().unwrap() {
        assert_ne!(row["session_id"], "session-stale");
    }
    // A session without recorded timestamps keeps its file-name day.
    assert_eq!(session(&bounded, "session-undated")["day"], "1999-01-02");
}

#[test]
fn default_output_redacts_local_paths_and_transcript_content() {
    let fixture = Fixture::new();
    let private = fixture.root().join("private").join("PRIVATE_WORKSPACE");
    fixture.rollout(
        "2026/09/20/rollout-private.jsonl",
        &[
            meta(
                "session_private",
                "base secret text",
                "2026-09-20T10:00:00Z",
            ),
            context("fixture-model", "high", private.to_str().unwrap()),
            token_count(100),
            developer_message("developer secret text"),
            user_message("user secret text"),
        ],
    );
    let rollout = fixture.sessions().join("2026/09/20/rollout-private.jsonl");
    let report = fixture.json(&[]);
    let rendered = serde_json::to_string(&report).unwrap();
    let text = fixture.text(&[]);
    let root = fixture.root().to_string_lossy().into_owned();
    for secret in [
        "PRIVATE_WORKSPACE",
        "private secret text",
        "developer secret text",
        "user secret text",
        root.as_str(),
        rollout.to_str().unwrap(),
    ] {
        assert!(!rendered.contains(secret), "json leaks {secret}");
        assert!(!text.contains(secret), "text leaks {secret}");
    }
    // The identities and recorded sizes are still reported.
    assert_eq!(
        session(&report, "session_private")["project"]
            .as_str()
            .unwrap()
            .strip_prefix("sha256:")
            .unwrap()
            .len(),
        64
    );
    assert_eq!(
        session(&report, "session_private")["context"]["instruction_base_bytes"],
        "base secret text".len()
    );
    assert!(text.contains("sessions-root sha256:"), "{text}");
    assert_eq!(
        session(&report, "session_private")["session_id"],
        "session_private"
    );
}

#[test]
fn private_sources_record_local_identities_outside_the_report() {
    let fixture = Fixture::new();
    let private = fixture.root().join("private").join("PRIVATE_WORKSPACE");
    let rollout = fixture.rollout(
        "2026/09/20/rollout-private.jsonl",
        &[
            meta("session_private", "", "2026-09-20T10:00:00Z"),
            context("fixture-model", "high", private.to_str().unwrap()),
            token_count(100),
        ],
    );
    let destination = fixture.root().join("sources.json");
    let output = fixture.report(&["--private-sources", destination.to_str().unwrap()]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!String::from_utf8_lossy(&output.stdout).contains("PRIVATE_WORKSPACE"));
    let record: Value = serde_json::from_slice(&fs::read(&destination).unwrap()).unwrap();
    assert_eq!(record["projects"].as_array().unwrap().len(), 1);
    assert_eq!(record["projects"][0]["label"], "PRIVATE_WORKSPACE");
    assert_eq!(
        record["projects"][0]["identity"],
        session(&fixture.json(&[]), "session_private")["project"]
    );
    assert_eq!(
        record["sources"][0]["bytes"],
        fs::metadata(&rollout).unwrap().len()
    );
    assert_eq!(
        record["sources"][0]["sha256"],
        format!("{:x}", Sha256::digest(fs::read(&rollout).unwrap()))
    );
}

#[test]
fn marathon_session_reports_context_repayment() {
    let fixture = Fixture::new();
    let mut events = vec![meta("session_marathon", "", "2026-09-19T00:00:00Z")];
    events.push(context(
        "fixture-model",
        "high",
        &workspace("alpha-workspace"),
    ));
    for index in 1..=40u64 {
        events.push(usage_record(
            &format!("turn_{index}"),
            &format!("resp_{index}"),
            1000,
            index * 1000,
            &format!("2026-09-19T{:02}:{:02}:00Z", index / 60, index % 60),
        ));
    }
    fixture.rollout("2026/09/19/rollout-marathon.jsonl", &events);

    let report = fixture.json(&[]);
    let marathon = session(&report, "session_marathon");
    assert_eq!(marathon["response_count"], 40);
    assert_eq!(marathon["context"]["summed_turn_input_tokens"], 40000);
    assert_eq!(marathon["context"]["final_turn_input_tokens"], 1000);
    assert_eq!(marathon["context"]["repayment_multiplier"], json!(40.0));
    assert_eq!(marathon["context"]["cached_input_ratio"], json!(0.5));
    assert_eq!(marathon["usage"]["input_tokens"], 40000);
    assert_eq!(marathon["usage"]["total_tokens"], 60000);
    assert_eq!(
        report["totals"]["context"]["repayment_multiplier"],
        json!(40.0)
    );
    let text = fixture.text(&[]);
    assert!(text.contains("repayment=40.000"), "{text}");
}

#[test]
fn missing_turn_identity_marks_turn_metrics_unavailable() {
    let fixture = Fixture::new();
    let mut anonymous = usage_record("turn_one", "resp_one", 100, 100, "2026-09-20T10:01:00Z");
    anonymous["payload"]
        .as_object_mut()
        .unwrap()
        .remove("turn_id");
    let mut anonymous_two = usage_record("turn_two", "resp_two", 200, 300, "2026-09-20T10:02:00Z");
    anonymous_two["payload"]
        .as_object_mut()
        .unwrap()
        .remove("turn_id");
    fixture.rollout(
        "2026/09/20/rollout-anonymous.jsonl",
        &[
            meta("session_anonymous", "", "2026-09-20T10:00:00Z"),
            context("fixture-model", "high", &workspace("alpha-workspace")),
            anonymous,
            anonymous_two,
        ],
    );
    fixture.rollout(
        "2026/09/20/rollout-older.jsonl",
        &[
            meta("session-older", "", "2026-09-20T11:00:00Z"),
            context("fixture-model", "high", &workspace("alpha-workspace")),
            token_count(200),
        ],
    );

    let report = fixture.json(&[]);
    let anonymous = session(&report, "session_anonymous");
    assert_eq!(anonymous["usage_basis"], "thread_cumulative");
    assert_eq!(anonymous["usage"]["input_tokens"], 300);
    assert_eq!(
        anonymous["context"]["turn_metrics_unavailable"],
        "missing_turn_identity"
    );
    assert_eq!(anonymous["context"]["repayment_multiplier"], Value::Null);
    assert_eq!(
        anonymous["context"]["summed_turn_input_tokens"],
        Value::Null
    );
    assert_eq!(
        session(&report, "session-older")["context"]["turn_metrics_unavailable"],
        "no_turn_records"
    );
    assert_eq!(report["coverage"]["sessions_without_turn_metrics"], 2);
    assert_eq!(report["totals"]["context"]["sessions_with_turn_metrics"], 0);
}

#[test]
fn sessions_without_recorded_usage_stay_null_and_warned() {
    let fixture = Fixture::new();
    fixture.rollout(
        "2026/09/20/rollout-empty.jsonl",
        &[
            meta("session_empty", "", "2026-09-20T10:00:00Z"),
            context("fixture-model", "high", &workspace("alpha-workspace")),
        ],
    );
    fixture.rollout(
        "2026/09/20/rollout-older.jsonl",
        &[
            meta("session-older", "", "2026-09-20T11:00:00Z"),
            context("fixture-model", "high", &workspace("alpha-workspace")),
            token_count(200),
        ],
    );
    let report = fixture.json(&[]);
    let empty = session(&report, "session_empty");
    assert_eq!(empty["usage_basis"], Value::Null);
    assert_eq!(empty["usage"]["input_tokens"], Value::Null);
    assert!(
        empty["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|code| code == "missing_session_usage")
    );
    let coverage = &report["coverage"];
    assert_eq!(coverage["sessions_without_usage"], 1);
    assert_eq!(coverage["usage_basis"]["unavailable"], 1);
    assert_eq!(coverage["warning_counts"]["missing_session_usage"], 1);
    // The measured session is unaffected.
    assert_eq!(report["totals"]["usage"]["input_tokens"], 200);
    assert_eq!(report["totals"]["missing_usage_sessions"], 1);
}

#[test]
fn interval_report_separates_attributable_usage_from_lifetime_totals() {
    let fixture = Fixture::new();
    fixture.rollout(
        "2026/09/19/rollout-old-plus-new.jsonl",
        &[
            meta("session_old_new", "", "2026-09-19T09:00:00Z"),
            context("fixture-model", "high", &workspace("alpha-workspace")),
            token_count_at(100_000, "2026-09-19T10:00:00Z"),
            response_record(
                "turn_one",
                "resp_one",
                1_000,
                101_000,
                "2026-09-20T10:00:00Z",
            ),
        ],
    );
    let activity = fixture.json(&[]);
    assert_eq!(activity["accounting"]["version"], 1);
    assert_eq!(activity["accounting"]["mode"], "activity");
    assert_eq!(activity["accounting"]["interval"], Value::Null);
    assert_eq!(
        activity["accounting"]["day_basis"],
        "session_start_lifetime"
    );
    assert_eq!(activity["interval"], Value::Null);
    let lifetime = session(&activity, "session_old_new");
    assert_eq!(lifetime["usage_basis"], "thread_cumulative");
    assert_eq!(lifetime["usage"]["total_tokens"], 151_500);
    assert_eq!(lifetime["interval"], Value::Null);

    let interval = fixture.json(&["--interval", "2026-09-20..2026-09-21"]);
    assert_eq!(interval["accounting"]["mode"], "interval");
    assert_eq!(
        interval["accounting"]["day_basis"],
        "session_start_lifetime"
    );
    let window = &interval["accounting"]["interval"];
    assert_eq!(window["start"], "2026-09-20T00:00:00Z");
    assert_eq!(window["end"], "2026-09-21T00:00:00Z");
    assert_eq!(window["basis"], "recorded_event_timestamp");
    assert_eq!(window["boundary"], "start_inclusive_end_exclusive_utc");
    assert!(
        window["policy"]["reset"]
            .as_str()
            .unwrap()
            .contains("unknown")
    );
    let row = session(&interval, "session_old_new");
    // The lifetime total stays available and labeled; the interval answer is
    // separate and only counts the attributable response.
    assert_eq!(row["usage"]["total_tokens"], 151_500);
    assert_eq!(row["interval"]["basis"], "response_delta");
    assert_eq!(row["interval"]["usage"]["total_tokens"], 1_500);
    assert_eq!(row["interval"]["models"][0], "fixture-model");
    assert_eq!(interval["interval"]["usage"]["total_tokens"], 1_500);
    assert_eq!(interval["interval"]["unallocated"]["total_tokens"], 0);
    assert_eq!(
        interval["interval"]["coverage"]["response_events_inside"],
        1
    );
    assert_eq!(
        interval["interval"]["coverage"]["response_events_before"],
        0
    );
    assert_eq!(interval["interval"]["coverage"]["sessions_with_usage"], 1);
    assert_eq!(interval["interval"]["by_model"][0]["key"], "fixture-model");
    assert_eq!(
        interval["interval"]["by_model"][0]["usage"]["total_tokens"],
        1_500
    );
    assert_eq!(interval["interval"]["by_day"][0]["key"], "2026-09-20");
    assert_eq!(
        interval["interval"]["by_day"][0]["usage"]["total_tokens"],
        1_500
    );
    assert_eq!(interval["interval"]["day_unresolved"]["total_tokens"], 0);
}

#[test]
fn interval_cumulative_boundaries_resets_and_missing_timestamps_stay_unknown() {
    let fixture = Fixture::new();
    fixture.rollout(
        "2026/09/19/rollout-cumulative.jsonl",
        &[
            meta("session_cumulative", "", "2026-09-19T22:00:00Z"),
            context("fixture-model", "high", &workspace("alpha-workspace")),
            token_count_at(1_000, "2026-09-19T23:00:00Z"),
            token_count_at(3_000, "2026-09-20T01:00:00Z"),
            token_count_at(6_000, "2026-09-20T05:00:00Z"),
            token_count_at(4_000, "2026-09-20T06:00:00Z"),
            token_count_at(9_000, "2026-09-20T07:00:00Z"),
            token_count_at(10_000, "2026-09-20T23:30:00Z"),
            token_count_at(11_000, "2026-09-21T00:30:00Z"),
            token_count(11_900),
        ],
    );
    let interval = fixture.json(&["--interval", "2026-09-20..2026-09-22"]);
    let row = session(&interval, "session_cumulative");
    assert_eq!(row["interval"]["basis"], "cumulative_increment");
    // The lifetime snapshot stays visible beside the interval answer.
    assert_eq!(row["usage"]["total_tokens"], 17_850);
    assert_eq!(row["interval"]["usage"]["total_tokens"], 15_000);
    // The step that crosses midnight is attributable to the window but not to
    // one day, so the day view states the remainder instead of guessing.
    assert_eq!(row["interval"]["day_unresolved"]["total_tokens"], 1_500);
    // Boundary-crossing, reset and undated amounts stay out of the interval.
    assert_eq!(row["interval"]["unallocated"]["total_tokens"], 4_350);
    assert_eq!(
        row["interval"]["unallocated_events"]["boundary_crossing"],
        1
    );
    assert_eq!(row["interval"]["unallocated_events"]["counter_reset"], 1);
    assert_eq!(
        row["interval"]["unallocated_events"]["missing_timestamp"],
        1
    );
    let coverage = &interval["interval"]["coverage"];
    assert_eq!(coverage["cumulative_steps_inside"], 5);
    assert_eq!(coverage["cumulative_steps_boundary"], 1);
    assert_eq!(coverage["cumulative_steps_reset"], 1);
    assert_eq!(coverage["cumulative_steps_undated"], 1);
    assert_eq!(coverage["cumulative_steps_day_crossing"], 1);
    assert_eq!(coverage["sessions_with_unallocated"], 1);
    assert_eq!(interval["interval"]["by_day"][0]["key"], "2026-09-20");
    assert_eq!(
        interval["interval"]["by_day"][0]["usage"]["total_tokens"],
        13_500
    );
    assert_eq!(
        interval["interval"]["day_unresolved"]["total_tokens"],
        1_500
    );
    assert_eq!(interval["interval"]["usage"]["total_tokens"], 15_000);
}

#[test]
fn interval_counts_duplicates_once_and_keeps_retries_and_conflicts_visible() {
    let fixture = Fixture::new();
    fixture.rollout(
        "2026/09/20/rollout-identity.jsonl",
        &[
            meta("session_identity", "", "2026-09-20T09:00:00Z"),
            turn_context("turn_one", "fixture-model", "high"),
            response_record("turn_one", "resp_dup", 1_000, 1_000, "2026-09-20T10:00:00Z"),
            response_record("turn_one", "resp_dup", 1_000, 2_000, "2026-09-20T10:00:00Z"),
            response_record("turn_one", "resp_retry", 500, 2_500, "2026-09-20T10:05:00Z"),
            response_record(
                "turn_two",
                "resp_conflict",
                400,
                2_900,
                "2026-09-20T11:00:00Z",
            ),
            response_record(
                "turn_two",
                "resp_conflict",
                700,
                3_600,
                "2026-09-20T11:00:00Z",
            ),
        ],
    );
    let interval = fixture.json(&["--interval", "2026-09-20..2026-09-21"]);
    let row = session(&interval, "session_identity");
    // The repeated stable identity is counted once; the distinct retry on the
    // same turn stays counted.
    assert_eq!(row["interval"]["usage"]["total_tokens"], 2_250);
    // The conflicting identity's amount stays unknown instead of sampled.
    assert_eq!(row["interval"]["unallocated"]["total_tokens"], Value::Null);
    assert_eq!(
        row["interval"]["unallocated_events"]["conflicting_response_id"],
        1
    );
    let coverage = &interval["interval"]["coverage"];
    assert_eq!(coverage["response_events_inside"], 2);
    assert_eq!(coverage["response_events_conflicting"], 1);
    assert!(
        row["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|code| code == "conflicting_response_id")
    );
}

#[test]
fn interval_attributes_model_switches_per_unit_and_resolves_days_by_recorded_day() {
    let fixture = Fixture::new();
    fixture.rollout(
        "2026/09/20/rollout-switch.jsonl",
        &[
            meta("session_switch", "", "2026-09-20T23:00:00Z"),
            turn_context("turn_one", "model-one", "high"),
            response_record("turn_one", "resp_one", 1_000, 1_000, "2026-09-20T23:59:30Z"),
            turn_context("turn_two", "model-two", "low"),
            response_record("turn_two", "resp_two", 2_000, 3_000, "2026-09-21T00:00:30Z"),
        ],
    );
    let interval = fixture.json(&["--interval", "2026-09-20..2026-09-22"]);
    let row = session(&interval, "session_switch");
    // A mixed session is never assigned one guessed model as its single model.
    assert_eq!(row["model"], Value::Null);
    assert_eq!(row["interval"]["models"], json!(["model-one", "model-two"]));
    assert_eq!(row["interval"]["efforts"], json!(["high", "low"]));
    let models = interval["interval"]["by_model"].as_array().unwrap();
    assert_eq!(models.len(), 2);
    assert_eq!(models[0]["key"], "model-one");
    assert_eq!(models[0]["usage"]["total_tokens"], 1_500);
    assert_eq!(models[1]["key"], "model-two");
    assert_eq!(models[1]["usage"]["total_tokens"], 3_000);
    let days = interval["interval"]["by_day"].as_array().unwrap();
    assert_eq!(days[0]["key"], "2026-09-20");
    assert_eq!(days[0]["usage"]["total_tokens"], 1_500);
    assert_eq!(days[1]["key"], "2026-09-21");
    assert_eq!(days[1]["usage"]["total_tokens"], 3_000);
}

#[test]
fn interval_missing_cumulative_counters_stay_unknown_instead_of_zero() {
    let fixture = Fixture::new();
    let mut incomplete = counts(200);
    incomplete.as_object_mut().unwrap().remove("output_tokens");
    fixture.rollout(
        "2026/09/20/rollout-incomplete.jsonl",
        &[
            meta("session_incomplete", "", "2026-09-20T09:00:00Z"),
            context("fixture-model", "high", &workspace("alpha-workspace")),
            token_count_at(100, "2026-09-20T10:00:00Z"),
            json!({"type":"event_msg","timestamp":"2026-09-20T11:00:00Z","payload":{"type":"token_count",
                "info":{"total_token_usage":incomplete}}}),
        ],
    );
    let interval = fixture.json(&["--interval", "2026-09-20..2026-09-21"]);
    let row = session(&interval, "session_incomplete");
    // The recorded counters are attributed; the counter with no usable
    // endpoint stays unknown rather than being reported as zero.
    assert_eq!(row["interval"]["usage"]["input_tokens"], 100);
    assert_eq!(row["interval"]["usage"]["total_tokens"], 150);
    assert_eq!(row["interval"]["usage"]["output_tokens"], Value::Null);
    assert_eq!(
        row["interval"]["unallocated_events"]["incomplete_counters"],
        1
    );
    assert_eq!(
        interval["interval"]["coverage"]["cumulative_steps_incomplete"],
        1
    );
}

#[test]
fn interval_never_reports_an_unknown_amount_as_zero() {
    let fixture = Fixture::new();
    fixture.rollout(
        "2026/09/20/rollout-only-conflict.jsonl",
        &[
            meta("session_only_conflict", "", "2026-09-20T09:00:00Z"),
            context("fixture-model", "high", &workspace("alpha-workspace")),
            response_record(
                "turn_one",
                "resp_only",
                1_000,
                1_000,
                "2026-09-20T10:00:00Z",
            ),
            response_record("turn_one", "resp_only", 700, 1_700, "2026-09-20T10:00:00Z"),
        ],
    );
    let interval = fixture.json(&["--interval", "2026-09-20..2026-09-21"]);
    let row = session(&interval, "session_only_conflict");
    assert_eq!(row["interval"]["usage"]["total_tokens"], Value::Null);
    assert_eq!(row["interval"]["unallocated"]["total_tokens"], Value::Null);
    assert_eq!(
        row["interval"]["unallocated_events"]["conflicting_response_id"],
        1
    );
    assert_eq!(
        interval["interval"]["coverage"]["response_events_inside"],
        0
    );
    assert!(
        interval["interval"]["by_model"]
            .as_array()
            .unwrap()
            .is_empty(),
        "an unknown amount is not attributed to a model"
    );
}

#[test]
fn incremental_reuse_matches_full_scans_and_reduces_reads() {
    let fixture = Fixture::new();
    fixture.rollout(
        "2026/09/25/rollout-history.jsonl",
        &[
            meta("session_history", "", "2026-09-25T10:00:00Z"),
            context("fixture-model", "high", &workspace("alpha-workspace")),
            token_count(10_000),
        ],
    );
    let active = fixture.rollout(
        "2026/09/26/rollout-active.jsonl",
        &[
            meta("session_active", "", "2026-09-26T10:00:00Z"),
            context("fixture-model", "high", &workspace("alpha-workspace")),
            delta_record("turn_one", "resp_one", 1_000, "2026-09-26T10:05:00Z"),
        ],
    );
    let full = fixture.json(&[]);
    let first = fixture.json(&["--incremental"]);
    assert_eq!(first["incremental"]["version"], 1);
    assert_eq!(first["incremental"]["enabled"], true);
    assert_eq!(first["incremental"]["files_reused"], 0);
    assert_eq!(first["incremental"]["files_full_parsed"], 2);
    assert_eq!(first["incremental"]["checkpoints_written"], 2);
    assert_eq!(first["incremental"]["invalidations"]["no_checkpoint"], 2);
    assert!(first["incremental"]["bytes_read"].as_u64().unwrap() > 0);
    assert_eq!(comparable(&full), comparable(&first));

    let second = fixture.json(&["--incremental"]);
    assert_eq!(second["incremental"]["files_reused"], 2);
    assert_eq!(second["incremental"]["bytes_read"], 0);
    assert_eq!(second["incremental"]["events_parsed"], 0);
    assert!(
        second["incremental"]["invalidations"]
            .as_object()
            .unwrap()
            .is_empty()
    );
    assert_eq!(comparable(&full), comparable(&second));

    let active_size = fs::metadata(&active).unwrap().len();
    let appended = format!(
        "{}\n",
        delta_record("turn_two", "resp_two", 2_000, "2026-09-26T10:10:00Z")
    );
    append_line(&active, &appended);
    let full_after = fixture.json(&[]);
    let third = fixture.json(&["--incremental"]);
    assert_eq!(comparable(&full_after), comparable(&third));
    assert_eq!(session(&third, "session_active")["response_count"], 2);
    assert_eq!(third["incremental"]["files_reused"], 1);
    assert_eq!(third["incremental"]["files_full_parsed"], 1);
    assert_eq!(third["incremental"]["invalidations"]["file_grew"], 1);
    // Growth alone cannot prove the parsed prefix unchanged, so the changed
    // file is read again while the unchanged history is not.
    assert_eq!(
        third["incremental"]["bytes_read"],
        active_size + appended.len() as u64
    );
    assert!(
        third["incremental"]["bytes_read"].as_u64().unwrap()
            < third["incremental"]["bytes_discovered"].as_u64().unwrap()
    );
}

#[cfg(windows)]
#[test]
fn incremental_reuse_rejects_same_size_mutations_with_preserved_time() {
    use std::io::Write;
    let fixture = Fixture::new();
    let path = fixture.rollout(
        "2026/09/27/rollout-mutable.jsonl",
        &[
            meta("session_mutable", "", "2026-09-27T10:00:00Z"),
            context("fixture-model", "high", &workspace("alpha-workspace")),
            token_count(100_000),
        ],
    );
    let first = fixture.json(&["--incremental"]);
    assert_eq!(first["incremental"]["files_full_parsed"], 1);
    let metadata = fs::metadata(&path).unwrap();
    let modified = metadata.modified().unwrap();
    let mut bytes = fs::read(&path).unwrap();
    let text = String::from_utf8(bytes.clone()).unwrap();
    let position = text
        .find("150000")
        .expect("the fixture records its lifetime total");
    bytes[position..position + 6].copy_from_slice(b"250000");
    let mut handle = fs::OpenOptions::new().write(true).open(&path).unwrap();
    handle.write_all(&bytes).unwrap();
    handle.sync_all().unwrap();
    handle.set_modified(modified).unwrap();
    drop(handle);
    assert_eq!(fs::metadata(&path).unwrap().len(), metadata.len());
    let preserved = fs::metadata(&path).unwrap().modified().unwrap();
    assert!(
        preserved
            .duration_since(modified)
            .is_ok_and(|delta| delta.as_millis() < 1),
        "the mutation must preserve size and modification time"
    );

    let full = fixture.json(&[]);
    let incremental = fixture.json(&["--incremental"]);
    assert_eq!(comparable(&full), comparable(&incremental));
    assert_eq!(incremental["incremental"]["files_reused"], 0);
    assert_eq!(
        incremental["incremental"]["invalidations"]["file_modified"],
        1
    );
    assert_eq!(
        session(&incremental, "session_mutable")["usage"]["total_tokens"],
        250_000
    );
}

#[test]
fn incremental_reuse_handles_partial_tails_truncation_and_replacement() {
    let fixture = Fixture::new();
    let path = fixture.rollout(
        "2026/09/28/rollout-changing.jsonl",
        &[
            meta("session_changing", "", "2026-09-28T10:00:00Z"),
            context("fixture-model", "high", &workspace("alpha-workspace")),
            token_count(100),
        ],
    );
    let full = fixture.json(&[]);
    let first = fixture.json(&["--incremental"]);
    assert_eq!(comparable(&full), comparable(&first));

    // An append that ends inside an event: the partial line is coverage, not
    // an event, and the next scan consumes the completed event exactly once.
    let complete = format!(
        "{}\n",
        delta_record("turn_one", "resp_one", 1_000, "2026-09-28T11:00:00Z")
    );
    let (head, tail) = complete.split_at(complete.len() - 8);
    append_line(&path, head);
    let full_partial = fixture.json(&[]);
    let partial = fixture.json(&["--incremental"]);
    assert_eq!(comparable(&full_partial), comparable(&partial));
    assert_eq!(partial["coverage"]["corrupt_lines"], 1);
    assert_eq!(partial["incremental"]["invalidations"]["file_grew"], 1);
    append_line(&path, tail);
    let full_completed = fixture.json(&[]);
    let completed = fixture.json(&["--incremental"]);
    assert_eq!(comparable(&full_completed), comparable(&completed));
    assert_eq!(session(&completed, "session_changing")["response_count"], 1);
    assert_eq!(completed["coverage"]["corrupt_lines"], 0);

    // Truncation and replacement fall back to full parsing with equal results.
    fs::write(
        &path,
        format!("{}\n", meta("session_changing", "", "2026-09-28T10:00:00Z")),
    )
    .unwrap();
    let full_truncated = fixture.json(&[]);
    let truncated = fixture.json(&["--incremental"]);
    assert_eq!(comparable(&full_truncated), comparable(&truncated));
    assert_eq!(truncated["incremental"]["files_reused"], 0);
    assert_eq!(
        truncated["incremental"]["invalidations"]["file_truncated"],
        1
    );
    fs::remove_file(&path).unwrap();
    fs::write(
        &path,
        format!(
            "{}\n{}\n{}\n",
            meta("session_changing", "", "2026-09-28T12:00:00Z"),
            context("fixture-model", "low", &workspace("alpha-workspace")),
            token_count(300)
        ),
    )
    .unwrap();
    let full_replaced = fixture.json(&[]);
    let replaced = fixture.json(&["--incremental"]);
    assert_eq!(comparable(&full_replaced), comparable(&replaced));
    assert_eq!(replaced["incremental"]["files_reused"], 0);
    let invalidations = replaced["incremental"]["invalidations"]
        .as_object()
        .unwrap();
    assert_eq!(invalidations.len(), 1);
    assert_eq!(invalidations.values().next().unwrap(), &json!(1));
}

#[test]
fn corrupt_foreign_and_missing_cache_state_fall_back_to_full_parsing() {
    let fixture = Fixture::new();
    fixture.rollout(
        "2026/09/29/rollout-cached.jsonl",
        &[
            meta("session_cached", "", "2026-09-29T10:00:00Z"),
            context("fixture-model", "high", &workspace("alpha-workspace")),
            token_count(500),
        ],
    );
    let full = fixture.json(&[]);
    let first = fixture.json(&["--incremental"]);
    assert_eq!(comparable(&full), comparable(&first));
    let mut stored = checkpoints(&fixture);
    assert_eq!(stored.len(), 1);
    let stored = stored.remove(0);

    // Corrupt bytes are ignored, never trusted.
    fs::write(&stored, b"not a checkpoint").unwrap();
    let corrupt = fixture.json(&["--incremental"]);
    assert_eq!(comparable(&full), comparable(&corrupt));
    assert_eq!(corrupt["incremental"]["files_reused"], 0);
    assert_eq!(
        corrupt["incremental"]["invalidations"]["checkpoint_corrupt"],
        1
    );

    // A checkpoint written by another parser version is discarded.
    let foreign =
        fs::read_to_string(&stored)
            .unwrap()
            .replacen("\"parser\":1", "\"parser\":999", 1);
    fs::write(&stored, foreign).unwrap();
    let discarded = fixture.json(&["--incremental"]);
    assert_eq!(comparable(&full), comparable(&discarded));
    assert_eq!(discarded["incremental"]["files_reused"], 0);
    assert_eq!(
        discarded["incremental"]["invalidations"]["checkpoint_version_mismatch"],
        1
    );

    // Missing cache state costs a full parse, not a failure.
    fs::remove_file(&stored).unwrap();
    let missing = fixture.json(&["--incremental"]);
    assert_eq!(comparable(&full), comparable(&missing));
    assert_eq!(missing["incremental"]["invalidations"]["no_checkpoint"], 1);
}

#[test]
fn interval_requests_reject_unusable_windows_and_absent_text_presentation() {
    let fixture = Fixture::new();
    fixture.rollout(
        "2026/09/20/rollout-window.jsonl",
        &[
            meta("session_window", "", "2026-09-20T10:00:00Z"),
            context("fixture-model", "high", &workspace("alpha-workspace")),
            token_count(100),
        ],
    );
    for argument in [
        "2026-09-21..2026-09-20",
        "2026-09-20..2026-09-20",
        "2026-09-20",
        "not-a-date..2026-09-21",
    ] {
        let output = fixture.report(&["--interval", argument]);
        assert_eq!(output.status.code(), Some(2), "{argument}");
        assert!(output.stdout.is_empty(), "{argument}");
        let error = String::from_utf8(output.stderr).unwrap();
        assert!(error.contains("--interval"), "{argument}: {error}");
    }
    // The bounded text presentation cannot state the interval accounting yet.
    let output = fixture.report(&["--interval", "2026-09-20..2026-09-21", "--format", "text"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("--format json")
    );
    // The window applies to report only.
    let output = fixture.run(&[
        "findings",
        "--sessions",
        fixture.sessions().to_str().unwrap(),
        "--interval",
        "2026-09-20..2026-09-21",
    ]);
    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("report only")
    );
}
