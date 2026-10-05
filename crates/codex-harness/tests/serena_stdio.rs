//! End-to-end Serena proxy acceptance through the actual broker service.
//! Requires HARNESS_CODE_TOOLS_REGISTRY for the adopted foreign package.
#![cfg(windows)]

use harness_core::{
    broker_endpoint, broker_launch,
    broker_state::BrokerRoot,
    process::{Cancellation, Deadline},
};
use serde_json::{Value, json};

#[test]
#[ignore = "requires explicit HARNESS_CODE_TOOLS_REGISTRY for the adopted Serena package"]
fn native_semantic_acceptance_checks_real_backends_and_edit_readback() {
    let registry = std::env::var_os("HARNESS_CODE_TOOLS_REGISTRY").expect("explicit registry");
    let output = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .args(["mcp", "serena-check", "--source"])
        .arg(repo())
        .arg("--registry")
        .arg(&registry)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["status"], "semantic-ready");
    // Pascal is exercised only where discovery adopted the shared pasls row.
    let inventory: Value =
        serde_json::from_slice(&fs::read(registry.as_os_str()).unwrap()).unwrap();
    let pascal = inventory["languages"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|row| row["id"] == "delphi" && row["status"] == "adopted");
    let expected = if pascal {
        json!(["rust", "python", "pascal"])
    } else {
        json!(["rust", "python"])
    };
    assert_eq!(report["languages"], expected);
    assert_eq!(report["owned_state_removed"], true);
    assert_eq!(
        report["manager_sha256"],
        harness_core::build_identity::hash_file(Path::new(env!("CARGO_BIN_EXE_codex-harness")))
            .unwrap()
    );
}

#[test]
fn native_semantic_acceptance_fails_on_missing_dependencies() {
    let root = tempfile::tempdir().unwrap();
    let registry = root.path().join("registry.json");
    fs::write(&registry, r#"{"mcp":[],"languages":[]}"#).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .args(["mcp", "serena-check", "--source"])
        .arg(repo())
        .arg("--registry")
        .arg(registry)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("adopted console entrypoint"));
    assert!(!String::from_utf8_lossy(&output.stdout).contains("semantic-ready"));
}

// Test-owned relay: forward to the real broker, then reset a completed response.
struct ResetRelay {
    receipt: PathBuf,
    original: Vec<u8>,
    stop: Arc<std::sync::atomic::AtomicBool>,
    faults: Arc<Mutex<(String, usize, usize)>>,
    worker: Option<thread::JoinHandle<()>>,
}

