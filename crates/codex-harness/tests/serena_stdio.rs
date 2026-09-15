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
    sync::mpsc,
    thread,
    time::Duration,
};

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn entry() -> PathBuf {
    repo().join("tools/code-tools/serena_entry.py")
}

fn adopted_python() -> PathBuf {
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
            .unwrap()["paths"]["python"]
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

struct Proxy {
    child: Child,
    responses: mpsc::Receiver<String>,
}

impl Proxy {
    fn start(
        _root: &Path,
        codex_home: &Path,
        serena_home: &Path,
        registry: &Path,
        project: &Path,
        python: &Path,
    ) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
            .arg("mcp")
            .arg("serena")
            .arg("--python")
            .arg(python)
            .arg("--entry")
            .arg(entry())
            .arg("--registry")
            .arg(registry)
            .arg("--codex-home")
            .arg(codex_home)
            .arg("--serena-home")
            .arg(serena_home)
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
        }
    }

    fn request(&mut self, id: u64, method: &str, params: Value) -> Value {
        {
            let stdin = self.child.stdin.as_mut().expect("proxy stdin");
            writeln!(
                stdin,
                "{}",
                json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
            )
            .unwrap();
            stdin.flush().unwrap();
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
                    panic!("proxy closed before answering request {id}");
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
        assert!(status.success(), "proxy exit: {status:?}");
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

#[test]
#[ignore = "requires explicit HARNESS_CODE_TOOLS_REGISTRY for the adopted Serena package"]
fn mcp_serena_proxy_shares_one_worker_and_filters_the_catalogue() {
    let python = adopted_python();
    let registry = PathBuf::from(
        std::env::var_os("HARNESS_CODE_TOOLS_REGISTRY").expect("explicit adopted registry"),
    );
    let root = tempfile::tempdir().unwrap();
    eprintln!("Serena proxy end-to-end root: {}", root.path().display());
    let codex_home = root.path().join("codex-home");
    let serena_home = root.path().join("serena-home");
    fs::create_dir_all(&serena_home).unwrap();
    fs::write(serena_home.join("serena_config.yml"), "projects: []\n").unwrap();
    let alpha = crate_project(root.path(), "proxy alpha", 401);
    let beta = crate_project(root.path(), "proxy beta", 402);

    let mut first = Proxy::start(
        root.path(),
        &codex_home,
        &serena_home,
        &registry,
        &alpha,
        &python,
    );
    let initialize = json!({
        "protocolVersion": "2024-11-05",
        "capabilities": {},
        "clientInfo": {"name": "proxy-e2e", "version": "0.1.0"}
    });
    let reply = first.request(1, "initialize", initialize.clone());
    assert_eq!(reply["result"]["serverInfo"]["name"], "Serena");

    let catalogue = first.request(2, "tools/list", json!({}));
    let names: Vec<&str> = catalogue["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect();
    assert!(names.contains(&"find_symbol"));
    assert!(names.contains(&"get_symbols_overview"));
    assert!(!names.contains(&"onboarding"));
    assert!(!names.contains(&"list_memories"));
    assert!(!names.contains(&"read_memory"));

    let search = first.request(
        3,
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
    let mut second = Proxy::start(
        root.path(),
        &codex_home,
        &serena_home,
        &registry,
        &alpha,
        &python,
    );
    let reply = second.request(1, "initialize", initialize.clone());
    assert_eq!(reply["result"]["serverInfo"]["name"], "Serena");
    let status = broker_status(&codex_home);
    assert_eq!(status["backend"]["workers"].as_array().unwrap().len(), 1);
    assert_eq!(status["backend"]["clients"], 2, "{status}");

    // A different project gets its own isolated worker.
    let mut third = Proxy::start(
        root.path(),
        &codex_home,
        &serena_home,
        &registry,
        &beta,
        &python,
    );
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

    // Client EOF closes each proxy cleanly; the broker keeps serving.
    first.finish();
    second.finish();
    third.finish();
    let status = broker_status(&codex_home);
    assert_eq!(status["backend"]["workers"].as_array().unwrap().len(), 2);

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
