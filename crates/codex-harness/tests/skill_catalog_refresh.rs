//! Model-free ordinary TUI probe: host skill catalogue after mid-turn compaction.
//! Uses an owned CODEX_HOME, a distinct canned local Responses provider, and
//! ConPTY + Windows Jobs. Never redefines reserved provider openai.
#![cfg(windows)]
#[path = "fixtures/native_sampling.rs"]
mod native_sampling;
#[path = "fixtures/skill_catalog_responses.rs"]
mod skill_catalog_responses;
#[path = "fixtures/synthetic_role_build.rs"]
mod synthetic_role_build;

use harness_core::{
    build_identity,
    console::{ConsoleSession, ConsoleSpec},
    process::{Cancellation, CommandSpec, Deadline, StopReason},
};
use serde_json::{Value, json};
use skill_catalog_responses::{
    ALL_TOOLS_END, ALL_TOOLS_MARKER, ALL_TOOLS_PROBE_PROMPT, API_KEY, API_KEY_ENV, CALL_ID,
    CHILD_CALL_ID, CHILD_PROMPT, COMPACT_PROMPT, CannedResponses, EARLY_SKILL, NEXT_TURN_PROMPT,
    PROVIDER_ID, RESUME_PROMPT, RETIRE_PROMPT, RequestKind, SUMMARY_TEXT, UNRELATED_PROMPT,
    USER_PROMPT,
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
/// Explicit native resume probe markers: the executor role text before and
/// after the kit source change, and the project skills the resume must
/// reconcile. Kept distinct from every other probe's names.
const RESUME_ROLE_ALPHA: &str = "RESUME_ROLE_TEXT_ALPHA_9d21";
const RESUME_ROLE_BRAVO: &str = "RESUME_ROLE_TEXT_BRAVO_5fe8";
const RESUME_EARLY_SKILL: &str = "resume_probe_early";
const RESUME_LATE_SKILL: &str = "resume_probe_late";
const RESUME_UNRELATED_SKILL: &str = "resume_probe_unrelated";

/// Explicit native catalogue probe names for the repository scope, the
/// explicit shared-skill disablement and the env-pinned canary farm. Kept
/// distinct from every other probe's names.
const CATALOGUE_PROJECT_SKILL: &str = "project_probe";
const CATALOGUE_USER_UNRELATED_SKILL: &str = "user_unrelated";
const CATALOGUE_EXPLICITLY_DISABLED_SKILL: &str = "cargo-fast";

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

/// The synthetic managed home both role probes run with: the `ds` profile
/// mirrors the ds/deepseek-flash/deepseek/max binding with only its transport
/// pointed at the canned local Responses server, and the base configuration
/// trusts the owned workspace. `extra` appends probe-specific configuration.
fn write_role_probe_home(home: &Path, case: &Path, canned: &CannedResponses, extra: &str) {
    fs::write(
        home.join("ds.config.toml"),
        format!(
            "model = \"deepseek-flash\"\nmodel_provider = \"deepseek\"\nmodel_reasoning_effort = \"max\"\n\n[features]\ncode_mode = false\n\n[model_providers.deepseek]\nname = \"Canned role probe Responses\"\nbase_url = \"{}\"\nenv_key = \"{API_KEY_ENV}\"\nwire_api = \"responses\"\nrequires_openai_auth = false\nsupports_websockets = false\nrequest_max_retries = 0\nstream_max_retries = 0\n",
            canned.base_url()
        ),
    )
    .unwrap();
    fs::write(
        home.join("config.toml"),
        format!(
            "approval_policy = \"never\"\nsandbox_mode = \"danger-full-access\"\nweb_search = \"disabled\"\n\n[windows]\nsandbox = \"unelevated\"\n\n[features]\nhooks = false\napps = false\nmemories = false\ngoals = false\nplugins = false\nskill_search = true\n\n[projects.{}]\ntrust_level = \"trusted\"\n{extra}",
            json_escape(case)
        ),
    )
    .unwrap();
    fs::write(
        home.join("auth.json"),
        serde_json::to_vec_pretty(&json!({"auth_mode": "apikey", "OPENAI_API_KEY": API_KEY}))
            .unwrap(),
    )
    .unwrap();
}

/// The `-c` composition the managed host applies for one role: the profile
/// overrides first, then the role launch composition from the recorded build.
fn role_args(
    home: &Path,
    case: &Path,
    role: harness_core::role_context::Role,
) -> (String, Vec<std::ffi::OsString>) {
    let profile = harness_core::orchestration_config::profile_config(home, "ds").unwrap();
    let mut args: Vec<std::ffi::OsString> = Vec::new();
    for config in harness_core::orchestration_config::profile_config_overrides(home, "ds").unwrap()
    {
        args.push("-c".into());
        args.push(config.into());
    }
    let launch = harness_core::native_launcher::role_launch(
        home,
        case,
        Some(&profile),
        &toml::Table::new(),
        role,
    )
    .unwrap();
    args.extend(launch.args.iter().map(std::ffi::OsString::from));
    (launch.revision, args)
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

fn git_command(dir: &Path) -> Command {
    let mut command =
        Command::new(std::env::var_os("HARNESS_GIT").unwrap_or_else(|| "git.exe".into()));
    command.current_dir(dir);
    command
}

/// One linked worktree of the real kit repository, created for one explicit
/// native probe and removed best-effort on drop. The registration lives in
/// the shared repository, so cleanup runs even when the probe panics.
struct LinkedKitWorktree {
    repo: PathBuf,
    path: PathBuf,
}

impl LinkedKitWorktree {
    fn add(repo: &Path, path: &Path) -> Self {
        let output = git_command(repo)
            .args(["worktree", "add", "-q", "--detach"])
            .arg(path)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "linked worktree add failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        Self {
            repo: repo.to_path_buf(),
            path: path.to_path_buf(),
        }
    }
}

impl Drop for LinkedKitWorktree {
    fn drop(&mut self) {
        let _ = git_command(&self.repo)
            .args(["worktree", "remove", "--force"])
            .arg(&self.path)
            .status();
        let _ = git_command(&self.repo).args(["worktree", "prune"]).status();
        if self.path.exists() {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
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
        // The remainder route passes this exact value; the command must accept it.
        .args(["--limit", "1048576"])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(1),
        "unavailable native discovery must not report success"
    );
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("native_discovery=unavailable"), "{text}");
    assert!(text.contains("remedy=restore the native read"), "{text}");
    assert!(
        text.contains("native launch registration is unreadable"),
        "{text}"
    );
    assert!(
        !text.contains("local_probe"),
        "unavailable native discovery must not fall back to a local scan: {text}"
    );
}

/// An explicitly requested role resolves before the native read: a home whose
/// installed build cannot supply the role refuses with the exact missing
/// input instead of reporting a set the managed session would not see.
#[test]
fn catalogue_role_refuses_missing_role_inputs_before_the_native_read() {
    let root = tempfile::Builder::new()
        .prefix("skill-catalogue-role-refusal-")
        .tempdir()
        .unwrap();
    let case = root.path().join("case");
    let home = root.path().join("codex");
    fs::create_dir_all(&case).unwrap();
    fs::create_dir_all(&home).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .args(["skills", "catalogue", "--case"])
        .arg(&case)
        .arg("--codex-home")
        .arg(&home)
        .args(["--role", "lead"])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(2),
        "a role the installed build cannot supply refuses the command"
    );
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(text.contains("role context for --role lead"), "{text}");
    assert!(
        text.contains("no native launch registration is installed"),
        "{text}"
    );
}

/// The role the catalogue composes onto the native read reaches the discovery
/// child exactly as a managed session's host renders it: the owned developer
/// text and the skill memberships travel as `-c` pairs on the app-server
/// invocation. The discovery double fails its first request on purpose, so
/// the private evidence root with the exact request survives for reading.
#[test]
fn catalogue_role_composes_developer_text_and_memberships_onto_the_native_read() {
    let root = tempfile::Builder::new()
        .prefix("skill-catalogue-role-composition-")
        .tempdir()
        .unwrap();
    let case = root.path().join("case");
    let home = root.path().join("codex");
    fs::create_dir_all(&case).unwrap();
    fs::create_dir_all(&home).unwrap();
    let source = synthetic_role_build::kit_source(root.path());
    let build = synthetic_role_build::recorded_build(root.path(), &source);
    synthetic_role_build::register(&home, root.path(), &build);
    let upstream = PathBuf::from(env!("CARGO_BIN_EXE_harness-launch-fixture"));
    let output = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .args(["skills", "catalogue", "--case"])
        .arg(&case)
        .arg("--codex-home")
        .arg(&home)
        .arg("--upstream")
        .arg(&upstream)
        .args(["--role", "executor"])
        .env("HARNESS_LAUNCH_FIXTURE_MODE", "discovery")
        .env("HARNESS_DISCOVERY_FIXTURE", "init-error")
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let evidence = stderr
        .lines()
        .find_map(|line| line.strip_prefix("native skill discovery failed; private evidence: "))
        .expect("the refused discovery names its private evidence root")
        .trim()
        .to_owned();
    let request: Value =
        serde_json::from_slice(&fs::read(Path::new(&evidence).join("request.json")).unwrap())
            .unwrap();
    let arguments: Vec<&str> = request["arguments"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap())
        .collect();
    assert_eq!(arguments[0], "app-server", "{arguments:?}");
    assert_eq!(arguments[1], "--stdio", "{arguments:?}");
    let developer = arguments
        .iter()
        .find(|argument| argument.starts_with("developer_instructions="))
        .expect("the executor developer text travels as an override");
    assert!(
        developer.contains("Fixture executor responsibility"),
        "{developer}"
    );
    let memberships = arguments
        .iter()
        .find(|argument| argument.starts_with("skills.config="))
        .expect("the role's skill memberships travel as an override");
    assert!(memberships.contains("coordinate"), "{memberships}");
    assert!(memberships.contains("SKILL.md"), "{memberships}");
    assert!(memberships.contains("enabled = false"), "{memberships}");
    assert!(
        !memberships.contains("shared"),
        "an allowed shared skill stays enabled: {memberships}"
    );
}

