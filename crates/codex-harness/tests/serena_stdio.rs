//! End-to-end Serena proxy acceptance through the actual broker service.
//! Requires HARNESS_CODE_TOOLS_REGISTRY for the adopted foreign package.
#![cfg(windows)]

use harness_core::{
    broker_endpoint, broker_launch,
    broker_state::BrokerRoot,
    process::{Cancellation, Deadline},
};
use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex, mpsc},
    thread,
    time::Duration,
};

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn adopted_console() -> PathBuf {
    let path = PathBuf::from(
        std::env::var_os("HARNESS_CODE_TOOLS_REGISTRY").expect("explicit adopted registry"),
    );
    let inventory: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    PathBuf::from(
        inventory["mcp"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["id"] == "serena")
            .unwrap()["paths"]["console_entrypoint"]
            .as_str()
            .unwrap(),
    )
}

fn crate_project(root: &Path, name: &str, marker: i32) -> PathBuf {
    let project = root.join(name);
    fs::create_dir_all(project.join(".serena")).unwrap();
    fs::create_dir_all(project.join("src")).unwrap();
    fs::write(
        project.join(".serena/project.yml"),
        format!("project_name: '{name}'\nlanguage_servers:\n- rust\nencoding: utf-8\n"),
    )
    .unwrap();
    fs::write(
        project.join("Cargo.toml"),
        format!("[package]\nname = \"proxy-{marker}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
    )
    .unwrap();
    fs::write(
        project.join("src/lib.rs"),
        format!(
            "pub fn shared() -> i32 {{ {marker} }}\npub fn local_use() -> i32 {{ shared() }}\n"
        ),
    )
    .unwrap();
    project
}

fn python_project(root: &Path, name: &str, marker: i32) -> PathBuf {
    let project = root.join(name);
    fs::create_dir_all(project.join(".serena")).unwrap();
    fs::create_dir_all(project.join("src")).unwrap();
    fs::write(
        project.join(".serena/project.yml"),
        format!(
            "project_name: '{name}'\nlanguage_servers:\n- python_basedpyright\nencoding: utf-8\n"
        ),
    )
    .unwrap();
    fs::write(
        project.join("pyproject.toml"),
        format!("[project]\nname = \"py-{marker}\"\nversion = \"0.1.0\"\n"),
    )
    .unwrap();
    fs::write(
        project.join("src/mod.py"),
        format!(
            "def shared() -> int:\n    return {marker}\n\n\ndef broken() -> int:\n    return 'not an int'\n"
        ),
    )
    .unwrap();
    project
}

struct Proxy {
    child: Child,
    responses: mpsc::Receiver<String>,
    stderr: Arc<Mutex<Vec<String>>>,
}

impl Proxy {
    fn start(
        _root: &Path,
        codex_home: &Path,
        registry: &Path,
        project: &Path,
        console: &Path,
    ) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
            .arg("mcp")
            .arg("serena")
            .arg("--serena")
            .arg(console)
            .arg("--registry")
            .arg(registry)
            .arg("--codex-home")
            .arg(codex_home)
            .arg("--source-root")
            .arg(repo())
            .arg("--connection-seconds")
            .arg("900")
            .current_dir(project)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("proxy spawn");
        let stdout = child.stdout.take().unwrap();
        let stderr = Arc::new(Mutex::new(Vec::new()));
        let error_lines = Arc::clone(&stderr);
        let error_stream = child.stderr.take().unwrap();
        thread::spawn(move || {
            let reader = BufReader::new(error_stream);
            for line in reader.lines().map_while(Result::ok) {
                let mut lines = error_lines.lock().unwrap();
                lines.push(line);
                if lines.len() > 40 {
                    lines.remove(0);
                }
            }
        });
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines() {
                match line {
                    Ok(line) => {
                        if sender.send(line).is_err() {
                            return;
                        }
                    }
                    Err(_) => return,
                }
            }
        });
        Self {
            child,
            responses: receiver,
            stderr,
        }
    }

    fn request(&mut self, id: u64, method: &str, params: Value) -> Value {
        {
            let stdin = self.child.stdin.as_mut().expect("proxy stdin");
            let written = writeln!(
                stdin,
                "{}",
                json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
            );
            if let Err(error) = written.and_then(|_| stdin.flush()) {
                panic!(
                    "proxy write for request {id} ({method}) failed: {error}; proxy stderr: {:?}",
                    self.stderr.lock().unwrap()
                );
            }
        }
        self.response(id)
    }

    fn response(&mut self, id: u64) -> Value {
        let deadline = Instant::now() + Duration::from_secs(240);
        while Instant::now() < deadline {
            match self.responses.recv_timeout(Duration::from_millis(500)) {
                Ok(line) => {
                    let value: Value = serde_json::from_str(line.trim()).unwrap();
                    if value["id"] == json!(id) {
                        return value;
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    panic!(
                        "proxy closed before answering request {id}; proxy stderr: {:?}",
                        self.stderr.lock().unwrap()
                    );
                }
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
            }
        }
        panic!("proxy request {id} timed out");
    }

    fn finish(mut self) {
        drop(self.child.stdin.take());
        let deadline = Instant::now() + Duration::from_secs(30);
        let status = loop {
            match self.child.try_wait().unwrap() {
                Some(status) => break status,
                None if Instant::now() > deadline => panic!("proxy did not exit"),
                None => thread::sleep(Duration::from_millis(50)),
            }
        };
        assert!(
            status.success(),
            "proxy exit: {status:?}; proxy stderr: {:?}",
            self.stderr.lock().unwrap()
        );
    }
}