impl ResetRelay {
    fn start(codex_home: &Path) -> Self {
        use std::{
            net::{Ipv4Addr, TcpListener},
            sync::atomic::{AtomicBool, Ordering},
        };
        let record: Value = serde_json::from_slice(
            &fs::read(codex_home.join("harness/runtime/serena-broker.json")).unwrap(),
        )
        .unwrap();
        let receipt = PathBuf::from(record["root"].as_str().unwrap()).join("endpoint.json");
        let original = fs::read(&receipt).unwrap();
        let mut endpoint: Value = serde_json::from_slice(&original).unwrap();
        let port = endpoint["port"].as_u64().unwrap() as u16;
        let token = endpoint["token"].as_str().unwrap().to_owned();
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        listener.set_nonblocking(true).unwrap();
        endpoint["port"] = json!(listener.local_addr().unwrap().port());
        let stop = Arc::new(AtomicBool::new(false));
        let faults = Arc::new(Mutex::new((String::new(), 0, 0)));
        let thread_stop = stop.clone();
        let thread_faults = faults.clone();
        let worker = thread::spawn(move || {
            while !thread_stop.load(Ordering::Acquire) {
                let (mut stream, _) = match listener.accept() {
                    Ok(peer) => peer,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                        continue;
                    }
                    Err(error) => panic!("relay accept: {error}"),
                };
                stream.set_nonblocking(false).unwrap();
                let deadline = Deadline::after(Duration::from_secs(240)).unwrap();
                let cancel = Cancellation::default();
                let request =
                    harness_core::broker_http::read_request(&mut stream, &token, deadline, &cancel)
                        .unwrap();
                let operation = request["operation"].as_str().unwrap();
                let response = harness_core::broker_http::exchange(
                    port,
                    &token,
                    operation,
                    &request["payload"],
                    deadline,
                    &cancel,
                );
                let reset = {
                    let mut faults = thread_faults.lock().unwrap();
                    if operation == "request/invoke"
                        && request["payload"]["payload"]["params"]["name"] == faults.0
                    {
                        faults.2 += 1;
                        if faults.1 > 0 {
                            faults.1 -= 1;
                            true
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                };
                if reset {
                    assert!(response.is_ok(), "relay backend: {response:?}");
                    use std::os::windows::io::AsRawSocket;
                    use windows_sys::Win32::Networking::WinSock::{
                        LINGER, SO_LINGER, SOL_SOCKET, setsockopt,
                    };
                    let linger = LINGER {
                        l_onoff: 1,
                        l_linger: 0,
                    };
                    assert_eq!(
                        unsafe {
                            setsockopt(
                                stream.as_raw_socket() as _,
                                SOL_SOCKET,
                                SO_LINGER,
                                (&linger as *const LINGER).cast(),
                                std::mem::size_of::<LINGER>() as i32,
                            )
                        },
                        0
                    );
                    drop(stream);
                } else {
                    let body = match response {
                        Ok(value) => json!({"result": value}),
                        Err(error) => json!({"error": error.to_string()}),
                    };
                    harness_core::broker_http::write_response(
                        &mut stream,
                        200,
                        &body,
                        deadline,
                        &cancel,
                    )
                    .unwrap();
                }
            }
        });
        fs::write(&receipt, serde_json::to_vec(&endpoint).unwrap()).unwrap();
        Self {
            receipt,
            original,
            stop,
            faults,
            worker: Some(worker),
        }
    }
    fn arm(&self, tool: &str, failures: usize) {
        *self.faults.lock().unwrap() = (tool.into(), failures, 0);
    }
    fn calls(&self) -> usize {
        self.faults.lock().unwrap().2
    }
}

impl Drop for ResetRelay {
    fn drop(&mut self) {
        fs::write(&self.receipt, &self.original).unwrap();
        self.stop.store(true, std::sync::atomic::Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let outcome = worker.join();
            if !thread::panicking() {
                outcome.unwrap();
            }
        }
    }
}

struct OwnedBroker(PathBuf);
impl OwnedBroker {
    fn crash(&self) {
        let record: Value = serde_json::from_slice(
            &fs::read(self.0.join("harness/runtime/serena-broker.json")).unwrap(),
        )
        .unwrap();
        let root = BrokerRoot::open(Path::new(record["root"].as_str().unwrap())).unwrap();
        let broker_endpoint::Observation::Ready { owner, .. } =
            broker_endpoint::observe(&root).unwrap()
        else {
            panic!("owned broker absent")
        };
        assert!(owner.terminate(137).unwrap());
        let until = Instant::now() + Duration::from_secs(10);
        while owner.is_running().unwrap() && Instant::now() < until {
            thread::sleep(Duration::from_millis(20));
        }
        assert!(!owner.is_running().unwrap());
    }
}
impl Drop for OwnedBroker {
    fn drop(&mut self) {
        let path = self.0.join("harness/runtime/serena-broker.json");
        let result = (|| -> std::io::Result<()> {
            if !path.exists() {
                return Ok(());
            }
            let record: Value = serde_json::from_slice(&fs::read(path)?)?;
            let root = BrokerRoot::open(Path::new(record["root"].as_str().unwrap()))?;
            let outcome = broker_launch::retire(
                &root,
                Deadline::after(Duration::from_secs(60))?,
                &Cancellation::default(),
            )?;
            if matches!(outcome, broker_launch::Retirement::Pending { .. }) {
                return Err(std::io::Error::other("owned test broker cleanup pending"));
            }
            Ok(())
        })();
        if !thread::panicking() {
            result.unwrap();
        } else if let Err(error) = result {
            eprintln!("owned broker cleanup: {error}");
        }
    }
}

impl Drop for Proxy {
    fn drop(&mut self) {
        drop(self.child.stdin.take());
        let until = Instant::now() + Duration::from_secs(10);
        while self.child.try_wait().ok().flatten().is_none() && Instant::now() < until {
            thread::sleep(Duration::from_millis(20));
        }
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

#[test]
#[ignore = "requires explicit HARNESS_CODE_TOOLS_REGISTRY for the adopted Serena package"]
fn real_serena_recovers_reset_without_replaying_edits() {
    let console = adopted_console();
    let registry = PathBuf::from(std::env::var_os("HARNESS_CODE_TOOLS_REGISTRY").unwrap());
    let root = tempfile::tempdir().unwrap();
    eprintln!("Serena reset acceptance root: {}", root.path().display());
    let codex_home = root.path().join("codex-home");
    let cleanup = OwnedBroker(codex_home.clone());
    let project = crate_project(root.path(), "reset alpha", 601);
    let mut proxy = Proxy::start(root.path(), &codex_home, &registry, &project, &console);
    let reply = proxy.request(
        1,
        "initialize",
        json!({
            "protocolVersion": "2024-11-05", "capabilities": {},
            "clientInfo": {"name": "reset-acceptance", "version": "0.1.0"}
        }),
    );
    assert_eq!(reply["result"]["serverInfo"]["name"], "Serena", "{reply}");
    assert!(symbol_text(&mut proxy, 2).contains("601"));
    let anchor: Value = serde_json::from_slice(
        &fs::read(codex_home.join("harness/runtime/serena-broker.json")).unwrap(),
    )
    .unwrap();
    let log = fs::read_to_string(Path::new(anchor["root"].as_str().unwrap()).join("service.log"))
        .unwrap();
    let memory: Value = serde_json::from_str(
        log.lines()
            .find_map(|line| line.strip_prefix("serena-broker: memory "))
            .expect("actual service Job memory readback"),
    )
    .unwrap();
    assert!(
        memory["broker_limit_bytes"].as_u64().unwrap()
            > memory["worker_limit_bytes"].as_u64().unwrap()
                * memory["worker_capacity"].as_u64().unwrap()
    );
    let relay = ResetRelay::start(&codex_home);
    let query = json!({"name": "get_symbols_overview", "arguments": {
        "relative_path": "src/lib.rs", "depth": 0, "max_answer_chars": 3800
    }});
    relay.arm("get_symbols_overview", 1);
    let reply = proxy.request(3, "tools/call", query.clone());
    assert_eq!(reply["id"], 3);
    assert!(
        reply.get("error").is_none(),
        "read must recover the reset: {reply}"
    );
    assert!(reply["result"].to_string().contains("shared"), "{reply}");
    assert_eq!(relay.calls(), 2, "one bounded replay");
    relay.arm("get_symbols_overview", 3);
    let reply = proxy.request(4, "tools/call", query.clone());
    assert_eq!(reply["error"]["code"], -32603, "{reply}");
    assert_eq!(relay.calls(), 2, "persistent failure stops after one retry");
    assert!(
        reply["error"]["message"]
            .as_str()
            .unwrap()
            .contains("recovery"),
        "{reply}"
    );
    relay.arm("insert_after_symbol", 1);
    let reply = proxy.request(
        5,
        "tools/call",
        json!({
            "name": "insert_after_symbol", "arguments": {
                "relative_path": "src/lib.rs", "name_path": "shared",
                "body": "\npub fn inserted_once() -> i32 { 602 }\n"
            }
        }),
    );
    assert_eq!(reply["error"]["code"], -32603, "{reply}");
    assert_eq!(relay.calls(), 1, "uncertain edit must never replay");
    let source = fs::read_to_string(project.join("src/lib.rs")).unwrap();
    assert_eq!(source.matches("fn inserted_once").count(), 1, "{source}");
    let reply = proxy.request(6, "tools/call", query);
    assert!(reply.get("error").is_none(), "{reply}");
    assert!(
        reply["result"].to_string().contains("inserted_once"),
        "{reply}"
    );
    let beta = crate_project(root.path(), "reset beta", 701);
    let activation = json!({"name": "activate_project", "arguments": {"project": beta}});
    relay.arm("activate_project", 1);
    let reply = proxy.request(60, "tools/call", activation.clone());
    assert_eq!(reply["id"], 60);
    assert!(
        reply.get("error").is_none(),
        "activation must recover: {reply}"
    );
    assert_ne!(reply["result"]["isError"], true, "{reply}");
    assert_eq!(
        relay.calls(),
        2,
        "activation retries once after its response is lost"
    );
    assert!(symbol_text(&mut proxy, 61).contains("701"));
    relay.arm("activate_project", 3);
    let reply = proxy.request(62, "tools/call", activation);
    assert_eq!(reply["error"]["code"], -32603, "{reply}");
    assert_eq!(relay.calls(), 2, "persistent activation failure is bounded");
    relay.arm("", 0);
    assert!(symbol_text(&mut proxy, 63).contains("701"));
    let reply = proxy.request(
        64,
        "tools/call",
        json!({
            "name":"activate_project", "arguments":{"project":project}
        }),
    );
    assert!(reply.get("error").is_none(), "{reply}");
    assert!(symbol_text(&mut proxy, 65).contains("601"));
    drop(relay);
    let status = broker_status(&codex_home);
    let pid = worker_pid(&status, &project).unwrap();
    let worker = status["backend"]["workers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|worker| worker["pid"] == pid)
        .unwrap();
    let identity = harness_core::process::ProcessIdentity {
        pid: worker["pid"].as_u64().unwrap() as u32,
        creation_time: worker["creation_time"].as_u64().unwrap(),
    };
    let owner = harness_core::process_service::ServiceProcess::inspect(
        identity,
        &console,
        &harness_core::process_service::current_user().unwrap(),
    )
    .unwrap()
    .expect("owned test worker identity");
    assert!(owner.terminate(137).unwrap());
    let until = Instant::now() + Duration::from_secs(10);
    while owner.is_running().unwrap() && Instant::now() < until {
        thread::sleep(Duration::from_millis(20));
    }
    assert!(!owner.is_running().unwrap());
    assert!(symbol_text(&mut proxy, 7).contains("601"));
    assert_ne!(
        worker_pid(&broker_status(&codex_home), &project),
        Some(u64::from(identity.pid))
    );
    cleanup.crash();
    // Existing stdio conversation restores its cached route after broker exit.
    assert!(symbol_text(&mut proxy, 8).contains("601"));
    proxy.finish();
}

use std::{
    fs,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex, mpsc},
    thread,
    time::Duration,
};

#[test]
fn closed_worker_retains_native_exit_and_memory_evidence() {
    let root = tempfile::tempdir().unwrap();
    let mut command =
        harness_core::process::CommandSpec::new(env!("CARGO_BIN_EXE_harness-process-fixture"));
    command.args = vec![
        "exit-code".into(),
        root.path().join("exit.json").into(),
        "42".into(),
    ];
    let cancel = Cancellation::default();
    let mut session = harness_core::serena::Session::start_shared(
        command,
        root.path().join("stderr.txt"),
        &cancel,
    )
    .unwrap();
    let until = Instant::now() + Duration::from_secs(10);
    while session.is_alive() && Instant::now() < until {
        thread::sleep(Duration::from_millis(10));
    }
    assert!(!session.is_alive(), "owned exit fixture did not stop");
    let error = session
        .request(
            "ping",
            json!({}),
            Deadline::after(Duration::from_secs(5)).unwrap(),
        )
        .unwrap_err();
    assert!(
        matches!(
            error.kind(),
            std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::UnexpectedEof
        ),
        "{error:?}"
    );
    let context = session.failure_context();
    assert!(context.contains("exit_code=0x0000002a"), "{context}");
    assert!(context.contains("worker_job_peak_bytes="), "{context}");
    assert!(!context.contains("unavailable"), "{context}");
    let outcome = session.close().unwrap();
    assert_eq!(outcome.process_exit_code, 42);
    assert_eq!(outcome.job.active_processes, 0);
}

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
        "get_diagnostics_for_symbol",
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

    // Symbol-scoped diagnostics follow the explicit route: a known Rust
    // error is reported for the changed symbol and clears after the fix.
    fs::write(
        alpha.join("src/lib.rs"),
        "pub fn shared() -> i32 { 401 }\npub fn broken() -> i32 { \"not an i32\" }\n",
    )
    .unwrap();
    let broken = first.request(
        5,
        "tools/call",
        json!({
            "name": "get_diagnostics_for_symbol",
            "arguments": {
                "name_path": "broken",
                "reference_file": "src/lib.rs"
            }
        }),
    );
    let text: String = broken["result"]["content"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|item| item["text"].as_str())
        .collect();
    assert!(text.contains("broken") && text.contains("E0308"), "{text}");
    fs::write(
        alpha.join("src/lib.rs"),
        "pub fn shared() -> i32 { 401 }\npub fn broken() -> i32 { 7 }\n",
    )
    .unwrap();
    // Diagnostics publish asynchronously after an external file change, so
    // the explicit route retries briefly instead of trusting a stale
    // snapshot.
    let mut text = String::new();
    for attempt in 0..20 {
        let cleared = first.request(
            6 + attempt,
            "tools/call",
            json!({
                "name": "get_diagnostics_for_symbol",
                "arguments": {
                    "name_path": "broken",
                    "reference_file": "src/lib.rs"
                }
            }),
        );
        text = cleared["result"]["content"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|item| item["text"].as_str())
            .collect();
        if !text.contains("broken") || text == "{}" {
            break;
        }
        thread::sleep(Duration::from_millis(500));
    }
    assert!(
        !text.contains("broken") || text == "{}",
        "the known Rust diagnostic did not clear: {text}"
    );
    // The excluded text-search tool stays refused by the worker itself.
    let refused_search = first.request(
        30,
        "tools/call",
        json!({"name": "search_for_pattern", "arguments": {"pattern": "shared"}}),
    );
    assert_eq!(
        refused_search["result"]["isError"], true,
        "{refused_search}"
    );

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
    // The same known diagnostic stays bounded when it is requested for the
    // symbol, and it clears through the managed symbol route after a fix.
    let symbol = fourth.request(
        4,
        "tools/call",
        json!({
            "name": "get_diagnostics_for_symbol",
            "arguments": {
                "name_path": "broken",
                "reference_file": "src/mod.py"
            }
        }),
    );
    let text: String = symbol["result"]["content"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|item| item["text"].as_str())
        .collect();
    assert!(text.contains("reportReturnType"), "{text}");
    fs::write(
        python.join("src/mod.py"),
        "def shared() -> int:\n    return 411\n\n\ndef broken() -> int:\n    return 12\n",
    )
    .unwrap();
    // The Python backend publishes asynchronously too; the bounded retry
    // keeps the clearance claim tied to a fresh answer.
    let mut text = String::new();
    for attempt in 0..20 {
        let cleared = fourth.request(
            5 + attempt,
            "tools/call",
            json!({
                "name": "get_diagnostics_for_symbol",
                "arguments": {
                    "name_path": "broken",
                    "reference_file": "src/mod.py"
                }
            }),
        );
        text = cleared["result"]["content"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|item| item["text"].as_str())
            .collect();
        if !text.contains("reportReturnType") {
            break;
        }
        thread::sleep(Duration::from_millis(500));
    }
    assert!(
        !text.contains("reportReturnType"),
        "the known Python diagnostic did not clear: {text}"
    );
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