/// The selected profile travels with the composed role context: the profile
/// installed as `selected` disables the shared kit skill, so the composed
/// `skills.config` the native read receives keeps it disabled even though the
/// executor role allows it, while `other` leaves it enabled and profile
/// routing values never compose. The reported role context names role,
/// profile and revision without changing with the profile, and nothing on
/// disk changes. Fixture-only composition evidence through the real catalogue
/// entry point: the discovery double records the exact composed context; the
/// installed native CLI's consumption of it is covered by the explicit native
/// probe that follows.
#[test]
fn catalogue_role_profile_keeps_the_selected_profiles_context_and_disablement() {
    let root = tempfile::Builder::new()
        .prefix("skill-catalogue-role-profile-")
        .tempdir()
        .unwrap();
    let case = root.path().join("case");
    let home = root.path().join("codex");
    fs::create_dir_all(&case).unwrap();
    fs::create_dir_all(&home).unwrap();
    let source = synthetic_role_build::kit_source(root.path());
    let build = synthetic_role_build::recorded_build(root.path(), &source);
    synthetic_role_build::register(&home, root.path(), &build);
    let shared = source.join("skills/shared/SKILL.md");
    let shared_canonical = shared.canonicalize().unwrap();
    let coordinate = source
        .join("skills/coordinate/SKILL.md")
        .canonicalize()
        .unwrap();
    fs::write(
        home.join("selected.config.toml"),
        format!(
            "model = \"fixture-selected-routing\"\ndeveloper_instructions = \"Selected profile guidance\"\n\n[[skills.config]]\npath = {}\nenabled = false\n",
            serde_json::to_string(shared.to_string_lossy().as_ref()).unwrap()
        ),
    )
    .unwrap();
    fs::write(
        home.join("other.config.toml"),
        "model = \"fixture-other-routing\"\n",
    )
    .unwrap();
    let before: Vec<(PathBuf, Vec<u8>)> = [
        home.join("selected.config.toml"),
        home.join("other.config.toml"),
        source.join("global/roles.toml"),
        shared.clone(),
    ]
    .into_iter()
    .map(|path| {
        let bytes = fs::read(&path).unwrap();
        (path, bytes)
    })
    .collect();
    let upstream = PathBuf::from(env!("CARGO_BIN_EXE_harness-launch-fixture"));
    let catalogue = |profile: Option<&str>| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_codex-harness"));
        command
            .args(["skills", "catalogue", "--case"])
            .arg(&case)
            .arg("--codex-home")
            .arg(&home)
            .arg("--upstream")
            .arg(&upstream)
            .args(["--role", "executor"])
            .env("HARNESS_LAUNCH_FIXTURE_MODE", "discovery")
            .env("HARNESS_DISCOVERY_FIXTURE", "init-error");
        if let Some(profile) = profile {
            command.args(["--profile", profile]);
        }
        command.output().unwrap()
    };
    let revision = |stdout: &str| {
        stdout
            .lines()
            .find_map(|line| {
                line.strip_prefix("role_context=role=executor;profile=")
                    .and_then(|rest| rest.split_once(";revision="))
                    .map(|(_, revision)| revision.to_owned())
            })
            .expect("the catalogue reports the composed role context")
    };

    let (selected_args, selected_out) = refused_catalogue_context(&catalogue(Some("selected")));
    assert!(
        selected_out.starts_with("role_context=role=executor;profile=selected;revision="),
        "{selected_out}"
    );
    assert!(
        !selected_args.iter().any(|argument| {
            argument.starts_with("model=")
                || argument.starts_with("model_provider=")
                || argument.starts_with("model_reasoning_effort=")
        }),
        "profile routing values never compose onto the catalogue: {selected_args:?}"
    );
    let selected_developer = selected_args
        .iter()
        .find(|argument| argument.starts_with("developer_instructions="))
        .expect("the executor developer text travels as an override");
    assert!(
        selected_developer.contains("Fixture executor responsibility"),
        "{selected_developer}"
    );
    assert!(
        selected_developer.contains("Selected profile guidance"),
        "{selected_developer}"
    );
    let selected_config = selected_args
        .iter()
        .find(|argument| argument.starts_with("skills.config="))
        .expect("the skill context travels as an override");
    assert!(
        composed_disablement(selected_config, &shared_canonical),
        "the selected profile's disablement reaches the composed context: {selected_config}"
    );
    assert!(
        composed_disablement(selected_config, &coordinate),
        "the executor memberships stay composed: {selected_config}"
    );

    let (other_args, other_out) = refused_catalogue_context(&catalogue(Some("other")));
    assert!(
        other_out.starts_with("role_context=role=executor;profile=other;revision="),
        "{other_out}"
    );
    let other_config = other_args
        .iter()
        .find(|argument| argument.starts_with("skills.config="))
        .expect("the skill context travels as an override");
    assert!(
        !composed_disablement(other_config, &shared_canonical),
        "another profile leaves the shared skill enabled: {other_config}"
    );
    assert!(
        composed_disablement(other_config, &coordinate),
        "the executor memberships stay composed: {other_config}"
    );

    let (plain_args, plain_out) = refused_catalogue_context(&catalogue(None));
    assert!(
        plain_out.starts_with("role_context=role=executor;profile=none;revision="),
        "{plain_out}"
    );
    let plain_config = plain_args
        .iter()
        .find(|argument| argument.starts_with("skills.config="))
        .expect("the skill context travels as an override");
    assert!(
        !composed_disablement(plain_config, &shared_canonical),
        "without a selected profile no profile layer composes: {plain_config}"
    );

    let selected_revision = revision(&selected_out);
    assert_eq!(
        selected_revision,
        revision(&other_out),
        "{selected_out}{other_out}"
    );
    assert_eq!(
        selected_revision,
        revision(&plain_out),
        "{selected_out}{plain_out}"
    );

    let missing = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .args(["skills", "catalogue", "--case"])
        .arg(&case)
        .arg("--codex-home")
        .arg(&home)
        .args(["--role", "executor", "--profile", "absent"])
        .output()
        .unwrap();
    assert_eq!(
        missing.status.code(),
        Some(2),
        "{}",
        String::from_utf8_lossy(&missing.stderr)
    );
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&missing.stdout),
        String::from_utf8_lossy(&missing.stderr)
    );
    assert!(
        text.contains("role context for --role executor --profile absent")
            && text.contains("is not installed"),
        "a profile that is not installed refuses instead of composing nothing: {text}"
    );

    let without_role = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .args(["skills", "catalogue", "--case"])
        .arg(&case)
        .arg("--codex-home")
        .arg(&home)
        .args(["--profile", "selected"])
        .output()
        .unwrap();
    assert_eq!(
        without_role.status.code(),
        Some(2),
        "{}",
        String::from_utf8_lossy(&without_role.stderr)
    );
    let without_role_text = format!(
        "{}{}",
        String::from_utf8_lossy(&without_role.stdout),
        String::from_utf8_lossy(&without_role.stderr)
    );
    assert!(
        without_role_text.contains("pass --role lead|executor"),
        "profile context without a role refuses instead of guessing one: {without_role_text}"
    );

    for (path, bytes) in &before {
        assert_eq!(
            &fs::read(path).unwrap(),
            bytes,
            "{} changed",
            path.display()
        );
    }
}

