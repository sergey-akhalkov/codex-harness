//! Ported deterministic usage oracles, through the actual native CLI outside the
//! checkout. Synthetic JSONL only; model names here are inert fixture data.
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    ffi::OsString,
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Output},
};

struct Fixture(tempfile::TempDir);
impl Fixture {
    fn new() -> Self {
        Self(tempfile::tempdir().unwrap())
    }
    fn path(&self, name: &str) -> PathBuf {
        self.0.path().join(name)
    }
    fn rollout(&self, name: &str, events: Vec<Value>) -> PathBuf {
        let path = self.path(name);
        fs::write(
            &path,
            events.iter().map(|v| format!("{v}\n")).collect::<String>(),
        )
        .unwrap();
        path
    }
    fn run(&self, paths: &[PathBuf], args: &[OsString]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_codex-harness"))
            .arg("delegation-usage")
            .args(paths)
            .args(args)
            .current_dir(self.0.path())
            .output()
            .unwrap()
    }
    fn report(&self, paths: &[PathBuf]) -> Value {
        let output = self.run(paths, &[]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stderr.is_empty());
        serde_json::from_slice(&output.stdout).unwrap()
    }
    fn markdown(&self, paths: &[PathBuf]) -> String {
        let output = self.run(paths, &["--format".into(), "markdown".into()]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }
}
fn meta(id: &str, parent: Option<&str>) -> Value {
    let mut result = json!({"type":"session_meta","payload":{"id":id,"session_id":id,"model_provider":"openai"}});
    if let Some(parent) = parent {
        result["payload"]["source"] =
            json!({"subagent":{"thread_spawn":{"parent_thread_id":parent}}});
    }
    result
}
fn context(model: &str, effort: &str) -> Value {
    json!({"type":"turn_context","payload":{"model":model,"effort":effort}})
}
fn counts(amount: u64) -> Value {
    json!({"input_tokens":amount,"cached_input_tokens":amount/2,"output_tokens":amount/2,"reasoning_output_tokens":amount/5,"total_tokens":amount+amount/2})
}
fn tokens(amount: u64) -> Value {
    json!({"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":counts(amount),"last_token_usage":{"total_tokens":999999}}}})
}
fn base() -> Vec<Value> {
    vec![
        meta("parent", None),
        context("gpt-6-astra", "high"),
        tokens(10),
    ]
}
fn stamped(mut value: Value, second: u64) -> Value {
    value["timestamp"] = format!("2026-09-08T00:{:02}:{:02}Z", second / 60, second % 60).into();
    value
}
fn response(id: &str, amount: u64, second: u64) -> Value {
    stamped(
        json!({"type":"token_usage_record","payload":{"response_id":id,"usage":counts(amount),"turn_token_usage":{"total_tokens":999999},"thread_token_usage":{"total_tokens":999999}}}),
        second,
    )
}
fn message(role: &str, text: &str, phase: Option<&str>, second: u64) -> Value {
    stamped(
        json!({"type":"response_item","payload":{"type":"message","role":role,"phase":phase,"content":[{"type":"input_text","text":text}]}}),
        second,
    )
}
fn code(report: &Value, wanted: &str) -> bool {
    report["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v["code"] == wanted)
}
const ORDERS: [[usize; 3]; 6] = [
    [0, 1, 2],
    [0, 2, 1],
    [1, 0, 2],
    [1, 2, 0],
    [2, 0, 1],
    [2, 1, 0],
];

#[test]
fn public_markdown_has_no_named_workspace_exceptions() {
    let f = Fixture::new();
    for name in ["fixture", "neutral", "direct", "synthetic-private-consumer"] {
        let mut rows = base();
        let workspace = f.path(name);
        rows[1]["payload"]["cwd"] = json!(workspace);
        let path = f.rollout("input.jsonl", rows);
        let report = f.report(std::slice::from_ref(&path));
        assert_eq!(report["totals"]["total_tokens"], 15);
        let markdown = f.markdown(&[path]);
        assert!(!markdown.contains(&format!("| {name} |")));
        assert!(!markdown.contains(workspace.to_str().unwrap()));
        assert!(markdown.contains("workspace-"));
    }
}

#[test]
fn legacy_json_keeps_project_labels_but_markdown_hides_private_identity() {
    let f = Fixture::new();
    let mut rows = base();
    let private_workspace = f.path("secret-client");
    rows[1]["payload"]["cwd"] = json!(private_workspace);
    let path = f.rollout("input.jsonl", rows);
    let report = f.report(std::slice::from_ref(&path));
    assert_eq!(report["threads"][0]["project"], "secret-client");
    assert_eq!(report["threads"][0]["id"], "parent");
    assert!(
        !report
            .to_string()
            .contains(private_workspace.to_str().unwrap())
    );
    let markdown = f.markdown(&[path]);
    assert!(!markdown.contains("secret-client"));
    assert!(markdown.contains("workspace-"));
    assert!(!markdown.contains("| parent |"));
}

#[test]
fn last_cumulative_snapshot_once_and_duplicate_paths_and_ids() {
    let f = Fixture::new();
    let mut events = base();
    events.extend([tokens(20), tokens(20)]);
    let a = f.rollout("a", events);
    let b = f.rollout(
        "copy",
        vec![
            meta("parent", None),
            context("gpt-6-astra", "high"),
            tokens(20),
        ],
    );
    let r = f.report(&[a.clone(), a.clone(), f.path(".").join("a"), b]);
    assert_eq!(r["threads"].as_array().unwrap().len(), 1);
    assert_eq!(r["totals"]["total_tokens"], 30);
    assert_eq!(r["by_provider"]["OpenAI"]["cached_input_tokens"], 10);
    assert_eq!(r["totals"]["reasoning_output_tokens"], 4);
    assert_eq!(r["partial"], false);
}
#[test]
fn parent_child_grandchild_multiple_providers() {
    let f = Fixture::new();
    let p = f.rollout(
        "p",
        vec![
            meta("parent", None),
            context("gpt-6-astra", "high"),
            tokens(20),
        ],
    );
    let c = f.rollout(
        "c",
        vec![
            meta("child", Some("parent")),
            context("xai/grok-4.6", "xhigh"),
            tokens(40),
        ],
    );
    let g = f.rollout(
        "g",
        vec![
            meta("grandchild", Some("child")),
            context("gpt-6-astra", "max"),
            tokens(10),
        ],
    );
    let r = f.report(&[p, c, g]);
    assert_eq!(r["threads"][0]["parent_id"], Value::Null);
    assert_eq!(r["threads"][1]["parent_id"], "parent");
    assert_eq!(r["threads"][2]["parent_id"], "child");
    assert_eq!(r["by_provider"]["OpenAI"]["total_tokens"], 45);
    assert_eq!(r["by_provider"]["xai"]["total_tokens"], 60);
    assert_eq!(r["totals"]["total_tokens"], 105);
    assert_eq!(r["partial"], false);
}
#[test]
fn top_level_parent_metadata() {
    let f = Fixture::new();
    let mut m = meta("child", None);
    m["payload"]["parent_thread_id"] = "parent".into();
    let p = f.rollout("c", vec![m, context("gpt-6-astra", "high"), tokens(10)]);
    assert_eq!(f.report(&[p])["threads"][0]["parent_id"], "parent");
}
#[test]
fn missing_usage_is_not_zero() {
    let f = Fixture::new();
    let missing = f.rollout(
        "missing",
        vec![meta("missing", None), context("gpt-6-astra", "high")],
    );
    let zero = f.rollout(
        "zero",
        vec![
            meta("zero", None),
            context("gpt-6-astra", "high"),
            tokens(0),
        ],
    );
    let r = f.report(&[missing.clone(), zero]);
    assert!(r["threads"][0]["total_tokens"].is_null());
    assert_eq!(r["threads"][0]["missing_usage"], true);
    assert_eq!(r["threads"][1]["total_tokens"], 0);
    assert_eq!(r["threads"][1]["missing_usage"], false);
    assert_eq!(r["by_provider"]["OpenAI"]["partial"], true);
    assert!(f.report(&[missing])["totals"]["total_tokens"].is_null());
}
#[test]
fn rate_limit_only_event_does_not_erase_total() {
    let f = Fixture::new();
    let mut events = base();
    events.push(json!({"type":"event_msg","payload":{"type":"token_count","info":null}}));
    let p = f.rollout("a", events);
    assert_eq!(f.report(&[p])["totals"]["total_tokens"], 15);
}
#[test]
fn latest_invalid_counter_is_unknown_not_earlier_total() {
    let f = Fixture::new();
    for value in [
        Value::Null,
        json!(true),
        json!(-1),
        json!(1.5),
        json!("20"),
        json!([]),
        json!({}),
    ] {
        let mut events = base();
        let mut last = tokens(20);
        last["payload"]["info"]["total_token_usage"]["total_tokens"] = value;
        events.push(last);
        let p = f.rollout("a", events);
        let r = f.report(&[p]);
        assert!(r["threads"][0]["total_tokens"].is_null());
        assert_eq!(r["threads"][0]["input_tokens"], 20);
        assert_eq!(r["partial"], true);
    }
}
#[test]
fn missing_field_not_invented() {
    let f = Fixture::new();
    let mut events = base();
    events[2]["payload"]["info"]["total_token_usage"]
        .as_object_mut()
        .unwrap()
        .remove("cached_input_tokens");
    let p = f.rollout("a", events);
    let r = f.report(&[p]);
    assert!(r["totals"]["cached_input_tokens"].is_null());
    assert_eq!(r["totals"]["total_tokens"], 15);
}
#[test]
fn unknown_and_mixed_models_not_credited_to_provider() {
    let f = Fixture::new();
    let mut absent = context("gpt-6-astra", "high");
    absent["payload"]["model"] = Value::Null;
    for (contexts, warning) in [
        (
            vec![context("other/model", "high")],
            "unsupported_or_missing_model",
        ),
        (
            vec![context("gpt-5.6-terra", "high")],
            "unsupported_or_missing_model",
        ),
        (
            vec![
                context("gpt-6-astra", "high"),
                context("xai/grok-4.6", "high"),
            ],
            "mixed_model_attribution",
        ),
        (
            vec![context("gpt-6-astra", "high"), absent],
            "missing_model_context",
        ),
    ] {
        let mut events = vec![meta("parent", None)];
        events.extend(contexts);
        events.push(tokens(10));
        let p = f.rollout("a", events);
        let r = f.report(&[p]);
        assert!(r["threads"][0]["provider"].is_null());
        assert_eq!(r["totals"]["total_tokens"], 15);
        assert_eq!(r["unattributed"]["total_tokens"], 15);
        assert_eq!(r["by_provider"]["OpenAI"]["thread_count"], 0);
        assert!(code(&r, warning));
    }
}
#[test]
fn mixed_reasoning_is_not_last_effort() {
    let f = Fixture::new();
    let mut events = base();
    events.extend([context("gpt-6-astra", "max"), tokens(20)]);
    let p = f.rollout("a", events);
    let r = f.report(&[p]);
    assert!(r["threads"][0]["reasoning"].is_null());
    assert!(code(&r, "mixed_reasoning"));
}
#[test]
fn corrupt_truncated_and_invalid_utf8_salvages_known_usage() {
    let f = Fixture::new();
    let p = f.rollout("a", base());
    fs::OpenOptions::new()
        .append(true)
        .open(&p)
        .unwrap()
        .write_all(b"\xff\n{\"secret\":\"DO_NOT_LEAK\"\n")
        .unwrap();
    let r = f.report(&[p]);
    assert_eq!(r["totals"]["total_tokens"], 15);
    assert_eq!(r["partial"], true);
    assert!(code(&r, "corrupt_jsonl"));
    assert!(!r.to_string().contains("DO_NOT_LEAK"));
}
#[test]
fn unreadable_empty_and_malformed_shapes() {
    let f = Fixture::new();
    let p = f.rollout(
        "a",
        vec![
            json!([]),
            json!({"type":[]}),
            json!({"type":"event_msg","payload":[]}),
        ],
    );
    let r = f.report(&[p, f.path("DO_NOT_LEAK")]);
    assert_eq!(r["partial"], true);
    assert!(r["totals"]["total_tokens"].is_null());
    assert!(code(&r, "unreadable_input"));
    assert!(!r.to_string().contains("DO_NOT_LEAK"));
    assert!(code(&f.report(&[]), "no_inputs"));
    let empty = f.rollout("empty", vec![]);
    assert!(code(&f.report(&[empty]), "missing_thread_id"));
}
#[test]
fn missing_or_conflicting_identity_cannot_contribute() {
    let f = Fixture::new();
    for (events, warning) in [
        (
            vec![context("gpt-6-astra", "high"), tokens(10)],
            "missing_thread_id",
        ),
        (
            vec![
                meta("a", None),
                meta("unrelated", None),
                context("gpt-6-astra", "high"),
                tokens(10),
            ],
            "conflicting_thread_ids",
        ),
    ] {
        let p = f.rollout("a", events);
        let r = f.report(&[p]);
        assert!(r["threads"][0]["id"].is_null());
        assert!(r["totals"]["total_tokens"].is_null());
        assert_eq!(r["partial"], true);
        assert!(code(&r, warning));
    }
}
#[test]
fn child_session_id_is_shared_context_not_a_second_thread() {
    let f = Fixture::new();
    let mut p_events = base();
    p_events.push(json!({"type":"event_msg","payload":{"type":"item_completed","item":{"type":"CollabAgentToolCall","receiver_thread_ids":["child"]}}}));
    let p = f.rollout("p", p_events);
    let mut m = meta("child", Some("parent"));
    m["payload"]["session_id"] = "parent".into();
    m["payload"]["parent_thread_id"] = "parent".into();
    let c = f.rollout(
        "c",
        vec![
            m,
            context("xai/grok-4.6", "xhigh"),
            tokens(40),
            response("resp_child", 40, 0),
        ],
    );
    let r = f.report(&[p, c]);
    assert_eq!(r["threads"].as_array().unwrap().len(), 2);
    let c = &r["threads"][1];
    assert_eq!(c["id"], "child");
    assert_eq!(c["parent_id"], "parent");
    assert_eq!(c["total_tokens"], 60);
    assert_eq!(c["response_count"], 1);
    assert_eq!(c["response_usages"]["resp_child"]["total_tokens"], 60);
    assert_eq!(r["totals"]["total_tokens"], 75);
    assert_eq!(r["responses"]["response_count"], 1);
    assert!(!code(&r, "conflicting_thread_ids"));
}
#[test]
fn inherited_second_meta_keeps_child_id() {
    let f = Fixture::new();
    let mut first = meta("child", Some("parent"));
    first["payload"]["session_id"] = "parent".into();
    first["payload"]["parent_thread_id"] = "parent".into();
    first["payload"]["forked_from_id"] = "parent".into();
    let p = f.rollout(
        "c",
        vec![
            first,
            meta("parent", None),
            context("xai/grok-4.6", "xhigh"),
            tokens(40),
            response("resp_child", 40, 0),
        ],
    );
    let r = f.report(&[p]);
    let row = &r["threads"][0];
    assert_eq!(row["id"], "child");
    assert_eq!(row["parent_id"], "parent");
    assert_eq!(row["total_tokens"], 60);
    assert_eq!(row["response_count"], 1);
    assert!(code(&r, "forked_or_compacted_history"));
}
#[test]
fn conflicting_duplicates_invalidate_usage_in_every_order() {
    let f = Fixture::new();
    let paths = [
        f.rollout("a", base()),
        f.rollout(
            "b",
            vec![
                meta("parent", None),
                context("gpt-6-astra", "high"),
                tokens(20),
            ],
        ),
        f.rollout("c", base()),
    ];
    for order in ORDERS {
        let r = f.report(&order.map(|i| paths[i].clone()));
        assert_eq!(r["threads"].as_array().unwrap().len(), 1);
        assert!(r["totals"]["total_tokens"].is_null());
        assert!(code(&r, "conflicting_duplicate_id"));
    }
}
#[test]
fn conflicting_duplicate_parent_and_model_not_claimed() {
    let f = Fixture::new();
    let a = f.rollout(
        "a",
        vec![
            meta("child", Some("one")),
            context("gpt-6-astra", "high"),
            tokens(10),
        ],
    );
    let b = f.rollout(
        "b",
        vec![
            meta("child", Some("two")),
            context("xai/grok-4.6", "high"),
            tokens(10),
        ],
    );
    let r = f.report(&[a, b]);
    for field in ["parent_id", "model", "provider", "total_tokens"] {
        assert!(r["threads"][0][field].is_null());
    }
}
#[test]
fn cumulative_decrease_uses_last_and_warns() {
    let f = Fixture::new();
    let p = f.rollout(
        "a",
        vec![
            meta("parent", None),
            context("gpt-6-astra", "high"),
            tokens(40),
            tokens(10),
        ],
    );
    let r = f.report(&[p]);
    assert_eq!(r["totals"]["total_tokens"], 15);
    assert!(code(&r, "cumulative_usage_decreased"));
}
#[test]
fn redaction_and_actual_cli_output() {
    let f = Fixture::new();
    let mut events = base();
    events[0]["payload"]["base_instructions"] = "PRIVATE_PROMPT".into();
    events[0]["payload"]["auth"] = "RAW_AUTH".into();
    events[0]["payload"]["cwd"] = "PRIVATE_PATH".into();
    events.push(json!({"type":"response_item","payload":{"content":"PRIVATE_MESSAGE"}}));
    let p = f.rollout("PRIVATE_FILENAME λ", events);
    let output = f.path("report.json");
    let run = f.run(
        std::slice::from_ref(&p),
        &["--output".into(), output.as_os_str().to_owned()],
    );
    assert_eq!(run.status.code(), Some(0));
    let rendered = fs::read_to_string(&output).unwrap();
    let blob = format!(
        "{rendered}{}{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    for secret in [
        "PRIVATE_PROMPT",
        "RAW_AUTH",
        "PRIVATE_PATH",
        "PRIVATE_FILENAME",
        "PRIVATE_MESSAGE",
    ] {
        assert!(!blob.contains(secret));
    }
    let parsed: Value = serde_json::from_str(&rendered).unwrap();
    assert_eq!(parsed["totals"]["total_tokens"], 15);
    assert_eq!(f.report(&[p]), parsed);
}
#[test]
fn cli_cannot_overwrite_input() {
    let f = Fixture::new();
    let p = f.rollout("a", base());
    let before = fs::read(&p).unwrap();
    let output = f.run(
        std::slice::from_ref(&p),
        &["--output".into(), p.as_os_str().to_owned()],
    );
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(fs::read(&p).unwrap(), before);
}

#[test]
fn repeated_stable_response_ids_count_once() {
    let f = Fixture::new();
    let mut events = base();
    events.extend([response("resp_aaa", 10, 0), response("resp_aaa", 10, 60)]);
    let a = f.rollout("a", events);
    let mut events = base();
    events.push(response("resp_aaa", 10, 120));
    let b = f.rollout("b", events);
    let r = f.report(&[a, b]);
    assert_eq!(r["responses"]["response_count"], 1);
    assert_eq!(r["responses"]["total_tokens"], 15);
    assert_eq!(r["totals"]["total_tokens"], 15);
}
#[test]
fn conflicting_response_usage_invalidates_all_copies_in_every_order() {
    let f = Fixture::new();
    let paths = [("a", 10), ("b", 20), ("c", 10)].map(|(name, amount)| {
        let mut events = base();
        events.push(response("resp_aaa", amount, 0));
        f.rollout(name, events)
    });
    for order in ORDERS {
        let r = f.report(&order.map(|i| paths[i].clone()));
        assert_eq!(r["responses"]["response_count"], 1);
        assert!(r["responses"]["total_tokens"].is_null());
        assert_eq!(r["responses"]["conflicting_response_ids"], 1);
        assert_eq!(r["totals"]["total_tokens"], 15);
        assert!(r["threads"][0]["response_usages"]["resp_aaa"]["total_tokens"].is_null());
        assert!(code(&r, "conflicting_duplicate_id"));
    }
}
#[test]
fn missing_response_field_is_not_a_conflicting_identity() {
    let f = Fixture::new();
    let mut incomplete = response("resp_partial", 10, 0);
    incomplete["payload"]["usage"]
        .as_object_mut()
        .unwrap()
        .remove("total_tokens");
    let mut events = base();
    events.push(incomplete);
    let p = f.rollout("a", events);
    let r = f.report(std::slice::from_ref(&p));
    assert_eq!(r["responses"]["conflicting_response_ids"], 0);
    assert!(r["responses"]["total_tokens"].is_null());
    assert_eq!(r["responses"]["partial"], true);
    assert!(f.markdown(&[p]).contains("| Reconciled total | unknown |"));
}
#[test]
fn divergent_series_do_not_claim_a_reconciled_total() {
    let f = Fixture::new();
    let mut events = base();
    events[2] = tokens(100);
    events.push(response("resp_one", 10, 0));
    let p = f.rollout("a", events);
    let r = f.report(std::slice::from_ref(&p));
    assert_eq!(r["totals"]["total_tokens"], 150);
    assert_eq!(r["responses"]["total_tokens"], 15);
    let md = f.markdown(&[p]);
    assert!(md.contains("| Reconciled total | unknown |"));
    assert!(md.contains("| Response input / cached / uncached | 10 / 5 / 5 |"));
}
#[test]
fn forked_history_does_not_add_compaction_usage() {
    let f = Fixture::new();
    let mut events = base();
    events.push(response("resp_one", 10, 0));
    events.push(json!({"type":"compacted","payload":{"window_id":"win2","compaction_response_id":"resp_one","latest_token_usage_record":{"response_id":"resp_one","usage":counts(10)}}}));
    let p = f.rollout("a", events);
    let r = f.report(&[p]);
    assert_eq!(r["threads"][0]["compacted_windows"], 1);
    assert_eq!(r["responses"]["response_count"], 1);
    assert_eq!(r["totals"]["total_tokens"], 15);
    assert!(code(&r, "forked_or_compacted_history"));
}
#[test]
fn cached_and_new_input_are_separated() {
    let f = Fixture::new();
    let mut events = base();
    events[2] = tokens(20);
    let p = f.rollout("a", events);
    let r = f.report(&[p]);
    let row = &r["threads"][0];
    assert_eq!(row["input_tokens"], 20);
    assert_eq!(row["cached_input_tokens"], 10);
    assert_eq!(row["output_tokens"], 10);
    assert_eq!(row["reasoning_output_tokens"], 4);
}
#[test]
fn reasoning_included_in_output_not_added() {
    let f = Fixture::new();
    let mut events = base();
    events[2] = tokens(20);
    events.push(response("resp_out", 20, 0));
    let p = f.rollout("a", events);
    let r = f.report(&[p]);
    let row = &r["threads"][0]["response_usages"]["resp_out"];
    assert_eq!(row["output_tokens"], 10);
    assert_eq!(row["reasoning_output_tokens"], 4);
    assert_eq!(row["total_tokens"], 30);
}
#[test]
fn missing_child_stays_partial() {
    let f = Fixture::new();
    let mut events = base();
    events.push(json!({"type":"event_msg","payload":{"type":"item_completed","item":{"type":"CollabAgentToolCall","receiver_thread_ids":["child"]}}}));
    let p = f.rollout("p", events);
    let r = f.report(&[p]);
    assert_eq!(r["missing_children"][0]["child_id"], "child");
    assert_eq!(r["partial"], true);
    assert!(code(&r, "missing_child"));
    assert_eq!(r["threads"][0]["partial"], true);
    assert_eq!(r["threads"][0]["total_tokens"], 15);
}
#[test]
fn partial_concurrent_and_reset_window_limitations() {
    let f = Fixture::new();
    let p = f.rollout(
        "p",
        vec![
            stamped(meta("parent", None), 0),
            stamped(context("gpt-6-astra", "high"), 1),
            stamped(tokens(10), 600),
        ],
    );
    let c = f.rollout(
        "c",
        vec![
            stamped(meta("child", Some("parent")), 300),
            stamped(context("xai/grok-4.6", "xhigh"), 301),
            stamped(tokens(40), 480),
        ],
    );
    let paths = [p, c];
    let r = f.report(&paths);
    assert_eq!(r["overlapping_elapsed"], true);
    assert!(code(&r, "concurrent_or_overlapping_elapsed"));
    let md = f.markdown(&paths);
    assert!(md.contains("reset windows are incomparable"));
    assert!(!md.contains('%'));
}
#[test]
fn hook_text_and_actual_continuation() {
    let f = Fixture::new();
    let mut events = base();
    events.extend([
        message(
            "user",
            "<hook_prompt hook_run_id=abc>diagnostic</hook_prompt>",
            None,
            1,
        ),
        message(
            "user",
            "<turn_aborted> The user interrupted the previous turn",
            None,
            2,
        ),
        message("assistant", "working", Some("commentary"), 3),
        stamped(
            json!({"type":"event_msg","payload":{"type":"task_started"}}),
            4,
        ),
        stamped(
            json!({"type":"event_msg","payload":{"type":"task_complete","duration_ms":1500}}),
            5,
        ),
    ]);
    let p = f.rollout("a", events);
    let r = f.report(&[p]);
    let row = &r["threads"][0];
    for key in [
        "hook_messages",
        "continuation_notices",
        "task_started",
        "commentary_messages",
        "actual_continuations",
    ] {
        assert_eq!(row[key], 1, "{key}");
    }
    assert!(row["hook_chars"].as_u64().unwrap() > 10);
    assert!(row["elapsed_seconds"].as_u64().unwrap() >= 1);
    assert_eq!(row["turn_duration_ms_sum"], 1500);
}
#[test]
fn ordinary_turns_are_not_continuations() {
    let f = Fixture::new();
    let mut events = base();
    events.extend([
        stamped(
            json!({"type":"event_msg","payload":{"type":"task_started"}}),
            3,
        ),
        message("assistant", "status", Some("commentary"), 4),
        stamped(
            json!({"type":"event_msg","payload":{"type":"task_complete","duration_ms":900}}),
            5,
        ),
    ]);
    let p = f.rollout("a", events);
    let r = f.report(std::slice::from_ref(&p));
    let row = &r["threads"][0];
    assert_eq!(row["task_started"], 1);
    assert_eq!(row["commentary_messages"], 1);
    assert_eq!(row["continuation_notices"], 0);
    assert_eq!(row["actual_continuations"], 0);
    let md = f.markdown(&[p]);
    assert!(md.contains("Ordinary turns started"));
    assert!(md.contains("| Actual continuations | 0 |"));
    assert!(md.contains("Automatic context occurrences / chars"));
}
#[test]
fn intervening_ordinary_user_request_is_not_a_continuation() {
    let f = Fixture::new();
    let mut events = base();
    events.extend([
        message(
            "user",
            "<turn_aborted> The user interrupted the previous turn",
            None,
            2,
        ),
        message("user", "please continue with a new question", None, 3),
        stamped(
            json!({"type":"event_msg","payload":{"type":"task_started"}}),
            4,
        ),
    ]);
    let p = f.rollout("a", events);
    let r = f.report(&[p]);
    let row = &r["threads"][0];
    assert_eq!(row["continuation_notices"], 1);
    assert_eq!(row["user_messages"], 1);
    assert_eq!(row["actual_continuations"], 0);
}
#[test]
fn two_triggers_share_one_resumed_turn() {
    let f = Fixture::new();
    let mut events = base();
    events.extend([
        message(
            "user",
            "<turn_aborted> The user interrupted the previous turn",
            None,
            2,
        ),
        message(
            "user",
            "resume after tool-host restart. if you are still working, continue.",
            None,
            3,
        ),
        stamped(
            json!({"type":"event_msg","payload":{"type":"task_started"}}),
            4,
        ),
        response("resp_resume", 10, 5),
    ]);
    let p = f.rollout("a", events);
    let r = f.report(&[p]);
    assert_eq!(r["threads"][0]["continuation_notices"], 2);
    assert_eq!(r["threads"][0]["actual_continuations"], 1);
}
#[test]
fn markdown_omits_raw_identities_and_quota_conversion() {
    let f = Fixture::new();
    let mut events = base();
    events.push(response("resp_secret", 10, 0));
    let p = f.rollout("PRIVATE_FILENAME", events);
    let md = f.markdown(&[p]);
    for expected in [
        "gpt-6-astra",
        "OpenAI",
        "not a quota share",
        "not weekly quota",
        "root x1",
        "Reconciled total",
        "disagreement is unresolved",
    ] {
        assert!(md.contains(expected));
    }
    for absent in [
        "resp_secret",
        "PRIVATE_FILENAME",
        "harness-grok-reliability",
    ] {
        assert!(!md.contains(absent));
    }
}
#[test]
fn cli_markdown_and_private_hashes_omit_paths() {
    let f = Fixture::new();
    let mut events = base();
    events.extend([
        response("resp_secret", 10, 0),
        message(
            "user",
            "<hook_prompt hook_run_id=abc>secret</hook_prompt>",
            None,
            1,
        ),
    ]);
    let p = f.rollout("PRIVATE_FILENAME", events);
    let output = f.path("report.md");
    let private = f.path("sources.json");
    let result = f.run(
        std::slice::from_ref(&p),
        &[
            "--format".into(),
            "markdown".into(),
            "--output".into(),
            output.as_os_str().to_owned(),
            "--private-sources".into(),
            private.as_os_str().to_owned(),
        ],
    );
    assert_eq!(result.status.code(), Some(0));
    let rendered = fs::read_to_string(&output).unwrap();
    let sources: Value = serde_json::from_slice(&fs::read(&private).unwrap()).unwrap();
    let blob = format!(
        "{rendered}{sources}{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    for secret in [
        "PRIVATE_FILENAME",
        "resp_secret",
        "secret</hook_prompt>",
        p.to_str().unwrap(),
    ] {
        assert!(!blob.contains(secret));
    }
    assert_eq!(sources["sources"].as_array().unwrap().len(), 1);
    assert_eq!(
        sources["sources"][0]["sha256"],
        format!("{:x}", Sha256::digest(fs::read(&p).unwrap()))
    );
    assert!(rendered.contains("gpt-6-astra"));
}

#[test]
fn input_aliases_and_colliding_output_destinations_are_preserved() {
    let f = Fixture::new();
    let p = f.rollout("input.jsonl", base());
    let original = fs::read(&p).unwrap();
    let hard = f.path("hardlink.json");
    fs::hard_link(&p, &hard).unwrap();
    let mut aliases = vec![hard];
    #[cfg(windows)]
    {
        let link = f.path("symlink.json");
        std::os::windows::fs::symlink_file(&p, &link).unwrap();
        aliases.push(link);
    }
    for alias in aliases {
        for option in ["--output", "--private-sources"] {
            let result = f.run(
                std::slice::from_ref(&p),
                &[option.into(), alias.as_os_str().to_owned()],
            );
            assert_eq!(result.status.code(), Some(2));
            assert_eq!(fs::read(&p).unwrap(), original);
        }
    }
    let output = f.path("report.json");
    fs::write(&output, b"previous report").unwrap();
    let result = f.run(
        &[p],
        &[
            "--output".into(),
            output.as_os_str().to_owned(),
            "--private-sources".into(),
            output.as_os_str().to_owned(),
        ],
    );
    assert_eq!(result.status.code(), Some(2));
    assert_eq!(fs::read(&output).unwrap(), b"previous report");
}
#[test]
fn option_equals_separator_help_and_failures_use_the_native_entrypoint() {
    let f = Fixture::new();
    let p = f.rollout("-leading.jsonl", base());
    let output = f.path("result.json");
    let result = f.run(
        &[],
        &[
            OsString::from(format!("--output={}", output.display())),
            "--format=json".into(),
            "--".into(),
            p.as_os_str().to_owned(),
        ],
    );
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(
        serde_json::from_slice::<Value>(&fs::read(output).unwrap()).unwrap()["totals"]["total_tokens"],
        15
    );
    let help = f.run(&[p], &["--help".into()]);
    assert_eq!(help.status.code(), Some(0));
    assert!(
        String::from_utf8(help.stdout)
            .unwrap()
            .contains("--private-sources")
    );
    for args in [
        vec!["--format".into(), "unknown".into()],
        vec!["--output".into()],
        vec!["--bad".into()],
    ] {
        let result = f.run(&[], &args);
        assert_eq!(result.status.code(), Some(2));
        assert!(result.stdout.is_empty());
    }
}