use std::time::Instant;

fn broker_status(codex_home: &Path) -> Value {
    let anchor = codex_home.join("harness/runtime/serena-broker.json");
    let record: Value = serde_json::from_slice(&fs::read(&anchor).unwrap()).unwrap();
    let root = BrokerRoot::open(Path::new(record["root"].as_str().unwrap())).unwrap();
    let Ok(broker_endpoint::Observation::Ready { endpoint, .. }) = broker_endpoint::observe(&root)
    else {
        panic!("serena broker endpoint missing");
    };
    harness_core::broker_http::exchange(
        endpoint.port,
        endpoint.token(),
        "status",
        &json!({}),
        Deadline::after(Duration::from_secs(10)).unwrap(),
        &Cancellation::default(),
    )
    .unwrap()
}

/// The pool status entry serving one canonical project root.
fn worker_pid(status: &Value, project: &Path) -> Option<u64> {
    let expected = fs::canonicalize(project).unwrap();
    status["backend"]["workers"]
        .as_array()?
        .iter()
        .find_map(|worker| {
            let path = PathBuf::from(worker["project"].as_str()?);
            (fs::canonicalize(path).ok()? == expected).then(|| worker["pid"].as_u64())?
        })
}

fn symbol_text(proxy: &mut Proxy, id: u64) -> String {
    let response = proxy.request(
        id,
        "tools/call",
        json!({
            "name": "find_symbol",
            "arguments": {
                "relative_path": "src/lib.rs",
                "name_path_pattern": "shared",
                "include_body": true
            }
        }),
    );
    response["result"]["content"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|item| item["text"].as_str())
        .collect()
}

