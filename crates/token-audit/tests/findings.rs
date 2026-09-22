//! Findings contract, detectors, basis filter and ranking over synthetic
//! mixed-format rollout files.
use serde_json::{Value, json};
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

    fn sessions(&self) -> PathBuf {
        self.root.path().join("sessions")
    }

    fn home(&self) -> PathBuf {
        self.home.path().to_path_buf()
    }

    fn rollout(&self, relative: &str, events: &[Value]) {
        let path = self.sessions().join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let text: String = events.iter().map(|event| format!("{event}\n")).collect();
        fs::write(&path, text).unwrap();
    }

    fn findings(&self, extra: &[&str]) -> Output {
        let sessions = self.sessions();
        let mut args = vec!["findings", "--sessions", sessions.to_str().unwrap()];
        args.extend_from_slice(extra);
        Command::new(env!("CARGO_BIN_EXE_token-audit"))
            .args(args)
            .env("CODEX_HOME", self.home())
            .output()
            .unwrap()
    }

    fn json(&self, extra: &[&str]) -> Value {
        let output = self.findings(extra);
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
        let output = self.findings(&args);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }
}

fn meta(id: &str) -> Value {
    json!({"type":"session_meta","payload":{"id":id,"session_id":id,
        "base_instructions":{"text":"base prompt"}}})
}

fn context(model: &str, effort: &str) -> Value {
    json!({"type":"turn_context","payload":{"model":model,"effort":effort,"cwd":"D:/work/fixture"}})
}

fn turn_usage(turn: &str, response: &str, input: u64, output: u64) -> Value {
    json!({"type":"token_usage_record","payload":{"turn_id":turn,"response_id":response,
        "usage":{"input_tokens":input,"cached_input_tokens":input/2,"output_tokens":output,
        "reasoning_output_tokens":0,"total_tokens":input+output},
        "turn_token_usage":{"input_tokens":input,"cached_input_tokens":input/2,
            "output_tokens":output,"reasoning_output_tokens":0,"total_tokens":input+output},
        "thread_token_usage":{"input_tokens":input,"cached_input_tokens":input/2,
            "output_tokens":output,"reasoning_output_tokens":0,"total_tokens":input+output}}})
}

fn tool_call(call: &str, name: &str) -> Value {
    json!({"type":"response_item","payload":{"type":"function_call","call_id":call,"name":name}})
}

fn tool_output(call: &str, bytes: usize) -> Value {
    json!({"type":"response_item","payload":{"type":"function_call_output","call_id":call,
        "output":"x".repeat(bytes)}})
}

fn ids(report: &Value) -> Vec<String> {
    report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|finding| finding["id"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn contract_and_measured_only_filter_with_hidden_counts() {
    let fixture = Fixture::new();
    // Two turns with 12x repayment (240k summed input over 20k final input).
    fixture.rollout(
        "2026/09/20/repaid.jsonl",
        &[
            meta("repaid"),
            context("model-a", "medium"),
            turn_usage("t1", "r1", 220_000, 500),
            turn_usage("t2", "r2", 20_000, 400),
            // Tool mass far above the 1 MiB floor, basis estimated.
            tool_call("c1", "exec_command"),
            tool_output("c1", 2 * 1024 * 1024),
        ],
    );
    let measured = fixture.json(&[]);
    assert_eq!(measured["schema_version"], 1);
    assert_eq!(measured["command"], "findings");
    assert_eq!(measured["measured_only"], true);
    assert_eq!(measured["hidden_by_basis"]["inferred"], 1);
    let found = ids(&measured);
    assert!(
        found.iter().all(|id| !id.starts_with("tool-output-mass")),
        "{found:?}"
    );
    for finding in measured["findings"].as_array().unwrap() {
        assert!(finding["mass_tokens"].as_u64().unwrap() > 0);
        assert!(finding["basis"] == "measured");
        assert!(
            finding["owner"].as_str().unwrap().ends_with(".md")
                || finding["owner"].as_str().unwrap().ends_with("SKILL.md")
        );
        assert!(finding["validation"]["method"].as_str().unwrap().len() > 8);
        assert!(finding["validation"]["metric"].as_str().unwrap().len() > 8);
        assert!(
            finding["validation"]["command"]
                .as_str()
                .unwrap()
                .starts_with("token-audit")
        );
        let sessions = finding["evidence"]["session_ids"].as_array().unwrap();
        assert!(!sessions.is_empty());
        assert!(
            !format!("{finding}").contains("D:/work"),
            "raw paths must stay private"
        );
    }

    let all = fixture.json(&["--all-bases"]);
    assert_eq!(all["measured_only"], false);
    assert!(
        ids(&all)
            .iter()
            .any(|id| id.starts_with("tool-output-mass"))
    );
    let text = fixture.text(&[]);
    assert!(text.contains("basis=measured"), "{text}");
    assert!(text.contains("hidden inferred=1"), "{text}");
}

#[test]
fn detectors_rank_by_mass_and_suppress_zero_mass() {
    let fixture = Fixture::new();
    // Outlier: total far above the median of the other five sessions.
    for index in 0..5 {
        fixture.rollout(
            &format!("2026/09/20/small{index}.jsonl"),
            &[
                meta(&format!("small{index}")),
                context("model-a", "medium"),
                turn_usage("t1", "r1", 1_000, 1_000),
            ],
        );
    }
    // High-effort marathon with 2x repayment and a huge input total.
    let mut marathon = vec![meta("marathon"), context("model-a", "max")];
    for turn in 0..8 {
        marathon.push(turn_usage(
            &format!("t{turn}"),
            &format!("r{turn}"),
            if turn == 7 { 100_000 } else { 900_000 },
            50_000,
        ));
    }
    fixture.rollout("2026/09/20/marathon.jsonl", &marathon);

    let report = fixture.json(&[]);
    let found = ids(&report);
    assert!(
        found.iter().any(|id| id.starts_with("effort-mix:")),
        "{found:?}"
    );
    assert!(
        found
            .iter()
            .any(|id| id.starts_with("session-outlier:marathon")),
        "{found:?}"
    );
    assert!(
        found
            .iter()
            .any(|id| id.starts_with("context-repayment:marathon")),
        "{found:?}"
    );
    // Ranking: masses are non-increasing.
    let masses: Vec<u64> = report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|finding| finding["mass_tokens"].as_u64().unwrap())
        .collect();
    let mut sorted = masses.clone();
    sorted.sort_unstable_by(|left, right| right.cmp(left));
    assert_eq!(masses, sorted);
    // Small sessions carry no zero-mass findings.
    assert!(found.iter().all(|id| !id.contains("small")), "{found:?}");
}

#[test]
fn low_worth_sessions_and_nested_base_instructions_feed_findings() {
    let fixture = Fixture::new();
    fixture.rollout(
        "2026/09/20/lowworth.jsonl",
        &[
            meta("lowworth"),
            context("model-a", "low"),
            turn_usage("t1", "r1", 250_000, 200),
            turn_usage("t2", "r2", 250_000, 100),
        ],
    );
    let report = fixture.json(&[]);
    assert!(
        ids(&report)
            .iter()
            .any(|id| id.starts_with("low-worth-session:lowworth")),
        "{report}"
    );
    // The same fixture through report proves nested base instruction bytes
    // now feed the instruction floor.
    let sessions = fixture.sessions();
    let output = Command::new(env!("CARGO_BIN_EXE_token-audit"))
        .args([
            "report",
            "--sessions",
            sessions.to_str().unwrap(),
            "--format",
            "json",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        value["sessions"][0]["context"]["instruction_base_bytes"],
        11
    );
}
