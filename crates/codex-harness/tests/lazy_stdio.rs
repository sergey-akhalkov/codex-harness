use harness_core::{
    cancellable_pipe::anonymous_pipe,
    lazy_stdio::{LazyWorker, Lease, Rejection, WorkerLaunch},
    mcp_session::Session,
    mcp_stdio,
    process::{Cancellation, CommandSpec, Deadline, Limits},
};
use serde_json::{Value, json};
use std::{
    fs,
    io::{self, BufRead, Write},
    path::PathBuf,
    sync::Mutex,
    thread,
    time::Duration,
};

fn launch(root: &std::path::Path) -> WorkerLaunch {
    let mut command = CommandSpec::new(PathBuf::from(env!(
        "CARGO_BIN_EXE_harness-mcp-probe-fixture"
    )));
    command.args = vec!["--stdio-session".into()];
    WorkerLaunch {
        command,
        stderr: root.join("worker-stderr.txt"),
        limits: Limits::default(),
        initialize: json!({
            "protocolVersion":"2024-11-05",
            "capabilities":{},
            "clientInfo":{"name":"lazy-stdio-test","version":"0.1.0"}
        }),
        expected_server: Some("harness-stdio-fixture".into()),
        startup: Duration::from_secs(20),
    }
}

fn root(tag: &str) -> std::path::PathBuf {
    let dir =
        std::env::temp_dir().join(format!("harness-lazy-stdio-{}-{}", tag, std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn echo(value: u32) -> Value {
    json!({"value": value})
}

#[test]
fn end_to_end_forwarding_through_the_serving_loop() {
    let root = root("end-to-end");
    let lazy = LazyWorker::new(launch(&root), Duration::from_secs(60), None, None, None).unwrap();
    let definitions = vec![
        json!({"name":"echo","inputSchema":{"type":"object","properties":{}}}),
        json!({"name":"wait","inputSchema":{"type":"object","properties":{}}}),
    ];
    let session = Session::new("lazy-forward-test", definitions).unwrap();
    let (server_input, client_write) = anonymous_pipe(4096).unwrap();
    let (client_read, server_output) = anonymous_pipe(4096).unwrap();
    let connection = Cancellation::default();
    let serve_cancel = connection.clone();
    let server = {
        let lazy = std::sync::Arc::clone(&lazy);
        thread::spawn(move || {
            mcp_stdio::serve_fallible(
                session,
                server_input,
                server_output,
                &serve_cancel,
                Deadline::after(Duration::from_secs(60)).unwrap(),
                move |operation| {
                    lazy.call(
                        &operation.name,
                        operation.arguments.clone(),
                        operation.deadline,
                        &operation.cancellation,
                    )
                },
            )
        })
    };
    let mut writer = io::BufWriter::new(client_write);
    let mut reader = io::BufReader::new(client_read);
    let mut seen: Vec<Value> = Vec::new();
    let request =
        |id: u64, method: &str, params: Value, writer: &mut io::BufWriter<std::fs::File>| {
            writeln!(
                writer,
                "{}",
                json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
            )
            .unwrap();
            writer.flush().unwrap();
        };
    let notify = |method: &str, params: Value, writer: &mut io::BufWriter<std::fs::File>| {
        writeln!(
            writer,
            "{}",
            json!({"jsonrpc":"2.0","method":method,"params":params})
        )
        .unwrap();
        writer.flush().unwrap();
    };
    let read = |reader: &mut io::BufReader<std::fs::File>| -> Value {
        let mut line = String::new();
        let count = reader.read_line(&mut line).unwrap();
        assert!(count > 0, "serving loop closed early");
        let value: Value =
            serde_json::from_str(line.trim()).expect("only JSON-RPC leaves the forwarder");
        assert_eq!(value["jsonrpc"], "2.0");
        value
    };

    request(
        1,
        "initialize",
        json!({
            "protocolVersion":"2024-11-05",
            "capabilities":{},
            "clientInfo":{"name":"lazy-e2e","version":"0.1.0"}
        }),
        &mut writer,
    );
    let reply = read(&mut reader);
    assert_eq!(reply["id"], 1);
    assert_eq!(reply["result"]["serverInfo"]["name"], "lazy-forward-test");
    seen.push(reply);
    notify("notifications/initialized", json!({}), &mut writer);

    request(2, "tools/list", json!({}), &mut writer);
    let reply = read(&mut reader);
    assert_eq!(reply["id"], 2);
    assert_eq!(reply["result"]["tools"][0]["name"], "echo");
    seen.push(reply);
    assert_eq!(lazy.generation(), 0);

    request(
        3,
        "tools/call",
        json!({"name":"echo","arguments":echo(9)}),
        &mut writer,
    );
    let reply = read(&mut reader);
    assert_eq!(reply["id"], 3);
    assert_eq!(reply["result"]["structuredContent"]["value"], 9);
    seen.push(reply);
    assert_eq!(lazy.generation(), 1);

    // A cancelled in-flight request is suppressed and the connection stays
    // usable through a restarted worker generation.
    request(
        4,
        "tools/call",
        json!({"name":"wait","arguments":json!({})}),
        &mut writer,
    );
    notify(
        "notifications/cancelled",
        json!({"requestId":4,"reason":"fixture cancellation"}),
        &mut writer,
    );
    request(
        5,
        "tools/call",
        json!({"name":"echo","arguments":echo(10)}),
        &mut writer,
    );
    let reply = read(&mut reader);
    assert_eq!(reply["id"], 5);
    assert_eq!(reply["result"]["structuredContent"]["value"], 10);
    seen.push(reply);
    assert_eq!(lazy.generation(), 2);

    // Client EOF closes the connection; the owned tree is reclaimed.
    drop(writer);
    let result = server.join().unwrap();
    result.unwrap();
    lazy.close().unwrap();
    assert!(!lazy.has_worker());
    assert!(seen.iter().all(|value| value["jsonrpc"] == "2.0"));
}

#[test]
fn worker_starts_lazily_and_is_reused_between_requests() {
    let root = root("lazy-reuse");
    let worker = LazyWorker::new(launch(&root), Duration::from_secs(60), None, None, None).unwrap();
    assert_eq!(worker.generation(), 0);
    assert!(!worker.has_worker());
    let result = worker
        .call(
            "echo",
            echo(7),
            Deadline::after(Duration::from_secs(20)).unwrap(),
            &Cancellation::default(),
        )
        .unwrap();
    assert_eq!(result["structuredContent"]["value"], 7);
    assert_eq!(worker.generation(), 1);
    let identity = worker.worker_identity().unwrap();
    let result = worker
        .call(
            "echo",
            echo(8),
            Deadline::after(Duration::from_secs(20)).unwrap(),
            &Cancellation::default(),
        )
        .unwrap();
    assert_eq!(result["structuredContent"]["value"], 8);
    assert_eq!(worker.generation(), 1);
    assert_eq!(worker.worker_identity().unwrap(), identity);
    assert!(root.join("worker-stderr.txt").is_file());
    worker.close().unwrap();
}

#[test]
fn idle_watchdog_retires_the_worker_and_preserves_foreign_processes() {
    let root = root("idle-foreign");
    let worker =
        LazyWorker::new(launch(&root), Duration::from_millis(150), None, None, None).unwrap();
    let result = worker
        .call(
            "echo",
            echo(1),
            Deadline::after(Duration::from_secs(20)).unwrap(),
            &Cancellation::default(),
        )
        .unwrap();
    assert_eq!(result["structuredContent"]["value"], 1);
    // A foreign process with the same executable is never adopted or touched.
    let mut foreign = std::process::Command::new(env!("CARGO_BIN_EXE_harness-mcp-probe-fixture"))
        .arg("--stdio-session")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut waited = 0;
    while worker.has_worker() && waited < 100 {
        thread::sleep(Duration::from_millis(25));
        waited += 1;
    }
    assert!(
        !worker.has_worker(),
        "idle retirement must retire the worker"
    );
    let retirement = worker.retirement().unwrap();
    assert_eq!(retirement.exit_code, 0);
    assert!(foreign.try_wait().unwrap().is_none());
    foreign.kill().unwrap();
    let _ = foreign.wait();
    // The next request starts a fresh worker generation.
    let result = worker
        .call(
            "echo",
            echo(2),
            Deadline::after(Duration::from_secs(20)).unwrap(),
            &Cancellation::default(),
        )
        .unwrap();
    assert_eq!(result["structuredContent"]["value"], 2);
    assert_eq!(worker.generation(), 2);
    worker.close().unwrap();
}

#[test]
fn cancellation_reclaims_the_tree_and_the_next_request_restarts() {
    let root = root("cancel-restart");
    let worker = LazyWorker::new(launch(&root), Duration::from_secs(60), None, None, None).unwrap();
    worker
        .call(
            "echo",
            echo(1),
            Deadline::after(Duration::from_secs(20)).unwrap(),
            &Cancellation::default(),
        )
        .unwrap();
    let cancellation = Cancellation::default();
    {
        let cancellation = cancellation.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(100));
            cancellation.cancel();
        });
    }
    let suppressed = worker
        .call(
            "wait",
            json!({}),
            Deadline::after(Duration::from_secs(30)).unwrap(),
            &cancellation,
        )
        .unwrap();
    assert_eq!(suppressed["content"], json!([]));
    assert!(!worker.has_worker());
    assert!(worker.retirement().is_some());
    let result = worker
        .call(
            "echo",
            echo(3),
            Deadline::after(Duration::from_secs(20)).unwrap(),
            &Cancellation::default(),
        )
        .unwrap();
    assert_eq!(result["structuredContent"]["value"], 3);
    assert_eq!(worker.generation(), 2);
    worker.close().unwrap();
}

