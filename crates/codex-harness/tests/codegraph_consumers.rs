//! Opt-in native Codex consumers of the installed global MCP configuration.
//! Inputs and detailed evidence stay outside the shared checkout. The exec/child
//! probe additionally requires explicit model-call opt-in.
#![cfg(windows)]
use harness_core::{
    broker_endpoint::{self, Observation},
    broker_http,
    broker_state::BrokerRoot,
    cancellable_pipe::{CancellablePipe, anonymous_pipe},
    codegraph_generation::{GenerationStore, StorageLimits},
    codegraph_store,
    dependency_discovery::local_path,
    process::{Cancellation, CommandSpec, Deadline, Job, Limits, OwnedProcess},
};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

fn deadline(seconds: u64) -> Deadline {
    Deadline::after(Duration::from_secs(seconds)).unwrap()
}
fn input(name: &str) -> PathBuf {
    local_path(&PathBuf::from(std::env::var_os(name).expect(name))).unwrap()
}

fn broker_status() -> Value {
    let path = harness_core::codegraph_account::existing_root()
        .unwrap()
        .unwrap();
    let root = BrokerRoot::open(&path).unwrap();
    let Observation::Ready { endpoint, .. } = broker_endpoint::observe(&root).unwrap() else {
        panic!("Installed account broker must be ready");
    };
    broker_http::exchange(
        endpoint.port,
        endpoint.token(),
        "status",
        &json!({}),
        deadline(5),
        &Cancellation::default(),
    )
    .unwrap()
}

