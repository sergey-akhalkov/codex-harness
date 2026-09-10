#![cfg(windows)]
use harness_core::{
    broker_endpoint::{self, Observation},
    broker_launch,
    broker_state::BrokerRoot,
    cancellable_pipe::{CancellablePipe, anonymous_pipe},
    mcp_protocol::{Decoder, READ_CHUNK},
    process::{Cancellation, CommandSpec, Deadline, Job, Limits, OwnedProcess, StopReason},
};
use serde_json::{Value, json};
use std::{
    fs::{self, File},
    path::{Path, PathBuf},
    time::Duration,
};

struct Prepared {
    prepared: Option<harness_core::broker_state::PreparedRoot>,
}

impl Prepared {
    fn new() -> Self {
        Self {
            prepared: Some(BrokerRoot::prepare().unwrap()),
        }
    }

    fn root(&self) -> &BrokerRoot {
        self.prepared.as_ref().unwrap().root()
    }
}

impl Drop for Prepared {
    fn drop(&mut self) {
        let prepared = self.prepared.take().unwrap();
        let cancel = Cancellation::default();
        if let Ok(deadline) = Deadline::after(Duration::from_secs(8)) {
            let _ = broker_launch::retire(prepared.root(), deadline, &cancel);
        }
        if std::thread::panicking() {
            let root = prepared.keep();
            eprintln!(
                "retained owned broker failure root: {}",
                root.path().display()
            );
        }
    }
}

struct Client {
    job: Option<Job>,
    child: OwnedProcess,
    input: Option<CancellablePipe>,
    output: CancellablePipe,
    decoder: Decoder,
    root: tempfile::TempDir,
    cancel: Cancellation,
}

impl Client {
    fn start(
        executable: &Path,
        owned: &Path,
        catalogue: &Path,
        broker_root: &Path,
        seconds: u64,
    ) -> Self {
        let root = tempfile::tempdir().unwrap();
        let (input, writer) = anonymous_pipe(4096).unwrap();
        let (reader, output) = anonymous_pipe(4096).unwrap();
        let mut command = CommandSpec::new(env!("CARGO_BIN_EXE_codex-harness"));
        command.args = vec![
            "mcp".into(),
            "codebase-memory".into(),
            "--executable".into(),
            executable.into(),
            "--cache".into(),
            owned.join("cache").into_os_string(),
            "--runtime".into(),
            owned.join("runtime").into_os_string(),
            "--account".into(),
            owned.join("account").into_os_string(),
            "--catalogue-file".into(),
            catalogue.into(),
            "--connection-seconds".into(),
            seconds.to_string().into(),
            "--broker-root".into(),
            broker_root.into(),
        ];
        command.current_dir = Some(root.path().into());
        command.stdin = Some(input);
        command.stdout = Some(output);
        command.stderr = Some(File::create(root.path().join("stderr")).unwrap());
        let job = Job::new(Limits {
            memory_bytes: Some(512 * 1024 * 1024),
            cpu_percent: Some(25.0),
        })
        .unwrap();
        let child = job.spawn(&command).unwrap();
        drop(command);
        let cancel = Cancellation::default();
        Self {
            job: Some(job),
            child,
            input: Some(CancellablePipe::writer(writer, cancel.clone()).unwrap()),
            output: CancellablePipe::reader(reader, cancel.clone()).unwrap(),
            decoder: Decoder::default(),
            root,
            cancel,
        }
    }

    fn raw(&mut self, bytes: &[u8]) {
        for chunk in bytes.chunks(READ_CHUNK) {
            self.input
                .as_mut()
                .unwrap()
                .write_all(
                    chunk,
                    Deadline::after(Duration::from_secs(3)).unwrap(),
                    &self.cancel,
                )
                .unwrap();
        }
    }

    fn send(&mut self, value: Value) {
        let mut bytes = serde_json::to_vec(&value).unwrap();
        bytes.push(b'\n');
        self.raw(&bytes);
    }

    fn reply(&mut self) -> Value {
        self.reply_with_timeout(Duration::from_secs(8))
    }