#[test]
fn deadline_failure_answers_an_error_and_restarts_cleanly() {
    let root = root("deadline-restart");
    let worker = LazyWorker::new(launch(&root), Duration::from_secs(60), None, None, None).unwrap();
    let result = worker
        .call(
            "wait",
            json!({}),
            Deadline::after(Duration::from_millis(200)).unwrap(),
            &Cancellation::default(),
        )
        .unwrap();
    assert_eq!(result["isError"], true);
    assert!(
        result["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("reclaimed")
    );
    assert!(!worker.has_worker());
    let result = worker
        .call(
            "echo",
            echo(4),
            Deadline::after(Duration::from_secs(20)).unwrap(),
            &Cancellation::default(),
        )
        .unwrap();
    assert_eq!(result["structuredContent"]["value"], 4);
    assert_eq!(worker.generation(), 2);
    worker.close().unwrap();
}

#[test]
fn immediate_worker_exit_is_a_start_failure_not_a_connection_failure() {
    let root = root("start-failure");
    let mut command = CommandSpec::new(PathBuf::from("C:/Windows/System32/cmd.exe"));
    command.args = vec!["/c".into(), "exit".into(), "0".into()];
    let launch = WorkerLaunch {
        command,
        stderr: root.join("worker-stderr.txt"),
        limits: Limits::default(),
        initialize: json!({
            "protocolVersion":"2024-11-05",
            "capabilities":{},
            "clientInfo":{"name":"lazy-stdio-test","version":"0.1.0"}
        }),
        expected_server: Some("harness-stdio-fixture".into()),
        startup: Duration::from_secs(20),
    };
    let worker = LazyWorker::new(launch, Duration::from_secs(60), None, None, None).unwrap();
    let result = worker
        .call(
            "echo",
            echo(1),
            Deadline::after(Duration::from_secs(20)).unwrap(),
            &Cancellation::default(),
        )
        .unwrap();
    assert_eq!(result["isError"], true);
    assert!(
        result["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("failed to start")
    );
    assert!(!worker.has_worker());
    worker.close().unwrap();
}

#[test]
fn admission_lease_wraps_each_request() {
    let root = root("admission");
    #[derive(Default)]
    struct Counts {
        acquired: u32,
        released: u32,
    }
    struct AdmissionLease {
        counts: std::sync::Arc<Mutex<Counts>>,
        fail: bool,
    }
    impl Lease for AdmissionLease {
        fn acquire(
            &mut self,
            _deadline: &Deadline,
            _cancellation: &Cancellation,
        ) -> io::Result<()> {
            if self.fail {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "fixture admission refused",
                ));
            }
            self.counts.lock().unwrap().acquired += 1;
            Ok(())
        }

        fn release(&mut self) -> io::Result<()> {
            self.counts.lock().unwrap().released += 1;
            Ok(())
        }
    }
    let counts = std::sync::Arc::new(Mutex::new(Counts::default()));
    let lease_counts = std::sync::Arc::clone(&counts);
    let lease_for = Box::new(move |_name: &str, _arguments: &Value| {
        Some(Box::new(AdmissionLease {
            counts: std::sync::Arc::clone(&lease_counts),
            fail: false,
        }) as Box<dyn Lease>)
    });
    let worker = LazyWorker::new(
        launch(&root),
        Duration::from_secs(60),
        Some(lease_for),
        None,
        None,
    )
    .unwrap();
    let result = worker
        .call(
            "echo",
            echo(5),
            Deadline::after(Duration::from_secs(20)).unwrap(),
            &Cancellation::default(),
        )
        .unwrap();
    assert_eq!(result["structuredContent"]["value"], 5);
    assert_eq!(counts.lock().unwrap().acquired, 1);
    assert_eq!(counts.lock().unwrap().released, 1);
    worker.close().unwrap();
}

