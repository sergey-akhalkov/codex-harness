//! Model-free ordinary TUI probe: host skill catalogue after mid-turn compaction.
//! Uses an owned CODEX_HOME, a distinct canned local Responses provider, and
//! ConPTY + Windows Jobs. Never redefines reserved provider openai.
#![cfg(windows)]
#[path = "fixtures/native_sampling.rs"]
mod native_sampling;
#[path = "fixtures/skill_catalog_responses.rs"]
mod skill_catalog_responses;

use harness_core::{
    build_identity,
    console::{ConsoleSession, ConsoleSpec},
    process::{Cancellation, CommandSpec, Deadline, StopReason},
};
use serde_json::{Value, json};
use skill_catalog_responses::{
    API_KEY, API_KEY_ENV, CALL_ID, COMPACT_PROMPT, CannedResponses, EARLY_SKILL, NEXT_TURN_PROMPT,
    PROVIDER_ID, RequestKind, SUMMARY_TEXT, USER_PROMPT,
};
use std::{
    fs, io,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const EXPECTED_SHA: &str = "444a3f0008050605cae73cd9b7a2dcac61294062dfaab56dd20430fd6498518b";
const EXPECTED_LEN: u64 = 295_408_944;
const PINNED_SOURCE: &str = "3d2ee51ca2d5db578f328aa75e20aa22c0197c9a";
const LATE_SKILL: &str = "skill_catalog_late_refresh_probe";
const LATE_MARKER: &str = "SKILL_CATALOG_LATE_REFRESH_MARKER_7f3c2a91";

fn write_json(path: &Path, value: &Value) {
    fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
}

fn rows(path: &Path) -> Vec<Value> {
    fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

fn session_rows(home: &Path) -> Vec<Value> {
    fn walk(path: &Path, result: &mut Vec<Value>) {
        let Ok(entries) = fs::read_dir(path) else {
            return;
        };
        for entry in entries {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                walk(&entry.path(), result);
            } else if entry.path().extension().is_some_and(|ext| ext == "jsonl") {
                result.extend(rows(&entry.path()));
            }
        }
    }
    let mut result = Vec::new();
    walk(&home.join("sessions"), &mut result);
    result
}

fn send_line(session: &ConsoleSession, text: &str) -> io::Result<()> {
    session.send(text)?;
    std::thread::sleep(Duration::from_millis(250));
    session.send("\r")
}

fn wait_for(
    session: &ConsoleSession,
    root: &Path,
    seconds: u64,
    predicate: impl Fn() -> bool,
) -> bool {
    let until = Instant::now() + Duration::from_secs(seconds);
    while Instant::now() < until {
        let _ = fs::write(root.join("terminal.txt"), session.transcript());
        if predicate() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    let _ = fs::write(root.join("terminal.txt"), session.transcript());
    false
}

fn filtered_path() -> std::ffi::OsString {
    let child_path = std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .filter(|entry| {
            !entry.components().any(|part| {
                part.as_os_str()
                    .to_string_lossy()
                    .eq_ignore_ascii_case("WindowsApps")
            })
        })
        .collect::<Vec<_>>();
    std::env::join_paths(child_path).unwrap()
}

fn json_escape(path: &Path) -> String {
    serde_json::to_string(&path.to_str().unwrap().to_lowercase()).unwrap()
}

fn write_skill(dir: &Path, name: &str, marker: &str) {
    fs::create_dir_all(dir).unwrap();
    fs::write(
        dir.join("SKILL.md"),
        format!(
            "---\nname: {name}\ndescription: Owned model-free catalogue freshness probe skill.\n---\nMarker {marker}. Do not invoke this skill.\n"
        ),
    )
    .unwrap();
}

fn git_identity(root: &Path) {
    let status = Command::new(std::env::var_os("HARNESS_GIT").unwrap_or_else(|| "git.exe".into()))
        .args(["init", "-q"])
        .current_dir(root)
        .status()
        .unwrap();
    assert!(status.success(), "owned workspace git init failed");
    fs::write(root.join("README.md"), "skill catalog refresh workspace\n").unwrap();
}

fn contains_marker(body: &str, marker: &str) -> bool {
    body.contains(marker)
        || body.contains(
            &serde_json::to_string(marker)
                .unwrap()
                .trim_matches('"')
                .to_string(),
        )
}

#[test]
#[ignore = "explicit owned ordinary TUI + canned local Responses; no live model"]
fn ordinary_tui_host_skill_catalog_after_mid_turn_compaction() {
    let exe = PathBuf::from(
        std::env::var_os("HARNESS_NATIVE_CODEX")
            .expect("set HARNESS_NATIVE_CODEX to the pinned original native executable"),
    );
    let experimental_context =
        match std::env::var("HARNESS_CATALOG_EXPERIMENTAL_CONTEXT").as_deref() {
            Ok("true") => true,
            Ok("false") | Err(_) => false,
            _ => panic!("HARNESS_CATALOG_EXPERIMENTAL_CONTEXT must be true or false"),
        };
    assert!(
        exe.is_absolute(),
        "native executable must be selected by absolute path"
    );
    assert!(
        exe.is_file(),
        "ordinary CLI executable missing: {}",
        exe.display()
    );
    let sha = build_identity::hash_file(&exe).unwrap();
    assert_eq!(sha, EXPECTED_SHA, "ordinary CLI SHA drifted");
    assert_eq!(fs::metadata(&exe).unwrap().len(), EXPECTED_LEN);

    let evidence = tempfile::Builder::new()
        .prefix("skill-catalog-refresh-")
        .tempdir()
        .unwrap()
        .keep();
    println!("skill catalog evidence: {}", evidence.display());
    let home = evidence.join("home");
    let workspace = evidence.join("workspace");
    let skills_home = home.join("skills");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&workspace).unwrap();
    git_identity(&workspace);
    write_skill(
        &skills_home.join(EARLY_SKILL),
        EARLY_SKILL,
        "SKILL_CATALOG_EARLY_MARKER",
    );

    let canned = CannedResponses::spawn(&evidence).unwrap();
    let trusted = json_escape(&workspace);
    fs::write(
        home.join("config.toml"),
        format!(
            r#"model = "gpt-6-astra"
model_provider = "{PROVIDER_ID}"
model_reasoning_effort = "low"
approval_policy = "never"
sandbox_mode = "danger-full-access"
web_search = "disabled"
model_auto_compact_token_limit = 8000
model_auto_compact_token_limit_scope = "total"
model_context_window = 20000
compact_prompt = "{COMPACT_PROMPT}"
[model_providers.{PROVIDER_ID}]
name = "Canned skill catalog Responses"
base_url = "{}"
env_key = "{API_KEY_ENV}"
wire_api = "responses"
requires_openai_auth = false
supports_websockets = false
request_max_retries = 0
stream_max_retries = 0
[windows]
sandbox = "unelevated"
[features]
hooks = false
apps = false
multi_agent = false
multi_agent_v2 = false
memories = false
goals = false
plugins = true
skill_search = true
deferred_executor = false
executor_capability_discovery = false
deferred_tool_world_state = false
skip_host_skill_discovery = false
[features.context_management]
experimental_mode = {experimental_context}
[projects.{trusted}]
trust_level = "trusted"
"#,
            canned.base_url()
        ),
    )
    .unwrap();
    fs::write(
        home.join("auth.json"),
        serde_json::to_vec_pretty(&json!({
            "auth_mode": "apikey",
            "OPENAI_API_KEY": API_KEY
        }))
        .unwrap(),
    )
    .unwrap();

    write_json(
        &evidence.join("identity.json"),
        &json!({
            "codex_exe": exe,
            "sha256": sha,
            "bytes": EXPECTED_LEN,
            "version": "0.153.4",
            "pinned_source": PINNED_SOURCE,
            "provider": PROVIDER_ID,
            "base_url": canned.base_url(),
            "transport": "owned canned local Responses HTTP on 127.0.0.1 ephemeral port; not reserved openai and not the live 10100 route",
            "model": "gpt-6-astra",
            "experimental_context": experimental_context,
            "hooks": false
        }),
    );

    let mut command = CommandSpec::new(&exe);
    command.args = vec!["--no-alt-screen".into()];
    command.current_dir = Some(workspace.clone());
    command
        .env
        .insert("CODEX_HOME".into(), Some(home.clone().into_os_string()));
    command.env.insert(API_KEY_ENV.into(), Some(API_KEY.into()));
    command.env.insert("PATH".into(), Some(filtered_path()));
    command.env.insert("OPENAI_API_KEY".into(), None);
    command.env.insert("CODEX_API_KEY".into(), None);

    let mut spec = ConsoleSpec::new(command);
    spec.limits.memory_bytes = Some(512 * 1024 * 1024);
    spec.limits.cpu_percent = Some(50.0);
    let session = ConsoleSession::spawn(spec).unwrap();
    write_json(
        &evidence.join("started.json"),
        &json!({
            "pid": session.identity().pid,
            "creation_time": session.identity().creation_time
        }),
    );

    let ready = wait_for(&session, &evidence, 40, || {
        let text = session.transcript();
        text.contains("gpt-6-astra") || text.contains("OpenAI Codex") || text.contains("Codex")
    });
    write_json(&evidence.join("ready.json"), &json!({"tui_ready": ready}));
    if !ready {
        persist_partial(&evidence, &home, &canned, &session, "tui-not-ready");
        panic!(
            "ordinary TUI did not become ready; evidence {}",
            evidence.display()
        );
    }
    std::thread::sleep(Duration::from_secs(2));
    send_line(&session, "/rename Skill catalog refresh").unwrap();
    let named = wait_for(&session, &evidence, 20, || {
        rows(&home.join("session_index.jsonl"))
            .iter()
            .any(|row| row["thread_name"] == "Skill catalog refresh")
    });
    write_json(&evidence.join("named.json"), &json!({"named": named}));
    if !named {
        persist_partial(&evidence, &home, &canned, &session, "rename-failed");
        panic!("literal /rename did not confirm; no model prompt submitted");
    }

    send_line(&session, USER_PROMPT).unwrap();
    let first = canned.wait_for(Duration::from_secs(45), |reqs| {
        reqs.iter()
            .any(|req| req.kind == RequestKind::FirstSampling)
    });
    write_json(
        &evidence.join("first-http.json"),
        &json!({
            "first_sampling": first,
            "kinds": canned.requests().iter().map(|r| format!("{:?}", r.kind)).collect::<Vec<_>>()
        }),
    );
    if !first {
        persist_partial(&evidence, &home, &canned, &session, "no-first-http");
        panic!(
            "canned transport never received the first sampling request; evidence {}",
            evidence.display()
        );
    }

    let tool_seen = wait_for(&session, &evidence, 40, || {
        session_rows(&home).iter().any(|row| {
            row["type"] == "response_item"
                && row["payload"]["type"] == "function_call"
                && row["payload"]["call_id"] == CALL_ID
        })
    });
    write_json(
        &evidence.join("tool.json"),
        &json!({"tool_seen": tool_seen}),
    );

    assert!(tool_seen, "matching native tool call was not observed");
    let tool_in_flight = !session_rows(&home).iter().any(|row| {
        row["type"] == "response_item"
            && row["payload"]["type"] == "function_call_output"
            && row["payload"]["call_id"] == CALL_ID
    });
    assert!(
        tool_in_flight,
        "tool finished before the late skill could be created"
    );
    write_skill(&skills_home.join(LATE_SKILL), LATE_SKILL, LATE_MARKER);
    write_json(
        &evidence.join("late-skill.json"),
        &json!({
            "name": LATE_SKILL,
            "marker": LATE_MARKER,
            "unix_ms": SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis(),
            "tool_in_flight": tool_in_flight,
            "path": skills_home.join(LATE_SKILL).join("SKILL.md")
        }),
    );
    // Wait for both observed requests, not merely the start of compaction.
    // The canned exec yields after 30s, allowing the ordinary 10s watcher
    // throttle to pass. Elapsed time is not itself proof of watcher delivery.
    let compact_and_continuation = canned.wait_for(Duration::from_secs(90), |reqs| {
        reqs.iter().any(|req| req.kind == RequestKind::Compact)
            && reqs.iter().any(|req| req.kind == RequestKind::Continuation)
    });
    let continuation = canned
        .requests()
        .into_iter()
        .find(|req| req.kind == RequestKind::Continuation);

    if compact_and_continuation {
        assert!(
            wait_for(&session, &evidence, 20, || session_rows(&home).iter().any(
                |row| row["type"] == "event_msg" && row["payload"]["type"] == "task_complete"
            )),
            "first turn did not finish before positive control"
        );
        send_line(&session, NEXT_TURN_PROMPT).unwrap();
        let _ = canned.wait_for(Duration::from_secs(40), |reqs| {
            reqs.iter().any(|req| req.kind == RequestKind::NextTurn)
        });
        let _ = wait_for(&session, &evidence, 20, || {
            session_rows(&home)
                .iter()
                .filter(|row| {
                    row["type"] == "event_msg" && row["payload"]["type"] == "task_complete"
                })
                .count()
                >= 2
        });
    }

    send_line(&session, "/quit").unwrap();
    let result = session
        .wait(
            Deadline::after(Duration::from_secs(25)).unwrap(),
            &Cancellation::default(),
            Duration::from_secs(5),
        )
        .unwrap();
    fs::write(evidence.join("terminal.txt"), &result.transcript).unwrap();
    write_json(
        &evidence.join("process.json"),
        &json!({"outcome": result.outcome, "truncated": result.output_truncated}),
    );

    let requests = canned.requests();
    let sampling = native_sampling::read(&home.join("logs_2.sqlite"))
        .expect("native sampling evidence unavailable");
    let rollout = session_rows(&home);
    fs::write(
        evidence.join("rollout.jsonl"),
        rollout
            .iter()
            .map(|row| serde_json::to_string(row).unwrap())
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .unwrap();
    write_json(&evidence.join("sampling.json"), &json!(sampling));
    write_json(
        &evidence.join("http-summary.json"),
        &json!(requests
            .iter()
            .map(|req| json!({
                "seq": req.seq,
                "kind": format!("{:?}", req.kind),
                "path": req.path,
                "unix_ms": req.unix_ms,
                "contains_late_skill": contains_marker(&req.body, LATE_SKILL) || contains_marker(&req.body, LATE_MARKER),
                "contains_early_skill": contains_marker(&req.body, EARLY_SKILL),
                "contains_compact_prompt": contains_marker(&req.body, COMPACT_PROMPT),
                "contains_summary": contains_marker(&req.body, SUMMARY_TEXT),
            }))
            .collect::<Vec<_>>()),
    );

    for record in &sampling {
        assert_eq!(record["model"], "gpt-6-astra");
        assert_eq!(record["context_management"], experimental_context);
    }
    assert!(!sampling.is_empty(), "missing native sampling identity");
    assert!(
        requests
            .iter()
            .any(|req| req.kind == RequestKind::FirstSampling),
        "missing first sampling request"
    );
    for req in requests.iter().filter(|req| req.method == "POST") {
        let body: Value = serde_json::from_str(&req.body).expect("complete native request JSON");
        assert_eq!(body["model"], "gpt-6-astra");
    }
    let compact = requests.iter().any(|req| req.kind == RequestKind::Compact);

    let report = json!({
        "passed_first_http": true,
        "experimental_context": experimental_context,
        "compaction_observed": compact,
        "continuation_observed": continuation.is_some(),
        "late_skill_in_first_continuation": continuation.as_ref().map(|req| contains_marker(&req.body, LATE_SKILL) || contains_marker(&req.body, LATE_MARKER)),
        "late_skill_in_next_turn": requests.iter().any(|req| req.kind == RequestKind::NextTurn && (contains_marker(&req.body, LATE_SKILL) || contains_marker(&req.body, LATE_MARKER))),
        "runtime_limit": if compact { Value::Null } else { json!("fixture did not observe an automatic compact request after the outstanding tool call; report the observed request kinds rather than unsupported") },
        "process_reason": format!("{:?}", result.outcome.reason),
        "exit_code": result.outcome.exit_code,
        "evidence": evidence,
    });
    write_json(&evidence.join("acceptance.json"), &report);
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
    assert_eq!(result.outcome.reason, StopReason::Exited);
    assert_eq!(result.outcome.exit_code, 0);
    assert_eq!(result.outcome.job.active_processes, 0);
    assert!(!result.output_truncated);
    assert!(
        compact_and_continuation,
        "automatic compaction/first continuation missing; see acceptance.json"
    );
    assert!(
        rollout.iter().any(|row| row["type"] == "compacted"),
        "native compaction event missing"
    );
    assert_eq!(
        report["late_skill_in_next_turn"], true,
        "fresh-turn catalogue positive control failed"
    );
    let kinds = requests
        .iter()
        .filter(|request| request.method == "POST")
        .map(|request| request.kind)
        .collect::<Vec<_>>();
    assert_eq!(
        kinds,
        vec![
            RequestKind::FirstSampling,
            RequestKind::Compact,
            RequestKind::Continuation,
            RequestKind::NextTurn
        ],
        "unexpected sampling order or retries"
    );
    let first = requests
        .iter()
        .find(|request| request.kind == RequestKind::FirstSampling)
        .unwrap();
    assert!(contains_marker(&first.body, EARLY_SKILL));
    assert!(
        !contains_marker(&first.body, LATE_SKILL) && !contains_marker(&first.body, LATE_MARKER)
    );
    for output in rollout.iter().filter(|row| {
        row["type"] == "response_item"
            && row["payload"]["type"] == "function_call_output"
            && row["payload"]["call_id"] == CALL_ID
    }) {
        let text = serde_json::to_string(output).unwrap();
        assert!(!contains_marker(&text, LATE_SKILL) && !contains_marker(&text, LATE_MARKER));
    }
    assert!(!SUMMARY_TEXT.contains(LATE_SKILL) && !SUMMARY_TEXT.contains(LATE_MARKER));
}

fn persist_partial(
    evidence: &Path,
    home: &Path,
    canned: &CannedResponses,
    session: &ConsoleSession,
    stage: &str,
) {
    let _ = fs::write(evidence.join("terminal.txt"), session.transcript());
    write_json(
        &evidence.join("partial.json"),
        &json!({
            "stage": stage,
            "http": canned.requests().iter().map(|req| format!("{:?}", req.kind)).collect::<Vec<_>>(),
            "session_index": rows(&home.join("session_index.jsonl")),
        }),
    );
}
