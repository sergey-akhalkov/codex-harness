//! Baseline save/diff loop: immutable uniquely named publication, structural
//! validation separated from comparability, coverage projection and bounded
//! detail over synthetic rollout files. Fixtures are library-built scans when
//! the recorded timestamp or coverage must be controlled.
use serde_json::{Value, json};
#[cfg(windows)]
use std::os::windows::fs::OpenOptionsExt;
use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    process::{Command, Output, Stdio},
};
use token_audit::{
    BaselineDiff, Bucket, ContextAggregate, CoverageReport, Report, Scan, SessionContext,
    SessionRow, TokenTotals, baseline_diff, now, save_baseline,
};

/// `FILE_SHARE_READ`: open a file so that replacing it is a sharing violation.
#[cfg(windows)]
const FILE_SHARE_READ: u32 = 0x0000_0001;

struct Fixture {
    root: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        Self {
            root: tempfile::tempdir().unwrap(),
        }
    }

    fn sessions(&self) -> PathBuf {
        self.root.path().join("sessions")
    }

    fn home(&self) -> PathBuf {
        self.root.path().join("codex-home")
    }

    fn baselines(&self) -> PathBuf {
        self.home()
            .join("harness")
            .join("token-audit")
            .join("baselines")
    }

    fn rollout(&self, relative: &str, events: &[Value]) {
        let path = self.sessions().join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let text: String = events.iter().map(|event| format!("{event}\n")).collect();
        fs::write(&path, text).unwrap();
    }

    fn run(&self, args: &[&str]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_token-audit"));
        command.args(args).env("CODEX_HOME", self.home());
        command.output().unwrap()
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

    fn save_name(&self) -> String {
        let output = self.baseline("save", &[]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice::<Value>(&output.stdout).unwrap()["baseline"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    fn latest_name(&self) -> String {
        fs::read_to_string(self.baselines().join("latest"))
            .unwrap()
            .trim()
            .to_owned()
    }

    /// Names of published snapshots, sorted; staging and pointer files are
    /// not snapshots.
    fn snapshot_names(&self) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(self.baselines())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with("baseline-") && name.ends_with(".json"))
            .collect();
        names.sort();
        names
    }
}

fn meta(id: &str) -> Value {
    json!({"type":"session_meta","payload":{"id":id,"session_id":id,
        "base_instructions":{"text":"base prompt"}}})
}

fn context(model: &str, effort: &str, cwd: &str) -> Value {
    json!({"type":"turn_context","payload":{"model":model,"effort":effort,"cwd":cwd}})
}

/// A cumulative thread snapshot, the older recorded usage format.
fn usage(turn: &str, response: &str, input: u64) -> Value {
    json!({"type":"token_usage_record","payload":{"turn_id":turn,"response_id":response,
        "usage":{"input_tokens":input,"cached_input_tokens":input/2,"output_tokens":100,
            "reasoning_output_tokens":0,"total_tokens":input+100},
        "turn_token_usage":{"input_tokens":input,"cached_input_tokens":input/2,"output_tokens":100,
            "reasoning_output_tokens":0,"total_tokens":input+100},
        "thread_token_usage":{"input_tokens":input,"cached_input_tokens":input/2,"output_tokens":100,
            "reasoning_output_tokens":0,"total_tokens":input+100}}})
}

/// Per-response counters without a cumulative snapshot.
fn delta(turn: &str, response: &str, input: u64) -> Value {
    json!({"type":"token_usage_record","payload":{"turn_id":turn,"response_id":response,
        "usage":{"input_tokens":input,"cached_input_tokens":input/2,"output_tokens":100,
            "reasoning_output_tokens":0,"total_tokens":input+100}}})
}

fn session_events(id: &str, input: u64) -> Vec<Value> {
    vec![
        meta(id),
        context("model-a", "medium", "D:/work/fixture"),
        usage("t1", "r1", input),
    ]
}

/// One library-built scan with a fixed recorded timestamp and controlled
/// per-session usage presence.
fn scan_with(stamp: chrono::DateTime<chrono::Utc>, sessions: &[(&str, Option<u64>)]) -> Scan {
    let mut rows = Vec::new();
    // Recorded totals start at zero and stay unknown only for a counter no
    // session recorded, matching the analyzer accumulator.
    let mut totals = TokenTotals {
        input_tokens: Some(0),
        cached_input_tokens: Some(0),
        output_tokens: Some(0),
        reasoning_output_tokens: Some(0),
        total_tokens: Some(0),
    };
    let mut basis = BTreeMap::new();
    let mut missing = 0;
    let mut coverage = CoverageReport {
        files_scanned: sessions.len(),
        sessions: sessions.len(),
        ..CoverageReport::default()
    };
    for (id, tokens) in sessions {
        let usage = tokens.map_or_else(TokenTotals::default, |tokens| TokenTotals {
            input_tokens: Some(tokens),
            cached_input_tokens: Some(tokens / 2),
            output_tokens: Some(100),
            reasoning_output_tokens: Some(0),
            total_tokens: Some(tokens + 100),
        });
        let (usage_basis, row_basis) = match tokens {
            Some(_) => (Some("thread_cumulative"), "thread_cumulative"),
            None => (None, "unavailable"),
        };
        *basis.entry(row_basis.to_owned()).or_default() += 1;
        if tokens.is_none() {
            missing += 1;
        }
        for (total, value) in [
            (&mut totals.input_tokens, usage.input_tokens),
            (&mut totals.cached_input_tokens, usage.cached_input_tokens),
            (&mut totals.output_tokens, usage.output_tokens),
            (
                &mut totals.reasoning_output_tokens,
                usage.reasoning_output_tokens,
            ),
            (&mut totals.total_tokens, usage.total_tokens),
        ] {
            if let Some(value) = value {
                *total = (*total).and_then(|total| total.checked_add(value));
            }
        }
        rows.push(SessionRow {
            session_id: Some((*id).to_owned()),
            project: Some("sha256:fixture-project".to_owned()),
            model: Some("fixture-model".to_owned()),
            effort: Some("medium".to_owned()),
            day: Some("2026-09-20".to_owned()),
            first_timestamp: None,
            last_timestamp: None,
            response_count: usize::from(tokens.is_some()),
            conflicting_response_ids: 0,
            formats: if tokens.is_some() {
                vec!["token_usage_record"]
            } else {
                Vec::new()
            },
            usage_basis,
            usage,
            context: SessionContext::default(),
            tool_output_bytes: BTreeMap::new(),
            elapsed_seconds: None,
            partial: tokens.is_none(),
            warnings: if tokens.is_none() {
                vec!["missing_session_usage".to_owned()]
            } else {
                Vec::new()
            },
        });
    }
    coverage.usage_basis = basis.clone();
    coverage.sessions_without_usage = missing;
    coverage.partial = missing > 0;
    let bucket = Bucket {
        key: "fixture".to_owned(),
        sessions: sessions.len(),
        responses: rows.iter().map(|row| row.response_count).sum(),
        usage_basis: basis,
        usage: totals.clone(),
        missing_usage_sessions: missing,
        context: ContextAggregate::default(),
        partial: missing > 0,
    };
    let report = Report {
        schema_version: 1,
        command: "report",
        generated_at: stamp.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        sessions_root: "sha256:fixture-root".to_owned(),
        window_days: None,
        files_discovered: sessions.len(),
        sessions: rows,
        by_project: vec![bucket.clone()],
        by_model: vec![bucket.clone()],
        by_effort: vec![bucket.clone()],
        by_day: vec![bucket.clone()],
        totals: bucket,
        coverage,
        limitation: "fixture scan",
    };
    Scan {
        report,
        projects: BTreeMap::new(),
        inputs: Vec::new(),
    }
}

fn scan_at(stamp: chrono::DateTime<chrono::Utc>, id: &str, tokens: u64) -> Scan {
    scan_with(stamp, &[(id, Some(tokens))])
}

/// The retained locator path printed by a bounded comparison presentation.
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

#[test]
fn save_records_hashed_aggregates_and_latest_pointer() {
    let fixture = Fixture::new();
    fixture.rollout("2026/09/20/a.jsonl", &session_events("session-one", 10_000));
    let output = fixture.baseline("save", &[]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let receipt: Value = serde_json::from_slice(&output.stdout).unwrap();
    let directory = PathBuf::from(receipt["directory"].as_str().unwrap());
    let name = receipt["baseline"].as_str().unwrap();
    assert!(
        name.starts_with("baseline-") && name.ends_with(".json"),
        "{name}"
    );
    assert_eq!(fixture.latest_name(), name);
    let snapshot = fs::read_to_string(directory.join(name)).unwrap();
    assert!(!snapshot.contains("D:/work"), "raw paths must stay private");
    assert!(!snapshot.contains("base prompt"), "no transcript content");
    let value: Value = serde_json::from_str(&snapshot).unwrap();
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["mode"], "activity");
    assert_eq!(value["analyzer_version"], 1);
    assert_eq!(value["coverage"]["sessions"], 1);
    assert_eq!(value["coverage"]["usage_basis"]["thread_cumulative"], 1);
    assert_eq!(value["sessions"][0]["usage_basis"], "thread_cumulative");
}

#[test]
fn diff_reports_movement_and_marks_incompatible_snapshots() {
    let fixture = Fixture::new();
    fixture.rollout("2026/09/20/a.jsonl", &session_events("session-one", 10_000));
    let saved = fixture.baseline("save", &[]);
    assert!(saved.status.success());
    let saved_name = fixture.latest_name();

    // The same session grows, and a new session appears.
    fixture.rollout("2026/09/20/a.jsonl", &session_events("session-one", 20_000));
    fixture.rollout("2026/09/21/b.jsonl", &session_events("session-two", 5_000));
    let output = fixture.baseline("diff", &[]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let diff: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(diff["snapshot_status"], "valid");
    assert_eq!(diff["compatible"], true);
    assert_eq!(diff["comparable"], true);
    assert_eq!(diff["version_changed"], false);
    assert_eq!(diff["baseline"], saved_name.as_str());
    let sessions: Vec<String> = diff["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|movement| {
            format!(
                "{}={}",
                movement["session_id"].as_str().unwrap(),
                movement["status"].as_str().unwrap()
            )
        })
        .collect();
    assert!(
        sessions.contains(&"session-one=same".to_string()),
        "{sessions:?}"
    );
    assert!(
        sessions.contains(&"session-two=new".to_string()),
        "{sessions:?}"
    );

    // A foreign schema version is an explicit version change, never a guess.
    let latest = fixture.baselines().join(&saved_name);
    let mut snapshot: Value = serde_json::from_str(&fs::read_to_string(&latest).unwrap()).unwrap();
    snapshot["schema_version"] = json!(999);
    fs::write(&latest, serde_json::to_string_pretty(&snapshot).unwrap()).unwrap();
    let incompatible = fixture.baseline("diff", &[]);
    assert!(incompatible.status.success());
    let diff: Value = serde_json::from_slice(&incompatible.stdout).unwrap();
    assert_eq!(diff["compatible"], false);
    assert_eq!(diff["snapshot_status"], "unsupported_version");
    assert_eq!(diff["version_changed"], true);
    assert_eq!(diff["comparable"], false);
    assert!(
        diff["incompatibility"].as_str().unwrap().contains("999"),
        "{diff}"
    );
    // No fabricated baseline: the unusable snapshot contributes no movement
    // and no zero totals.
    assert_eq!(diff["sessions"].as_array().unwrap().len(), 0);
    assert!(diff["totals"]["baseline_sessions"].is_null());
    assert!(diff["totals"]["baseline_total_tokens"].is_null());
    assert!(diff["totals"]["delta_total_tokens"].is_null());
}

#[test]
fn same_reported_timestamp_saves_publish_distinct_immutable_snapshots() {
    let directory = tempfile::tempdir().unwrap();
    let stamp = now();
    let first_name =
        save_baseline(directory.path(), &scan_at(stamp, "session-one", 10_000)).unwrap();
    let first_bytes = fs::read(directory.path().join(&first_name)).unwrap();
    let second_name =
        save_baseline(directory.path(), &scan_at(stamp, "session-two", 20_000)).unwrap();
    assert_ne!(
        first_name, second_name,
        "saves in one clock second must not collide"
    );
    assert_eq!(
        fs::read(directory.path().join(&first_name)).unwrap(),
        first_bytes,
        "a successful snapshot is never rewritten"
    );
    let pointer = fs::read_to_string(directory.path().join("latest")).unwrap();
    assert_eq!(pointer, second_name);
    let first: Value = serde_json::from_slice(&first_bytes).unwrap();
    assert_eq!(first["sessions"][0]["session_id"], "session-one");
    assert_eq!(first["sessions"][0]["usage"]["total_tokens"], 10_100);
    let second: Value =
        serde_json::from_slice(&fs::read(directory.path().join(&second_name)).unwrap()).unwrap();
    assert_eq!(second["sessions"][0]["session_id"], "session-two");
    assert_eq!(second["sessions"][0]["usage"]["total_tokens"], 20_100);
}

#[test]
fn concurrent_cli_saves_keep_both_snapshots_and_a_complete_latest() {
    let fixture = Fixture::new();
    fixture.rollout("2026/09/20/a.jsonl", &session_events("session-one", 10_000));
    let sessions = fixture.sessions();
    let spawn = || {
        let mut command = Command::new(env!("CARGO_BIN_EXE_token-audit"));
        command
            .args(["baseline", "save", "--sessions", sessions.to_str().unwrap()])
            .env("CODEX_HOME", fixture.home())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command.spawn().unwrap()
    };
    let first = spawn();
    let second = spawn();
    let outputs = [
        first.wait_with_output().unwrap(),
        second.wait_with_output().unwrap(),
    ];
    let names: Vec<String> = outputs
        .iter()
        .map(|output| {
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            serde_json::from_slice::<Value>(&output.stdout).unwrap()["baseline"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect();
    assert_ne!(names[0], names[1], "each save needs its own snapshot name");
    let latest = fixture.latest_name();
    assert!(
        names.contains(&latest),
        "latest {latest} must name one successful save"
    );
    for name in &names {
        let record: Value =
            serde_json::from_str(&fs::read_to_string(fixture.baselines().join(name)).unwrap())
                .unwrap();
        assert_eq!(record["schema_version"], 1);
        assert!(record["totals"]["sessions"].as_u64().unwrap() >= 1);
    }
    for requested in [names[0].as_str(), names[1].as_str(), "latest"] {
        let output = fixture.baseline("diff", &["--baseline", requested]);
        assert!(
            output.status.success(),
            "{requested}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[cfg(windows)]
#[test]
fn a_writer_stopped_at_the_pointer_update_preserves_published_evidence() {
    let fixture = Fixture::new();
    fixture.rollout("2026/09/20/a.jsonl", &session_events("session-one", 10_000));
    let original_latest = fixture.save_name();
    let directory = fixture.baselines();
    let original_bytes = fs::read(directory.join(&original_latest)).unwrap();
    let original_names = fixture.snapshot_names();

    // Holding the pointer without delete sharing makes the next writer's
    // pointer replacement fail after its snapshot is already published: the
    // exact on-disk state an interruption at that boundary leaves behind.
    let pointer = fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(directory.join("latest"))
        .unwrap();
    fixture.rollout("2026/09/20/a.jsonl", &session_events("session-one", 20_000));
    let blocked = fixture.baseline("save", &[]);
    assert_eq!(
        blocked.status.code(),
        Some(2),
        "{}",
        String::from_utf8_lossy(&blocked.stderr)
    );
    let error = String::from_utf8(blocked.stderr).unwrap();
    assert!(error.contains("was published"), "{error}");
    assert!(
        error.contains("latest pointer could not be updated"),
        "{error}"
    );
    drop(pointer);

    // The previous pointer and snapshot are untouched, the new snapshot is
    // complete and readable, and no temporary publication file is left behind.
    assert_eq!(
        fs::read_to_string(directory.join("latest")).unwrap(),
        original_latest
    );
    assert_eq!(
        fs::read(directory.join(&original_latest)).unwrap(),
        original_bytes
    );
    let names = fixture.snapshot_names();
    assert_eq!(names.len(), original_names.len() + 1, "{names:?}");
    for name in &original_names {
        assert!(names.contains(name), "{names:?}");
    }
    let published = names
        .iter()
        .find(|name| !original_names.contains(name))
        .unwrap();
    let record: Value =
        serde_json::from_str(&fs::read_to_string(directory.join(published)).unwrap()).unwrap();
    assert_eq!(record["sessions"][0]["usage"]["total_tokens"], 20_100);
    let leftovers: Vec<String> = fs::read_dir(&directory)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".tmp") || name.ends_with(".staging"))
        .collect();
    assert!(
        leftovers.is_empty(),
        "leftover publication files: {leftovers:?}"
    );

    // A later save completes normally and overwrites neither evidence file.
    let recovered = fixture.save_name();
    assert_ne!(&recovered, published);
    assert_eq!(fixture.latest_name(), recovered);
    for name in names {
        assert!(directory.join(&name).is_file(), "{name}");
    }
}

#[test]
fn orphan_publication_artifacts_and_earlier_snapshots_survive_a_later_save() {
    let fixture = Fixture::new();
    fixture.rollout("2026/09/20/a.jsonl", &session_events("session-one", 10_000));
    let original_latest = fixture.save_name();
    let directory = fixture.baselines();

    // States an interrupted writer can leave behind: an empty reserved name,
    // a staging leftover and a complete snapshot the pointer never named.
    let reserved = directory.join("baseline-20260929120000-0000000000000000001.json");
    fs::write(&reserved, b"").unwrap();
    let staging = directory.join("baseline-20260929120000-0000000000000000002.json.staging");
    fs::write(&staging, b"{\"schema_version\":1").unwrap();
    let unreferenced = directory.join("baseline-20260929120000-0000000000000000003.json");
    fs::write(
        &unreferenced,
        fs::read(directory.join(&original_latest)).unwrap(),
    )
    .unwrap();

    // The pointer still resolves to the complete published snapshot.
    let output = fixture.baseline("diff", &["--baseline", "latest"]);
    assert!(output.status.success());
    let diff: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(diff["baseline"], original_latest.as_str());
    assert_eq!(diff["snapshot_status"], "valid");

    // A reserved-but-never-published name is an explicit invalid snapshot,
    // not a fabricated zero baseline.
    let name = reserved.file_stem().unwrap().to_str().unwrap();
    let output = fixture.baseline("diff", &["--baseline", name]);
    assert!(output.status.success());
    let diff: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(diff["snapshot_status"], "invalid");
    assert_eq!(diff["compatible"], false);
    assert!(diff["totals"]["baseline_total_tokens"].is_null());
    assert!(diff["totals"]["baseline_sessions"].is_null());
    assert_eq!(diff["sessions"].as_array().unwrap().len(), 0);

    // A later save succeeds and leaves every earlier file untouched.
    let before: Vec<(String, Vec<u8>)> = fs::read_dir(&directory)
        .unwrap()
        .filter(|entry| entry.as_ref().unwrap().file_type().unwrap().is_file())
        .map(|entry| {
            let entry = entry.unwrap();
            let name = entry.file_name().to_string_lossy().into_owned();
            (name, fs::read(entry.path()).unwrap())
        })
        .collect();
    let saved = fixture.save_name();
    assert!(directory.join(&saved).is_file());
    for (name, bytes) in &before {
        if name == "latest" {
            continue;
        }
        assert_eq!(
            &fs::read(directory.join(name)).unwrap(),
            bytes,
            "{name} changed"
        );
    }
}

#[test]
fn snapshot_validation_version_and_comparability_stay_distinct() {
    let directory = tempfile::tempdir().unwrap();
    let stamp = now();
    let name = save_baseline(directory.path(), &scan_at(stamp, "session-one", 10_000)).unwrap();
    let path = directory.path().join(&name);
    let record: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    let matching = scan_at(stamp, "session-one", 10_000);

    // Corrupt JSON is an invalid snapshot, never a crash or a zero baseline.
    fs::write(&path, b"not json").unwrap();
    let diff = baseline_diff(&path, &name, &matching).unwrap();
    assert_eq!(diff.snapshot_status, "invalid");
    assert!(!diff.compatible);
    assert!(!diff.comparable);
    assert!(!diff.version_changed);
    assert!(diff.totals.baseline_sessions.is_none());
    assert!(diff.totals.baseline_total_tokens.is_none());
    assert!(diff.totals.delta_total_tokens.is_none());
    assert!(diff.sessions.is_empty());
    assert!(
        diff.snapshot_reason
            .as_deref()
            .unwrap()
            .contains("not valid JSON"),
        "{:?}",
        diff.snapshot_reason
    );

    // The expected schema number with a missing required field is invalid
    // rather than compatible.
    let mut missing = record.clone();
    missing.as_object_mut().unwrap().remove("totals");
    fs::write(&path, serde_json::to_string_pretty(&missing).unwrap()).unwrap();
    let diff = baseline_diff(&path, &name, &matching).unwrap();
    assert_eq!(diff.snapshot_status, "invalid");
    assert!(!diff.compatible);
    assert_eq!(diff.snapshot_schema_version, Some(1));
    assert!(
        diff.snapshot_reason.as_deref().unwrap().contains("totals"),
        "{:?}",
        diff.snapshot_reason
    );

    // A record whose totals contradict its sessions is not a valid baseline.
    let mut inconsistent = record.clone();
    inconsistent["totals"]["sessions"] = json!(7);
    fs::write(&path, serde_json::to_string_pretty(&inconsistent).unwrap()).unwrap();
    let diff = baseline_diff(&path, &name, &matching).unwrap();
    assert_eq!(diff.snapshot_status, "invalid");
    assert!(
        diff.snapshot_reason.as_deref().unwrap().contains("totals"),
        "{:?}",
        diff.snapshot_reason
    );

    // A foreign schema version stays a version change with an explicit reason.
    let mut foreign = record.clone();
    foreign["schema_version"] = json!(999);
    fs::write(&path, serde_json::to_string_pretty(&foreign).unwrap()).unwrap();
    let diff = baseline_diff(&path, &name, &matching).unwrap();
    assert_eq!(diff.snapshot_status, "unsupported_version");
    assert_eq!(diff.snapshot_schema_version, Some(999));
    assert_eq!(diff.current_schema_version, 1);
    assert!(diff.version_changed);
    assert!(!diff.compatible);
    assert!(!diff.comparable);

    // The same valid snapshot can be format-compatible while the populations
    // differ in root, window, mode or analyzer semantics.
    fs::write(&path, serde_json::to_string_pretty(&record).unwrap()).unwrap();
    let diff = baseline_diff(&path, &name, &matching).unwrap();
    assert_eq!(diff.snapshot_status, "valid");
    assert!(diff.compatible);
    assert!(diff.comparable, "{:?}", diff.comparability);

    let mut other_root = scan_at(stamp, "session-one", 10_000);
    other_root.report.sessions_root = "sha256:another-root".to_owned();
    let diff = baseline_diff(&path, &name, &other_root).unwrap();
    assert!(diff.compatible);
    assert!(!diff.comparable);
    assert!(
        diff.comparability
            .iter()
            .any(|reason| reason.contains("root")),
        "{:?}",
        diff.comparability
    );

    let mut windowed = scan_at(stamp, "session-one", 10_000);
    windowed.report.window_days = Some(7);
    let diff = baseline_diff(&path, &name, &windowed).unwrap();
    assert!(diff.compatible);
    assert!(!diff.comparable);
    assert!(
        diff.comparability
            .iter()
            .any(|reason| reason.contains("window")),
        "{:?}",
        diff.comparability
    );

    let mut interval = record.clone();
    interval["mode"] = json!("interval");
    fs::write(&path, serde_json::to_string_pretty(&interval).unwrap()).unwrap();
    let diff = baseline_diff(&path, &name, &matching).unwrap();
    assert!(diff.compatible);
    assert!(!diff.comparable);
    assert!(
        diff.comparability
            .iter()
            .any(|reason| reason.contains("mode")),
        "{:?}",
        diff.comparability
    );

    let mut other_analyzer = record.clone();
    other_analyzer["analyzer_version"] = json!(2);
    fs::write(
        &path,
        serde_json::to_string_pretty(&other_analyzer).unwrap(),
    )
    .unwrap();
    let diff = baseline_diff(&path, &name, &matching).unwrap();
    assert!(diff.compatible);
    assert!(!diff.comparable);
    assert!(
        diff.comparability
            .iter()
            .any(|reason| reason.contains("analyzer")),
        "{:?}",
        diff.comparability
    );

    // A legacy snapshot without comparison metadata stays inspectable, with
    // its recorded movement and an explicit weaker status.
    let mut legacy = record.clone();
    for key in ["mode", "analyzer_version", "coverage"] {
        legacy.as_object_mut().unwrap().remove(key);
    }
    for session in legacy["sessions"].as_array_mut().unwrap() {
        for key in ["usage_basis", "partial", "warnings"] {
            session.as_object_mut().unwrap().remove(key);
        }
    }
    fs::write(&path, serde_json::to_string_pretty(&legacy).unwrap()).unwrap();
    let diff = baseline_diff(&path, &name, &matching).unwrap();
    assert_eq!(diff.snapshot_status, "legacy");
    assert!(diff.compatible);
    assert!(!diff.comparable);
    assert!(
        diff.comparability
            .iter()
            .any(|reason| reason.contains("legacy")),
        "{:?}",
        diff.comparability
    );
    assert_eq!(diff.sessions.len(), 1);
    assert_eq!(diff.sessions[0].baseline_total_tokens, Some(10_100));
    assert_eq!(diff.sessions[0].baseline_usage_basis, None);
    assert!(diff.coverage.baseline.is_none());
    assert!(!diff.coverage.degraded);
}

#[test]
fn lost_usage_cannot_masquerade_as_a_saving() {
    let directory = tempfile::tempdir().unwrap();
    let stamp = now();
    let baseline = scan_with(
        stamp,
        &[("session-one", Some(10_000)), ("session-two", Some(50_000))],
    );
    let name = save_baseline(directory.path(), &baseline).unwrap();
    let current = scan_with(
        stamp,
        &[("session-one", Some(10_000)), ("session-two", None)],
    );
    let diff = baseline_diff(&directory.path().join(&name), &name, &current).unwrap();
    assert!(diff.compatible);
    assert!(diff.coverage.degraded, "{:?}", diff.coverage.reasons);
    assert!(
        diff.coverage
            .reasons
            .iter()
            .any(|reason| reason.contains("without recorded usage increased 0 -> 1")),
        "{:?}",
        diff.coverage.reasons
    );
    assert!(!diff.comparable);
    assert!(
        diff.comparability
            .iter()
            .any(|reason| reason.starts_with("coverage:")),
        "{:?}",
        diff.comparability
    );

    // The vanished subtotal stays unknown: no fabricated zero, no delta.
    let movement = diff
        .sessions
        .iter()
        .find(|movement| movement.session_id == "session-two")
        .unwrap();
    assert_eq!(movement.current_total_tokens, None);
    assert_eq!(movement.delta_total_tokens, None);
    assert_eq!(movement.current_usage_basis, None);
    assert_eq!(
        movement.baseline_usage_basis.as_deref(),
        Some("thread_cumulative")
    );

    // Totals keep the recorded sides and label the changed coverage.
    assert_eq!(diff.totals.baseline_total_tokens, Some(60_200));
    assert_eq!(diff.totals.current_total_tokens, Some(10_100));
    assert_eq!(diff.totals.current_missing_usage_sessions, 1);
    assert_eq!(diff.totals.current_usage_basis.get("unavailable"), Some(&1));

    let text = diff.render_text(&token_audit::Detail::Unavailable("fixture".to_owned()));
    assert!(text.contains("coverage degraded"), "{text}");
    assert!(
        text.contains("a lower recorded subtotal is not a saving"),
        "{text}"
    );
    assert!(
        text.contains("without recorded usage increased 0 -> 1"),
        "{text}"
    );
    assert!(text.contains("not comparable: coverage:"), "{text}");
    assert!(
        text.contains(
            "total 50100 -> unknown delta unknown basis thread_cumulative -> unavailable"
        ),
        "{text}"
    );
}

#[test]
fn usage_basis_changes_stay_labeled_through_snapshot_and_diff() {
    let fixture = Fixture::new();
    fixture.rollout("2026/09/20/a.jsonl", &session_events("session-one", 10_000));
    let saved = fixture.baseline("save", &[]);
    assert!(saved.status.success());
    // The next scan of the same session records only per-response deltas.
    fixture.rollout(
        "2026/09/20/a.jsonl",
        &[
            meta("session-one"),
            context("model-a", "medium", "D:/work/fixture"),
            delta("t1", "r1", 10_000),
        ],
    );
    let output = fixture.baseline("diff", &["--format", "json"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let diff: Value = serde_json::from_slice(&output.stdout).unwrap();
    let movement = &diff["sessions"][0];
    assert_eq!(movement["session_id"], "session-one");
    assert_eq!(movement["baseline_usage_basis"], "thread_cumulative");
    assert_eq!(movement["current_usage_basis"], "response_sum");
    assert_eq!(
        diff["totals"]["baseline_usage_basis"]["thread_cumulative"],
        1
    );
    assert_eq!(diff["totals"]["current_usage_basis"]["response_sum"], 1);
    let output = fixture.baseline("diff", &["--format", "text"]);
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(
        text.contains("basis thread_cumulative -> response_sum"),
        "{text}"
    );
}

#[test]
fn bounded_presentation_and_stable_detail_cover_thousands_of_movements() {
    const SESSIONS: usize = 2_500;
    const ADDED: usize = 400;
    let fixture = Fixture::new();
    let rollout = |fixture: &Fixture, index: usize, amount: u64| {
        fixture.rollout(
            &format!("2026/09/{:02}/rollout-{index:04}.jsonl", 1 + index % 20),
            &[
                meta(&format!("session-{index:04}")),
                context(
                    &format!("model-{:02}", index % 17),
                    &format!("effort-{}", index % 3),
                    &format!("D:/work/workspace-{index:04}"),
                ),
                usage("t1", "r1", amount),
            ],
        );
    };
    for index in 0..SESSIONS {
        rollout(&fixture, index, 10_000);
    }
    let saved = fixture.baseline("save", &[]);
    assert!(
        saved.status.success(),
        "{}",
        String::from_utf8_lossy(&saved.stderr)
    );
    // Every baseline session grows by its own amount; new sessions appear.
    for index in 0..SESSIONS {
        rollout(&fixture, index, 10_000 + 1 + index as u64);
    }
    for index in SESSIONS..SESSIONS + ADDED {
        rollout(&fixture, index, 5_000);
    }

    let output = fixture.baseline("diff", &["--format", "text"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(
        text.len() <= BaselineDiff::TEXT_BYTES,
        "presentation is {} bytes, bound {}",
        text.len(),
        BaselineDiff::TEXT_BYTES
    );
    let total = SESSIONS + ADDED;
    assert!(
        text.contains(&format!(
            "sessions ranked by absolute recorded token movement, showing {} of {total}; {} omitted from this presentation",
            BaselineDiff::TEXT_ROWS,
            total - BaselineDiff::TEXT_ROWS
        )),
        "{text}"
    );
    let retained = locator(&text);
    let complete: Value = serde_json::from_str(&fs::read_to_string(&retained).unwrap()).unwrap();
    let complete_sessions = complete["sessions"].as_array().unwrap();
    assert_eq!(complete_sessions.len(), total);
    assert_eq!(complete["by_project"].as_array().unwrap().len(), total);
    assert_eq!(complete["snapshot_status"], "valid");
    assert_eq!(complete["comparable"], true);

    // The complete totals agree with the bounded presentation.
    let totals_line = text
        .lines()
        .find(|line| line.starts_with("totals "))
        .unwrap();
    let totals = &complete["totals"];
    for expected in [
        format!(
            "sessions {} -> {}",
            totals["baseline_sessions"], totals["current_sessions"]
        ),
        format!(
            "total {} -> {}",
            totals["baseline_total_tokens"], totals["current_total_tokens"]
        ),
        format!("delta {:+}", totals["delta_total_tokens"].as_i64().unwrap()),
    ] {
        assert!(totals_line.contains(&expected), "{totals_line}");
    }

    // The detail page starts with exactly the movements the presentation
    // selected, so a presented row and a paged row cannot disagree.
    let presented: Vec<&str> = text
        .lines()
        .filter(|line| line.starts_with("session "))
        .map(|line| line.split_whitespace().nth(1).unwrap())
        .collect();
    assert_eq!(presented.len(), BaselineDiff::TEXT_ROWS);
    let output = fixture.run(&[
        "detail",
        "--diff",
        retained.to_str().unwrap(),
        "--sessions",
        "--limit",
        "20",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let page: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(page["total"], total);
    assert_eq!(page["count"], BaselineDiff::TEXT_ROWS);
    let page_ids: Vec<&str> = page["movements"]
        .as_array()
        .unwrap()
        .iter()
        .map(|movement| movement["session_id"].as_str().unwrap())
        .collect();
    assert_eq!(page_ids, presented);

    // An omitted session is retrievable by identity from the same comparison.
    let omitted = complete_sessions
        .iter()
        .find(|movement| !presented.contains(&movement["session_id"].as_str().unwrap()))
        .unwrap();
    let omitted_id = omitted["session_id"].as_str().unwrap();
    let output = fixture.run(&[
        "detail",
        "--diff",
        retained.to_str().unwrap(),
        "--session",
        omitted_id,
    ]);
    assert!(output.status.success());
    let record: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(record["count"], 1);
    assert_eq!(record["movements"][0]["session_id"], omitted_id);
    assert_eq!(
        record["movements"][0]["delta_total_tokens"],
        omitted["delta_total_tokens"]
    );

    // Paging neither loses nor duplicates rows and returns the complete
    // deterministic order.
    let mut collected: Vec<String> = Vec::new();
    let mut offset = 0;
    loop {
        let offset_text = offset.to_string();
        let output = fixture.run(&[
            "detail",
            "--diff",
            retained.to_str().unwrap(),
            "--sessions",
            "--offset",
            &offset_text,
            "--limit",
            "1000",
        ]);
        assert!(output.status.success());
        let page: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(page["total"], total);
        let movements = page["movements"].as_array().unwrap();
        if movements.is_empty() {
            break;
        }
        for movement in movements {
            collected.push(movement["session_id"].as_str().unwrap().to_owned());
        }
        offset += movements.len();
    }
    assert_eq!(collected.len(), total);
    let mut unique = collected.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), total, "paging must not duplicate rows");
    let mut expected: Vec<String> = complete_sessions
        .iter()
        .map(|movement| movement["session_id"].as_str().unwrap().to_owned())
        .collect();
    expected.sort();
    assert_eq!(unique, expected, "paging must not lose rows");

    // A repeated page is byte-identical, and an omitted group is retrievable.
    let repeat = |offset: &str| {
        fixture.run(&[
            "detail",
            "--diff",
            retained.to_str().unwrap(),
            "--sessions",
            "--offset",
            offset,
            "--limit",
            "50",
        ])
    };
    let first = repeat("500");
    let second = repeat("500");
    assert!(first.status.success());
    assert_eq!(first.stdout, second.stdout, "paging must be stable");
    let presented_projects: Vec<&str> = text
        .lines()
        .filter(|line| line.starts_with("project "))
        .map(|line| line.split_whitespace().nth(1).unwrap())
        .collect();
    let omitted_project = complete["by_project"]
        .as_array()
        .unwrap()
        .iter()
        .find(|movement| !presented_projects.contains(&movement["key"].as_str().unwrap()))
        .unwrap();
    let group = format!("project:{}", omitted_project["key"].as_str().unwrap());
    let output = fixture.run(&[
        "detail",
        "--diff",
        retained.to_str().unwrap(),
        "--group",
        &group,
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let record: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(record["count"], 1);
    assert_eq!(
        record["movements"][0]["key"],
        omitted_project["key"].as_str().unwrap()
    );

    // Explicit full JSON stays the complete machine contract.
    let output = fixture.baseline("diff", &["--format", "json"]);
    assert!(output.status.success());
    let machine: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(machine["sessions"].as_array().unwrap().len(), total);
    assert_eq!(machine["totals"], complete["totals"]);
}

#[test]
fn expired_or_mismatched_diff_detail_is_an_explicit_error() {
    let fixture = Fixture::new();
    fixture.rollout("2026/09/20/a.jsonl", &session_events("session-one", 10_000));
    let saved = fixture.baseline("save", &[]);
    assert!(saved.status.success());
    let output = fixture.baseline("diff", &["--format", "text"]);
    assert!(output.status.success());
    let retained = locator(&String::from_utf8(output.stdout).unwrap());
    assert!(retained.is_file());

    // An evicted comparison is an explicit error, never a different scan.
    fs::remove_file(&retained).unwrap();
    let output = fixture.run(&["detail", "--diff", retained.to_str().unwrap(), "--sessions"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let error = String::from_utf8(output.stderr).unwrap();
    assert!(
        error.contains("retained baseline comparison not found"),
        "{error}"
    );

    // A retained report is not a comparison.
    let report = fixture.run(&[
        "report",
        "--sessions",
        fixture.sessions().to_str().unwrap(),
        "--format",
        "text",
    ]);
    assert!(report.status.success());
    let report_path = locator(&String::from_utf8(report.stdout).unwrap());
    let output = fixture.run(&[
        "detail",
        "--diff",
        report_path.to_str().unwrap(),
        "--sessions",
    ]);
    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("not a baseline diff"),
        "a mismatched retained kind is an explicit error"
    );

    // Selector misuse stays a usage error.
    for args in [
        vec!["detail", "--diff", "unused", "--session"],
        vec!["detail", "--diff", "unused", "--group", "project"],
        vec!["detail", "--diff", "unused", "--groups"],
        vec!["detail", "--diff", "unused", "--sessions", "--limit", "0"],
        vec![
            "detail",
            "--diff",
            "unused",
            "--sessions",
            "--limit",
            "100000",
        ],
        vec!["detail", "--diff", "unused", "--offset", "5"],
    ] {
        let output = fixture.run(&args);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert!(output.stdout.is_empty(), "{args:?}");
    }
}