#[test]
#[ignore = "requires explicit HARNESS_CODE_TOOLS_REGISTRY for the adopted Serena package"]
fn mcp_serena_proxy_serves_the_native_tool_selection_to_isolated_projects() {
    let console = adopted_console();
    let registry = PathBuf::from(
        std::env::var_os("HARNESS_CODE_TOOLS_REGISTRY").expect("explicit adopted registry"),
    );
    let root = tempfile::tempdir().unwrap();
    eprintln!("Serena proxy end-to-end root: {}", root.path().display());
    let codex_home = root.path().join("codex-home");
    let alpha = crate_project(root.path(), "proxy alpha", 401);
    let beta = crate_project(root.path(), "proxy beta", 402);
    let python = python_project(root.path(), "proxy python", 411);
    let accepted = [
        "activate_project",
        "find_declaration",
        "find_implementations",
        "find_referencing_symbols",
        "find_symbol",
        "get_diagnostics_for_file",
        "get_symbols_overview",
        "insert_after_symbol",
        "insert_before_symbol",
        "rename_symbol",
        "replace_in_files",
        "replace_symbol_body",
        "safe_delete_symbol",
    ];
    let excluded = [
        // The generated selection.
        "onboarding",
        "initial_instructions",
        "get_current_config",
        "list_memories",
        "read_memory",
        "write_memory",
        "edit_memory",
        "delete_memory",
        "rename_memory",
        "search_for_pattern",
        // The built-in `codex` context selection.
        "create_text_file",
        "read_file",
        "execute_shell_command",
        "replace_content",
        "find_file",
        "list_dir",
    ];
    let initialize = json!({
        "protocolVersion": "2024-11-05",
        "capabilities": {},
        "clientInfo": {"name": "proxy-e2e", "version": "0.1.0"}
    });

    let mut first = Proxy::start(root.path(), &codex_home, &registry, &alpha, &console);
    let reply = first.request(1, "initialize", initialize.clone());
    assert_eq!(reply["result"]["serverInfo"]["name"], "Serena");
    let instructions = reply["result"]["instructions"]
        .as_str()
        .expect("the managed connection prompt is present");
    assert!(!instructions.is_empty());

    let catalogue = first.request(2, "tools/list", json!({}));
    let names: Vec<&str> = catalogue["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect();
    assert_eq!(names.len(), accepted.len(), "{names:?}");
    for name in accepted {
        assert!(names.contains(&name), "{name} missing from {names:?}");
    }
    for name in excluded {
        assert!(!names.contains(&name), "{name} still exposed in {names:?}");
        assert!(
            !instructions.contains(name),
            "the managed guidance names {name}: {instructions}"
        );
    }

    // An excluded tool is refused by the worker itself, not by a proxy filter.
    let refused = first.request(
        3,
        "tools/call",
        json!({"name": "read_memory", "arguments": {"memory_name": "probe"}}),
    );
    assert_eq!(refused["result"]["isError"], true, "{refused}");
    let text: String = refused["result"]["content"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|item| item["text"].as_str())
        .collect();
    assert!(text.contains("Unknown tool"), "{text}");

    let search = first.request(
        4,
        "tools/call",
        json!({
            "name": "find_symbol",
            "arguments": {
                "relative_path": "src/lib.rs",
                "name_path_pattern": "shared",
                "include_body": true
            }
        }),
    );
    let text: String = search["result"]["content"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|item| item["text"].as_str())
        .collect();
    assert!(text.contains("401"), "{text}");

    // A second stdio client of the same project shares the broker worker.
    let mut second = Proxy::start(root.path(), &codex_home, &registry, &alpha, &console);
    let reply = second.request(1, "initialize", initialize.clone());
    assert_eq!(reply["result"]["serverInfo"]["name"], "Serena");
    let status = broker_status(&codex_home);
    assert_eq!(status["backend"]["workers"].as_array().unwrap().len(), 1);
    assert_eq!(status["backend"]["clients"], 2, "{status}");

    // A different project gets its own isolated worker.
    let mut third = Proxy::start(root.path(), &codex_home, &registry, &beta, &console);
    let reply = third.request(1, "initialize", initialize.clone());
    assert_eq!(reply["result"]["serverInfo"]["name"], "Serena");
    let search = third.request(
        2,
        "tools/call",
        json!({
            "name": "find_symbol",
            "arguments": {
                "relative_path": "src/lib.rs",
                "name_path_pattern": "shared",
                "include_body": true
            }
        }),
    );
    let text: String = search["result"]["content"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|item| item["text"].as_str())
        .collect();
    assert!(text.contains("402"), "{text}");
    let status = broker_status(&codex_home);
    assert_eq!(status["backend"]["workers"].as_array().unwrap().len(), 2);

    // Representative Python operations and honest diagnostics use the same
    // managed route with its own isolated worker.
    let mut fourth = Proxy::start(root.path(), &codex_home, &registry, &python, &console);
    let reply = fourth.request(1, "initialize", initialize.clone());
    assert_eq!(reply["result"]["serverInfo"]["name"], "Serena");
    let search = fourth.request(
        2,
        "tools/call",
        json!({
            "name": "find_symbol",
            "arguments": {
                "relative_path": "src/mod.py",
                "name_path_pattern": "shared",
                "include_body": true
            }
        }),
    );
    let text: String = search["result"]["content"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|item| item["text"].as_str())
        .collect();
    assert!(text.contains("411"), "{text}");
    let diagnostics = fourth.request(
        3,
        "tools/call",
        json!({"name": "get_diagnostics_for_file", "arguments": {"relative_path": "src/mod.py"}}),
    );
    let text: String = diagnostics["result"]["content"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|item| item["text"].as_str())
        .collect();
    assert!(text.contains("reportReturnType"), "{text}");
    let status = broker_status(&codex_home);
    assert_eq!(status["backend"]["workers"].as_array().unwrap().len(), 3);

    // Client EOF closes each proxy cleanly; the broker keeps serving.
    first.finish();
    second.finish();
    third.finish();
    fourth.finish();
    let status = broker_status(&codex_home);
    assert_eq!(status["backend"]["workers"].as_array().unwrap().len(), 3);

    let anchor = codex_home.join("harness/runtime/serena-broker.json");
    let record: Value = serde_json::from_slice(&fs::read(&anchor).unwrap()).unwrap();
    let root = BrokerRoot::open(Path::new(record["root"].as_str().unwrap())).unwrap();
    let retirement = broker_launch::retire(
        &root,
        Deadline::after(Duration::from_secs(30)).unwrap(),
        &Cancellation::default(),
    )
    .unwrap();
    assert!(!matches!(
        retirement,
        broker_launch::Retirement::Pending { .. }
    ));
}

#[test]
#[ignore = "requires explicit HARNESS_CODE_TOOLS_REGISTRY for the adopted Serena package"]
fn grown_location_record_still_completes_handshake_after_deliveries() {
    let console = adopted_console();
    let registry = PathBuf::from(
        std::env::var_os("HARNESS_CODE_TOOLS_REGISTRY").expect("explicit adopted registry"),
    );
    let root = tempfile::tempdir().unwrap();
    let codex_home = root.path().join("codex-home");
    let project = crate_project(root.path(), "proxy growth", 403);
    // Reproduce a location record grown by repeated deliveries: many dead
    // generations, larger than the old fixed read bound.
    let mut locations = Vec::new();
    let mut generations = Vec::new();
    for seed in 0u32..30 {
        let location = BrokerRoot::prepare().unwrap().keep().path().to_path_buf();
        generations.push(json!({
            "source": format!("{seed:064x}"),
            "root": location,
        }));
        locations.push(location);
    }
    let runtime = codex_home.join("harness/runtime");
    fs::create_dir_all(&runtime).unwrap();
    let anchor = runtime.join("serena-broker.json");
    let record = json!({
        "owner": "codex-harness-serena-broker",
        "account": harness_core::process_service::current_user().unwrap(),
        "root": locations[0],
        "generations": generations,
    });
    let bytes = serde_json::to_vec(&record).unwrap();
    assert!(bytes.len() > 4096, "the fixture must stay grown");
    fs::write(&anchor, &bytes).unwrap();

    let mut proxy = Proxy::start(root.path(), &codex_home, &registry, &project, &console);
    let reply = proxy.request(
        1,
        "initialize",
        json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "proxy-growth", "version": "0.1.0"}
        }),
    );
    assert_eq!(reply["result"]["serverInfo"]["name"], "Serena", "{reply}");
    let catalogue = proxy.request(2, "tools/list", json!({}));
    let names: Vec<&str> = catalogue["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect();
    assert!(names.contains(&"find_symbol"), "{names:?}");
    proxy.finish();

    let rewritten: Value = serde_json::from_slice(&fs::read(&anchor).unwrap()).unwrap();
    assert_eq!(
        rewritten["generations"].as_array().unwrap().len(),
        1,
        "{rewritten}"
    );
    assert!(
        fs::metadata(&anchor).unwrap().len() < 4096,
        "the rewritten record must be bounded"
    );
    let broker = BrokerRoot::open(Path::new(rewritten["root"].as_str().unwrap())).unwrap();
    let retirement = broker_launch::retire(
        &broker,
        Deadline::after(Duration::from_secs(30)).unwrap(),
        &Cancellation::default(),
    )
    .unwrap();
    assert!(!matches!(
        retirement,
        broker_launch::Retirement::Pending { .. }
    ));
    drop(broker);
    for location in locations {
        let _ = fs::remove_dir_all(location);
    }
}