fn settled_broker(projects: &[PathBuf], clients: usize) -> Value {
    let until = deadline(60);
    loop {
        let status = broker_status();
        let backend = &status["backend"];
        let active = backend["projects"].as_array().unwrap();
        assert_eq!(backend["clients"], clients, "{status}");
        assert_eq!(
            active
                .iter()
                .filter(|entry| projects
                    .iter()
                    .any(|project| entry["root"] == json!(project)))
                .count(),
            projects.len(),
            "{status}"
        );
        if backend["queued"] == 0
            && projects.iter().all(|project| {
                active.iter().any(|entry| {
                    entry["root"] == json!(project)
                        && entry["observing"] == true
                        && entry["pending"] == false
                        && entry["observation_error"].is_null()
                        && entry["runtime"]["busy"] != true
                        && entry["runtime"]["failed"].is_null()
                })
            })
        {
            return status;
        }
        assert!(
            !until.expired(),
            "Installed broker did not settle: {status}"
        );
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn closed_project(project: &Path, clients: usize) -> Value {
    let until = deadline(15);
    loop {
        let status = broker_status();
        let backend = &status["backend"];
        if backend["clients"] == clients
            && backend["projects"].as_array().unwrap().iter().any(|entry| {
                entry["root"] == json!(project)
                    && entry["observing"] == false
                    && entry["runtime"]["busy"] != true
            })
        {
            return status;
        }
        assert!(
            !until.expired(),
            "Last client did not retire observation: {status}"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

struct Consumer {
    job: Option<Job>,
    child: OwnedProcess,
    input: Option<CancellablePipe>,
    output: CancellablePipe,
    cancel: Cancellation,
    pending: Vec<u8>,
    id: u64,
    thread: String,
    evidence: PathBuf,
}
impl Consumer {
    fn start(executable: &Path, home: &Path, project: &Path, evidence: &Path) -> Self {
        fs::create_dir(evidence).unwrap();
        let (stdin, write) = anonymous_pipe(4096).unwrap();
        let (read, stdout) = anonymous_pipe(4096).unwrap();
        let mut command = CommandSpec::new(executable);
        command.args = vec!["app-server".into(), "--stdio".into()];
        command.current_dir = Some(project.into());
        command.env.insert("CODEX_HOME".into(), Some(home.into()));
        command.stdin = Some(stdin);
        command.stdout = Some(stdout);
        command.stderr = Some(fs::File::create(evidence.join("stderr.txt")).unwrap());
        // Own cleanup of the whole Codex consumer. Each installed provider
        // keeps its own limits; capping Codex plus every retained MCP here
        // would impose a different environment from an ordinary CLI session.
        let job = Job::new(Limits {
            memory_bytes: None,
            cpu_percent: None,
        })
        .unwrap();
        let child = job.spawn(&command).unwrap();
        drop(command);
        let cancel = Cancellation::default();
        let mut consumer = Self {
            job: Some(job),
            child,
            input: Some(CancellablePipe::writer(write, cancel.clone()).unwrap()),
            output: CancellablePipe::reader(read, cancel.clone()).unwrap(),
            cancel,
            pending: Vec::new(),
            id: 0,
            thread: String::new(),
            evidence: evidence.into(),
        };
        let initialized = consumer.request(
            "initialize",
            json!({
                "clientInfo":{"name":"harness-codegraph-consumer","version":"1"},
                "capabilities":{"experimentalApi":true}
            }),
            30,
        );
        assert_eq!(
            local_path(Path::new(initialized["codexHome"].as_str().unwrap())).unwrap(),
            home
        );
        consumer.send(json!({"method":"initialized"}));
        consumer
    }
    fn send(&mut self, value: Value) {
        let mut bytes = serde_json::to_vec(&value).unwrap();
        bytes.push(b'\n');
        self.input
            .as_mut()
            .unwrap()
            .write_all(&bytes, deadline(5), &self.cancel)
            .unwrap();
    }
    fn request(&mut self, method: &str, params: Value, seconds: u64) -> Value {
        self.id += 1;
        self.send(json!({"id":self.id,"method":method,"params":params}));
        let until = deadline(seconds);
        loop {
            if let Some(end) = self.pending.iter().position(|byte| *byte == b'\n') {
                let line: Vec<u8> = self.pending.drain(..=end).collect();
                let value: Value = serde_json::from_slice(&line).unwrap();
                if value.get("id") != Some(&json!(self.id)) {
                    continue;
                }
                fs::write(
                    self.evidence.join(format!("response-{}.json", self.id)),
                    serde_json::to_vec_pretty(&value).unwrap(),
                )
                .unwrap();
                assert!(value.get("error").is_none(), "{method}: {value}");
                return value["result"].clone();
            }
            let next = self.output.read(4096, until, &self.cancel).unwrap();
            assert!(
                !next.is_empty(),
                "Codex closed during {method}; see {:?}",
                self.evidence
            );
            self.pending.extend_from_slice(&next);
            assert!(
                self.pending.len() <= 8 * 1024 * 1024,
                "bounded app-server frame"
            );
        }
    }
    fn open_thread(&mut self, project: &Path) -> Value {
        let opened = self.request(
            "thread/start",
            json!({"cwd":project,"model":"gpt-6-astra",
            "allowProviderModelFallback":false,"ephemeral":false}),
            90,
        );
        self.thread = opened["thread"]["id"].as_str().unwrap().into();
        assert_eq!(opened["model"], "gpt-6-astra", "{opened}");
        opened
    }
    fn tool(&mut self, name: &str, arguments: Value) -> Value {
        let value = self.server_tool("codegraph", name, arguments);
        assert!(serde_json::to_vec(&value).unwrap().len() <= 4096);
        value
    }
    fn server_tool(&mut self, server: &str, name: &str, arguments: Value) -> Value {
        let value = self.request(
            "mcpServer/tool/call",
            json!({
                "threadId":self.thread,"server":server,"tool":name,"arguments":arguments
            }),
            if name == "codegraph_index" { 660 } else { 90 },
        );
        assert_ne!(value["isError"], true, "{value}");
        value
    }
    fn query(&mut self, project: &Path, symbol: &str, expected_file: Option<&str>) -> Value {
        let value = self.tool("codegraph_search", json!({"query":symbol}));
        assert_eq!(
            local_path(Path::new(
                value["structuredContent"]["root"].as_str().unwrap()
            ))
            .unwrap(),
            project
        );
        let text = value.to_string();
        if let Some(file) = expected_file {
            assert!(text.contains(file), "{value}");
        }
        value
    }
    fn finish(mut self) {
        self.input.take().unwrap().close(deadline(5)).unwrap();
        let outcome = self
            .job
            .take()
            .unwrap()
            .wait(
                &self.child,
                deadline(10),
                &Cancellation::default(),
                Duration::from_secs(5),
            )
            .unwrap();
        assert_eq!(outcome.job.active_processes, 0);
    }
}

fn committed(project: &Path, after: u64, symbol: &str, file: Option<&str>) -> u64 {
    let store = GenerationStore::open(project, StorageLimits::default()).unwrap();
    let until = deadline(90);
    loop {
        if let Some(handle) = store.committed_handle().unwrap() {
            let generation = handle.generation.unwrap();
            if generation > after {
                let rows = codegraph_store::symbol_files(
                    &handle.database,
                    symbol,
                    deadline(5),
                    &Cancellation::default(),
                )
                .unwrap();
                let present = file.map_or(rows.is_empty(), |file| {
                    rows.iter()
                        .any(|row| row.replace('\\', "/").ends_with(file))
                });
                if present {
                    return generation;
                }
            }
        }
        assert!(
            !until.expired(),
            "automatic committed generation did not contain {symbol} at {file:?}"
        );
        std::thread::sleep(Duration::from_millis(150));
    }
}

#[test]
#[ignore = "explicit native Codex, installed global home and private output; three owned roots; no model calls"]
fn installed_three_project_consumers_refresh_and_share() {
    let executable = input("CODEGRAPH_CONSUMER_CODEX");
    let home = input("CODEGRAPH_CONSUMER_HOME");
    let output = input("CODEGRAPH_CONSUMER_OUTPUT");
    assert!(
        !output.starts_with(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .parent()
                .unwrap()
        )
    );
    let run = tempfile::Builder::new()
        .prefix("installed-consumers-")
        .tempdir_in(&output)
        .unwrap()
        .keep();
    let before_config = fs::read(home.join("config.toml")).unwrap();
    fs::write(run.join("before-config.toml"), &before_config).unwrap();
    let before_instructions = fs::read(home.join("AGENTS.md")).unwrap();
    assert!(String::from_utf8_lossy(&before_instructions).contains("CodeGraph"));
    let mut projects = Vec::new();
    let mut clients = Vec::new();
    let mut generations = Vec::new();
    for number in 0..3 {
        let project = run.join(format!("project-{number}"));
        fs::create_dir_all(project.join("src")).unwrap();
        assert!(
            std::process::Command::new("git.exe")
                .args(["init", "--quiet"])
                .arg(&project)
                .status()
                .unwrap()
                .success()
        );
        fs::write(
            project.join("Cargo.toml"),
            format!("[package]\nname=\"consumer-{number}\"\nversion=\"0.1.0\"\nedition=\"2024\"\n"),
        )
        .unwrap();
        fs::write(
            project.join("src/lib.rs"),
            "pub fn installed_target() -> u32 { 1 }\n",
        )
        .unwrap();
        let project = local_path(&project).unwrap();
        let mut client = Consumer::start(
            &executable,
            &home,
            &project,
            &run.join(format!("client-{number}")),
        );
        let opened = client.open_thread(&project);
        assert!(
            opened["instructionSources"]
                .as_array()
                .unwrap()
                .iter()
                .any(|path| path.as_str().is_some_and(|path| Path::new(path)
                    .file_name()
                    .is_some_and(|name| name == "AGENTS.md"))),
            "{opened}"
        );
        client.tool("codegraph_index", json!({}));
        generations.push(committed(
            &project,
            0,
            "installed_target",
            Some("src/lib.rs"),
        ));
        client.query(&project, "installed_target", Some("src/lib.rs"));
        projects.push(project);
        clients.push(Some(client));
    }
    let mut duplicate = Consumer::start(
        &executable,
        &home,
        &projects[0],
        &run.join("client-duplicate"),
    );
    duplicate.open_thread(&projects[0]);
    // App-server starts this thread's MCP clients on its first actual call.
    duplicate.query(&projects[0], "installed_target", Some("src/lib.rs"));
    // Retained language tools can create Cargo.lock during initialization.
    // Let automatic refresh in all roots finish before comparing a reusable
    // backend: fair root switching deliberately retires the previous worker.
    let broker = settled_broker(&projects, 4);
    fs::write(
        run.join("shared-broker.json"),
        serde_json::to_vec_pretty(&broker).unwrap(),
    )
    .unwrap();
    let first =
        clients[0]
            .as_mut()
            .unwrap()
            .query(&projects[0], "installed_target", Some("src/lib.rs"));
    let shared = duplicate.query(&projects[0], "installed_target", Some("src/lib.rs"));
    assert_eq!(
        first["structuredContent"]["worker"],
        shared["structuredContent"]["worker"]
    );
    for project in &projects {
        fs::write(
            project.join("src/probe.rs"),
            "pub fn installed_added() {}\n",
        )
        .unwrap();
    }
    for number in 0..3 {
        generations[number] = committed(
            &projects[number],
            generations[number],
            "installed_added",
            Some("src/probe.rs"),
        );
    }
    for project in &projects {
        for value in 0..16 {
            fs::write(
                project.join("src/probe.rs"),
                format!("pub fn installed_changed() -> u32 {{ {value} }}\n"),
            )
            .unwrap();
        }
    }
    for number in 0..3 {
        generations[number] = committed(
            &projects[number],
            generations[number],
            "installed_changed",
            Some("src/probe.rs"),
        );
    }
    for project in &projects {
        fs::rename(project.join("src/probe.rs"), project.join("src/renamed.rs")).unwrap();
    }
    for number in 0..3 {
        generations[number] = committed(
            &projects[number],
            generations[number],
            "installed_changed",
            Some("src/renamed.rs"),
        );
        clients[number].as_mut().unwrap().query(
            &projects[number],
            "installed_changed",
            Some("src/renamed.rs"),
        );
    }
    clients[0].take().unwrap().finish();
    for project in &projects {
        fs::remove_file(project.join("src/renamed.rs")).unwrap();
    }
    for number in 0..3 {
        generations[number] = committed(
            &projects[number],
            generations[number],
            "installed_changed",
            None,
        );
    }
    duplicate.query(&projects[0], "installed_target", Some("src/lib.rs"));
    duplicate.finish();
    let closed = closed_project(&projects[0], 2);
    fs::write(
        run.join("closed-project.json"),
        serde_json::to_vec_pretty(&closed).unwrap(),
    )
    .unwrap();
    generations[0] = GenerationStore::open(&projects[0], StorageLimits::default())
        .unwrap()
        .committed_handle()
        .unwrap()
        .unwrap()
        .generation
        .unwrap();
    fs::write(
        projects[0].join("src/offline.rs"),
        "pub fn installed_offline() {}\n",
    )
    .unwrap();
    fs::write(
        projects[1].join("src/live.rs"),
        "pub fn installed_live() {}\n",
    )
    .unwrap();
    generations[1] = committed(
        &projects[1],
        generations[1],
        "installed_live",
        Some("src/live.rs"),
    );
    assert_eq!(
        GenerationStore::open(&projects[0], StorageLimits::default())
            .unwrap()
            .committed_handle()
            .unwrap()
            .unwrap()
            .generation,
        Some(generations[0]),
        "the last closed project's observation must stop while another project stays active"
    );
    let mut reopened = Consumer::start(
        &executable,
        &home,
        &projects[0],
        &run.join("client-reopened"),
    );
    reopened.open_thread(&projects[0]);
    let catalogue = reopened.request(
        "mcpServerStatus/list",
        json!({"threadId":reopened.thread,"limit":100,"detail":"toolsAndAuthOnly"}),
        90,
    );
    assert!(
        catalogue["data"]
            .as_array()
            .unwrap()
            .iter()
            .any(|server| server["name"] == "codegraph"
                && server["tools"]
                    .as_object()
                    .is_some_and(|tools| !tools.is_empty())),
        "{catalogue}"
    );
    generations[0] = committed(
        &projects[0],
        generations[0],
        "installed_offline",
        Some("src/offline.rs"),
    );
    reopened.query(&projects[0], "installed_offline", Some("src/offline.rs"));
    reopened.finish();
    for client in clients.into_iter().flatten() {
        client.finish();
    }
    let after_config = fs::read(home.join("config.toml")).unwrap();
    fs::write(run.join("after-config.toml"), &after_config).unwrap();
    let parse_config = |bytes: &[u8]| {
        toml::from_str::<Value>(
            std::str::from_utf8(bytes)
                .unwrap()
                .trim_start_matches('\u{feff}'),
        )
        .unwrap()
    };
    let mut before_values = parse_config(&before_config);
    let mut after_values = parse_config(&after_config);
    // Codex itself records trust for each newly opened root. That ordinary
    // consumer bookkeeping is allowed only for this run's owned projects;
    // every other configuration value must stay semantically unchanged.
    let before_projects = before_values
        .as_object_mut()
        .unwrap()
        .remove("projects")
        .unwrap_or_else(|| Value::Object(Default::default()));
    let after_projects = after_values
        .as_object_mut()
        .unwrap()
        .remove("projects")
        .unwrap_or_else(|| Value::Object(Default::default()));
    assert_eq!(
        after_values,
        before_values,
        "Native Codex changed configuration values; see private before/after files in {}",
        run.display()
    );
    for (key, value) in before_projects.as_object().unwrap() {
        assert_eq!(
            after_projects.get(key),
            Some(value),
            "existing project trust changed for {key}"
        );
    }
    for (key, value) in after_projects.as_object().unwrap() {
        if before_projects.as_object().unwrap().contains_key(key) {
            continue;
        }
        let run_text = run.to_string_lossy().to_lowercase();
        let owned_root = key
            .to_lowercase()
            .strip_prefix(&run_text)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('\\') || rest.starts_with('/'));
        assert!(owned_root, "unexpected new trust record {key} = {value}");
        assert_eq!(
            value,
            &json!({"trust_level": "trusted"}),
            "unexpected trust value for new owned root {key}"
        );
    }
    assert_eq!(
        fs::read(home.join("AGENTS.md")).unwrap(),
        before_instructions
    );
    fs::write(run.join("report.json"), serde_json::to_vec_pretty(&json!({"passed":true,"model_calls":0,
        "projects":projects,"generations":generations,"same_root_worker":shared["structuredContent"]["worker"],
        "config_values_preserved":true,"config_reformatted":before_config != after_config,
        "global_config_preserved":true,"automatic_before_queries":true})).unwrap()).unwrap();
    println!("Private installed-consumer evidence: {}", run.display());
}

#[test]
#[ignore = "explicit native Codex/global home, private output, and a saved owned CLI probe thread; no model calls"]
fn installed_saved_thread_resumes_and_forks_with_codegraph() {
    let executable = input("CODEGRAPH_CONSUMER_CODEX");
    let home = input("CODEGRAPH_CONSUMER_HOME");
    let output = input("CODEGRAPH_CONSUMER_OUTPUT");
    let project = input("CODEGRAPH_CONSUMER_RESUME_PROJECT");
    let thread =
        std::env::var("CODEGRAPH_CONSUMER_RESUME_THREAD").expect("explicit owned saved thread");
    let symbol = std::env::var("CODEGRAPH_CONSUMER_RESUME_SYMBOL").expect("explicit source oracle");
    let run = tempfile::Builder::new()
        .prefix("resumed-consumers-")
        .tempdir_in(&output)
        .unwrap()
        .keep();
    let mut client = Consumer::start(&executable, &home, &project, &run.join("resumed"));
    let resumed = client.request(
        "thread/resume",
        json!({"threadId":thread,"cwd":project,"model":"gpt-6-astra"}),
        90,
    );
    assert_eq!(resumed["model"], "gpt-6-astra");
    assert_eq!(resumed["thread"]["id"], thread);
    client.thread = thread.clone();
    let first = client.query(&project, &symbol, None);
    assert!(first.to_string().contains(&symbol));
    let forked = client.request(
        "thread/fork",
        json!({"threadId":thread,"cwd":project,"model":"gpt-6-astra"}),
        90,
    );
    client.thread = forked["thread"]["id"].as_str().unwrap().into();
    assert_ne!(client.thread, thread);
    assert_eq!(forked["model"], "gpt-6-astra");
    let fork_answer = client.query(&project, &symbol, None);
    assert!(fork_answer.to_string().contains(&symbol));
    client.finish();
    fs::write(
        run.join("report.json"),
        serde_json::to_vec_pretty(&json!({"passed":true,"model_calls":0,
        "resumed_thread":thread,"forked_thread":forked["thread"]["id"],"project":project}))
        .unwrap(),
    )
    .unwrap();
    println!("Private resumed-consumer evidence: {}", run.display());
}

#[test]
#[ignore = "explicit installed launcher, owned indexed project and model-probe opt-in; one Astra parent and preferred middle child"]
fn installed_exec_and_tool_capable_child_use_global_codegraph() {
    assert_eq!(
        std::env::var("CODEGRAPH_CONSUMER_MODEL_PROBES").as_deref(),
        Ok("1")
    );
    let powershell = input("CODEGRAPH_CONSUMER_POWERSHELL");
    let launcher = input("CODEGRAPH_CONSUMER_LAUNCHER");
    let home = input("CODEGRAPH_CONSUMER_HOME");
    let project = input("CODEGRAPH_CONSUMER_RESUME_PROJECT");
    let output = input("CODEGRAPH_CONSUMER_OUTPUT");
    let symbol = std::env::var("CODEGRAPH_CONSUMER_RESUME_SYMBOL").expect("explicit owned symbol");
    // The accepted default is the Astra parent; an explicit model allows the
    // same bounded probe when that account quota is unavailable (for example
    // the separately authorized xAI/Grok route).
    let model =
        std::env::var("CODEGRAPH_CONSUMER_EXEC_MODEL").unwrap_or_else(|_| "gpt-6-astra".into());
    let run = tempfile::Builder::new()
        .prefix("model-consumers-")
        .tempdir_in(&output)
        .unwrap()
        .keep();
    let prompt = format!(
        "This is an explicitly authorized, bounded read-only global MCP acceptance probe in an already indexed owned test repository. \
        Discover available tool names through functions.exec ALL_TOOLS if deferred. Call the actual CodeGraph search tool for exact symbol {symbol} once, \
        and verify the returned canonical root is your current working directory. Do not index, sync, edit files or invoke broad exploration. \
        Then spawn exactly one named middle child with fork_context=false and a concise brief: use the same current project, discover the actual CodeGraph tool, \
        search {symbol} once, and return the observed root and result. No recursive delegation. Preserve configured Grok middle; do not silently substitute models. \
        Wait for its actual result, verify it and close the child. Report HARNESS_GLOBAL_CHILD_VERIFIED only after both actual tool calls succeed with the correct root. \
        Preserve exact failures otherwise. No general repository investigation is needed for this fixed acceptance call."
    );
    let mut command = CommandSpec::new(&powershell);
    command.args = vec![
        "-NoLogo".into(),
        "-NoProfile".into(),
        "-File".into(),
        launcher.into(),
        "--harness-effort".into(),
        "routine".into(),
        "exec".into(),
        "--json".into(),
        "--skip-git-repo-check".into(),
        "-m".into(),
        model.clone().into(),
        prompt.into(),
    ];
    command.current_dir = Some(project.clone());
    command.env.insert("CODEX_HOME".into(), Some(home.into()));
    command.stdout = Some(fs::File::create(run.join("events.jsonl")).unwrap());
    command.stderr = Some(fs::File::create(run.join("stderr.txt")).unwrap());
    let job = Job::new(Limits {
        memory_bytes: None,
        cpu_percent: None,
    })
    .unwrap();
    let child = job.spawn(&command).unwrap();
    drop(command);
    let outcome = job
        .wait(
            &child,
            deadline(600),
            &Cancellation::default(),
            Duration::from_secs(10),
        )
        .unwrap();
    assert_eq!(outcome.job.active_processes, 0);
    assert_eq!(
        outcome.exit_code,
        0,
        "see private evidence {}",
        run.display()
    );
    let events = fs::read_to_string(run.join("events.jsonl")).unwrap();
    let rows: Vec<Value> = events
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let thread = rows
        .iter()
        .find(|row| row["type"] == "thread.started")
        .and_then(|row| row["thread_id"].as_str())
        .expect("persisted CLI thread");
    let final_message = rows
        .iter()
        .filter(|row| row["type"] == "item.completed" && row["item"]["type"] == "agent_message")
        .filter_map(|row| row["item"]["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        final_message.contains("HARNESS_GLOBAL_CHILD_VERIFIED"),
        "{final_message}; see {}",
        run.display()
    );
    // The marker also appears in an honest failure report ("is not
    // reported"). A passing probe must not report failure and must show an
    // actual CodeGraph tool call from the parent or the child in the same
    // structured event log.
    assert!(
        !final_message.to_lowercase().contains("is not reported"),
        "probe reported failure: {final_message}; see {}",
        run.display()
    );
    let codegraph_calls = rows
        .iter()
        .filter(|row| row["type"] == "item.completed")
        .filter(|row| {
            row["item"]["type"] == "mcp_tool_call" && row["item"]["server"] == "codegraph"
        })
        .count();
    assert!(
        codegraph_calls > 0,
        "no completed CodeGraph tool call in events; see {}",
        run.display()
    );
    // Keep structured native events for inspection of parent/child tool calls;
    // the model's marker alone is not the final acceptance decision.
    fs::write(
        run.join("report.json"),
        serde_json::to_vec_pretty(&json!({
            "model_reported_success":true,"requires_tool_event_review":true,"thread":thread,
            "model":model,"project":project,"symbol":symbol,"exit_code":outcome.exit_code
        }))
        .unwrap(),
    )
    .unwrap();
    println!(
        "Private model-consumer evidence requiring tool-event review: {}",
        run.display()
    );
}

#[test]
#[ignore = "explicit installed Codex/global home and private saved graph; owned source and browser targets; no model calls"]
fn installed_retained_tools_preserve_source_graph_and_browser_operations() {
    let executable = input("CODEGRAPH_CONSUMER_CODEX");
    let home = input("CODEGRAPH_CONSUMER_HOME");
    let output = input("CODEGRAPH_CONSUMER_OUTPUT");
    let graph = input("CODEGRAPH_CONSUMER_SAVED_GRAPH_PROJECT");
    let graph_query =
        std::env::var("CODEGRAPH_CONSUMER_GRAPH_QUERY").expect("explicit saved-graph oracle");
    let run = tempfile::Builder::new()
        .prefix("retained-consumers-")
        .tempdir_in(output)
        .unwrap()
        .keep();
    let project = run.join("project");
    fs::create_dir_all(project.join("src")).unwrap();
    assert!(
        std::process::Command::new("git.exe")
            .args(["init", "--quiet"])
            .arg(&project)
            .status()
            .unwrap()
            .success()
    );
    fs::write(
        project.join("Cargo.toml"),
        "[package]\nname=\"retained-probe\"\nversion=\"0.1.0\"\nedition=\"2024\"\n",
    )
    .unwrap();
    let original = "pub fn retained_target() -> u32 { 1 }\npub fn retained_caller() -> u32 { retained_target() }\n";
    fs::write(project.join("src/lib.rs"), original).unwrap();
    let mut client = Consumer::start(&executable, &home, &project, &run.join("client"));
    client.open_thread(&project);
    let inventory = client.request(
        "mcpServerStatus/list",
        json!({"threadId":client.thread,"limit":100,"detail":"toolsAndAuthOnly"}),
        90,
    );
    let servers = inventory["data"].as_array().unwrap();
    fs::write(
        run.join("consumer-job.json"),
        serde_json::to_vec_pretty(&client.job.as_ref().unwrap().snapshot().unwrap()).unwrap(),
    )
    .unwrap();
    for name in ["codegraph", "serena", "graphify", "nuphus"] {
        assert!(
            servers.iter().any(|server| server["name"] == name
                && server["tools"]
                    .as_object()
                    .is_some_and(|tools| !tools.is_empty())),
            "missing {name}; see private catalogue response in {}",
            run.display()
        );
    }
    assert!(!servers.iter().any(|server| matches!(
        server["name"].as_str(),
        Some("codebase-memory" | "harness-lsp")
    )));
    client.server_tool("serena", "initial_instructions", json!({}));
    let navigation = client.server_tool("serena", "find_symbol", json!({"relative_path":"src/lib.rs","name_path_pattern":"retained_target","include_body":true,"max_matches":1,"max_answer_chars":2000}));
    assert!(navigation.to_string().contains("retained_target"));
    let references = client.server_tool(
        "serena",
        "find_referencing_symbols",
        json!({"relative_path":"src/lib.rs","name_path":"retained_target","max_answer_chars":3000}),
    );
    assert!(references.to_string().contains("retained_caller"));
    client.server_tool("serena", "replace_symbol_body", json!({"relative_path":"src/lib.rs","name_path":"retained_target","body":"pub fn retained_target() -> u32 { 7 }"}));
    assert!(
        fs::read_to_string(project.join("src/lib.rs"))
            .unwrap()
            .contains("{ 7 }")
    );
    client.server_tool("serena", "replace_symbol_body", json!({"relative_path":"src/lib.rs","name_path":"retained_target","body":"pub fn retained_target() -> u32 { 1 }"}));
    assert_eq!(
        fs::read_to_string(project.join("src/lib.rs"))
            .unwrap()
            .replace("\r\n", "\n"),
        original
    );
    let graph_before = fs::read(graph.join("graphify-out/graph.json")).unwrap();
    client.server_tool("graphify", "graph_stats", json!({"project_path":graph}));
    let queried = client.server_tool("graphify", "query_graph", json!({"project_path":graph,"question":graph_query,"depth":1,"mode":"bfs","token_budget":500}));
    assert!(
        queried
            .to_string()
            .contains(&format!("NODE {graph_query} ")),
        "Expected saved node label; see private response in {}",
        run.display()
    );
    assert_eq!(
        fs::read(graph.join("graphify-out/graph.json")).unwrap(),
        graph_before
    );
    client.server_tool("nuphus", "desktop_windows_list", json!({}));
    client.server_tool("nuphus", "browser_new_tab", json!({"confirm":true}));
    client.server_tool("nuphus", "browser_evaluate", json!({"confirm":true,"script":"if(location.href !== 'about:blank') throw new Error('expected owned blank tab'); document.title='Harness retained acceptance'; document.body.innerHTML='<h1>Owned probe</h1><a href=\"#confirmed\">Confirm probe</a>'; 'owned-ready'"}));
    let snapshot = client.server_tool("nuphus", "browser_snapshot", json!({}));
    assert!(snapshot.to_string().contains("Confirm probe"));
    client.server_tool(
        "nuphus",
        "browser_click",
        json!({"selector":"a[href='#confirmed']","confirm":true}),
    );
    let effect = client.server_tool("nuphus", "browser_evaluate", json!({"confirm":true,"script":"if(document.title!=='Harness retained acceptance' || location.hash!=='#confirmed') throw new Error('owned browser effect missing'); 'HARNESS_BROWSER_EFFECT_CONFIRMED'"}));
    assert!(
        effect
            .to_string()
            .contains("HARNESS_BROWSER_EFFECT_CONFIRMED"),
        "{effect}"
    );
    client.finish();
    fs::write(run.join("report.json"), serde_json::to_vec_pretty(&json!({"passed":true,"model_calls":0,"source_restored":true,"saved_graph_preserved":true,"browser_effect":effect})).unwrap()).unwrap();
    println!("Private retained-consumer evidence: {}", run.display());
}
