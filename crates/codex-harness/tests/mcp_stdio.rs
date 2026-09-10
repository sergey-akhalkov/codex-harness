#![cfg(windows)]
use harness_core::{
    cancellable_pipe::{CancellablePipe, anonymous_pipe},
    mcp_protocol::{Decoder, Message, READ_CHUNK},
    process::{Cancellation, CommandSpec, Deadline, Job, Limits, OwnedProcess, StopReason},
};
use serde_json::{Value, json};
use std::{
    fs::{self, File},
    time::{Duration, Instant},
};

struct Server {
    job: Option<Job>,
    child: OwnedProcess,
    input: Option<CancellablePipe>,
    output: CancellablePipe,
    decoder: Decoder,
    root: tempfile::TempDir,
    cancel: Cancellation,
}

impl Server {
    fn start() -> Self {
        Self::start_with(None)
    }
    fn start_with(cbm: Option<(&std::path::Path, &std::path::Path, &std::path::Path)>) -> Self {
        let root = tempfile::tempdir().unwrap();
        let (input, writer) = anonymous_pipe(4096).unwrap();
        let (reader, output) = anonymous_pipe(4096).unwrap();
        let mut command = CommandSpec::new(env!("CARGO_BIN_EXE_harness-mcp-probe-fixture"));
        command.args.push("--stdio-session".into());
        if let Some((executable, owned, catalogue)) = cbm {
            command = CommandSpec::new(env!("CARGO_BIN_EXE_codex-harness"));
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
                "120".into(),
            ];
        }
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
                .unwrap();
            assert!(!bytes.is_empty(), "unexpected EOF");
            self.decoder.push(&bytes).unwrap();
        }
    }
    fn initialize(&mut self) {
        self.initialize_named("harness-stdio-fixture");
    }
    fn initialize_named(&mut self, name: &str) {
        self.send(json!({"jsonrpc":"2.0","id":"init","method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"owned-client","version":"1"}}}));
        let reply = self.reply();
        assert_eq!(reply["id"], "init");
        assert_eq!(reply["result"]["serverInfo"]["name"], name);
        self.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
    }
    fn call(&mut self, id: Value, name: &str) {
        self.send(json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":name,"arguments":{"message":"кириллица 日本"}}}));
    }
    fn eof(&mut self) {
        self.input
            .take()
            .unwrap()
            .close(Deadline::after(Duration::from_secs(2)).unwrap())
            .unwrap();
    }
    fn finish(mut self, expected: u32, timeout: Duration) -> String {
        let outcome = self
            .job
            .take()
            .unwrap()
            .wait(
                &self.child,
                Deadline::after(timeout).unwrap(),
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

#[test]
fn actual_stdio_preserves_unicode_exact_ids_and_output_purity() {
    let mut server = Server::start();
    server.initialize();
    for id in [json!(1), json!("1"), json!(u64::MAX)] {
        let frame = json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":"echo","arguments":{"message":"кириллица 日本"}}});
        // Exercise byte-level fragmentation, including UTF-8 code units.
        let bytes = Message::parse(&serde_json::to_vec(&frame).unwrap())
            .unwrap()
            .encode()
            .unwrap();
        for byte in bytes {
            server.raw(&[byte]);
        }
        let reply = server.reply();
        assert_eq!(reply["id"], id);
        assert_eq!(
            reply["result"]["structuredContent"]["message"],
            "кириллица 日本"
        );
    }
    server.send(json!({"jsonrpc":"2.0","id":"list","method":"tools/list"}));
    assert_eq!(
        server.reply()["result"]["tools"].as_array().unwrap().len(),
        5
    );
    server.eof();
    assert!(server.finish(0, Duration::from_secs(5)).is_empty());
}

#[test]
fn cancellation_reclaims_active_handler_before_queued_work_starts() {
    let mut server = Server::start();
    server.initialize();
    server.call(json!("waiting"), "wait");
    server.call(json!(7), "echo");
    server.send(json!({"jsonrpc":"2.0","id":"ping","method":"ping"}));
    assert_eq!(server.reply()["id"], "ping");
    let start = Instant::now();
    server.send(json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":"waiting"}}));
    let reply = server.reply();
    assert_eq!(reply["id"], 7, "cancelled request must not send a response");
    assert!(start.elapsed() >= Duration::from_millis(70));
    server.eof();
    assert!(server.finish(0, Duration::from_secs(5)).is_empty());
}

#[test]
fn eof_cancels_an_active_tool_and_finishes_cleanup() {
    let mut server = Server::start();
    server.initialize();
    server.call(json!("waiting"), "wait");
    server.send(json!({"jsonrpc":"2.0","id":99,"method":"ping"}));
    assert_eq!(server.reply()["id"], 99);
    server.eof();
    assert!(server.finish(0, Duration::from_secs(5)).is_empty());
}

#[test]
fn incomplete_input_is_a_nonzero_protocol_failure() {
    let mut server = Server::start();
    server.initialize();
    server.raw(b"{\"jsonrpc\":");
    server.eof();
    let error = server.finish(91, Duration::from_secs(5));
    assert!(
        error.contains("MCP stream ended before its final frame"),
        "{error}"
    );
}

#[test]
fn peer_not_draining_output_cannot_hold_the_server_or_input_worker() {
    let mut server = Server::start();
    server.initialize();
    server.call(json!(42), "flood");
    // Keep both peer endpoints open, consume no output, and require a natural
    // error exit. The outer Job timeout is a failing oracle, not the cleanup.
    let start = Instant::now();
    let error = server.finish(91, Duration::from_secs(12));
    assert!(start.elapsed() < Duration::from_secs(10));
    assert!(error.contains("deadline"), "{error}");
}

#[test]
fn a_cancelled_handler_panic_stops_the_connection_before_queued_work() {
    let mut server = Server::start();
    server.initialize();
    server.call(json!("waiting"), "wait-panic");
    server.send(json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"echo","arguments":{"mark_start":true}}}));
    server.send(json!({"jsonrpc":"2.0","id":99,"method":"ping"}));
    assert_eq!(server.reply()["id"], 99);
    server.send(json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":"waiting"}}));
    assert!(server.child.wait_for_exit(Duration::from_secs(5)).unwrap());
    assert!(
        !server.root.path().join("queued-started").exists(),
        "queue advanced after a worker panic"
    );
    let error = server.finish(91, Duration::from_secs(2));
    assert!(error.contains("cleanup not confirmed"), "{error}");
}

#[test]
fn a_backend_cleanup_error_survives_cancellation_and_eof_without_advancing_queue() {
    for eof in [false, true] {
        let mut server = Server::start();
        server.initialize();
        server.call(json!("waiting"), "wait-error");
        server.send(json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"echo","arguments":{"mark_start":true}}}));
        server.send(json!({"jsonrpc":"2.0","id":99,"method":"ping"}));
        assert_eq!(server.reply()["id"], 99);
        if eof {
            server.eof();
        } else {
            server.send(json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":"waiting"}}));
        }
        assert!(server.child.wait_for_exit(Duration::from_secs(5)).unwrap());
        assert!(!server.root.path().join("queued-started").exists());
        let error = server.finish(91, Duration::from_secs(2));
        assert!(error.contains("owned backend cleanup failure"), "{error}");
    }
}

fn saved_catalogue() -> Value {
    let names = [
        "index_repository",
        "search_graph",
        "query_graph",
        "trace_path",
        "get_code_snippet",
        "get_graph_schema",
        "get_architecture",
        "search_code",
        "list_projects",
        "delete_project",
        "index_status",
        "check_index_coverage",
        "detect_changes",
        "manage_adr",
        "ingest_traces",
    ];
    json!({"state":"catalogue-read","server":"codebase-memory-mcp",
        "protocol_version":"2024-11-05","artifact_sha256":harness_core::cbm_index::AUDITED_BUILD,
        "tool_count":15,"tool_calls_executed":false,
        "tools":names.into_iter().map(|name| json!({"name":name,"inputSchema":{"type":"object","properties":{}}})).collect::<Vec<_>>()})
}

#[test]
fn cbm_stdio_handshake_is_lazy_and_backend_failure_closes_the_connection() {
    let owned = tempfile::tempdir().unwrap();
    let catalogue = owned.path().join("catalogue.json");
    fs::write(&catalogue, serde_json::to_vec(&saved_catalogue()).unwrap()).unwrap();
    let missing = owned.path().join("missing.exe");
    let mut server = Server::start_with(Some((&missing, owned.path(), &catalogue)));
    server.initialize_named("codex-harness-codebase-memory");
    server.send(json!({"jsonrpc":"2.0","id":"list","method":"tools/list"}));
    assert_eq!(
        server.reply()["result"]["tools"].as_array().unwrap().len(),
        15
    );
    assert_eq!(fs::read_dir(owned.path()).unwrap().count(), 1);
    server.send(json!({"jsonrpc":"2.0","id":"bad-backend","method":"tools/call","params":{"name":"list_projects","arguments":{}}}));
    assert!(server.child.wait_for_exit(Duration::from_secs(5)).unwrap());
    assert_eq!(fs::read_dir(owned.path()).unwrap().count(), 1);
    assert!(!server.finish(2, Duration::from_secs(2)).is_empty());
}

#[test]
#[ignore = "explicit audited CBM, native-owned test root and catalogue paths required"]
fn actual_cbm_cli_stdio_indexes_queries_and_recovers_after_cancellation() {
    // Reuse the explicit native-owned cache prepared by core CBM acceptance.
    // No defaults point to installed/global state; this is an opt-in query check.
    let executable = std::path::PathBuf::from(
        std::env::var_os("HARNESS_CBM_EXECUTABLE").expect("explicit audited CBM required"),
    );
    let owned = std::path::PathBuf::from(
        std::env::var_os("HARNESS_CBM_TEST_ROOT").expect("explicit owned test root required"),
    );
    let catalogue = std::path::PathBuf::from(
        std::env::var_os("HARNESS_CBM_CATALOGUE").expect("explicit saved catalogue required"),
    );
    let prior: Value =
        serde_json::from_slice(&fs::read(owned.join("report.json")).unwrap()).unwrap();
    assert_eq!(
        prior["peer_clean_shutdown"], true,
        "native owned fixture prerequisite"
    );
    assert_eq!(prior["owned_ui_disabled"], true);
    let mut server = Server::start_with(Some((&executable, &owned, &catalogue)));
    server.initialize_named("codex-harness-codebase-memory");
    server.send(json!({"jsonrpc":"2.0","id":"index-owned","method":"tools/call",
        "params":{"name":"index_repository","arguments":{"repo_path":owned.join("repository"),"name":"native-cbm-coexist-owned","mode":"fast"}}}));
    let indexed = server.reply_with_timeout(Duration::from_secs(65));
    assert_eq!(indexed["id"], "index-owned");
    assert_eq!(indexed["result"]["isError"], false, "{indexed}");
    for (id, query, failed) in [
        (
            json!(u64::MAX),
            "MATCH (n:Function) WHERE n.name = 'alpha' RETURN n.name",
            false,
        ),
        (json!("ошибка-日本"), "MATCH ((( INVALID", true),
    ] {
        server.send(json!({"jsonrpc":"2.0","id":id,"method":"tools/call",
            "params":{"name":"query_graph","arguments":{"project":"native-cbm-coexist-owned","query":query,"format":"json"}}}));
        // The real tool may spend about ten seconds on its private daemon cycle.
        let response = server.reply_with_timeout(Duration::from_secs(65));
        assert_eq!(response["id"], id);
        assert_eq!(response["result"]["isError"], failed);
        if !failed {
            assert_eq!(
                response["result"]["structuredContent"]["rows"],
                json!([["alpha"]])
            );
        }
    }
    server.send(json!({"jsonrpc":"2.0","id":"cancel-owned","method":"tools/call",
        "params":{"name":"query_graph","arguments":{"project":"native-cbm-coexist-owned","query":"MATCH (n) RETURN n","format":"json"}}}));
    // Observe the real daemon's live operation-log handle before cancelling;
    // a handshake alone would exercise only the pre-spawn cancellation path.
    use std::os::windows::fs::OpenOptionsExt;
    let deadline = Instant::now() + Duration::from_secs(25);
    loop {
        let probe = fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(owned.join("cache/logs/cbm-daemon.log"));
        if matches!(probe, Err(ref error) if error.raw_os_error() == Some(32)) {
            break;
        }
        drop(probe); // Leave a real window for the daemon to acquire its writer.
        assert!(server.child.is_running().unwrap());
        assert!(
            Instant::now() < deadline,
            "no owned live daemon observed before cancellation"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    server.send(json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":"cancel-owned"}}));
    server.send(json!({"jsonrpc":"2.0","id":"after-cancel","method":"tools/call",
        "params":{"name":"query_graph","arguments":{"project":"native-cbm-coexist-owned","query":"MATCH (n:Function) WHERE n.name = 'alpha' RETURN n.name","format":"json"}}}));
    let after_cancel = server.reply_with_timeout(Duration::from_secs(65));
    assert_eq!(after_cancel["id"], "after-cancel");
    assert_eq!(
        after_cancel["result"]["structuredContent"]["rows"],
        json!([["alpha"]])
    );
    server.eof();
    assert!(server.finish(0, Duration::from_secs(10)).is_empty());
}
