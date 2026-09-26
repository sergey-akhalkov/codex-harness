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
    API_KEY, API_KEY_ENV, CALL_ID, CHILD_CALL_ID, CHILD_PROMPT, COMPACT_PROMPT, CannedResponses,
    EARLY_SKILL, NEXT_TURN_PROMPT, PROVIDER_ID, RESUME_PROMPT, RETIRE_PROMPT, RequestKind,
    SUMMARY_TEXT, UNRELATED_PROMPT, USER_PROMPT,
};
use std::{
    fs, io,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const EXPECTED_SHA: &str = "e4c11374bd9de8ad5c3b7617fd4654bb7839901edb0863f9930666863c7a021b";
const EXPECTED_LEN: u64 = 307_150_128;
const PINNED_SOURCE: &str = "installed-cli-0.155.0";
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

fn write_skill_described(dir: &Path, name: &str, description: &str, marker: &str) {
    fs::create_dir_all(dir).unwrap();
    fs::write(
        dir.join("SKILL.md"),
        format!(
            "---\nname: {name}\ndescription: {description}\n---\nMarker {marker}. Do not invoke this skill.\n"
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
            "version": "0.155.0",
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
    let continuation_req = requests
        .iter()
        .find(|request| request.kind == RequestKind::Continuation)
        .unwrap();
    assert!(
        !contains_marker(&continuation_req.body, COMPACT_PROMPT),
        "compact_prompt must not be treated as continuation catalogue delivery"
    );
    assert!(
        !contains_marker(&continuation_req.body, LATE_SKILL)
            && !contains_marker(&continuation_req.body, LATE_MARKER),
        "hooks-off compact continuation still omits a mid-turn skill"
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

#[test]
#[ignore = "explicit owned ordinary TUI + canned local Responses; no live model"]
fn ordinary_tui_same_turn_continuation_without_compact() {
    let exe = PathBuf::from(
        std::env::var_os("HARNESS_NATIVE_CODEX")
            .expect("set HARNESS_NATIVE_CODEX to the pinned original native executable"),
    );
    assert!(exe.is_absolute() && exe.is_file());
    let sha = build_identity::hash_file(&exe).unwrap();
    assert_eq!(sha, EXPECTED_SHA, "ordinary CLI SHA drifted");
    assert_eq!(fs::metadata(&exe).unwrap().len(), EXPECTED_LEN);

    let evidence = tempfile::Builder::new()
        .prefix("skill-same-turn-")
        .tempdir()
        .unwrap()
        .keep();
    println!("skill same-turn evidence: {}", evidence.display());
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

    let canned = CannedResponses::spawn_short(&evidence).unwrap();
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
model_auto_compact_token_limit = 1000000
model_context_window = 20000
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
    let ready = wait_for(&session, &evidence, 40, || {
        let text = session.transcript();
        text.contains("gpt-6-astra") || text.contains("OpenAI Codex") || text.contains("Codex")
    });
    if !ready {
        persist_partial(&evidence, &home, &canned, &session, "tui-not-ready");
        panic!(
            "ordinary TUI did not become ready; evidence {}",
            evidence.display()
        );
    }
    std::thread::sleep(Duration::from_secs(2));
    send_line(&session, "/rename Skill same turn").unwrap();
    let named = wait_for(&session, &evidence, 20, || {
        rows(&home.join("session_index.jsonl"))
            .iter()
            .any(|row| row["thread_name"] == "Skill same turn")
    });
    if !named {
        persist_partial(&evidence, &home, &canned, &session, "rename-failed");
        panic!("literal /rename did not confirm");
    }

    send_line(&session, USER_PROMPT).unwrap();
    let first = canned.wait_for(Duration::from_secs(45), |reqs| {
        reqs.iter()
            .any(|req| req.kind == RequestKind::FirstSampling)
    });
    if !first {
        persist_partial(&evidence, &home, &canned, &session, "no-first-http");
        panic!("missing first sampling; evidence {}", evidence.display());
    }
    let tool_seen = wait_for(&session, &evidence, 40, || {
        session_rows(&home).iter().any(|row| {
            row["type"] == "response_item"
                && row["payload"]["type"] == "function_call"
                && row["payload"]["call_id"] == CALL_ID
        })
    });
    assert!(tool_seen, "matching native tool call was not observed");
    write_skill(&skills_home.join(LATE_SKILL), LATE_SKILL, LATE_MARKER);
    let continuation = canned.wait_for(Duration::from_secs(45), |reqs| {
        reqs.iter().any(|req| req.kind == RequestKind::Continuation)
            && !reqs.iter().any(|req| req.kind == RequestKind::Compact)
    });
    if continuation {
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
    let requests = canned.requests();
    let continuation_req = requests
        .iter()
        .find(|request| request.kind == RequestKind::Continuation);
    let late_in_continuation = continuation_req.is_some_and(|req| {
        contains_marker(&req.body, LATE_SKILL) || contains_marker(&req.body, LATE_MARKER)
    });
    let late_in_next = requests.iter().any(|req| {
        req.kind == RequestKind::NextTurn
            && (contains_marker(&req.body, LATE_SKILL) || contains_marker(&req.body, LATE_MARKER))
    });
    let compact = requests.iter().any(|req| req.kind == RequestKind::Compact);
    let report = json!({
        "same_turn_continuation": continuation,
        "compact_observed": compact,
        "late_skill_in_same_turn_continuation": late_in_continuation,
        "late_skill_in_next_turn": late_in_next,
        "kinds": requests.iter().map(|req| format!("{:?}", req.kind)).collect::<Vec<_>>(),
        "process_reason": format!("{:?}", result.outcome.reason),
        "evidence": evidence,
    });
    write_json(&evidence.join("acceptance.json"), &report);
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
    assert!(
        continuation,
        "same-turn continuation missing; see {}",
        evidence.display()
    );
    assert!(!compact, "compact ran; this probe is the non-compact path");
    assert_eq!(
        report["late_skill_in_next_turn"], true,
        "fresh-turn catalogue positive control failed"
    );
    assert!(
        !late_in_continuation,
        "hooks-off same-turn continuation must not be mistaken for catalogue delivery; see {}",
        evidence.display()
    );
}

#[test]
#[ignore = "explicit owned ordinary TUI + canned local Responses; no live model"]
fn ordinary_tui_same_turn_reads_live_skill_body() {
    let exe = pinned_exe();
    let evidence = tempfile::Builder::new()
        .prefix("skill-same-turn-body-")
        .tempdir()
        .unwrap()
        .keep();
    println!("skill same-turn-body evidence: {}", evidence.display());
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
    let canned = CannedResponses::spawn_short(&evidence).unwrap();
    write_probe_config(&home, &workspace, &canned, "", false);
    let session = spawn_probe_tui(&exe, &home, &workspace);
    ready_named(&session, &evidence, &home, "SkillSameTurnBody");
    send_line(&session, USER_PROMPT).unwrap();
    assert!(
        canned.wait_for(Duration::from_secs(45), |reqs| reqs
            .iter()
            .any(|req| req.kind == RequestKind::FirstSampling)),
        "missing first sampling"
    );
    let late = skills_home.join(LATE_SKILL);
    write_skill(&late, LATE_SKILL, LATE_MARKER);
    canned.set_skill_read(&late.join("SKILL.md"));
    let continued = canned.wait_for(Duration::from_secs(45), |reqs| {
        reqs.iter().any(|req| req.kind == RequestKind::Continuation)
    });
    let body = canned.wait_for(Duration::from_secs(45), |reqs| {
        reqs.iter().any(|req| req.kind == RequestKind::SkillBody)
    });
    send_line(&session, "/quit").unwrap();
    let _ = session
        .wait(
            Deadline::after(Duration::from_secs(25)).unwrap(),
            &Cancellation::default(),
            Duration::from_secs(5),
        )
        .unwrap();
    let requests = canned.requests();
    let continuation = requests
        .iter()
        .find(|req| req.kind == RequestKind::Continuation);
    let follow = requests
        .iter()
        .find(|req| req.kind == RequestKind::SkillBody);
    let report = json!({
        "continuation": continued,
        "skill_body_http": body,
        "late_in_continuation_catalogue": continuation.as_ref().map(|req| contains_marker(&req.body, LATE_SKILL) || contains_marker(&req.body, LATE_MARKER)),
        "late_marker_in_followup": follow.as_ref().map(|req| contains_marker(&req.body, LATE_MARKER)),
        "kinds": requests.iter().map(|req| format!("{:?}", req.kind)).collect::<Vec<_>>(),
        "evidence": evidence,
    });
    write_json(&evidence.join("acceptance.json"), &report);
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
    assert!(
        continued,
        "same-turn continuation missing; see {}",
        evidence.display()
    );
    assert!(
        body,
        "same-turn live skill body follow-up missing; see {}",
        evidence.display()
    );
    assert_eq!(
        report["late_in_continuation_catalogue"], false,
        "catalogue injection is still next-turn"
    );
    assert_eq!(
        report["late_marker_in_followup"], true,
        "same-turn exec did not observe the live SKILL.md revision"
    );
}

#[test]
#[ignore = "explicit owned ordinary TUI + canned local Responses; no live model"]
fn ordinary_tui_same_turn_runs_skills_identity() {
    let exe = pinned_exe();
    let evidence = tempfile::Builder::new()
        .prefix("skill-same-turn-identity-")
        .tempdir()
        .unwrap()
        .keep();
    println!("skill same-turn-identity evidence: {}", evidence.display());
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
    let canned = CannedResponses::spawn_short(&evidence).unwrap();
    write_probe_config(&home, &workspace, &canned, "", false);
    let session = spawn_probe_tui(&exe, &home, &workspace);
    ready_named(&session, &evidence, &home, "SkillSameTurnIdentity");
    send_line(&session, USER_PROMPT).unwrap();
    assert!(
        canned.wait_for(Duration::from_secs(45), |reqs| reqs
            .iter()
            .any(|req| req.kind == RequestKind::FirstSampling)),
        "missing first sampling"
    );
    let late = skills_home.join(LATE_SKILL);
    write_skill(&late, LATE_SKILL, LATE_MARKER);
    let harness = env!("CARGO_BIN_EXE_codex-harness").replace('\'', "''");
    let path = late.to_string_lossy().replace('\'', "''");
    canned.set_exec_cmd(format!(
        "& '{harness}' skills identity --path '{path}' --operation update"
    ));
    let continued = canned.wait_for(Duration::from_secs(45), |reqs| {
        reqs.iter().any(|req| req.kind == RequestKind::Continuation)
    });
    let body = canned.wait_for(Duration::from_secs(45), |reqs| {
        reqs.iter().any(|req| req.kind == RequestKind::SkillBody)
    });
    send_line(&session, "/quit").unwrap();
    let _ = session
        .wait(
            Deadline::after(Duration::from_secs(25)).unwrap(),
            &Cancellation::default(),
            Duration::from_secs(5),
        )
        .unwrap();
    let follow = canned
        .requests()
        .into_iter()
        .find(|req| req.kind == RequestKind::SkillBody);
    let report = json!({
        "continuation": continued,
        "skill_body_http": body,
        "identity_in_followup": follow.as_ref().map(|req| {
            contains_marker(&req.body, "delivery_complete") || contains_marker(&req.body, LATE_SKILL)
        }),
        "tokens_refunded_true": follow.as_ref().map(|req| contains_marker(&req.body, "\"tokens_refunded\": true") || contains_marker(&req.body, "tokens_refunded\":true")),
        "kinds": canned.requests().iter().map(|req| format!("{:?}", req.kind)).collect::<Vec<_>>(),
        "evidence": evidence,
    });
    write_json(&evidence.join("acceptance.json"), &report);
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
    assert!(continued);
    assert!(
        body,
        "identity follow-up missing; see {}",
        evidence.display()
    );
    assert_eq!(report["identity_in_followup"], true);
    assert_eq!(report["tokens_refunded_true"], false);
}

#[test]
#[ignore = "explicit owned ordinary TUI + canned local Responses; no live model"]
fn ordinary_tui_same_turn_identity_sees_updated_revision() {
    let exe = pinned_exe();
    let evidence = tempfile::Builder::new()
        .prefix("skill-same-turn-update-")
        .tempdir()
        .unwrap()
        .keep();
    println!("skill same-turn-update evidence: {}", evidence.display());
    let home = evidence.join("home");
    let workspace = evidence.join("workspace");
    let skills_home = home.join("skills");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&workspace).unwrap();
    git_identity(&workspace);
    let early = skills_home.join(EARLY_SKILL);
    write_skill(&early, EARLY_SKILL, "SKILL_CATALOG_EARLY_MARKER");
    let canned = CannedResponses::spawn_short(&evidence).unwrap();
    write_probe_config(&home, &workspace, &canned, "", false);
    let session = spawn_probe_tui(&exe, &home, &workspace);
    ready_named(&session, &evidence, &home, "SkillSameTurnUpdate");
    send_line(&session, USER_PROMPT).unwrap();
    assert!(
        canned.wait_for(Duration::from_secs(45), |reqs| reqs
            .iter()
            .any(|req| req.kind == RequestKind::FirstSampling)),
        "missing first sampling"
    );
    write_skill(&early, EARLY_SKILL, "SKILL_CATALOG_UPDATED_MARKER");
    let harness = env!("CARGO_BIN_EXE_codex-harness");
    let outside = Command::new(harness)
        .args(["skills", "identity", "--path"])
        .arg(&early)
        .arg("--operation")
        .arg("update")
        .output()
        .unwrap();
    assert!(outside.status.success());
    let expected: Value = serde_json::from_slice(&outside.stdout).unwrap();
    let revision = expected["revision"].as_str().unwrap().to_owned();
    let harness_ps = harness.replace('\'', "''");
    let path = early.to_string_lossy().replace('\'', "''");
    canned.set_exec_cmd(format!(
        "& '{harness_ps}' skills identity --path '{path}' --operation update"
    ));
    let body = canned.wait_for(Duration::from_secs(45), |reqs| {
        reqs.iter().any(|req| req.kind == RequestKind::SkillBody)
    });
    send_line(&session, "/quit").unwrap();
    let _ = session
        .wait(
            Deadline::after(Duration::from_secs(25)).unwrap(),
            &Cancellation::default(),
            Duration::from_secs(5),
        )
        .unwrap();
    let follow = canned
        .requests()
        .into_iter()
        .find(|req| req.kind == RequestKind::SkillBody);
    let report = json!({
        "skill_body_http": body,
        "expected_revision": revision,
        "revision_in_followup": follow.as_ref().map(|req| contains_marker(&req.body, &revision)),
        "kinds": canned.requests().iter().map(|req| format!("{:?}", req.kind)).collect::<Vec<_>>(),
        "evidence": evidence,
    });
    write_json(&evidence.join("acceptance.json"), &report);
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
    assert!(
        body,
        "update identity follow-up missing; see {}",
        evidence.display()
    );
    assert_eq!(report["revision_in_followup"], true);
}

#[test]
#[ignore = "explicit owned ordinary TUI + canned local Responses; no live model"]
fn ordinary_tui_same_turn_identity_after_delete_is_incomplete() {
    let exe = pinned_exe();
    let evidence = tempfile::Builder::new()
        .prefix("skill-same-turn-retire-")
        .tempdir()
        .unwrap()
        .keep();
    println!("skill same-turn-retire evidence: {}", evidence.display());
    let home = evidence.join("home");
    let workspace = evidence.join("workspace");
    let skills_home = home.join("skills");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&workspace).unwrap();
    git_identity(&workspace);
    let early = skills_home.join(EARLY_SKILL);
    write_skill(&early, EARLY_SKILL, "SKILL_CATALOG_EARLY_MARKER");
    let canned = CannedResponses::spawn_short(&evidence).unwrap();
    write_probe_config(&home, &workspace, &canned, "", false);
    let session = spawn_probe_tui(&exe, &home, &workspace);
    ready_named(&session, &evidence, &home, "SkillSameTurnRetire");
    send_line(&session, USER_PROMPT).unwrap();
    assert!(
        canned.wait_for(Duration::from_secs(45), |reqs| reqs
            .iter()
            .any(|req| req.kind == RequestKind::FirstSampling)),
        "missing first sampling"
    );
    let harness = env!("CARGO_BIN_EXE_codex-harness").replace('\'', "''");
    let path = early.to_string_lossy().replace('\'', "''");
    fs::remove_dir_all(&early).unwrap();
    canned.set_exec_cmd(format!(
        "& '{harness}' skills identity --path '{path}' --operation retire"
    ));
    let body = canned.wait_for(Duration::from_secs(45), |reqs| {
        reqs.iter().any(|req| req.kind == RequestKind::SkillBody)
    });
    send_line(&session, "/quit").unwrap();
    let _ = session
        .wait(
            Deadline::after(Duration::from_secs(25)).unwrap(),
            &Cancellation::default(),
            Duration::from_secs(5),
        )
        .unwrap();
    let continuation = canned
        .requests()
        .into_iter()
        .find(|req| req.kind == RequestKind::Continuation);
    let follow = canned
        .requests()
        .into_iter()
        .find(|req| req.kind == RequestKind::SkillBody);
    let report = json!({
        "skill_body_http": body,
        "early_in_continuation_catalogue": continuation.as_ref().map(|req| contains_marker(&req.body, EARLY_SKILL)),
        "delivery_complete_true": follow.as_ref().map(|req| {
            contains_marker(&req.body, "\"delivery_complete\": true")
                || contains_marker(&req.body, "delivery_complete\":true")
        }),
        "tokens_refunded_true": follow.as_ref().map(|req| {
            contains_marker(&req.body, "\"tokens_refunded\": true")
                || contains_marker(&req.body, "tokens_refunded\":true")
        }),
        "kinds": canned.requests().iter().map(|req| format!("{:?}", req.kind)).collect::<Vec<_>>(),
        "evidence": evidence,
    });
    write_json(&evidence.join("acceptance.json"), &report);
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
    assert!(
        body,
        "retire identity follow-up missing; see {}",
        evidence.display()
    );
    assert_eq!(report["delivery_complete_true"], false);
    assert_eq!(report["tokens_refunded_true"], false);
}

#[test]
#[ignore = "explicit owned ordinary TUI + canned local Responses; no live model"]
fn ordinary_tui_same_turn_identity_honors_disablement() {
    let exe = pinned_exe();
    let evidence = tempfile::Builder::new()
        .prefix("skill-same-turn-disable-")
        .tempdir()
        .unwrap()
        .keep();
    println!("skill same-turn-disable evidence: {}", evidence.display());
    let home = evidence.join("home");
    let workspace = evidence.join("workspace");
    let skills_home = home.join("skills");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&workspace).unwrap();
    git_identity(&workspace);
    let early = skills_home.join(EARLY_SKILL);
    write_skill(&early, EARLY_SKILL, "SKILL_CATALOG_EARLY_MARKER");
    let canned = CannedResponses::spawn_short(&evidence).unwrap();
    write_probe_config(&home, &workspace, &canned, "", false);
    let session = spawn_probe_tui(&exe, &home, &workspace);
    ready_named(&session, &evidence, &home, "SkillSameTurnDisable");
    send_line(&session, USER_PROMPT).unwrap();
    assert!(
        canned.wait_for(Duration::from_secs(45), |reqs| reqs
            .iter()
            .any(|req| req.kind == RequestKind::FirstSampling)),
        "missing first sampling"
    );
    let disable = format!(
        "\n[[skills.config]]\npath = {}\nenabled = false\n",
        json_escape(&early.join("SKILL.md"))
    );
    write_probe_config(&home, &workspace, &canned, &disable, false);
    let harness = env!("CARGO_BIN_EXE_codex-harness").replace('\'', "''");
    let path = early.to_string_lossy().replace('\'', "''");
    let home_ps = home.to_string_lossy().replace('\'', "''");
    canned.set_exec_cmd(format!(
        "& '{harness}' skills identity --path '{path}' --codex-home '{home_ps}' --operation observe"
    ));
    let body = canned.wait_for(Duration::from_secs(45), |reqs| {
        reqs.iter().any(|req| req.kind == RequestKind::SkillBody)
    });
    send_line(&session, "/quit").unwrap();
    let _ = session
        .wait(
            Deadline::after(Duration::from_secs(25)).unwrap(),
            &Cancellation::default(),
            Duration::from_secs(5),
        )
        .unwrap();
    let follow = canned
        .requests()
        .into_iter()
        .find(|req| req.kind == RequestKind::SkillBody);
    let report = json!({
        "skill_body_http": body,
        "enabled_false": follow.as_ref().map(|req| {
            contains_marker(&req.body, "\"enabled\": false")
                || contains_marker(&req.body, "enabled\":false")
        }),
        "delivery_complete_true": follow.as_ref().map(|req| {
            contains_marker(&req.body, "\"delivery_complete\": true")
                || contains_marker(&req.body, "delivery_complete\":true")
        }),
        "tokens_refunded_true": follow.as_ref().map(|req| {
            contains_marker(&req.body, "\"tokens_refunded\": true")
                || contains_marker(&req.body, "tokens_refunded\":true")
        }),
        "kinds": canned.requests().iter().map(|req| format!("{:?}", req.kind)).collect::<Vec<_>>(),
        "evidence": evidence,
    });
    write_json(&evidence.join("acceptance.json"), &report);
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
    assert!(
        body,
        "disable identity follow-up missing; see {}",
        evidence.display()
    );
    assert_eq!(report["enabled_false"], true);
    assert_eq!(report["delivery_complete_true"], false);
    assert_eq!(report["tokens_refunded_true"], false);
}

#[test]
#[ignore = "explicit owned ordinary TUI + canned local Responses; no live model"]
fn ordinary_tui_resume_reconciles_disablement_and_new_skill() {
    let exe = PathBuf::from(
        std::env::var_os("HARNESS_NATIVE_CODEX")
            .expect("set HARNESS_NATIVE_CODEX to the pinned original native executable"),
    );
    assert!(exe.is_absolute() && exe.is_file());
    let sha = build_identity::hash_file(&exe).unwrap();
    assert_eq!(sha, EXPECTED_SHA);
    assert_eq!(fs::metadata(&exe).unwrap().len(), EXPECTED_LEN);

    let evidence = tempfile::Builder::new()
        .prefix("skill-resume-")
        .tempdir()
        .unwrap()
        .keep();
    println!("skill resume evidence: {}", evidence.display());
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

    let canned = CannedResponses::spawn_short(&evidence).unwrap();
    let trusted = json_escape(&workspace);
    let write_config = |disable_early: bool| {
        let disable = if disable_early {
            format!(
                "\n[[skills.config]]\npath = {}\nenabled = false\n",
                json_escape(&skills_home.join(EARLY_SKILL).join("SKILL.md"))
            )
        } else {
            String::new()
        };
        fs::write(
            home.join("config.toml"),
            format!(
                r#"model = "gpt-6-astra"
model_provider = "{PROVIDER_ID}"
model_reasoning_effort = "low"
approval_policy = "never"
sandbox_mode = "danger-full-access"
web_search = "disabled"
model_auto_compact_token_limit = 1000000
model_context_window = 20000
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
memories = false
goals = false
plugins = true
skill_search = true
[projects.{trusted}]
trust_level = "trusted"
{disable}
"#,
                canned.base_url()
            ),
        )
        .unwrap();
    };
    write_config(false);
    fs::write(
        home.join("auth.json"),
        serde_json::to_vec_pretty(&json!({
            "auth_mode": "apikey",
            "OPENAI_API_KEY": API_KEY
        }))
        .unwrap(),
    )
    .unwrap();

    let spawn = |args: Vec<std::ffi::OsString>| {
        let mut command = CommandSpec::new(&exe);
        command.args = args;
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
        ConsoleSession::spawn(spec).unwrap()
    };

    let session = spawn(vec!["--no-alt-screen".into()]);
    let ready = wait_for(&session, &evidence, 60, || {
        let text = session.transcript();
        text.contains("gpt-6-astra")
    });
    if !ready {
        persist_partial(&evidence, &home, &canned, &session, "tui-not-ready");
        panic!(
            "ordinary TUI did not become ready; evidence {}",
            evidence.display()
        );
    }
    std::thread::sleep(Duration::from_secs(3));
    send_line(&session, "/rename SkillResume").unwrap();
    let named = wait_for(&session, &evidence, 20, || {
        rows(&home.join("session_index.jsonl"))
            .iter()
            .any(|row| row["thread_name"] == "SkillResume")
    });
    if !named {
        persist_partial(&evidence, &home, &canned, &session, "rename-failed");
        panic!("literal /rename did not confirm");
    }
    send_line(&session, USER_PROMPT).unwrap();
    assert!(
        canned.wait_for(Duration::from_secs(45), |reqs| {
            reqs.iter()
                .any(|req| req.kind == RequestKind::FirstSampling)
        }),
        "missing first sampling"
    );
    assert!(
        canned.wait_for(Duration::from_secs(45), |reqs| {
            reqs.iter().any(|req| req.kind == RequestKind::Continuation)
        }),
        "first turn did not finish"
    );
    send_line(&session, "/quit").unwrap();
    let first_exit = session
        .wait(
            Deadline::after(Duration::from_secs(25)).unwrap(),
            &Cancellation::default(),
            Duration::from_secs(5),
        )
        .unwrap();
    assert_eq!(first_exit.outcome.reason, StopReason::Exited);

    write_skill(&skills_home.join(LATE_SKILL), LATE_SKILL, LATE_MARKER);
    write_config(true);

    let resumed = spawn(vec![
        "resume".into(),
        "--last".into(),
        "--no-alt-screen".into(),
    ]);
    let resume_ready = wait_for(&resumed, &evidence, 40, || {
        let text = resumed.transcript();
        text.contains("gpt-6-astra")
    });
    if !resume_ready {
        persist_partial(&evidence, &home, &canned, &resumed, "resume-tui-not-ready");
        panic!(
            "resume TUI did not become ready; evidence {}",
            evidence.display()
        );
    }
    std::thread::sleep(Duration::from_secs(2));
    send_line(&resumed, RESUME_PROMPT).unwrap();
    let resume_http = canned.wait_for(Duration::from_secs(45), |reqs| {
        reqs.iter().any(|req| req.kind == RequestKind::Resume)
    });
    send_line(&resumed, "/quit").unwrap();
    let _ = resumed
        .wait(
            Deadline::after(Duration::from_secs(25)).unwrap(),
            &Cancellation::default(),
            Duration::from_secs(5),
        )
        .unwrap();

    let requests = canned.requests();
    let first = requests
        .iter()
        .find(|req| req.kind == RequestKind::FirstSampling)
        .expect("first sampling");
    let resume = requests.iter().find(|req| req.kind == RequestKind::Resume);
    let report = json!({
        "resume_http": resume_http,
        "early_in_first": contains_marker(&first.body, EARLY_SKILL),
        "late_in_first": contains_marker(&first.body, LATE_SKILL),
        "early_in_resume": resume.as_ref().map(|req| contains_marker(&req.body, EARLY_SKILL)),
        "late_in_resume": resume.as_ref().map(|req| contains_marker(&req.body, LATE_SKILL) || contains_marker(&req.body, LATE_MARKER)),
        "kinds": requests.iter().map(|req| format!("{:?}", req.kind)).collect::<Vec<_>>(),
        "evidence": evidence,
    });
    write_json(&evidence.join("acceptance.json"), &report);
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
    assert_eq!(report["early_in_first"], true);
    assert_eq!(report["late_in_first"], false);
    assert!(
        resume_http,
        "resume sampling missing; see {}",
        evidence.display()
    );
    assert_eq!(
        report["early_in_resume"], true,
        "hooks-off resume still includes a skill disabled after the prior session"
    );
    assert_eq!(
        report["late_in_resume"], true,
        "new skill missing after resume"
    );
}

#[test]
#[ignore = "explicit owned ordinary TUI + canned local Responses; no live model"]
fn ordinary_tui_new_child_without_fork_sees_current_skills() {
    let exe = PathBuf::from(
        std::env::var_os("HARNESS_NATIVE_CODEX")
            .expect("set HARNESS_NATIVE_CODEX to the pinned original native executable"),
    );
    assert!(exe.is_absolute() && exe.is_file());
    let sha = build_identity::hash_file(&exe).unwrap();
    assert_eq!(sha, EXPECTED_SHA);
    assert_eq!(fs::metadata(&exe).unwrap().len(), EXPECTED_LEN);

    let evidence = tempfile::Builder::new()
        .prefix("skill-child-")
        .tempdir()
        .unwrap()
        .keep();
    println!("skill child evidence: {}", evidence.display());
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
    write_skill(&skills_home.join(LATE_SKILL), LATE_SKILL, LATE_MARKER);

    let canned = CannedResponses::spawn_child(&evidence).unwrap();
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
model_auto_compact_token_limit = 1000000
model_context_window = 20000
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
multi_agent = true
multi_agent_v2 = false
memories = false
goals = false
plugins = true
skill_search = true
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
    let ready = wait_for(&session, &evidence, 60, || {
        session.transcript().contains("gpt-6-astra")
    });
    if !ready {
        persist_partial(&evidence, &home, &canned, &session, "tui-not-ready");
        panic!(
            "ordinary TUI did not become ready; evidence {}",
            evidence.display()
        );
    }
    std::thread::sleep(Duration::from_secs(3));
    send_line(&session, "/rename SkillChild").unwrap();
    let named = wait_for(&session, &evidence, 20, || {
        rows(&home.join("session_index.jsonl"))
            .iter()
            .any(|row| row["thread_name"] == "SkillChild")
    });
    if !named {
        persist_partial(&evidence, &home, &canned, &session, "rename-failed");
        panic!("literal /rename did not confirm");
    }
    send_line(&session, USER_PROMPT).unwrap();
    let child_http = canned.wait_for(Duration::from_secs(60), |reqs| {
        reqs.iter()
            .any(|req| req.kind == RequestKind::ChildSampling)
    });
    send_line(&session, "/quit").unwrap();
    let _ = session
        .wait(
            Deadline::after(Duration::from_secs(25)).unwrap(),
            &Cancellation::default(),
            Duration::from_secs(5),
        )
        .unwrap();
    let requests = canned.requests();
    let child = requests
        .iter()
        .find(|req| req.kind == RequestKind::ChildSampling);
    let report = json!({
        "child_http": child_http,
        "early_in_child": child.as_ref().map(|req| contains_marker(&req.body, EARLY_SKILL)),
        "late_in_child": child.as_ref().map(|req| contains_marker(&req.body, LATE_SKILL) || contains_marker(&req.body, LATE_MARKER)),
        "child_prompt_in_child": child.as_ref().map(|req| contains_marker(&req.body, CHILD_PROMPT)),
        "kinds": requests.iter().map(|req| format!("{:?}", req.kind)).collect::<Vec<_>>(),
        "evidence": evidence,
    });
    write_json(&evidence.join("acceptance.json"), &report);
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
    assert!(
        child_http,
        "child sampling missing; see {}",
        evidence.display()
    );
    assert_eq!(report["early_in_child"], true);
    assert_eq!(report["late_in_child"], true);
}

#[test]
#[ignore = "explicit owned ordinary TUI + canned local Responses; no live model"]
fn ordinary_tui_running_child_continuation_after_mid_turn_skill() {
    let exe = PathBuf::from(
        std::env::var_os("HARNESS_NATIVE_CODEX")
            .expect("set HARNESS_NATIVE_CODEX to the pinned original native executable"),
    );
    assert!(exe.is_absolute() && exe.is_file());
    let sha = build_identity::hash_file(&exe).unwrap();
    assert_eq!(sha, EXPECTED_SHA);
    assert_eq!(fs::metadata(&exe).unwrap().len(), EXPECTED_LEN);

    let evidence = tempfile::Builder::new()
        .prefix("skill-running-child-")
        .tempdir()
        .unwrap()
        .keep();
    println!("skill running-child evidence: {}", evidence.display());
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

    let canned = CannedResponses::spawn_child(&evidence).unwrap();
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
model_auto_compact_token_limit = 1000000
model_context_window = 20000
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
multi_agent = true
multi_agent_v2 = false
memories = false
goals = false
plugins = true
skill_search = true
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
    let ready = wait_for(&session, &evidence, 60, || {
        session.transcript().contains("gpt-6-astra")
    });
    if !ready {
        persist_partial(&evidence, &home, &canned, &session, "tui-not-ready");
        panic!(
            "ordinary TUI did not become ready; evidence {}",
            evidence.display()
        );
    }
    std::thread::sleep(Duration::from_secs(3));
    send_line(&session, "/rename SkillRunningChild").unwrap();
    let named = wait_for(&session, &evidence, 20, || {
        rows(&home.join("session_index.jsonl"))
            .iter()
            .any(|row| row["thread_name"] == "SkillRunningChild")
    });
    if !named {
        persist_partial(&evidence, &home, &canned, &session, "rename-failed");
        panic!("literal /rename did not confirm");
    }
    send_line(&session, USER_PROMPT).unwrap();
    assert!(
        canned.wait_for(Duration::from_secs(45), |reqs| {
            reqs.iter()
                .any(|req| req.kind == RequestKind::ChildSampling)
        }),
        "child sampling missing"
    );
    let tool_seen = wait_for(&session, &evidence, 40, || {
        session_rows(&home).iter().any(|row| {
            row["type"] == "response_item"
                && row["payload"]["type"] == "function_call"
                && row["payload"]["call_id"] == CHILD_CALL_ID
        })
    });
    assert!(tool_seen, "child tool call was not observed");
    write_skill(&skills_home.join(LATE_SKILL), LATE_SKILL, LATE_MARKER);
    let continued = canned.wait_for(Duration::from_secs(45), |reqs| {
        reqs.iter()
            .any(|req| req.kind == RequestKind::ChildContinuation)
    });
    send_line(&session, "/quit").unwrap();
    let _ = session
        .wait(
            Deadline::after(Duration::from_secs(25)).unwrap(),
            &Cancellation::default(),
            Duration::from_secs(5),
        )
        .unwrap();
    let requests = canned.requests();
    let first_child = requests
        .iter()
        .find(|req| req.kind == RequestKind::ChildSampling)
        .expect("child sampling");
    let continuation = requests
        .iter()
        .find(|req| req.kind == RequestKind::ChildContinuation);
    let report = json!({
        "child_continuation": continued,
        "late_in_child_first": contains_marker(&first_child.body, LATE_SKILL) || contains_marker(&first_child.body, LATE_MARKER),
        "late_in_child_continuation": continuation.as_ref().map(|req| contains_marker(&req.body, LATE_SKILL) || contains_marker(&req.body, LATE_MARKER)),
        "early_in_child_continuation": continuation.as_ref().map(|req| contains_marker(&req.body, EARLY_SKILL)),
        "kinds": requests.iter().map(|req| format!("{:?}", req.kind)).collect::<Vec<_>>(),
        "evidence": evidence,
    });
    write_json(&evidence.join("acceptance.json"), &report);
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
    assert!(
        continued,
        "running child continuation missing; see {}",
        evidence.display()
    );
    assert_eq!(report["late_in_child_first"], false);
    assert_eq!(
        report["late_in_child_continuation"], false,
        "hooks-off running child continuation must not be mistaken for catalogue delivery"
    );
}

#[test]
#[ignore = "explicit owned ordinary TUI + canned local Responses; no live model"]
fn ordinary_tui_next_turn_disablement_and_unrelated_request() {
    let exe = pinned_exe();
    let evidence = tempfile::Builder::new()
        .prefix("skill-next-turn-")
        .tempdir()
        .unwrap()
        .keep();
    println!("skill next-turn evidence: {}", evidence.display());
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
    let canned = CannedResponses::spawn_short(&evidence).unwrap();
    write_probe_config(&home, &workspace, &canned, "", false);
    let session = spawn_probe_tui(&exe, &home, &workspace);
    ready_named(&session, &evidence, &home, "SkillNextTurn");
    send_line(&session, USER_PROMPT).unwrap();
    assert!(
        canned.wait_for(Duration::from_secs(45), |reqs| reqs
            .iter()
            .any(|req| req.kind == RequestKind::FirstSampling)),
        "missing first sampling"
    );
    assert!(
        canned.wait_for(Duration::from_secs(45), |reqs| reqs
            .iter()
            .any(|req| req.kind == RequestKind::Continuation)),
        "first turn did not finish"
    );
    write_skill(&skills_home.join(LATE_SKILL), LATE_SKILL, LATE_MARKER);
    let disable = format!(
        "\n[[skills.config]]\npath = {}\nenabled = false\n",
        json_escape(&skills_home.join(EARLY_SKILL).join("SKILL.md"))
    );
    write_probe_config(&home, &workspace, &canned, &disable, false);
    send_line(&session, UNRELATED_PROMPT).unwrap();
    let unrelated = canned.wait_for(Duration::from_secs(40), |reqs| {
        reqs.iter().any(|req| req.kind == RequestKind::Unrelated)
    });
    send_line(&session, NEXT_TURN_PROMPT).unwrap();
    let next = canned.wait_for(Duration::from_secs(40), |reqs| {
        reqs.iter().any(|req| req.kind == RequestKind::NextTurn)
    });
    send_line(&session, "/quit").unwrap();
    let _ = session
        .wait(
            Deadline::after(Duration::from_secs(25)).unwrap(),
            &Cancellation::default(),
            Duration::from_secs(5),
        )
        .unwrap();
    let requests = canned.requests();
    let unrelated_req = requests
        .iter()
        .find(|req| req.kind == RequestKind::Unrelated);
    let next_req = requests
        .iter()
        .find(|req| req.kind == RequestKind::NextTurn);
    let report = json!({
        "unrelated_http": unrelated,
        "next_http": next,
        "late_in_unrelated": unrelated_req.as_ref().map(|req| contains_marker(&req.body, LATE_SKILL) || contains_marker(&req.body, LATE_MARKER)),
        "late_marker_in_unrelated": unrelated_req.as_ref().map(|req| contains_marker(&req.body, LATE_MARKER)),
        "early_in_next": next_req.as_ref().map(|req| contains_marker(&req.body, EARLY_SKILL)),
        "late_in_next": next_req.as_ref().map(|req| contains_marker(&req.body, LATE_SKILL) || contains_marker(&req.body, LATE_MARKER)),
        "kinds": requests.iter().map(|req| format!("{:?}", req.kind)).collect::<Vec<_>>(),
        "evidence": evidence,
    });
    write_json(&evidence.join("acceptance.json"), &report);
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
    assert!(
        unrelated,
        "unrelated sampling missing; see {}",
        evidence.display()
    );
    assert!(
        next,
        "next-turn sampling missing; see {}",
        evidence.display()
    );
    assert_eq!(
        report["late_marker_in_unrelated"], false,
        "unrelated request loaded the skill body"
    );
    assert_eq!(report["late_in_next"], true);
    assert_eq!(
        report["early_in_next"], true,
        "hooks-off next turn still includes a skill disabled after the prior turn"
    );
}

#[test]
#[ignore = "explicit owned ordinary TUI + canned local Responses; no live model"]
fn ordinary_tui_manual_compact_after_late_skill() {
    let exe = pinned_exe();
    let evidence = tempfile::Builder::new()
        .prefix("skill-manual-compact-")
        .tempdir()
        .unwrap()
        .keep();
    println!("skill manual-compact evidence: {}", evidence.display());
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
    let canned = CannedResponses::spawn_short(&evidence).unwrap();
    write_probe_config(&home, &workspace, &canned, "", true);
    let session = spawn_probe_tui(&exe, &home, &workspace);
    ready_named(&session, &evidence, &home, "SkillManualCompact");
    send_line(&session, USER_PROMPT).unwrap();
    assert!(
        canned.wait_for(Duration::from_secs(45), |reqs| reqs
            .iter()
            .any(|req| req.kind == RequestKind::FirstSampling)),
        "missing first sampling"
    );
    assert!(
        canned.wait_for(Duration::from_secs(45), |reqs| reqs
            .iter()
            .any(|req| req.kind == RequestKind::Continuation)),
        "first turn did not finish"
    );
    write_skill(&skills_home.join(LATE_SKILL), LATE_SKILL, LATE_MARKER);
    send_line(&session, "/compact").unwrap();
    std::thread::sleep(Duration::from_secs(1));
    send_line(&session, "y").unwrap();
    let compact = canned.wait_for(Duration::from_secs(45), |reqs| {
        reqs.iter().any(|req| req.kind == RequestKind::Compact)
    });
    send_line(&session, NEXT_TURN_PROMPT).unwrap();
    let next = canned.wait_for(Duration::from_secs(40), |reqs| {
        reqs.iter().any(|req| req.kind == RequestKind::NextTurn)
    });
    send_line(&session, "/quit").unwrap();
    let _ = session
        .wait(
            Deadline::after(Duration::from_secs(25)).unwrap(),
            &Cancellation::default(),
            Duration::from_secs(5),
        )
        .unwrap();
    let requests = canned.requests();
    let compact_req = requests.iter().find(|req| req.kind == RequestKind::Compact);
    let continuation = requests.iter().find(|req| {
        req.kind == RequestKind::Continuation
            && compact_req.is_some_and(|compact| req.unix_ms >= compact.unix_ms)
    });
    let next_req = requests
        .iter()
        .find(|req| req.kind == RequestKind::NextTurn);
    let report = json!({
        "compact_http": compact,
        "next_http": next,
        "late_in_post_compact_continuation": continuation.as_ref().map(|req| contains_marker(&req.body, LATE_SKILL) || contains_marker(&req.body, LATE_MARKER)),
        "late_in_next": next_req.as_ref().map(|req| contains_marker(&req.body, LATE_SKILL) || contains_marker(&req.body, LATE_MARKER)),
        "kinds": requests.iter().map(|req| format!("{:?}", req.kind)).collect::<Vec<_>>(),
        "evidence": evidence,
    });
    write_json(&evidence.join("acceptance.json"), &report);
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
    assert!(
        compact,
        "manual compact missing; see {}",
        evidence.display()
    );
    assert_eq!(report["late_in_next"], true);
    assert_eq!(
        report["late_in_post_compact_continuation"], true,
        "manual compact after an idle turn should see a skill added while stopped"
    );
}

#[test]
#[ignore = "explicit owned ordinary TUI + canned local Responses; no live model"]
fn ordinary_tui_next_turn_update_description_and_delete_skill() {
    let exe = pinned_exe();
    let evidence = tempfile::Builder::new()
        .prefix("skill-update-retire-")
        .tempdir()
        .unwrap()
        .keep();
    println!("skill update-retire evidence: {}", evidence.display());
    let home = evidence.join("home");
    let workspace = evidence.join("workspace");
    let skills_home = home.join("skills");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&workspace).unwrap();
    git_identity(&workspace);
    const DESC_V1: &str = "SKILL_CATALOG_DESC_V1";
    const DESC_V2: &str = "SKILL_CATALOG_DESC_V2";
    write_skill_described(
        &skills_home.join(EARLY_SKILL),
        EARLY_SKILL,
        DESC_V1,
        "SKILL_CATALOG_EARLY_MARKER",
    );
    let canned = CannedResponses::spawn_short(&evidence).unwrap();
    write_probe_config(&home, &workspace, &canned, "", false);
    let session = spawn_probe_tui(&exe, &home, &workspace);
    ready_named(&session, &evidence, &home, "SkillUpdateRetire");
    send_line(&session, USER_PROMPT).unwrap();
    assert!(
        canned.wait_for(Duration::from_secs(45), |reqs| reqs
            .iter()
            .any(|req| req.kind == RequestKind::FirstSampling)),
        "missing first sampling"
    );
    assert!(
        canned.wait_for(Duration::from_secs(45), |reqs| reqs
            .iter()
            .any(|req| req.kind == RequestKind::Continuation)),
        "first turn did not finish"
    );
    write_skill_described(
        &skills_home.join(EARLY_SKILL),
        EARLY_SKILL,
        DESC_V2,
        "SKILL_CATALOG_EARLY_MARKER",
    );
    send_line(&session, NEXT_TURN_PROMPT).unwrap();
    let next = canned.wait_for(Duration::from_secs(40), |reqs| {
        reqs.iter().any(|req| req.kind == RequestKind::NextTurn)
    });
    fs::remove_dir_all(skills_home.join(EARLY_SKILL)).unwrap();
    send_line(&session, RETIRE_PROMPT).unwrap();
    let retired = canned.wait_for(Duration::from_secs(40), |reqs| {
        reqs.iter().any(|req| req.kind == RequestKind::RetireTurn)
    });
    send_line(&session, "/quit").unwrap();
    let _ = session
        .wait(
            Deadline::after(Duration::from_secs(25)).unwrap(),
            &Cancellation::default(),
            Duration::from_secs(5),
        )
        .unwrap();
    let requests = canned.requests();
    let first = requests
        .iter()
        .find(|req| req.kind == RequestKind::FirstSampling)
        .expect("first sampling");
    let next_req = requests
        .iter()
        .find(|req| req.kind == RequestKind::NextTurn);
    let retire_req = requests
        .iter()
        .find(|req| req.kind == RequestKind::RetireTurn);
    let report = json!({
        "next_http": next,
        "retire_http": retired,
        "early_in_first": contains_marker(&first.body, EARLY_SKILL),
        "v1_in_catalogue": contains_marker(&first.body, DESC_V1)
            || next_req
                .as_ref()
                .is_some_and(|req| contains_marker(&req.body, DESC_V1)),
        "v2_in_next": next_req.as_ref().map(|req| contains_marker(&req.body, DESC_V2)),
        "early_in_retire": retire_req
            .as_ref()
            .map(|req| contains_marker(&req.body, EARLY_SKILL)),
        "kinds": requests.iter().map(|req| format!("{:?}", req.kind)).collect::<Vec<_>>(),
        "evidence": evidence,
    });
    write_json(&evidence.join("acceptance.json"), &report);
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
    assert!(next, "update next-turn missing; see {}", evidence.display());
    assert!(retired, "retire turn missing; see {}", evidence.display());
    assert_eq!(report["early_in_first"], true);
    assert_eq!(
        report["v1_in_catalogue"], false,
        "custom descriptions are not in the injected catalogue on this CLI"
    );
    assert_eq!(
        report["early_in_retire"], true,
        "hooks-off next turn still lists a skill deleted after the prior turn"
    );
}

#[test]
#[ignore = "explicit owned ordinary TUI + canned local Responses; no live model"]
fn ordinary_tui_next_turn_after_watcher_delay_drops_deleted_skill() {
    let exe = pinned_exe();
    let evidence = tempfile::Builder::new()
        .prefix("skill-watcher-delete-")
        .tempdir()
        .unwrap()
        .keep();
    println!("skill watcher-delete evidence: {}", evidence.display());
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
    let canned = CannedResponses::spawn_short(&evidence).unwrap();
    write_probe_config(&home, &workspace, &canned, "", false);
    let session = spawn_probe_tui(&exe, &home, &workspace);
    ready_named(&session, &evidence, &home, "SkillWatcherDelete");
    send_line(&session, USER_PROMPT).unwrap();
    assert!(
        canned.wait_for(Duration::from_secs(45), |reqs| reqs
            .iter()
            .any(|req| req.kind == RequestKind::FirstSampling)),
        "missing first sampling"
    );
    assert!(
        canned.wait_for(Duration::from_secs(45), |reqs| reqs
            .iter()
            .any(|req| req.kind == RequestKind::Continuation)),
        "first turn did not finish"
    );
    fs::remove_dir_all(skills_home.join(EARLY_SKILL)).unwrap();
    write_skill(&skills_home.join(LATE_SKILL), LATE_SKILL, LATE_MARKER);
    std::thread::sleep(Duration::from_secs(15));
    send_line(&session, NEXT_TURN_PROMPT).unwrap();
    let next = canned.wait_for(Duration::from_secs(40), |reqs| {
        reqs.iter().any(|req| req.kind == RequestKind::NextTurn)
    });
    send_line(&session, "/quit").unwrap();
    let _ = session
        .wait(
            Deadline::after(Duration::from_secs(25)).unwrap(),
            &Cancellation::default(),
            Duration::from_secs(5),
        )
        .unwrap();
    let requests = canned.requests();
    let next_req = requests
        .iter()
        .find(|req| req.kind == RequestKind::NextTurn);
    let report = json!({
        "next_http": next,
        "early_in_next": next_req.as_ref().map(|req| contains_marker(&req.body, EARLY_SKILL)),
        "late_in_next": next_req.as_ref().map(|req| contains_marker(&req.body, LATE_SKILL) || contains_marker(&req.body, LATE_MARKER)),
        "kinds": requests.iter().map(|req| format!("{:?}", req.kind)).collect::<Vec<_>>(),
        "evidence": evidence,
        "watcher_wait_seconds": 15
    });
    write_json(&evidence.join("acceptance.json"), &report);
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
    assert!(
        next,
        "next-turn sampling missing; see {}",
        evidence.display()
    );
    assert_eq!(report["late_in_next"], true);
    assert_eq!(
        report["early_in_next"], true,
        "15s watcher wait still lists a deleted skill on the next turn"
    );
}

#[test]
#[ignore = "explicit owned ordinary TUI + canned local Responses; no live model"]
fn ordinary_tui_next_turn_reads_live_skill_body() {
    let exe = pinned_exe();
    let evidence = tempfile::Builder::new()
        .prefix("skill-body-read-")
        .tempdir()
        .unwrap()
        .keep();
    println!("skill body-read evidence: {}", evidence.display());
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
    let canned = CannedResponses::spawn_short(&evidence).unwrap();
    write_probe_config(&home, &workspace, &canned, "", false);
    let session = spawn_probe_tui(&exe, &home, &workspace);
    ready_named(&session, &evidence, &home, "SkillBodyRead");
    send_line(&session, USER_PROMPT).unwrap();
    assert!(
        canned.wait_for(Duration::from_secs(45), |reqs| reqs
            .iter()
            .any(|req| req.kind == RequestKind::FirstSampling)),
        "missing first sampling"
    );
    assert!(
        canned.wait_for(Duration::from_secs(45), |reqs| reqs
            .iter()
            .any(|req| req.kind == RequestKind::Continuation)),
        "first turn did not finish"
    );
    let late = skills_home.join(LATE_SKILL);
    write_skill(&late, LATE_SKILL, LATE_MARKER);
    canned.set_skill_read(&late.join("SKILL.md"));
    send_line(&session, NEXT_TURN_PROMPT).unwrap();
    let body = canned.wait_for(Duration::from_secs(45), |reqs| {
        reqs.iter().any(|req| req.kind == RequestKind::SkillBody)
    });
    send_line(&session, "/quit").unwrap();
    let _ = session
        .wait(
            Deadline::after(Duration::from_secs(25)).unwrap(),
            &Cancellation::default(),
            Duration::from_secs(5),
        )
        .unwrap();
    let requests = canned.requests();
    let follow = requests
        .iter()
        .find(|req| req.kind == RequestKind::SkillBody);
    let report = json!({
        "skill_body_http": body,
        "late_marker_in_followup": follow.as_ref().map(|req| contains_marker(&req.body, LATE_MARKER)),
        "kinds": requests.iter().map(|req| format!("{:?}", req.kind)).collect::<Vec<_>>(),
        "evidence": evidence,
    });
    write_json(&evidence.join("acceptance.json"), &report);
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
    assert!(
        body,
        "live skill body follow-up missing; see {}",
        evidence.display()
    );
    assert_eq!(
        report["late_marker_in_followup"], true,
        "next-turn exec did not observe the live SKILL.md revision"
    );
}

#[test]
#[ignore = "explicit owned ordinary TUI + canned local Responses; no live model"]
fn ordinary_tui_compact_continuation_reads_live_skill_body() {
    let exe = pinned_exe();
    let evidence = tempfile::Builder::new()
        .prefix("skill-compact-body-")
        .tempdir()
        .unwrap()
        .keep();
    println!("skill compact-body evidence: {}", evidence.display());
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
memories = false
goals = false
plugins = true
skill_search = true
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
    let session = spawn_probe_tui(&exe, &home, &workspace);
    ready_named(&session, &evidence, &home, "SkillCompactBody");
    send_line(&session, USER_PROMPT).unwrap();
    assert!(
        canned.wait_for(Duration::from_secs(45), |reqs| reqs
            .iter()
            .any(|req| req.kind == RequestKind::FirstSampling)),
        "missing first sampling"
    );
    let tool_seen = wait_for(&session, &evidence, 40, || {
        session_rows(&home).iter().any(|row| {
            row["type"] == "response_item"
                && row["payload"]["type"] == "function_call"
                && row["payload"]["call_id"] == CALL_ID
        })
    });
    assert!(tool_seen, "matching native tool call was not observed");
    let late = skills_home.join(LATE_SKILL);
    write_skill(&late, LATE_SKILL, LATE_MARKER);
    canned.set_skill_read(&late.join("SKILL.md"));
    let compact_and_body = canned.wait_for(Duration::from_secs(90), |reqs| {
        reqs.iter().any(|req| req.kind == RequestKind::Compact)
            && reqs.iter().any(|req| req.kind == RequestKind::SkillBody)
    });
    send_line(&session, "/quit").unwrap();
    let _ = session
        .wait(
            Deadline::after(Duration::from_secs(25)).unwrap(),
            &Cancellation::default(),
            Duration::from_secs(5),
        )
        .unwrap();
    let requests = canned.requests();
    let continuation = requests
        .iter()
        .find(|req| req.kind == RequestKind::Continuation);
    let follow = requests
        .iter()
        .find(|req| req.kind == RequestKind::SkillBody);
    let report = json!({
        "compact_and_body": compact_and_body,
        "late_in_continuation_catalogue": continuation.as_ref().map(|req| contains_marker(&req.body, LATE_SKILL) || contains_marker(&req.body, LATE_MARKER)),
        "late_marker_in_followup": follow.as_ref().map(|req| contains_marker(&req.body, LATE_MARKER)),
        "kinds": requests.iter().map(|req| format!("{:?}", req.kind)).collect::<Vec<_>>(),
        "evidence": evidence,
    });
    write_json(&evidence.join("acceptance.json"), &report);
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
    assert!(
        compact_and_body,
        "compact+body missing; see {}",
        evidence.display()
    );
    assert_eq!(
        report["late_in_continuation_catalogue"], false,
        "hooks-off compact continuation still omits a mid-turn skill"
    );
    assert_eq!(
        report["late_marker_in_followup"], true,
        "compact continuation exec did not observe the live SKILL.md revision"
    );
}

#[test]
#[ignore = "explicit owned ordinary TUI + canned local Responses; no live model"]
fn ordinary_tui_running_child_reads_live_skill_body() {
    let exe = pinned_exe();
    let evidence = tempfile::Builder::new()
        .prefix("skill-child-body-")
        .tempdir()
        .unwrap()
        .keep();
    println!("skill child-body evidence: {}", evidence.display());
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
    let canned = CannedResponses::spawn_child(&evidence).unwrap();
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
model_auto_compact_token_limit = 1000000
model_context_window = 20000
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
multi_agent = true
multi_agent_v2 = false
memories = false
goals = false
plugins = true
skill_search = true
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
    let session = spawn_probe_tui(&exe, &home, &workspace);
    ready_named(&session, &evidence, &home, "SkillChildBody");
    send_line(&session, USER_PROMPT).unwrap();
    assert!(
        canned.wait_for(Duration::from_secs(45), |reqs| reqs
            .iter()
            .any(|req| req.kind == RequestKind::ChildSampling)),
        "child sampling missing"
    );
    let tool_seen = wait_for(&session, &evidence, 40, || {
        session_rows(&home).iter().any(|row| {
            row["type"] == "response_item"
                && row["payload"]["type"] == "function_call"
                && row["payload"]["call_id"] == CHILD_CALL_ID
        })
    });
    assert!(tool_seen, "child tool call was not observed");
    let late = skills_home.join(LATE_SKILL);
    write_skill(&late, LATE_SKILL, LATE_MARKER);
    canned.set_skill_read(&late.join("SKILL.md"));
    let body = canned.wait_for(Duration::from_secs(45), |reqs| {
        reqs.iter().any(|req| req.kind == RequestKind::SkillBody)
    });
    send_line(&session, "/quit").unwrap();
    let _ = session
        .wait(
            Deadline::after(Duration::from_secs(25)).unwrap(),
            &Cancellation::default(),
            Duration::from_secs(5),
        )
        .unwrap();
    let requests = canned.requests();
    let continuation = requests
        .iter()
        .find(|req| req.kind == RequestKind::ChildContinuation);
    let follow = requests
        .iter()
        .find(|req| req.kind == RequestKind::SkillBody);
    let report = json!({
        "skill_body_http": body,
        "late_in_child_continuation_catalogue": continuation.as_ref().map(|req| contains_marker(&req.body, LATE_SKILL) || contains_marker(&req.body, LATE_MARKER)),
        "late_marker_in_followup": follow.as_ref().map(|req| contains_marker(&req.body, LATE_MARKER)),
        "kinds": requests.iter().map(|req| format!("{:?}", req.kind)).collect::<Vec<_>>(),
        "evidence": evidence,
    });
    write_json(&evidence.join("acceptance.json"), &report);
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
    assert!(
        body,
        "child live skill body follow-up missing; see {}",
        evidence.display()
    );
    assert_eq!(
        report["late_in_child_continuation_catalogue"], false,
        "running child catalogue still omits a mid-turn skill"
    );
    assert_eq!(
        report["late_marker_in_followup"], true,
        "running child exec did not observe the live SKILL.md revision"
    );
}

#[test]
fn catalogue_without_native_discovery_is_explicit_and_never_scans() {
    let root = tempfile::Builder::new()
        .prefix("skill-catalogue-unavailable-")
        .tempdir()
        .unwrap();
    let case = root.path().join("case");
    let home = root.path().join("codex");
    fs::create_dir_all(case.join(".agents/skills/local_probe")).unwrap();
    fs::create_dir_all(&home).unwrap();
    write_skill(
        &case.join(".agents/skills/local_probe"),
        "local_probe",
        "LOCAL_PROBE_MARKER",
    );
    let output = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .args(["skills", "catalogue", "--case"])
        .arg(&case)
        .arg("--codex-home")
        .arg(&home)
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(1),
        "unavailable native discovery must not report success"
    );
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("native_discovery=unavailable"), "{text}");
    assert!(text.contains("route=codex-harness skills usage"), "{text}");
    assert!(
        text.contains("native launch registration is unreadable"),
        "{text}"
    );
    assert!(
        !text.contains("local_probe"),
        "unavailable native discovery must not fall back to a local scan: {text}"
    );
}