#[test]
fn refused_admission_answers_an_error_and_keeps_the_worker() {
    let root = root("admission-refused");
    struct RefusedLease;
    impl Lease for RefusedLease {
        fn acquire(
            &mut self,
            _deadline: &Deadline,
            _cancellation: &Cancellation,
        ) -> io::Result<()> {
            Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "fixture admission refused",
            ))
        }

        fn release(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let worker = LazyWorker::new(
        launch(&root),
        Duration::from_secs(60),
        Some(Box::new(|_name: &str, _arguments: &Value| {
            Some(Box::new(RefusedLease) as Box<dyn Lease>)
        })),
        None,
        None,
    )
    .unwrap();
    let result = worker
        .call(
            "echo",
            echo(1),
            Deadline::after(Duration::from_secs(20)).unwrap(),
            &Cancellation::default(),
        )
        .unwrap();
    assert_eq!(result["isError"], true);
    assert!(
        result["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("admission was not granted")
    );
    assert!(worker.has_worker());
    assert_eq!(worker.generation(), 1);
    worker.close().unwrap();
}

#[test]
fn preflight_rejection_and_argument_rewrite_keep_the_worker() {
    let root = root("preflight");
    let worker = LazyWorker::new(
        launch(&root),
        Duration::from_secs(60),
        None,
        Some(Box::new(|name: &str, arguments: &Value| {
            if arguments["reject"] == true {
                return Ok(Err(Rejection::new("fixture refusal: rejected input")));
            }
            if name == "echo" && arguments["value"] == 1 {
                return Ok(Ok(Some(json!({"value": 41}))));
            }
            Ok(Ok(None))
        })),
        None,
    )
    .unwrap();
    let result = worker
        .call(
            "echo",
            json!({"reject": true}),
            Deadline::after(Duration::from_secs(20)).unwrap(),
            &Cancellation::default(),
        )
        .unwrap();
    assert_eq!(result["isError"], true);
    assert!(
        result["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("fixture refusal")
    );
    assert!(worker.has_worker());
    let result = worker
        .call(
            "echo",
            echo(1),
            Deadline::after(Duration::from_secs(20)).unwrap(),
            &Cancellation::default(),
        )
        .unwrap();
    assert_eq!(result["structuredContent"]["value"], 41);
    assert_eq!(worker.generation(), 1);
    worker.close().unwrap();
}

#[test]
fn after_hook_can_transform_or_fail_without_losing_the_worker() {
    let root = root("after-hook");
    let worker = LazyWorker::new(
        launch(&root),
        Duration::from_secs(60),
        None,
        None,
        Some(Box::new(
            |_name: &str, _arguments: &Value, result: Value| {
                if result["structuredContent"]["value"] == 13 {
                    return Err(io::Error::other("fixture result rejected"));
                }
                let mut result = result;
                result["structuredContent"]["doubled"] =
                    Value::from(result["structuredContent"]["value"].as_u64().unwrap() * 2);
                Ok(result)
            },
        )),
    )
    .unwrap();
    let result = worker
        .call(
            "echo",
            echo(6),
            Deadline::after(Duration::from_secs(20)).unwrap(),
            &Cancellation::default(),
        )
        .unwrap();
    assert_eq!(result["structuredContent"]["doubled"], 12);
    let result = worker
        .call(
            "echo",
            echo(13),
            Deadline::after(Duration::from_secs(20)).unwrap(),
            &Cancellation::default(),
        )
        .unwrap();
    assert_eq!(result["isError"], true);
    assert!(worker.has_worker());
    assert_eq!(worker.generation(), 1);
    worker.close().unwrap();
}
