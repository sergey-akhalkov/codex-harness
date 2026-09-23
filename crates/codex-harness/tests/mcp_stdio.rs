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
        Self::start_with()
    }
    fn start_with() -> Self {
        let root = tempfile::tempdir().unwrap();
        let (input, writer) = anonymous_pipe(4096).unwrap();
        let (reader, output) = anonymous_pipe(4096).unwrap();
        let mut command = CommandSpec::new(env!("CARGO_BIN_EXE_harness-mcp-probe-fixture"));
        command.args.push("--stdio-session".into());
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