#[test]
#[ignore = "explicit installed native CLI app-server skills/list; no model"]
fn catalogue_reports_the_native_effective_set_with_kit_identity() {
    let root = tempfile::Builder::new()
        .prefix("skill-catalogue-native-")
        .tempdir()
        .unwrap()
        .keep();
    println!("skill catalogue native evidence: {}", root.display());
    let case = root.join("case");
    let home = root.join("codex");
    let skills = case.join(".agents/skills");
    fs::create_dir_all(&skills).unwrap();
    fs::create_dir_all(&home).unwrap();
    write_skill(
        &skills.join("catalogue_shared_one"),
        "catalogue_shared_probe",
        "SHARED_ONE",
    );
    write_skill(
        &skills.join("catalogue_shared_two"),
        "catalogue_shared_probe",
        "SHARED_TWO",
    );
    write_skill(
        &skills.join("catalogue_disabled"),
        "catalogue_disabled_probe",
        "DISABLED",
    );
    let broken = skills.join("catalogue_broken");
    fs::create_dir_all(&broken).unwrap();
    fs::write(broken.join("SKILL.md"), "no frontmatter here").unwrap();
    fs::write(
        home.join("config.toml"),
        format!(
            "[[skills.config]]\npath = {}\nenabled = false\n",
            serde_json::to_string(
                &skills
                    .join("catalogue_disabled/SKILL.md")
                    .to_string_lossy()
                    .as_ref()
            )
            .unwrap()
        ),
    )
    .unwrap();
    let upstream = registered_upstream(&live_codex_home());
    let output = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .args(["skills", "catalogue", "--case"])
        .arg(&case)
        .arg("--codex-home")
        .arg(&home)
        .arg("--upstream")
        .arg(&upstream)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8_lossy(&output.stdout);
    fs::write(root.join("catalogue.txt"), text.as_bytes()).unwrap();
    let repo_lines: Vec<&str> = text
        .lines()
        .filter(|line| line.starts_with("name=") && line.contains(";scope=repo;"))
        .collect();
    let shared: Vec<&&str> = repo_lines
        .iter()
        .filter(|line| line.contains("name=catalogue_shared_probe;"))
        .collect();
    assert_eq!(
        shared.len(),
        2,
        "both distinct sources stay visible: {text}"
    );
    assert_ne!(
        shared[0].split("revision=").nth(1),
        shared[1].split("revision=").nth(1),
        "distinct sources keep distinct revisions: {text}"
    );
    assert!(
        text.contains("conflict: name=catalogue_shared_probe"),
        "{text}"
    );
    assert!(
        repo_lines.iter().any(|line| {
            line.contains("name=catalogue_disabled_probe;") && line.contains(";enabled=false;")
        }),
        "config disablement is retained: {text}"
    );
    assert!(
        text.contains("incomplete: native discovery error at"),
        "the broken package stays explicit: {text}"
    );
    assert!(text.contains("coverage=incomplete"), "{text}");
    assert!(text.contains("awareness=discovery-only"), "{text}");
}