/// The composed `-c` argument pairs and stdout of one refused fixture
/// discovery, read from the private evidence root the command names on stderr.
fn refused_catalogue_context(output: &std::process::Output) -> (Vec<String>, String) {
    assert_eq!(
        output.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let evidence = stderr
        .lines()
        .find_map(|line| line.strip_prefix("native skill discovery failed; private evidence: "))
        .expect("the refused discovery names its private evidence root")
        .trim()
        .to_owned();
    let request: Value =
        serde_json::from_slice(&fs::read(Path::new(&evidence).join("request.json")).unwrap())
            .unwrap();
    let arguments = request["arguments"]
        .as_array()
        .expect("the recorded request carries its arguments")
        .iter()
        .map(|value| {
            value
                .as_str()
                .expect("recorded arguments are text")
                .to_owned()
        })
        .collect();
    (
        arguments,
        String::from_utf8_lossy(&output.stdout).into_owned(),
    )
}

/// Whether one composed `skills.config` argument disables `path`.
fn composed_disablement(config: &str, path: &Path) -> bool {
    let composed: toml::Value =
        toml::from_str(config).expect("the composed skills.config parses as TOML");
    composed
        .get("skills")
        .and_then(|skills| skills.get("config"))
        .and_then(toml::Value::as_array)
        .expect("the composed context carries a skills.config array")
        .iter()
        .any(|entry| {
            entry
                .get("path")
                .and_then(toml::Value::as_str)
                .and_then(|value| Path::new(value).canonicalize().ok())
                .is_some_and(|value| value == path)
                && entry.get("enabled").and_then(toml::Value::as_bool) == Some(false)
        })
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

    // The correction: the omitted-entry route must retrieve the catalogue
    // itself, not the local usage report.
    let bounded = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .args(["skills", "catalogue", "--case"])
        .arg(&case)
        .arg("--codex-home")
        .arg(&home)
        .arg("--upstream")
        .arg(&upstream)
        .args(["--limit", "1024"])
        .output()
        .unwrap();
    assert!(bounded.status.success());
    let bounded = String::from_utf8_lossy(&bounded.stdout);
    assert!(bounded.contains("omitted="), "{bounded}");
    assert!(
        bounded.contains("route=codex-harness skills catalogue --limit 1048576"),
        "{bounded}"
    );
}

/// Explicit native probe: the installed CLI consumes the composed profile
/// layer, so the project skill a selected profile disables is not reported
/// enabled in that profile's catalogue while another profile reports it
/// enabled, and the composed role context names the selected profile
/// model-free. The installed native CLI and its launcher registration are
/// required; no model call is made.
#[test]
#[ignore = "explicit installed native CLI app-server skills/list; no model"]
fn catalogue_role_profile_native_read_consumes_the_selected_profiles_disablement() {
    let root = tempfile::Builder::new()
        .prefix("skill-catalogue-role-profile-native-")
        .tempdir()
        .unwrap()
        .keep();
    println!(
        "skill catalogue role profile native evidence: {}",
        root.display()
    );
    let case = root.join("case");
    let home = root.join("codex");
    fs::create_dir_all(&case).unwrap();
    fs::create_dir_all(&home).unwrap();
    let source = synthetic_role_build::kit_source(&root);
    let build = synthetic_role_build::recorded_build(&root, &source);
    synthetic_role_build::register(&home, &root, &build);
    let skill = case.join(".agents/skills/profile_probe/SKILL.md");
    fs::create_dir_all(skill.parent().unwrap()).unwrap();
    fs::write(
        &skill,
        "---\nname: profile_probe\ndescription: fixture\n---\nFixture profile probe",
    )
    .unwrap();
    fs::write(
        home.join("selected.config.toml"),
        format!(
            "[[skills.config]]\npath = {}\nenabled = false\n",
            serde_json::to_string(skill.to_string_lossy().as_ref()).unwrap()
        ),
    )
    .unwrap();
    fs::write(home.join("other.config.toml"), "# no profile layers\n").unwrap();
    let upstream = registered_upstream(&live_codex_home());
    let catalogue = |profile: &str| {
        let output = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
            .args(["skills", "catalogue", "--case"])
            .arg(&case)
            .arg("--codex-home")
            .arg(&home)
            .arg("--upstream")
            .arg(&upstream)
            .args(["--role", "executor", "--profile", profile])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).into_owned()
    };
    let selected = catalogue("selected");
    fs::write(root.join("catalogue-selected.txt"), selected.as_bytes()).unwrap();
    assert!(
        selected.contains("role_context=role=executor;profile=selected;revision="),
        "{selected}"
    );
    assert!(
        !selected.contains("name=profile_probe;scope=repo;enabled=true;"),
        "the selected profile disables the probe skill: {selected}"
    );
    let other = catalogue("other");
    fs::write(root.join("catalogue-other.txt"), other.as_bytes()).unwrap();
    assert!(
        other.contains("role_context=role=executor;profile=other;revision="),
        "{other}"
    );
    assert!(
        other.contains("name=profile_probe;scope=repo;enabled=true;"),
        "another profile keeps the probe skill enabled: {other}"
    );
}