#[test]
#[ignore = "requires explicit HARNESS_CODE_TOOLS_REGISTRY for the adopted Serena package"]
fn shared_broker_keeps_independent_roots_and_replaces_changed_configuration() {
    let console = adopted_console();
    let registry = PathBuf::from(
        std::env::var_os("HARNESS_CODE_TOOLS_REGISTRY").expect("explicit adopted registry"),
    );
    let root = tempfile::tempdir().unwrap();
    eprintln!("Serena broker independence root: {}", root.path().display());
    let codex_home = root.path().join("codex-home");
    let alpha_project = crate_project(root.path(), "broker alpha", 501);
    let beta_project = crate_project(root.path(), "broker beta", 502);
    let initialize = json!({
        "protocolVersion": "2024-11-05",
        "capabilities": {},
        "clientInfo": {"name": "broker-independence", "version": "0.1.0"}
    });

    // Two clients of one root share a worker; an independent root gets its own.
    // Each proxy prepares the shared generated home before the next starts;
    // concurrent first materialization of one CODEX_HOME is not part of this
    // acceptance.
    let mut first = Proxy::start(
        root.path(),
        &codex_home,
        &registry,
        &alpha_project,
        &console,
    );
    let reply = first.request(1, "initialize", initialize.clone());
    assert_eq!(reply["result"]["serverInfo"]["name"], "Serena", "{reply}");
    let mut second = Proxy::start(
        root.path(),
        &codex_home,
        &registry,
        &alpha_project,
        &console,
    );
    let reply = second.request(1, "initialize", initialize.clone());
    assert_eq!(reply["result"]["serverInfo"]["name"], "Serena", "{reply}");
    let mut third = Proxy::start(root.path(), &codex_home, &registry, &beta_project, &console);
    let reply = third.request(1, "initialize", initialize.clone());
    assert_eq!(reply["result"]["serverInfo"]["name"], "Serena", "{reply}");
    let alpha_text = symbol_text(&mut first, 2);
    assert!(alpha_text.contains("501"), "{alpha_text}");
    let shared_text = symbol_text(&mut second, 2);
    assert!(shared_text.contains("501"), "{shared_text}");
    let beta_text = symbol_text(&mut third, 2);
    assert!(beta_text.contains("502"), "{beta_text}");
    let status = broker_status(&codex_home);
    assert_eq!(status["backend"]["workers"].as_array().unwrap().len(), 2);
    let alpha_worker = worker_pid(&status, &alpha_project).expect("alpha worker");
    let beta_worker = worker_pid(&status, &beta_project).expect("beta worker");
    assert_ne!(alpha_worker, beta_worker);
    assert!(status["backend"]["counters"]["hits"].as_u64().unwrap() >= 1);
    assert_eq!(status["backend"]["counts"]["limit"], 3);
    assert_eq!(status["backend"]["memory"]["observation"], "unavailable");
    assert_eq!(
        status["backend"]["memory"]["configured_limit_bytes"],
        json!(4096u64 * 1024 * 1024)
    );
    assert_eq!(
        status["backend"]["interval"]["identity"]
            .as_str()
            .unwrap()
            .len(),
        64,
        "{status}"
    );
    assert!(
        status["backend"]["durations"]["request"]["count"]
            .as_u64()
            .unwrap()
            >= 3
    );

    // One client's ordered activation resolves the canonical root of another
    // project and moves only that client; shared clients keep their results.
    let activated = third.request(
        3,
        "tools/call",
        json!({
            "name": "activate_project",
            "arguments": {"project": alpha_project.to_string_lossy()}
        }),
    );
    assert!(activated.get("error").is_none(), "{activated}");
    assert_ne!(activated["result"]["isError"], json!(true), "{activated}");
    let moved = symbol_text(&mut third, 4);
    assert!(moved.contains("501"), "{moved}");
    let held = symbol_text(&mut first, 3);
    assert!(held.contains("501"), "{held}");
    let status = broker_status(&codex_home);
    assert_eq!(status["backend"]["workers"].as_array().unwrap().len(), 2);
    assert_eq!(worker_pid(&status, &alpha_project), Some(alpha_worker));
    assert_eq!(worker_pid(&status, &beta_project), Some(beta_worker));

    // A changed project configuration identity replaces that root's worker
    // without moving the other root's client.
    fs::write(
        alpha_project.join(".serena/project.yml"),
        "project_name: 'broker alpha'\nlanguage_servers:\n- rust\nencoding: utf-8\n# configuration revision 2\n",
    )
    .unwrap();
    let replaced = symbol_text(&mut first, 4);
    assert!(replaced.contains("501"), "{replaced}");
    let status = broker_status(&codex_home);
    let replacement = worker_pid(&status, &alpha_project).expect("replacement worker");
    assert_ne!(replacement, alpha_worker, "{status}");
    assert_eq!(worker_pid(&status, &beta_project), Some(beta_worker));
    assert!(
        status["backend"]["counters"]["cold_starts"]
            .as_u64()
            .unwrap()
            >= 3
    );
    let followed = symbol_text(&mut second, 5);
    assert!(followed.contains("501"), "{followed}");
    let final_status = broker_status(&codex_home);
    assert_eq!(
        worker_pid(&final_status, &alpha_project),
        Some(replacement),
        "{final_status}"
    );

    first.finish();
    second.finish();
    third.finish();
    let anchor = codex_home.join("harness/runtime/serena-broker.json");
    let record: Value = serde_json::from_slice(&fs::read(&anchor).unwrap()).unwrap();
    let broker = BrokerRoot::open(Path::new(record["root"].as_str().unwrap())).unwrap();
    let retirement = broker_launch::retire(
        &broker,
        Deadline::after(Duration::from_secs(60)).unwrap(),
        &Cancellation::default(),
    )
    .unwrap();
    assert!(!matches!(
        retirement,
        broker_launch::Retirement::Pending { .. }
    ));
}
