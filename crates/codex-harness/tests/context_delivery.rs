//! Opt-in actual TUI acceptance. Setup mode never submits a model prompt.
#![cfg(windows)]
#[path = "fixtures/native_sampling.rs"]
mod native_sampling;

use harness_core::{
    build_identity,
    console::{ConsoleSession, ConsoleSpec},
    process::{Cancellation, CommandSpec, Deadline, Job, Limits, StopReason},
};
use serde_json::{Value, json};
use std::{
    fs, io,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

fn env_path(name: &str) -> PathBuf {
    PathBuf::from(std::env::var_os(name).expect(name))
}

fn prepare_home(root: &Path, shell: &Path) -> (PathBuf, PathBuf) {
    assert!(
        root.is_absolute() && !root.exists(),
        "fixture root must be new"
    );
    let source = std::env::var_os("HARNESS_CONTEXT_SOURCE")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .to_owned()
        });
    let live = env_path("USERPROFILE").join(".codex");
    let metadata: Value =
        serde_json::from_slice(&fs::read(live.join("harness/installation.json")).unwrap()).unwrap();
    let auth: Value = serde_json::from_slice(&fs::read(live.join("auth.json")).unwrap()).unwrap();
    assert_eq!(auth["auth_mode"], "chatgpt");
    assert!(auth["OPENAI_API_KEY"].is_null());
    assert!(
        fs::read_to_string(live.join("config.toml"))
            .unwrap()
            .lines()
            .any(|line| line.trim() == "openai_base_url = \"http://127.0.0.1:10100/v1\"")
    );
    let home = root.join("home");
    let user = root.join("user");
    let workspace = root.join("workspace");
    for path in [&home, &user, &workspace] {
        fs::create_dir_all(path).unwrap();
    }
    let catalog =
        serde_json::to_string(live.join("opencodex-catalog.json").to_str().unwrap()).unwrap();
    let trusted = serde_json::to_string(&workspace.to_str().unwrap().to_lowercase()).unwrap();
    fs::write(
        home.join("config.toml"),
        format!(
            r#"model = "gpt-6-astra"
model_provider = "openai"
model_catalog_json = {catalog}
openai_base_url = "http://127.0.0.1:10100/v1"
approval_policy = "never"
sandbox_mode = "danger-full-access"
web_search = "disabled"
[features]
hooks = false
apps = false
multi_agent = false
multi_agent_v2 = false
memories = false
goals = true
[projects.{trusted}]
trust_level = "trusted"
"#
        ),
    )
    .unwrap();
    std::os::windows::fs::symlink_file(live.join("auth.json"), home.join("auth.json")).unwrap();
    let mut command = CommandSpec::new(shell);
    command.args = vec![
        "-NoLogo".into(),
        "-NoProfile".into(),
        "-File".into(),
        source.join("install.ps1").into_os_string(),
        "-Mode".into(),
        "Install".into(),
        "-CoreOnly".into(),
        "-CodexHome".into(),
        home.clone().into_os_string(),
        "-UserHome".into(),
        user.into_os_string(),
        "-PathScope".into(),
        "Process".into(),
        "-CodexCommand".into(),
        metadata["codexCommand"].as_str().unwrap().into(),
    ];
    command.current_dir = Some(workspace);
    command.stdout = Some(fs::File::create(root.join("install.stdout.txt")).unwrap());
    command.stderr = Some(fs::File::create(root.join("install.stderr.txt")).unwrap());
    let job = Job::new(Limits {
        memory_bytes: Some(512 * 1024 * 1024),
        cpu_percent: Some(50.0),
    })
    .unwrap();
    let child = job.spawn(&command).unwrap();
    let outcome = job
        .wait(
            &child,
            Deadline::after(Duration::from_secs(60)).unwrap(),
            &Cancellation::default(),
            Duration::from_secs(5),
        )
        .unwrap();
    write_json(&root.join("install-process.json"), &json!(outcome));
    assert_eq!(outcome.reason, StopReason::Exited);
    assert_eq!(outcome.exit_code, 0);
    let launcher = home.join("harness/bin/codex.ps1");
    (home, launcher)
}
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
            } else if entry.path().extension().is_some_and(|v| v == "jsonl") {
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
fn wait_for(session: &ConsoleSession, root: &Path, seconds: u64, predicate: impl Fn() -> bool) {
    let until = Instant::now() + Duration::from_secs(seconds);
    while Instant::now() < until {
        fs::write(root.join("terminal.txt"), session.transcript()).unwrap();
        if predicate() {
            return;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    panic!(
        "native context condition timed out; evidence {}",
        root.display()
    );
}

#[test]
#[ignore = "explicit retained owned Codex log paths; no model calls"]
fn sampling_reader_matches_retained_runtime_evidence() {
    let enabled = native_sampling::read(&env_path("HARNESS_CONTEXT_ENABLED_LOG")).unwrap();
    let disabled = native_sampling::read(&env_path("HARNESS_CONTEXT_DISABLED_LOG")).unwrap();
    assert!(!enabled.is_empty() && !disabled.is_empty());
    assert!(
        enabled
            .iter()
            .all(|v| v["model"] == "gpt-6-astra" && v["context_management"] == true)
    );
    assert!(
        disabled
            .iter()
            .all(|v| v["model"] == "gpt-6-astra" && v["context_management"] == false)
    );
    let auxiliary = native_sampling::read(&env_path("HARNESS_CONTEXT_AUX_LOG")).unwrap();
    assert!(
        auxiliary.iter().any(|row| row["model"] != "gpt-6-astra"),
        "the retained nonconforming TUI must expose its auxiliary model"
    );
    println!(
        "native sampling reader: {} enabled, {} disabled; auxiliary model detected",
        enabled.len(),
        disabled.len()
    );
}

#[test]
#[ignore = "explicit owned installed home and setup/model mode required"]
fn ordinary_context_delivery() {
    let root = env_path("HARNESS_CONTEXT_EVIDENCE");
    let shell = env_path("HARNESS_CONTEXT_SHELL");
    let (home, launcher) = if std::env::var_os("HARNESS_CONTEXT_HOME").is_some() {
        (
            env_path("HARNESS_CONTEXT_HOME"),
            env_path("HARNESS_CONTEXT_LAUNCHER"),
        )
    } else {
        prepare_home(&root, &shell)
    };
    let workspace = std::env::var_os("HARNESS_CONTEXT_WORKSPACE")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("workspace"));
    assert!(workspace.is_dir() && !home.join("sessions").exists());
    assert!(!root.join("request.json").exists());
    let mode = std::env::var("HARNESS_CONTEXT_MODE").unwrap();
    assert!(["setup", "enabled", "override", "rollback"].contains(&mode.as_str()));
    let profile = home.join("harness.config.toml");
    let profile_hash = build_identity::hash_file(&profile).unwrap();
    let base_hash = build_identity::hash_file(&home.join("config.toml")).unwrap();
    let mut command = CommandSpec::new(&shell);
    command.args = vec![
        "-NoLogo".into(),
        "-NoProfile".into(),
        "-File".into(),
        launcher.clone().into_os_string(),
        "--harness-effort".into(),
        "routine".into(),
        "--no-alt-screen".into(),
    ];
    if mode == "override" {
        command.args.extend([
            "-c".into(),
            "features.context_management.experimental_mode=false".into(),
        ]);
    }
    // Use the Windows spelling observed in native-persisted trust entries.
    let trusted = serde_json::to_string(&workspace.to_str().unwrap().to_lowercase()).unwrap();
    command.args.extend([
        "-c".into(),
        format!("projects={{ {trusted} = {{ trust_level = \"trusted\" }} }}").into(),
    ]);
    command.current_dir = Some(workspace.clone());
    command
        .env
        .insert("CODEX_HOME".into(), Some(home.clone().into_os_string()));
    let child_path = std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .filter(|entry| {
            !entry.components().any(|part| {
                part.as_os_str()
                    .to_string_lossy()
                    .eq_ignore_ascii_case("WindowsApps")
            })
        })
        .collect::<Vec<_>>();
    command.env.insert(
        "PATH".into(),
        Some(std::env::join_paths(child_path).unwrap()),
    );
    write_json(
        &root.join("request.json"),
        &json!({"mode":mode,"shell":shell,"launcher":launcher,
        "args":command.args.iter().map(|arg| arg.to_string_lossy()).collect::<Vec<_>>(),"cwd":workspace,"home":home,"profile_sha256":profile_hash,
        "base_sha256":base_hash,"model":"gpt-6-astra","effort":"low"}),
    );
    let mut spec = ConsoleSpec::new(command);
    spec.limits.memory_bytes = Some(512 * 1024 * 1024);
    spec.limits.cpu_percent = Some(50.0);
    let session = ConsoleSession::spawn(spec).unwrap();
    write_json(
        &root.join("started.json"),
        &json!({"pid":session.identity().pid,"creation_time":session.identity().creation_time}),
    );
    wait_for(&session, &root, 40, || {
        session.transcript().contains("gpt-6-astra")
    });
    std::thread::sleep(Duration::from_secs(2));
    send_line(&session, "/rename Native context delivery").unwrap();
    wait_for(&session, &root, 15, || {
        rows(&home.join("session_index.jsonl"))
            .iter()
            .any(|row| row["thread_name"] == "Native context delivery")
    });
    // A confirmed literal title prevents the CLI's automatic non-Astra title
    // request. Never submit a task if naming was not established.
    if mode != "setup" {
        send_line(&session, "Return exactly CONTEXT_DELIVERY_OK. This is a response check; no tools or file work are needed.").unwrap();
        wait_for(&session, &root, 100, || {
            session_rows(&home).iter().any(|row| {
                row["type"] == "event_msg"
                    && row["payload"]["type"] == "task_complete"
                    && row["payload"]["last_agent_message"]
                        .as_str()
                        .is_some_and(|text| text.trim() == "CONTEXT_DELIVERY_OK")
            })
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
    fs::write(root.join("terminal.txt"), &result.transcript).unwrap();
    write_json(
        &root.join("process.json"),
        &json!({"outcome":result.outcome,"truncated":result.output_truncated}),
    );
    assert_eq!(result.outcome.reason, StopReason::Exited);
    assert_eq!(result.outcome.exit_code, 0);
    assert!(!result.output_truncated);
    let sampling = native_sampling::read(&home.join("logs_2.sqlite")).unwrap();
    write_json(&root.join("sampling.json"), &json!(sampling));
    if mode == "setup" {
        assert!(sampling.is_empty());
    } else {
        assert!(
            !sampling.is_empty(),
            "missing logs do not prove model or runtime state"
        );
        for record in &sampling {
            assert_eq!(record["model"], "gpt-6-astra");
            assert_eq!(record["context_management"], mode == "enabled");
        }
        assert!(
            !session_rows(&home)
                .iter()
                .any(|row| row["type"] == "response_item"
                    && matches!(
                        row["payload"]["type"].as_str(),
                        Some("function_call" | "custom_tool_call")
                    )),
            "response check must perform no tool/file work"
        );
    }
    assert_eq!(build_identity::hash_file(&profile).unwrap(), profile_hash);
    assert_eq!(
        build_identity::hash_file(&home.join("config.toml")).unwrap(),
        base_hash
    );
    write_json(
        &root.join("acceptance.json"),
        &json!({"mode":mode,"passed":true,"sampling":sampling,
        "configuration_unchanged":true,"natural_exit":true}),
    );
    println!("Context {mode} acceptance: {}", root.display());
}
