//! Baseline save/diff loop over synthetic rollout files.
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
};

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
}

fn meta(id: &str) -> Value {
    json!({"type":"session_meta","payload":{"id":id,"session_id":id,
        "base_instructions":{"text":"base prompt"}}})
}

fn context(effort: &str) -> Value {
    json!({"type":"turn_context","payload":{"model":"model-a","effort":effort,"cwd":"D:/work/fixture"}})
}

fn usage(turn: &str, response: &str, input: u64) -> Value {
    json!({"type":"token_usage_record","payload":{"turn_id":turn,"response_id":response,
        "usage":{"input_tokens":input,"cached_input_tokens":input/2,"output_tokens":100,
            "reasoning_output_tokens":0,"total_tokens":input+100},
        "turn_token_usage":{"input_tokens":input,"cached_input_tokens":input/2,"output_tokens":100,
            "reasoning_output_tokens":0,"total_tokens":input+100},
        "thread_token_usage":{"input_tokens":input,"cached_input_tokens":input/2,"output_tokens":100,
            "reasoning_output_tokens":0,"total_tokens":input+100}}})
}

fn session_events(id: &str, effort: &str, input: u64) -> Vec<Value> {
    vec![meta(id), context(effort), usage("t1", "r1", input)]
}

#[test]
fn save_records_hashed_aggregates_and_latest_pointer() {
    let fixture = Fixture::new();
    fixture.rollout(
        "2026/09/20/a.jsonl",
        &session_events("session-one", "medium", 10_000),
    );
    let output = fixture.baseline("save", &[]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let receipt: Value = serde_json::from_slice(&output.stdout).unwrap();
    let directory = PathBuf::from(receipt["directory"].as_str().unwrap());
    let pointer = fs::read_to_string(directory.join("latest")).unwrap();
    assert!(pointer.starts_with("baseline-"), "{pointer}");
    let snapshot = fs::read_to_string(directory.join(pointer.trim())).unwrap();
    assert!(snapshot.contains("\"sessions_root\""), "{snapshot}");
    assert!(!snapshot.contains("D:/work"), "raw paths must stay private");
    assert!(!snapshot.contains("base prompt"), "no transcript content");
}

#[test]
fn diff_reports_movement_and_marks_incompatible_snapshots() {
    let fixture = Fixture::new();
    fixture.rollout(
        "2026/09/20/a.jsonl",
        &[
            meta("session-one"),
            context("medium"),
            usage("t1", "r1", 10_000),
        ],
    );
    let saved = fixture.baseline("save", &[]);
    assert!(saved.status.success());

    // The same session grows, and a new session appears.
    fixture.rollout(
        "2026/09/20/a.jsonl",
        &[
            meta("session-one"),
            context("medium"),
            usage("t1", "r1", 20_000),
        ],
    );
    fixture.rollout(
        "2026/09/21/b.jsonl",
        &session_events("session-two", "high", 5_000),
    );
    let output = fixture.baseline("diff", &[]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let diff: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(diff["compatible"], true);
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

    // A foreign schema version is an explicit incompatibility, never a guess.
    let receipt: Value = serde_json::from_slice(&saved.stdout).unwrap();
    let directory = PathBuf::from(receipt["directory"].as_str().unwrap());
    let latest = directory.join(fs::read_to_string(directory.join("latest")).unwrap().trim());
    let mut snapshot: Value = serde_json::from_str(&fs::read_to_string(&latest).unwrap()).unwrap();
    snapshot["schema_version"] = json!(999);
    fs::write(&latest, serde_json::to_string_pretty(&snapshot).unwrap()).unwrap();
    let incompatible = fixture.baseline("diff", &[]);
    assert!(incompatible.status.success());
    let diff: Value = serde_json::from_slice(&incompatible.stdout).unwrap();
    assert_eq!(diff["compatible"], false);
    assert!(
        diff["incompatibility"].as_str().unwrap().contains("999"),
        "{diff}"
    );
}