/// Explicit native probe: two simultaneously running managed role catalogues
/// in one synthetic installation, after one sequential catalogue warms the
/// shared home: on installed 0.162.1 two simultaneous first initializations
/// of one fresh home race and one exits with `failed to initialize state
/// runtime`, while a deployed home is already initialized. The recorded
/// build source is this checkout and the case lives in its integration-test
/// target tmp area, so the native repository scope lists this checkout's
/// classified `.agents/skills` at the canonical paths the role resolution
/// uses: the lead-only kit skills must stay disabled for the executor
/// catalogue while they stay enabled for the lead, every shared kit skill
/// outside the explicit disablement stays enabled, one explicitly disabled
/// shared skill stays disabled for both roles, and the unclassified project
/// skill stays enabled for both. The installed native CLI resolves its user
/// farm from the real OS profile and ignores the USERPROFILE/HOME pins
/// (observed on 0.162.1), so the env-pinned farm is a canary that must stay
/// undiscovered and untouched; deployment-world user-scope disablement
/// remains a global-deployment gate. Neither role may rewrite the synthetic
/// configuration or the session-local home. The installed native CLI
/// performs the catalogue read; no model call is made.
#[test]
#[ignore = "explicit installed native CLI skills/list over the workspace kit source; no model"]
fn catalogue_role_sessions_keep_independent_user_contexts() {
    // The installed native CLI refuses helper aliases and the sqlite state
    // runtime under the system temporary directory, so the fixture root
    // lives in the integration-test target tmp area instead of TEMP.
    let root = tempfile::Builder::new()
        .prefix("skill-catalogue-role-sessions-")
        .tempdir_in(env!("CARGO_TARGET_TMPDIR"))
        .unwrap()
        .keep();
    println!("skill catalogue role sessions evidence: {}", root.display());
    let case = root.join("case");
    let home = root.join("codex");
    let user = root.join("user");
    fs::create_dir_all(&case).unwrap();
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&user).unwrap();
    // The recorded build source is the workspace kit checkout itself, so the
    // recorded role classification resolves the same canonical skill paths
    // the native repository scope reports for the case below.
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let explicitly_disabled = source
        .join(".agents/skills")
        .join(CATALOGUE_EXPLICITLY_DISABLED_SKILL)
        .join("SKILL.md");
    assert!(
        explicitly_disabled.is_file(),
        "the workspace kit must classify the probe skill: {}",
        explicitly_disabled.display()
    );
    let build = synthetic_role_build::recorded_build(&root, &source);
    synthetic_role_build::register(&home, &root, &build);

    let project = case.join(format!(".agents/skills/{CATALOGUE_PROJECT_SKILL}"));
    write_skill(&project, CATALOGUE_PROJECT_SKILL, "PROJECT_MARKER");

    // Env-pinned canary farm: 0.162.1 resolves the user farm from the real OS
    // profile, so this skill must stay undiscovered and the tree untouched.
    let farm = user.join(".agents/skills");
    fs::create_dir_all(&farm).unwrap();
    write_skill(
        &farm.join(CATALOGUE_USER_UNRELATED_SKILL),
        CATALOGUE_USER_UNRELATED_SKILL,
        "USER_UNRELATED_MARKER",
    );

    // One shared kit skill is explicitly disabled for this session through
    // the selected profile layer; both role catalogues must preserve that
    // disablement for the canonical path the role resolution classifies.
    fs::write(
        home.join("ds.config.toml"),
        format!(
            "[[skills.config]]\npath = {}\nenabled = false\n",
            serde_json::to_string(explicitly_disabled.to_string_lossy().as_ref()).unwrap()
        ),
    )
    .unwrap();
    fs::write(home.join("config.toml"), "approval_policy = \"never\"\n").unwrap();

    let upstream = registered_upstream(&live_codex_home());
    let config_before = (
        fs::read(home.join("config.toml")).unwrap(),
        fs::read(home.join("ds.config.toml")).unwrap(),
    );
    let user_before = tree_paths(&user);

    let catalogue = |role: &str| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_codex-harness"));
        command
            .args(["skills", "catalogue", "--case"])
            .arg(&case)
            .arg("--codex-home")
            .arg(&home)
            .arg("--upstream")
            .arg(&upstream)
            .args(["--role", role, "--profile", "ds", "--limit", "1048576"]);
        command.env("USERPROFILE", &user);
        command.env("HOME", &user);
        command.stdout(std::process::Stdio::piped());
        command.stderr(std::process::Stdio::piped());
        command
    };
    // The installed native CLI initializes its per-home sqlite state lazily:
    // two app-server processes opening one fresh home at the same instant race
    // and one exits with `failed to initialize state runtime` (observed on
    // 0.162.1). A deployed home is already initialized, so one sequential
    // catalogue warms the shared home through the same entry point and the
    // two simultaneous role reads then start against the deployed shape.
    let warm = catalogue("lead").output().unwrap();
    assert!(
        warm.status.success(),
        "{}",
        String::from_utf8_lossy(&warm.stderr)
    );
    fs::write(root.join("catalogue-warm.txt"), &warm.stdout).unwrap();
    let lead = catalogue("lead").spawn().unwrap();
    let executor = catalogue("executor").spawn().unwrap();
    let lead = lead.wait_with_output().unwrap();
    let executor = executor.wait_with_output().unwrap();
    assert!(
        lead.status.success(),
        "{}",
        String::from_utf8_lossy(&lead.stderr)
    );
    assert!(
        executor.status.success(),
        "{}",
        String::from_utf8_lossy(&executor.stderr)
    );
    let lead_text = String::from_utf8_lossy(&lead.stdout).into_owned();
    let executor_text = String::from_utf8_lossy(&executor.stdout).into_owned();
    fs::write(root.join("catalogue-lead.txt"), lead_text.as_bytes()).unwrap();
    fs::write(
        root.join("catalogue-executor.txt"),
        executor_text.as_bytes(),
    )
    .unwrap();

    // Expectations come from the kit descriptor itself: shared skills belong
    // to both roles, the lead-only skills must stay disabled for the
    // executor, and the explicitly disabled shared skill stays disabled for
    // both roles without re-enabling anything.
    let descriptor: toml::Value =
        toml::from_str(&fs::read_to_string(source.join("global/roles.toml")).unwrap()).unwrap();
    let names = |value: &toml::Value| -> Vec<String> {
        value
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry.as_str().unwrap().to_owned())
            .collect()
    };
    let shared = names(&descriptor["shared_skills"]);
    let lead_only = names(&descriptor["roles"]["lead"]["skills"]);
    let executor_only = names(&descriptor["roles"]["executor"]["skills"]);
    let entry = |text: &str, name: &str, scope: &str| -> Option<bool> {
        text.lines()
            .find(|line| line.starts_with(&format!("name={name};scope={scope};")))
            .map(|line| line.starts_with(&format!("name={name};scope={scope};enabled=true;")))
    };
    let repo_disabled = |text: &str| -> usize {
        text.lines()
            .filter(|line| line.contains(";scope=repo;") && line.contains(";enabled=false;"))
            .count()
    };
    for (role, text) in [
        ("lead", lead_text.as_str()),
        ("executor", executor_text.as_str()),
    ] {
        assert!(
            text.contains(&format!("role_context=role={role};profile=ds;revision=")),
            "{text}"
        );
        assert!(
            !text.contains("omitted="),
            "--limit 1048576 must deliver every effective entry for {role}: {text}"
        );
        assert!(
            entry(text, CATALOGUE_PROJECT_SKILL, "repo") == Some(true),
            "the unclassified project skill stays enabled for {role}: {text}"
        );
        assert!(
            entry(text, CATALOGUE_EXPLICITLY_DISABLED_SKILL, "repo") == Some(false),
            "the explicit disablement is preserved for {role}: {text}"
        );
        assert!(
            !text.contains(CATALOGUE_USER_UNRELATED_SKILL),
            "the env-pinned farm stays undiscovered for {role}: {text}"
        );
        for name in &shared {
            if name.as_str() == CATALOGUE_EXPLICITLY_DISABLED_SKILL {
                continue;
            }
            assert!(
                entry(text, name, "repo") == Some(true),
                "the shared kit skill {name} stays enabled for {role}: {text}"
            );
        }
    }
    for name in &lead_only {
        assert!(
            entry(lead_text.as_str(), name, "repo") == Some(true),
            "the lead catalogue keeps {name}: {lead_text}"
        );
        assert!(
            entry(executor_text.as_str(), name, "repo") == Some(false),
            "the lead-owned {name} stays disabled for the executor: {executor_text}"
        );
    }
    for name in &executor_only {
        assert!(
            entry(executor_text.as_str(), name, "repo") == Some(true),
            "the executor catalogue keeps {name}: {executor_text}"
        );
    }
    assert_eq!(
        repo_disabled(&lead_text),
        1,
        "only the explicit disablement is disabled for the lead: {lead_text}"
    );
    assert_eq!(
        repo_disabled(&executor_text),
        lead_only.len() + 1,
        "only the explicit and lead-owned disablements are disabled for the executor: {executor_text}"
    );

    let config_after = (
        fs::read(home.join("config.toml")).unwrap(),
        fs::read(home.join("ds.config.toml")).unwrap(),
    );
    assert_eq!(
        config_before, config_after,
        "the role catalogues must not rewrite the synthetic configuration"
    );
    assert_eq!(
        user_before,
        tree_paths(&user),
        "the role catalogues must not write the session-local user home"
    );
}
/// Sorted relative paths of one directory tree; link entries are listed but
/// not followed, so a session-local home can be compared before and after an
/// external catalogue read.
fn tree_paths(root: &Path) -> Vec<String> {
    fn walk(root: &Path, path: &Path, out: &mut Vec<String>) {
        for entry in fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            let child = entry.path();
            out.push(
                child
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/"),
            );
            if entry.file_type().unwrap().is_dir() {
                walk(root, &child, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out.sort();
    out
}

/// The installed native TUI hosts a dynamic `codex_worktrees` tool server
/// into ordinary sessions while the native `worktrees` feature is on. The
/// static first model request can defer dynamic tool servers, so the effective
/// catalogue is read from the running session itself: one code-mode `exec`
/// call per bounded settle round reports the session's own `ALL_TOOLS`. The
/// managed selection (the shared configuration leaf and the managed
/// frontend's `-c features.worktrees=false` pair) must leave no worktree-named
/// tool in that runtime catalogue and no `codex_worktrees` marker in the
/// session's first-request surface. The unsuppressed control run proves the
/// probe observes the runtime surface while the feature is on: a control that
/// exposes nothing leaves this check with no observation and the task open
/// rather than passing vacuously. Driven over the installed native CLI TUI
/// against the canned local Responses server; no billed model call and no
/// shared configuration write.
#[test]
#[ignore = "explicit installed native CLI ordinary TUI + canned local Responses; no live model"]
fn installed_ordinary_tui_catalogue_drops_native_worktree_tools_under_the_managed_leaf() {
    let upstream = registered_upstream(&live_codex_home());
    assert!(upstream.is_absolute() && upstream.is_file());
    let version = Command::new(&upstream).arg("--version").output().unwrap();
    let version = String::from_utf8_lossy(&version.stdout).trim().to_owned();
    assert!(
        version.contains("0.162.1"),
        "unexpected native CLI: {version}"
    );

    let evidence = tempfile::Builder::new()
        .prefix("native-worktree-catalogue-")
        .tempdir()
        .unwrap()
        .keep();
    println!("native worktree catalogue evidence: {}", evidence.display());

    let control = installed_tui_all_tools(&upstream, &evidence, "control", &[], 3);
    let control_fragments = worktree_names(&control.names);
    assert!(
        !control.names.is_empty() && !control_fragments.is_empty(),
        "the unsuppressed control runtime catalogue exposed no codex_worktrees tool, so this check observed nothing and the task stays open: names {:?}; evidence {}",
        control.names,
        evidence.display()
    );

    let suppressed = installed_tui_all_tools(
        &upstream,
        &evidence,
        "managed-leaf",
        &["-c", "features.worktrees=false"],
        3,
    );
    assert!(
        !suppressed.names.is_empty(),
        "the managed-leaf run captured no runtime catalogue; evidence {}",
        evidence.display()
    );
    let suppressed_fragments = worktree_names(&suppressed.names);
    let surface_fragments = worktree_fragments(&suppressed.surface);
    assert!(
        suppressed_fragments.is_empty() && surface_fragments.is_empty(),
        "features.worktrees=false left the competing worktree surface in the session: runtime {suppressed_fragments:?}, request surface {surface_fragments:?}; evidence {}",
        evidence.display()
    );
    write_json(
        &evidence.join("acceptance.json"),
        &json!({
            "control_fragments": control_fragments,
            "control_names": control.names,
            "suppressed_fragments": suppressed_fragments,
            "suppressed_names": suppressed.names,
            "suppressed_surface_fragments": surface_fragments,
        }),
    );
    println!(
        "codex_worktrees control fragments {control_fragments:?}; suppressed fragments {suppressed_fragments:?}"
    );
}

/// Explicit native probe: a managed executor catalogue must stay free of
/// lead-only kit skills across every validated copy of the same kit
/// repository. The recorded build source is this checkout while the case is
/// a fresh linked worktree of the same repository, so the native repository
/// scope discovers the lead-only skill at the case checkout and the real
/// user farm discovers the main checkout copy; both must report
/// enabled=false for the executor while shared skills stay enabled in both
/// scopes and the same catalogue for the lead keeps the lead-only skill
/// enabled, proving the filter is role selection and not a blanket path or
/// name disable. An unrelated case carrying a same-name project skill stays
/// enabled. The explicit disablement recorded at the canonical source path
/// must also reach the case and user copies. The installed native CLI
/// performs every catalogue read; no model call is made.
#[test]
#[ignore = "explicit installed native CLI skills/list over a linked worktree of the kit source; no model"]
fn catalogue_role_executor_disables_lead_skills_across_linked_worktree_copies() {
    let root = tempfile::Builder::new()
        .prefix("skill-catalogue-linked-worktree-")
        .tempdir_in(env!("CARGO_TARGET_TMPDIR"))
        .unwrap()
        .keep();
    println!(
        "skill catalogue linked worktree evidence: {}",
        root.display()
    );
    let home = root.join("codex");
    fs::create_dir_all(&home).unwrap();
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let linked = root.join("linked");
    let linked_worktree = LinkedKitWorktree::add(&source, &linked);
    let build = synthetic_role_build::recorded_build(&root, &source);
    synthetic_role_build::register(&home, &root, &build);

    // Expectations come from the kit descriptor itself: shared skills belong
    // to both roles and the lead-only skills must stay absent for the
    // executor.
    let descriptor: toml::Value =
        toml::from_str(&fs::read_to_string(source.join("global/roles.toml")).unwrap()).unwrap();
    let names = |value: &toml::Value| -> Vec<String> {
        value
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry.as_str().unwrap().to_owned())
            .collect()
    };
    let shared = names(&descriptor["shared_skills"]);
    let lead_only = names(&descriptor["roles"]["lead"]["skills"]);
    let explicitly_disabled = source
        .join(".agents/skills")
        .join(CATALOGUE_EXPLICITLY_DISABLED_SKILL)
        .join("SKILL.md");
    assert!(
        explicitly_disabled.is_file(),
        "the workspace kit must classify the probe skill: {}",
        explicitly_disabled.display()
    );
    fs::write(
        home.join("ds.config.toml"),
        format!(
            "[[skills.config]]\npath = {}\nenabled = false\n",
            serde_json::to_string(explicitly_disabled.to_string_lossy().as_ref()).unwrap()
        ),
    )
    .unwrap();
    fs::write(home.join("config.toml"), "approval_policy = \"never\"\n").unwrap();

    let upstream = registered_upstream(&live_codex_home());
    let catalogue = |role: &str, case: &Path| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_codex-harness"));
        command
            .args(["skills", "catalogue", "--case"])
            .arg(case)
            .arg("--codex-home")
            .arg(&home)
            .arg("--upstream")
            .arg(&upstream)
            .args(["--role", role, "--profile", "ds", "--limit", "1048576"]);
        command.stdout(std::process::Stdio::piped());
        command.stderr(std::process::Stdio::piped());
        command.output().unwrap()
    };
    let executor = catalogue("executor", &linked);
    assert!(
        executor.status.success(),
        "{}",
        String::from_utf8_lossy(&executor.stderr)
    );
    let executor_text = String::from_utf8_lossy(&executor.stdout).into_owned();
    fs::write(
        root.join("catalogue-executor-linked.txt"),
        executor_text.as_bytes(),
    )
    .unwrap();
    let lead = catalogue("lead", &linked);
    assert!(
        lead.status.success(),
        "{}",
        String::from_utf8_lossy(&lead.stderr)
    );
    let lead_text = String::from_utf8_lossy(&lead.stdout).into_owned();
    fs::write(root.join("catalogue-lead-linked.txt"), lead_text.as_bytes()).unwrap();

    let entry = |text: &str, name: &str, scope: &str| -> Option<bool> {
        text.lines()
            .find(|line| line.starts_with(&format!("name={name};scope={scope};")))
            .map(|line| line.starts_with(&format!("name={name};scope={scope};enabled=true;")))
    };
    for (role, text) in [
        ("executor", executor_text.as_str()),
        ("lead", lead_text.as_str()),
    ] {
        assert!(
            text.contains(&format!("role_context=role={role};profile=ds;revision=")),
            "{text}"
        );
        assert!(
            !text.contains("omitted="),
            "--limit 1048576 must deliver every effective entry for {role}: {text}"
        );
    }
    for name in &lead_only {
        assert!(
            entry(executor_text.as_str(), name, "repo") == Some(false),
            "the case checkout copy of lead-only {name} must be disabled: {executor_text}"
        );
        assert!(
            entry(executor_text.as_str(), name, "user") == Some(false),
            "the user-farm copy of lead-only {name} must be disabled: {executor_text}"
        );
        assert!(
            !executor_text.lines().any(|line| {
                line.starts_with(&format!("name={name};")) && line.contains(";enabled=true;")
            }),
            "no discovered copy of {name} may stay enabled for the executor: {executor_text}"
        );
        assert!(
            entry(lead_text.as_str(), name, "repo") == Some(true),
            "the lead keeps {name}: {lead_text}"
        );
    }
    for name in &shared {
        if name == CATALOGUE_EXPLICITLY_DISABLED_SKILL {
            continue;
        }
        assert!(
            entry(executor_text.as_str(), name, "repo") == Some(true),
            "the shared skill {name} stays available in the case checkout: {executor_text}"
        );
        assert!(
            entry(executor_text.as_str(), name, "user") == Some(true),
            "the shared skill {name} stays available in the user farm: {executor_text}"
        );
    }
    for (role, text) in [
        ("executor", executor_text.as_str()),
        ("lead", lead_text.as_str()),
    ] {
        assert!(
            entry(text, CATALOGUE_EXPLICITLY_DISABLED_SKILL, "repo") == Some(false),
            "the explicit disablement reaches the case checkout for {role}: {text}"
        );
        assert!(
            entry(text, CATALOGUE_EXPLICITLY_DISABLED_SKILL, "user") == Some(false),
            "the explicit disablement reaches the user farm for {role}: {text}"
        );
    }

    // An unrelated case carrying a same-name project skill must stay enabled:
    // no foreign path may be disabled by name or similarity.
    let foreign_case = root.join("foreign");
    fs::create_dir_all(&foreign_case).unwrap();
    write_skill(
        &foreign_case.join(".agents/skills").join(&lead_only[0]),
        &lead_only[0],
        "FOREIGN_SAME_NAME_MARKER",
    );
    let foreign = catalogue("executor", &foreign_case);
    assert!(
        foreign.status.success(),
        "{}",
        String::from_utf8_lossy(&foreign.stderr)
    );
    let foreign_text = String::from_utf8_lossy(&foreign.stdout).into_owned();
    fs::write(
        root.join("catalogue-executor-foreign.txt"),
        foreign_text.as_bytes(),
    )
    .unwrap();
    // Discovery from inside the kit checkout surfaces both the canonical kit
    // copy and the case copy at repo scope; only the case copy stays enabled,
    // and the canonical kit copy stays disabled for the executor.
    let foreign_path = foreign_case.to_string_lossy().into_owned();
    let kit_path = source
        .join(".agents/skills")
        .join(&lead_only[0])
        .to_string_lossy()
        .into_owned();
    let path_entry = |text: &str, name: &str, path: &str, enabled: bool| {
        text.lines().any(|line| {
            line.starts_with(&format!("name={name};"))
                && line.contains(&format!(";enabled={enabled};"))
                && line.contains(path)
        })
    };
    assert!(
        path_entry(foreign_text.as_str(), &lead_only[0], &foreign_path, true),
        "an unrelated same-name project skill stays enabled: {foreign_text}"
    );
    assert!(
        path_entry(foreign_text.as_str(), &lead_only[0], &kit_path, false),
        "the canonical kit copy stays disabled for the executor: {foreign_text}"
    );
    drop(linked_worktree);
}

/// Explicit native probe of what installed 0.162.1 actually consumes for a
/// managed resume. Fresh starts consume the composed role text and the live
/// skill selection. A resumed thread keeps the developer text saved at its
/// start and retains the skill entries saved then, while skills added later
/// are picked up from the live surface; neither `config.developer_instructions`
/// nor `developerInstructions`
/// on `thread/resume` changes the consumed developer text: the configuration
/// variants produce byte-identical requests. The current revision's text
/// reaches turns only through a fresh conversation, which the restart
/// contrast captures. The installed native CLI is driven over stdio with the
/// same `-c` composition the managed host applies: profile `ds` mirrors the
/// ds/deepseek-flash/deepseek/max binding with only its transport pointed at
/// the canned local Responses server, and the role `-c` pairs come from the
/// same `role_launch` composer the managed host uses. Captured model request
/// bodies prove what each fresh and resumed turn actually consumed. No
/// billed model call and no shared configuration write.
#[test]
#[ignore = "explicit installed native CLI app-server over stdio + canned local Responses; no live model"]
fn native_managed_resume_keeps_saved_role_text_and_refreshes_the_live_catalogue() {
    let upstream = registered_upstream(&live_codex_home());
    assert!(upstream.is_absolute() && upstream.is_file());
    let version = Command::new(&upstream).arg("--version").output().unwrap();
    let version = String::from_utf8_lossy(&version.stdout).trim().to_owned();
    assert!(
        version.contains("0.162.1"),
        "unexpected native CLI: {version}"
    );

    let evidence = tempfile::Builder::new()
        .prefix("native-resume-role-")
        .tempdir()
        .unwrap()
        .keep();
    println!("native resume role evidence: {}", evidence.display());
    let case = evidence.join("case");
    let home = evidence.join("codex");
    fs::create_dir_all(&case).unwrap();
    fs::create_dir_all(&home).unwrap();
    git_identity(&case);
    let source = synthetic_role_build::kit_source(&evidence);
    fs::write(
        source.join("global/executor.md"),
        format!("Fixture executor responsibility\n\n{RESUME_ROLE_ALPHA}\n"),
    )
    .unwrap();
    let build = synthetic_role_build::recorded_build(&evidence, &source);
    synthetic_role_build::register(&home, &evidence, &build);
    let skills = case.join(".agents/skills");
    write_skill(
        &skills.join(RESUME_EARLY_SKILL),
        RESUME_EARLY_SKILL,
        "RESUME_EARLY_MARKER",
    );
    write_skill(
        &skills.join(RESUME_UNRELATED_SKILL),
        RESUME_UNRELATED_SKILL,
        "RESUME_UNRELATED_MARKER",
    );

    let canned = CannedResponses::spawn(&evidence).unwrap();
    write_role_probe_home(&home, &case, &canned, "");

    let (lead_revision, lead_args) =
        role_args(&home, &case, harness_core::role_context::Role::Lead);
    let lead_body = {
        let mut lead = RoleAppServer::spawn(&upstream, &home, &case, &lead_args, false, None);
        lead.initialize();
        let thread = lead.thread_start(&case);
        let body = turn_and_capture(&mut lead, &thread, &canned, 1);
        lead.shutdown();
        body
    };

    let (start_revision, mut start_args) =
        role_args(&home, &case, harness_core::role_context::Role::Executor);
    start_args.push("-c".into());
    start_args.push("agents.enabled=false".into());
    let mut threads = Vec::new();
    let mut start_bodies = Vec::new();
    {
        let mut executor = RoleAppServer::spawn(&upstream, &home, &case, &start_args, true, None);
        executor.initialize();
        for expected in 2..=4 {
            let thread = executor.thread_start(&case);
            let body = turn_and_capture(&mut executor, &thread, &canned, expected);
            threads.push(thread);
            start_bodies.push(body);
        }
        executor.shutdown();
    }

    fs::write(
        source.join("global/executor.md"),
        format!("Fixture executor responsibility\n\n{RESUME_ROLE_BRAVO}\n"),
    )
    .unwrap();
    let build = synthetic_role_build::recorded_build(&evidence, &source);
    synthetic_role_build::register(&home, &evidence, &build);
    let bravo_text =
        harness_core::role_context::resolve(&source, harness_core::role_context::Role::Executor)
            .unwrap()
            .developer_instructions;
    assert!(bravo_text.contains(RESUME_ROLE_BRAVO));
    fs::remove_dir_all(skills.join(RESUME_EARLY_SKILL)).unwrap();
    write_skill(
        &skills.join(RESUME_LATE_SKILL),
        RESUME_LATE_SKILL,
        "RESUME_LATE_MARKER",
    );

    let (resume_revision, mut resume_args) =
        role_args(&home, &case, harness_core::role_context::Role::Executor);
    assert_ne!(
        resume_revision, start_revision,
        "changing the owned role text must change the resolved revision"
    );
    resume_args.push("-c".into());
    resume_args.push("agents.enabled=false".into());
    let variants = [
        ("threadId", json!({"threadId": threads[0]})),
        (
            "config",
            json!({"threadId": threads[1], "config": {"developer_instructions": bravo_text}}),
        ),
        (
            "developerInstructions",
            json!({"threadId": threads[2], "developerInstructions": bravo_text}),
        ),
    ];
    let mut resume_answers = Vec::new();
    let mut resume_bodies = Vec::new();
    let restart_body = {
        let mut executor = RoleAppServer::spawn(&upstream, &home, &case, &resume_args, true, None);
        executor.initialize();
        for (index, (variant, params)) in variants.iter().enumerate() {
            let answer = executor.call("thread/resume", params.clone());
            resume_answers.push(json!({
                "variant": variant,
                "cwd": answer["thread"]["cwd"].clone(),
                "instructionSources": answer
                    .get("instructionSources")
                    .cloned()
                    .unwrap_or(Value::Null),
            }));
            let body = turn_and_capture(&mut executor, &threads[index], &canned, 5 + index);
            resume_bodies.push(body);
        }
        // A fresh conversation on the same live session consumes the current
        // revision: the restart carries the bravo text and the live skill
        // selection, while the resumed threads above keep the text and the
        // early entry saved at their start.
        let restart_thread = executor.thread_start(&case);
        let restart_body = turn_and_capture(&mut executor, &restart_thread, &canned, 8);
        executor.shutdown();
        restart_body
    };

    for (name, body) in start_bodies
        .iter()
        .enumerate()
        .map(|(index, body)| (format!("executor-start-{index}.body.json"), body))
        .chain(std::iter::once(("lead.body.json".to_owned(), &lead_body)))
        .chain(std::iter::once((
            "restart-contrast.body.json".to_owned(),
            &restart_body,
        )))
        .chain(
            resume_bodies
                .iter()
                .enumerate()
                .map(|(index, body)| (format!("resume-{index}.body.json"), body)),
        )
    {
        fs::write(evidence.join(name), body.as_bytes()).unwrap();
    }

    let report = json!({
        "evidence": evidence,
        "upstream": {
            "path": upstream,
            "version": version,
            "sha256": build_identity::hash_file(&upstream).unwrap(),
            "bytes": fs::metadata(&upstream).unwrap().len(),
        },
        "binding": {
            "profile": "ds",
            "model": "deepseek-flash",
            "provider": "deepseek",
            "reasoning_effort": "max",
        },
        "revisions": {
            "lead": lead_revision,
            "executor_start": start_revision,
            "executor_resume": resume_revision,
        },
        "lead": described(&lead_body),
        "executor_start": start_bodies.iter().map(|body| described(body)).collect::<Vec<_>>(),
        "executor_resume": resume_bodies.iter().map(|body| described(body)).collect::<Vec<_>>(),
        "restart": described(&restart_body),
        "resume_answers": resume_answers,
    });
    write_json(&evidence.join("report.json"), &report);
    println!("{}", serde_json::to_string_pretty(&report).unwrap());

    assert_eq!(report["lead"]["lead_text"], true);
    assert_eq!(report["lead"]["executor_text"], false);
    assert_eq!(report["lead"]["multi_agent"], true);
    for (index, start) in report["executor_start"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        assert_eq!(start["alpha"], true, "start body {index}");
        assert_eq!(start["executor_text"], true, "start body {index}");
        assert_eq!(start["early"], true, "start body {index}");
        assert_eq!(start["late"], false, "start body {index}");
        assert_eq!(start["unrelated"], true, "start body {index}");
        assert_eq!(
            start["spawn_agent"], false,
            "an executor session must not advertise agent spawning: {start}"
        );
        assert_eq!(
            start["multi_agent"], false,
            "an executor session must not advertise the orchestration tool: {start}"
        );
    }
    for (index, resume) in report["executor_resume"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        assert_eq!(
            resume["alpha"], true,
            "resumed turn {index} must keep the developer text saved at its start: {resume}"
        );
        assert_eq!(
            resume["bravo"], false,
            "a resumed thread must not load the newer role text retroactively: {resume}"
        );
        assert_eq!(
            resume["early"], true,
            "the entry saved at the start stays in the saved selection: {resume}"
        );
        assert_eq!(
            resume["late"], true,
            "the live catalogue supplies skills added after the start: {resume}"
        );
        assert_eq!(resume["unrelated"], true, "resumed turn {index}: {resume}");
        assert_eq!(
            resume["spawn_agent"], false,
            "an executor session must not advertise agent spawning: {resume}"
        );
        assert_eq!(
            resume["multi_agent"], false,
            "an executor session must not advertise the orchestration tool: {resume}"
        );
    }
    let resumed = report["executor_resume"].as_array().unwrap();
    assert_eq!(
        resumed[0], resumed[1],
        "config.developer_instructions must not change the consumed text"
    );
    assert_eq!(
        resumed[1], resumed[2],
        "developerInstructions must not change the consumed text"
    );
    for entry in report["resume_answers"].as_array().unwrap() {
        assert_eq!(
            entry["cwd"].as_str().map(str::to_lowercase),
            case.to_str().map(str::to_lowercase),
            "thread/resume keeps the session workspace: {entry}"
        );
        assert!(
            entry["instructionSources"].is_array(),
            "thread/resume reports instruction sources as a list: {entry}"
        );
    }
    assert_eq!(
        report["restart"]["bravo"], true,
        "a fresh conversation consumes the current role text"
    );
    assert_eq!(
        report["restart"]["alpha"], false,
        "a fresh conversation drops the predecessor role text"
    );
    assert_eq!(report["restart"]["executor_text"], true);
    assert_eq!(
        report["restart"]["early"], false,
        "a fresh conversation drops the removed skill entry"
    );
    assert_eq!(report["restart"]["late"], true);
    assert_eq!(report["restart"]["unrelated"], true);
    assert_eq!(report["restart"]["spawn_agent"], false);
    assert_eq!(report["restart"]["multi_agent"], false);
}

/// One native app-server child driven over stdio JSON-RPC, the same protocol
/// the managed control connection speaks over its WebSocket. Stdout is
/// drained on a reader thread; only matching request answers are consumed.
struct RoleAppServer {
    child: std::process::Child,
    stdin: Option<std::process::ChildStdin>,
    lines: std::sync::mpsc::Receiver<String>,
    transcript: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    diagnostics: std::sync::Arc<std::sync::Mutex<String>>,
    next_id: u64,
}

impl RoleAppServer {
    fn spawn(
        exe: &Path,
        home: &Path,
        case: &Path,
        args: &[std::ffi::OsString],
        executor: bool,
        user_home: Option<&Path>,
    ) -> Self {
        let mut command = Command::new(exe);
        command.arg("app-server");
        command.args(args);
        command.current_dir(case);
        command.env("CODEX_HOME", home);
        if let Some(user_home) = user_home {
            // Native skill discovery resolves the user farm from the OS home,
            // not CODEX_HOME; the probe pins it to a session-local directory.
            command.env("USERPROFILE", user_home);
            command.env("HOME", user_home);
        }
        command.env(API_KEY_ENV, API_KEY);
        command.env("PATH", filtered_path());
        command.env_remove("OPENAI_API_KEY");
        command.env_remove("CODEX_API_KEY");
        // The managed host strips a leaked parent identity before the
        // app-server starts; the probe must not attach to this session.
        command.env_remove("CODEX_SESSION_ID");
        command.env_remove("CODEX_THREAD_ID");
        command.env_remove("HARNESS_EXECUTOR_RUN");
        if executor {
            command.env("HARNESS_EXECUTOR_SESSION", "1");
        } else {
            command.env_remove("HARNESS_EXECUTOR_SESSION");
        }
        command
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        let mut child = command.spawn().unwrap();
        let stdin = child.stdin.take();
        let stdout = child.stdout.take().unwrap();
        let mut stderr = child.stderr.take().unwrap();
        let (sender, lines) = std::sync::mpsc::channel();
        let transcript = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        {
            let transcript = std::sync::Arc::clone(&transcript);
            std::thread::spawn(move || {
                use std::io::BufRead;
                for line in std::io::BufReader::new(stdout).lines() {
                    let Ok(line) = line else { break };
                    transcript.lock().unwrap().push(line.clone());
                    if sender.send(line).is_err() {
                        break;
                    }
                }
            });
        }
        let diagnostics = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        {
            let diagnostics = std::sync::Arc::clone(&diagnostics);
            std::thread::spawn(move || {
                use std::io::Read;
                let mut buffer = [0u8; 4096];
                while let Ok(read) = stderr.read(&mut buffer) {
                    if read == 0 {
                        break;
                    }
                    diagnostics
                        .lock()
                        .unwrap()
                        .push_str(&String::from_utf8_lossy(&buffer[..read]));
                }
            });
        }
        Self {
            child,
            stdin,
            lines,
            transcript,
            diagnostics,
            next_id: 0,
        }
    }

    fn write(&mut self, value: &Value) {
        use std::io::Write;
        let stdin = self.stdin.as_mut().expect("app-server stdin is open");
        writeln!(stdin, "{value}").unwrap();
        stdin.flush().unwrap();
    }

    fn call(&mut self, method: &str, params: Value) -> Value {
        self.next_id += 1;
        let id = self.next_id;
        self.write(&json!({"id": id, "method": method, "params": params}));
        let deadline = Instant::now() + Duration::from_secs(120);
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let line = self.lines.recv_timeout(remaining).unwrap_or_else(|_| {
                panic!(
                    "{method} was not answered; stderr:\n{}\nstdout tail:\n{}",
                    self.diagnostics.lock().unwrap(),
                    self.tail()
                )
            });
            let Ok(value) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if value.get("id").and_then(Value::as_u64) == Some(id) {
                if let Some(error) = value.get("error") {
                    panic!("{method} was rejected: {error}");
                }
                return value.get("result").cloned().unwrap_or(Value::Null);
            }
        }
    }

    fn initialize(&mut self) {
        self.call(
            "initialize",
            json!({
                "clientInfo": {"name": "harness-role-resume-probe", "version": "1"},
                "capabilities": {"experimentalApi": true},
            }),
        );
        self.write(&json!({"method": "initialized"}));
    }

    fn thread_start(&mut self, case: &Path) -> String {
        let started = self.call(
            "thread/start",
            json!({
                "cwd": case,
                "allowProviderModelFallback": false,
                "model": "deepseek-flash",
                "modelProvider": "deepseek",
                "approvalPolicy": "never",
                "sandbox": "danger-full-access",
                "config": {"model_reasoning_effort": "max"},
            }),
        );
        started["thread"]["id"]
            .as_str()
            .expect("thread/start answers with a thread identity")
            .to_owned()
    }

    fn turn(&mut self, thread: &str) {
        self.call(
            "turn/start",
            json!({
                "threadId": thread,
                "input": [{"type": "text", "text": RESUME_PROMPT}],
            }),
        );
    }

    fn tail(&self) -> String {
        let transcript = self.transcript.lock().unwrap();
        let start = transcript.len().saturating_sub(12);
        transcript[start..].join("\n")
    }

    fn shutdown(mut self) {
        self.stdin.take();
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if self.child.try_wait().unwrap().is_some() {
                return;
            }
            if Instant::now() >= deadline {
                let _ = self.child.kill();
                let _ = self.child.wait();
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

/// Submits one turn on `thread` and returns the raw body of the model request
/// it produced. `expected` is the one-based count of canned Resume requests,
/// so captures are attributed to turns by their exact order.
fn turn_and_capture(
    server: &mut RoleAppServer,
    thread: &str,
    canned: &CannedResponses,
    expected: usize,
) -> String {
    server.turn(thread);
    let captured = canned.wait_for(Duration::from_secs(120), |requests| {
        requests
            .iter()
            .filter(|request| request.kind == RequestKind::Resume)
            .count()
            >= expected
    });
    assert!(
        captured,
        "turn {expected} produced no captured model request"
    );
    canned
        .requests()
        .into_iter()
        .filter(|request| request.kind == RequestKind::Resume)
        .nth(expected - 1)
        .expect("captured model request")
        .body
}

/// The observable consumption markers of one captured model request.
fn described(body: &str) -> Value {
    let tools: Vec<String> = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|value| {
            value["tools"].as_array().map(|tools| {
                tools
                    .iter()
                    .filter_map(|tool| {
                        tool["name"]
                            .as_str()
                            .or_else(|| tool["function"]["name"].as_str())
                            .map(str::to_owned)
                    })
                    .collect()
            })
        })
        .unwrap_or_default();
    json!({
        "alpha": body.contains(RESUME_ROLE_ALPHA),
        "bravo": body.contains(RESUME_ROLE_BRAVO),
        "lead_text": body.contains("Fixture lead responsibility"),
        "executor_text": body.contains("Fixture executor responsibility"),
        "early": body.contains(RESUME_EARLY_SKILL),
        "late": body.contains(RESUME_LATE_SKILL),
        "unrelated": body.contains(RESUME_UNRELATED_SKILL),
        "shared": body.contains("- shared:"),
        "coordinate": body.contains("- coordinate:"),
        "project_probe": body.contains(&format!("- {CATALOGUE_PROJECT_SKILL}:")),
        "user_unrelated": body.contains(&format!("- {CATALOGUE_USER_UNRELATED_SKILL}:")),
        "spawn_agent": tools.iter().any(|name| name.contains("spawn")),
        "multi_agent": tools.iter().any(|name| name.contains("multi_agent")),
        "tools": tools,
    })
}

/// The visible tool surface of one captured model request: tool names reachable
/// through top-level `tools` entries, `additional_tools` input namespaces and
/// entries nested under both, plus the tool-definition text that carries any
/// deferred declarations.
fn tool_surface(body: &str) -> (Vec<String>, String) {
    fn visit(items: &[Value], names: &mut Vec<String>) {
        for item in items {
            if let Some(name) = item.get("name").and_then(Value::as_str) {
                names.push(name.to_owned());
            }
            if let Some(name) = item
                .get("function")
                .and_then(|function| function.get("name"))
                .and_then(Value::as_str)
            {
                names.push(name.to_owned());
            }
            if let Some(nested) = item.get("tools").and_then(Value::as_array) {
                visit(nested, names);
            }
        }
    }
    let mut names = Vec::new();
    let mut definitions = String::new();
    if let Ok(value) = serde_json::from_str::<Value>(body) {
        if let Some(tools) = value.get("tools").and_then(Value::as_array) {
            definitions.push_str(
                &serde_json::to_string(tools).expect("native tool definitions serialize"),
            );
            visit(tools, &mut names);
        }
        if let Some(items) = value.get("input").and_then(Value::as_array) {
            for item in items
                .iter()
                .filter(|item| item["type"] == "additional_tools")
            {
                definitions.push_str(&item.to_string());
                if let Some(tools) = item.get("tools").and_then(Value::as_array) {
                    visit(tools, &mut names);
                }
            }
        }
    }
    (names, definitions)
}

/// The competing native-worktree fragments of one effective tool surface: a
/// tool whose name mentions a worktree, or the `codex_worktrees` marker when
/// the surface defers that dynamic-tools server behind the exec orchestrator.
fn worktree_fragments(surface: &(Vec<String>, String)) -> Vec<String> {
    let mut fragments = surface
        .0
        .iter()
        .filter(|name| name.to_lowercase().contains("worktree"))
        .cloned()
        .collect::<Vec<_>>();
    if surface.1.contains("codex_worktrees") {
        fragments.push("deferred:codex_worktrees".to_owned());
    }
    fragments
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
    spawn_probe_tui_args(exe, home, workspace, &[])
}

fn spawn_probe_tui_args(
    exe: &Path,
    home: &Path,
    workspace: &Path,
    extra: &[&str],
) -> ConsoleSession {
    let mut command = CommandSpec::new(exe);
    command.args = vec!["--no-alt-screen".into()];
    command.args.extend(
        extra
            .iter()
            .map(|argument| std::ffi::OsString::from(*argument)),
    );
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

/// Runs one fresh installed-CLI TUI session against the canned local Responses
/// server and returns the effective tool surface of its first model request.
fn installed_tui_catalogue(
    exe: &Path,
    evidence: &Path,
    label: &str,
    extra: &[&str],
) -> (Vec<String>, String) {
    let root = evidence.join(label);
    let home = root.join("home");
    let workspace = root.join("workspace");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&workspace).unwrap();
    git_identity(&workspace);
    let canned = CannedResponses::spawn_short(&root).unwrap();
    write_probe_config(&home, &workspace, &canned, "", false);
    let session = spawn_probe_tui_args(exe, &home, &workspace, extra);
    ready_named(&session, &root, &home, &format!("WorktreeSurface{label}"));
    send_line(&session, USER_PROMPT).unwrap();
    let sampled = canned.wait_for(Duration::from_secs(60), |requests| {
        requests
            .iter()
            .any(|request| request.kind == RequestKind::FirstSampling)
    });
    assert!(
        sampled,
        "no first sampling request in the {label} catalogue run; evidence {}",
        root.display()
    );
    send_line(&session, "/quit").unwrap();
    let _ = session
        .wait(
            Deadline::after(Duration::from_secs(25)).unwrap(),
            &Cancellation::default(),
            Duration::from_secs(5),
        )
        .unwrap();
    let body = canned
        .requests()
        .into_iter()
        .find(|request| request.kind == RequestKind::FirstSampling)
        .expect("captured first sampling")
        .body;
    fs::write(root.join("first-sampling.body.json"), &body).unwrap();
    let surface = tool_surface(&body);
    write_json(
        &root.join("surface.json"),
        &json!({"names": &surface.0, "definitions": &surface.1}),
    );
    surface
}

/// One runtime catalogue reading from a fresh installed-CLI TUI session: the
/// nested-tool names the session itself reported through a code-mode `exec`
/// call, plus the session's static first-request surface for evidence.
struct RuntimeCatalogue {
    names: Vec<String>,
    surface: (Vec<String>, String),
}

/// The probe call id prefix the canned fixture issues for runtime readings.
const PROBE_CALL_PREFIX: &str = "canned-all-tools-probe-";

/// The runtime tool names one `exec` output item carries, if any. The session
/// exchanges them as one `|`-separated marker payload, a format that a JSON
/// string wrapper around the exec result cannot corrupt; an exec result can
/// arrive as raw text or as content parts, so both shapes are read.
fn probe_names(output: &Value) -> Option<Vec<String>> {
    let mut texts: Vec<&str> = Vec::new();
    match output {
        Value::String(text) => texts.push(text.as_str()),
        Value::Array(parts) => texts.extend(parts.iter().filter_map(|part| {
            part.as_str()
                .or_else(|| part.get("text").and_then(Value::as_str))
        })),
        _ => return None,
    }
    texts.into_iter().find_map(|text| {
        let start = text.find(ALL_TOOLS_MARKER)? + ALL_TOOLS_MARKER.len();
        let rest = &text[start..];
        let end = rest.find(ALL_TOOLS_END)?;
        let names = rest[..end]
            .split('|')
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
            .collect::<Vec<_>>();
        (!names.is_empty()).then_some(names)
    })
}

/// Every distinct runtime catalogue reading carried by `requests`, in arrival
/// order: the probe call id the fixture issued and the runtime tool names the
/// session reported through that code-mode `exec` call. Later requests repeat
/// earlier conversation items, so readings are deduplicated by call id; a new
/// entry means the session executed a newer probe call.
fn probe_readings(
    requests: &[skill_catalog_responses::CapturedRequest],
) -> Vec<(String, Vec<String>)> {
    let mut readings: Vec<(String, Vec<String>)> = Vec::new();
    for request in requests {
        let Ok(body) = serde_json::from_str::<Value>(&request.body) else {
            continue;
        };
        let Some(items) = body.get("input").and_then(Value::as_array) else {
            continue;
        };
        for item in items {
            if item.get("type").and_then(Value::as_str) != Some("custom_tool_call_output") {
                continue;
            }
            let Some(call_id) = item.get("call_id").and_then(Value::as_str) else {
                continue;
            };
            if !call_id.starts_with(PROBE_CALL_PREFIX)
                || readings.iter().any(|(seen, _)| seen == call_id)
            {
                continue;
            }
            if let Some(names) = item.get("output").and_then(probe_names) {
                readings.push((call_id.to_owned(), names));
            }
        }
    }
    readings
}

/// The competing native-worktree entries of one runtime catalogue.
fn worktree_names(names: &[String]) -> Vec<String> {
    names
        .iter()
        .filter(|name| name.to_lowercase().contains("worktree"))
        .cloned()
        .collect()
}

/// Runs one fresh installed-CLI TUI session against the canned local Responses
/// server and returns the session's settled runtime nested-tool catalogue.
/// The static first request can defer dynamic tool servers, so the probe has
/// the session run code-mode `exec` calls that report its own runtime tool
/// names; `rounds` bounds the settle turns, because tool availability can
/// change between calls. A caller that observes worktree-named tools stops
/// early; every other caller keeps probing to the round bound, so an early
/// empty reading is never the final word for the suppressed comparison.
fn installed_tui_all_tools(
    exe: &Path,
    evidence: &Path,
    label: &str,
    extra: &[&str],
    rounds: usize,
) -> RuntimeCatalogue {
    let root = evidence.join(label);
    let home = root.join("home");
    let workspace = root.join("workspace");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&workspace).unwrap();
    git_identity(&workspace);
    let canned = CannedResponses::spawn_all_tools(&root).unwrap();
    write_probe_config(&home, &workspace, &canned, "", false);
    let session = spawn_probe_tui_args(exe, &home, &workspace, extra);
    tui_all_tools_probe(&root, &home, &canned, &session, label, rounds)
}

/// The shared runtime-catalogue probe body: the session must already run
/// against the canned all-tools Responses server. Each of the bounded settle
/// rounds asks the session to report its own runtime tool names; the probe
/// stops early only when a worktree-named tool appeared, so a suppressed
/// comparison never accepts an early empty reading as its final word.
fn tui_all_tools_probe(
    root: &Path,
    home: &Path,
    canned: &CannedResponses,
    session: &ConsoleSession,
    label: &str,
    rounds: usize,
) -> RuntimeCatalogue {
    ready_named(&session, &root, &home, &format!("RuntimeTools{label}"));
    let mut names = Vec::new();
    for round in 1..=rounds {
        let before = probe_readings(&canned.requests());
        send_line(&session, ALL_TOOLS_PROBE_PROMPT).unwrap();
        let captured = canned.wait_for(Duration::from_secs(60), |requests| {
            probe_readings(requests).len() > before.len()
        });
        assert!(
            captured,
            "probe round {round} ({label}) captured no runtime catalogue output; evidence {}",
            root.display()
        );
        let readings = probe_readings(&canned.requests());
        let (call_id, observed) = readings.last().expect("a captured reading is present");
        assert!(
            !before.iter().any(|(earlier, _)| earlier == call_id),
            "probe round {round} ({label}) re-read an earlier probe output; evidence {}",
            root.display()
        );
        names = observed.clone();
        write_json(
            &root.join(format!("all-tools-round-{round}.json")),
            &json!({"call_id": call_id, "names": &names}),
        );
        if !worktree_names(&names).is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_secs(1));
    }
    send_line(&session, "/quit").unwrap();
    let _ = session.wait(
        Deadline::after(Duration::from_secs(25)).unwrap(),
        &Cancellation::default(),
        Duration::from_secs(5),
    );
    let first = canned
        .requests()
        .into_iter()
        .find(|request| request.kind == RequestKind::FirstSampling);
    if let Some(request) = &first {
        fs::write(root.join("first-sampling.body.json"), &request.body).unwrap();
    }
    let surface = first
        .map(|request| tool_surface(&request.body))
        .unwrap_or_default();
    write_json(
        &root.join("surface.json"),
        &json!({"names": &surface.0, "definitions": &surface.1}),
    );
    RuntimeCatalogue { names, surface }
}

fn ready_named(session: &ConsoleSession, evidence: &Path, home: &Path, name: &str) {
    let ready = wait_for(session, evidence, 60, || {
        session
            .transcript()
            .to_ascii_lowercase()
            .contains("gpt-6-astra")
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