fn live_codex_home() -> PathBuf {
    std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("USERPROFILE").unwrap()).join(".codex"))
}

fn registered_upstream(home: &Path) -> PathBuf {
    let registration = home.join("harness/native-launch.json");
    let bytes = fs::read(&registration).unwrap_or_else(|error| {
        panic!(
            "installed launcher registration {} is missing ({error}); the installed native CLI is required for this probe",
            registration.display()
        )
    });
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    PathBuf::from(
        value["upstream"]["executable"]
            .as_str()
            .expect("registration records the upstream executable"),
    )
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

fn pinned_exe() -> PathBuf {
    let exe = PathBuf::from(
        std::env::var_os("HARNESS_NATIVE_CODEX")
            .expect("set HARNESS_NATIVE_CODEX to the pinned original native executable"),
    );
    assert!(exe.is_absolute() && exe.is_file());
    let sha = build_identity::hash_file(&exe).unwrap();
    assert_eq!(sha, EXPECTED_SHA);
    assert_eq!(fs::metadata(&exe).unwrap().len(), EXPECTED_LEN);
    exe
}

fn write_probe_config(
    home: &Path,
    workspace: &Path,
    canned: &CannedResponses,
    extra: &str,
    compact_prompt: bool,
) {
    let trusted = json_escape(workspace);
    let compact = if compact_prompt {
        format!("compact_prompt = \"{COMPACT_PROMPT}\"\n")
    } else {
        String::new()
    };
    fs::write(
        home.join("config.toml"),
        format!(
            r#"model = "gpt-6-astra"
model_provider = "{PROVIDER_ID}"
model_reasoning_effort = "low"
approval_policy = "never"
sandbox_mode = "danger-full-access"
web_search = "disabled"
model_auto_compact_token_limit = 1000000
model_context_window = 20000
{compact}[model_providers.{PROVIDER_ID}]
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
memories = false
goals = false
plugins = true
skill_search = true
[projects.{trusted}]
trust_level = "trusted"
{extra}
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
}

fn spawn_probe_tui(exe: &Path, home: &Path, workspace: &Path) -> ConsoleSession {
    let mut command = CommandSpec::new(exe);
    command.args = vec!["--no-alt-screen".into()];
    command.current_dir = Some(workspace.to_path_buf());
    command.env.insert(
        "CODEX_HOME".into(),
        Some(home.to_path_buf().into_os_string()),
    );
    command.env.insert(API_KEY_ENV.into(), Some(API_KEY.into()));
    command.env.insert("PATH".into(), Some(filtered_path()));
    command.env.insert("OPENAI_API_KEY".into(), None);
    command.env.insert("CODEX_API_KEY".into(), None);
    let mut spec = ConsoleSpec::new(command);
    spec.limits.memory_bytes = Some(512 * 1024 * 1024);
    spec.limits.cpu_percent = Some(50.0);
    ConsoleSession::spawn(spec).unwrap()
}

fn ready_named(session: &ConsoleSession, evidence: &Path, home: &Path, name: &str) {
    let ready = wait_for(session, evidence, 60, || {
        session.transcript().contains("gpt-6-astra")
    });
    assert!(
        ready,
        "ordinary TUI did not become ready; evidence {}",
        evidence.display()
    );
    std::thread::sleep(Duration::from_secs(3));
    send_line(session, &format!("/rename {name}")).unwrap();
    let named = wait_for(session, evidence, 20, || {
        rows(&home.join("session_index.jsonl"))
            .iter()
            .any(|row| row["thread_name"] == name)
    });
    assert!(named, "literal /rename did not confirm");
}