    fn reply_with_timeout(&mut self, timeout: Duration) -> Value {
        let deadline = Deadline::after(timeout).unwrap();
        loop {
            if let Some(message) = self.decoder.next_message().unwrap() {
                return message.into_value();
            }
            let bytes = self
                .output
                .read(READ_CHUNK, deadline, &self.cancel)
                .unwrap_or_else(|error| {
                    panic!(
                        "client pipe: {error}; exit={:?}; stderr={}",
                        self.child.exit_code(),
                        fs::read_to_string(self.root.path().join("stderr")).unwrap_or_default()
                    )
                });
            assert!(
                !bytes.is_empty(),
                "unexpected EOF; exit={:?}; stderr={}",
                self.child.exit_code(),
                fs::read_to_string(self.root.path().join("stderr")).unwrap_or_default()
            );
            self.decoder.push(&bytes).unwrap();
        }
    }

    fn initialize(&mut self) {
        self.send(json!({"jsonrpc":"2.0","id":"init","method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"owned-client","version":"1"}}}));
        let reply = self.reply();
        assert_eq!(reply["id"], "init");
        assert_eq!(
            reply["result"]["serverInfo"]["name"],
            "codex-harness-codebase-memory"
        );
        self.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
    }

    fn list_tools(&mut self) {
        self.send(json!({"jsonrpc":"2.0","id":"list","method":"tools/list"}));
        let listed = self.reply();
        assert_eq!(listed["id"], "list");
        assert_eq!(listed["result"]["tools"].as_array().unwrap().len(), 15);
    }

    fn eof(&mut self) {
        self.input
            .take()
            .unwrap()
            .close(Deadline::after(Duration::from_secs(2)).unwrap())
            .unwrap();
    }

    fn finish(mut self, expected: u32) -> String {
        let outcome = self
            .job
            .take()
            .unwrap()
            .wait(
                &self.child,
                Deadline::after(Duration::from_secs(8)).unwrap(),
                &self.cancel,
                Duration::from_secs(3),
            )
            .unwrap();
        assert_eq!(outcome.reason, StopReason::Exited, "{outcome:?}");
        assert_eq!(
            outcome.exit_code,
            expected,
            "{}",
            fs::read_to_string(self.root.path().join("stderr")).unwrap()
        );
        assert_eq!(outcome.job.active_processes, 0);
        fs::read_to_string(self.root.path().join("stderr")).unwrap()
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        if let Some(job) = self.job.take() {
            let _ = job.terminate(130, Duration::from_secs(8));
        }
        if let Some(input) = self.input.take() {
            let _ = input.close(Deadline::after(Duration::from_secs(3)).unwrap());
        }
    }
}

fn endpoint_absent(root: &BrokerRoot) {
    assert!(
        matches!(broker_endpoint::observe(root).unwrap(), Observation::Absent),
        "broker endpoint was not absent"
    );
}

fn query_alpha(client: &mut Client, id: Value) {
    client.send(json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":"query_graph","arguments":{"project":"native-cbm-coexist-owned","query":"MATCH (n:Function) WHERE n.name = 'alpha' RETURN n.name","format":"json"}}}));
    let response = client.reply_with_timeout(Duration::from_secs(90));
    assert_eq!(response["id"], id);
    assert_eq!(response["result"]["isError"], false, "{response}");
    assert_eq!(
        response["result"]["structuredContent"]["rows"],
        json!([["alpha"]])
    );
}

#[test]
#[ignore = "explicit audited CBM, native-owned test root and catalogue paths required"]
fn actual_shared_cbm_clients_reuse_one_broker_and_retire_after_last_eof() {
    // Reuse the explicit native-owned cache prepared by core CBM acceptance.
    // No defaults point to installed/global state; this is an opt-in shared check.
    let executable = PathBuf::from(
        std::env::var_os("HARNESS_CBM_EXECUTABLE").expect("explicit audited CBM required"),
    );
    let owned = PathBuf::from(
        std::env::var_os("HARNESS_CBM_TEST_ROOT").expect("explicit owned test root required"),
    );
    let catalogue = PathBuf::from(
        std::env::var_os("HARNESS_CBM_CATALOGUE").expect("explicit saved catalogue required"),
    );
    let prior: Value =
        serde_json::from_slice(&fs::read(owned.join("report.json")).unwrap()).unwrap();
    assert_eq!(
        prior["peer_clean_shutdown"], true,
        "native owned fixture prerequisite"
    );
    assert_eq!(prior["owned_ui_disabled"], true);
    assert!(executable.is_file(), "audited CBM executable missing");
    assert!(catalogue.is_file(), "saved catalogue missing");
    assert!(
        owned.join("repository").is_dir(),
        "owned repository missing"
    );
    assert!(owned.join("cache").is_dir(), "owned cache missing");
    assert!(owned.join("runtime").is_dir(), "owned runtime missing");
    assert!(owned.join("account").is_dir(), "owned account missing");

    let prepared = Prepared::new();
    endpoint_absent(prepared.root());
    assert!(!prepared.root().path().join("service.log").exists());

    let mut first = Client::start(&executable, &owned, &catalogue, prepared.root().path(), 180);
    let mut second = Client::start(&executable, &owned, &catalogue, prepared.root().path(), 180);
    assert_ne!(first.child.identity().pid, second.child.identity().pid);

    first.initialize();
    first.list_tools();
    second.initialize();
    second.list_tools();
    endpoint_absent(prepared.root());
    assert!(!prepared.root().path().join("service.log").exists());

    first.send(json!({"jsonrpc":"2.0","id":"index-owned","method":"tools/call","params":{"name":"index_repository","arguments":{"repo_path":owned.join("repository"),"name":"native-cbm-coexist-owned","mode":"fast"}}}));
    let indexed = first.reply_with_timeout(Duration::from_secs(90));
    assert_eq!(indexed["id"], "index-owned");
    assert_eq!(indexed["result"]["isError"], false, "{indexed}");

    let Observation::Ready { endpoint, owner } = broker_endpoint::observe(prepared.root()).unwrap()
    else {
        panic!("ready endpoint missing");
    };
    let identity = owner.identity();
    assert_eq!(endpoint.pid, identity.pid);
    assert_eq!(endpoint.creation_time, identity.creation_time);
    assert!(owner.is_running().unwrap());
    assert!(!first.job.as_ref().unwrap().owns(identity).unwrap());
    assert!(!second.job.as_ref().unwrap().owns(identity).unwrap());
    assert!(prepared.root().path().join("service.log").is_file());

    query_alpha(&mut second, json!("query-owned"));

    first.eof();
    assert!(first.finish(0).is_empty());
    assert!(owner.is_running().unwrap());
    let Observation::Ready {
        endpoint: still,
        owner: still_owner,
    } = broker_endpoint::observe(prepared.root()).unwrap()
    else {
        panic!("broker did not stay ready after first client exit");
    };
    assert_eq!(still.pid, identity.pid);
    assert_eq!(still.creation_time, identity.creation_time);
    assert_eq!(still_owner.identity(), identity);

    query_alpha(&mut second, json!("ошибка-日本"));

    second.eof();
    assert!(second.finish(0).is_empty());
    let retirement = broker_launch::retire(
        prepared.root(),
        Deadline::after(Duration::from_secs(8)).unwrap(),
        &Cancellation::default(),
    )
    .unwrap();
    match retirement {
        broker_launch::Retirement::Exited { pid, exit_code } => {
            assert_eq!(pid, identity.pid);
            assert_eq!(exit_code, 0);
        }
        other => panic!("expected owned broker exit 0: {other:?}"),
    }
    assert_eq!(owner.exit_code().unwrap(), Some(0));
    endpoint_absent(prepared.root());
}

#[test]
#[ignore = "explicit audited CBM, owned cache/catalogue and real large project paths required"]
fn actual_shared_cbm_builds_full_large_index_and_queries_maintained_source() {
    let executable = PathBuf::from(
        std::env::var_os("HARNESS_CBM_EXECUTABLE").expect("explicit audited CBM required"),
    );
    let owned = PathBuf::from(
        std::env::var_os("HARNESS_CBM_TEST_ROOT").expect("explicit owned CBM root required"),
    );
    let catalogue = PathBuf::from(
        std::env::var_os("HARNESS_CBM_CATALOGUE").expect("explicit saved catalogue required"),
    );
    let project = PathBuf::from(
        std::env::var_os("HARNESS_CBM_LARGE_PROJECT")
            .expect("explicit real large project required"),
    );
    let relative_source = PathBuf::from(
        std::env::var_os("HARNESS_CBM_LARGE_SOURCE")
            .expect("explicit representative project-relative source required"),
    );
    assert!(
        relative_source
            .components()
            .all(|part| matches!(part, std::path::Component::Normal(_)))
    );
    assert!(project.join(relative_source).is_file());
    let symbol = std::env::var("HARNESS_CBM_LARGE_SYMBOL")
        .expect("explicit representative function name required");
    assert!(
        !symbol.is_empty()
            && symbol
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_')
    );
    let prior: Value =
        serde_json::from_slice(&fs::read(owned.join("report.json")).unwrap()).unwrap();
    assert_eq!(prior["peer_clean_shutdown"], true);
    assert_eq!(prior["owned_ui_disabled"], true);
    let prepared = Prepared::new();
    println!(
        "Owned native large-index broker: {}",
        prepared.root().path().display()
    );
    let mut client = Client::start(
        &executable,
        &owned,
        &catalogue,
        prepared.root().path(),
        1400,
    );
    client.initialize();
    // Both first publication and a subsequent full refresh must work. Source
    // files stay read-only; all graph writes select this explicit owned cache.
    for id in ["full-index", "refresh-index"] {
        client.send(
            json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{
            "name":"index_repository","arguments":{"repo_path":project,
            "name":"native-large-full-owned","mode":"full","persistence":false}}}),
        );
        let indexed = client.reply_with_timeout(Duration::from_secs(615));
        assert_eq!(indexed["id"], id);
        println!("Full-index result {id}: {}", indexed["result"]);
        if indexed["result"]["isError"] == true {
            // A resource failure is still a failed acceptance. Before reporting
            // it, prove the real MCP connection can expose the saved catalogue
            // and query again after the confirmed worker reclamation.
            let Observation::Ready { owner, .. } =
                broker_endpoint::observe(prepared.root()).unwrap()
            else {
                panic!("resource failure stopped the broker: {indexed}");
            };
            client.send(
                json!({"jsonrpc":"2.0","id":"after-index-error","method":"tools/call",
                "params":{"name":"list_projects","arguments":{}}}),
            );
            let following = client.reply_with_timeout(Duration::from_secs(65));
            assert_eq!(following["id"], "after-index-error");
            assert_eq!(following["result"]["isError"], false, "{following}");
            assert!(owner.is_running().unwrap());
            println!("MCP query after index failure succeeded on the same broker");
        }
        assert_eq!(indexed["result"]["isError"], false, "{indexed}");
    }
    client.send(json!({"jsonrpc":"2.0","id":"maintained-source","method":"tools/call","params":{
        "name":"query_graph","arguments":{"project":"native-large-full-owned",
        "query":format!("MATCH (n:Function) WHERE n.name = '{symbol}' RETURN n.name"),"format":"json"}}}));
    let response = client.reply_with_timeout(Duration::from_secs(65));
    assert_eq!(response["id"], "maintained-source");
    assert_eq!(response["result"]["isError"], false, "{response}");
    assert!(
        response["result"]["structuredContent"]["rows"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row == &json!([symbol]))
    );
    client.eof();
    assert!(client.finish(0).is_empty());
    let retirement = broker_launch::retire(
        prepared.root(),
        Deadline::after(Duration::from_secs(8)).unwrap(),
        &Cancellation::default(),
    )
    .unwrap();
    assert!(
        matches!(
            retirement,
            broker_launch::Retirement::Exited { exit_code: 0, .. }
        ),
        "{retirement:?}"
    );
    endpoint_absent(prepared.root());
}
