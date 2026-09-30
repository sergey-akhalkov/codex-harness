//! Real owned xAI shim generations on loopback: endpoint ownership identity,
//! generation continuity across an update, explicit recovery, reclamation and
//! owner-release retirement. Every model request in these tests goes to an
//! owned synthetic loopback upstream through a synthetic secret; no OAuth,
//! provider request or production model call is made.
#![cfg(windows)]

use harness_core::{
    native_launcher::{
        ensure_xai_endpoint, ensure_xai_shim_at, loopback_listener_owner, retire_xai_endpoint,
    },
    process::{CommandSpec, Job, Limits},
};
use std::{
    env, fs, io,
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

/// Synthetic credential: it must reach only a verified owned endpoint.
const SENTINEL: &str = "Bearer harness-xai-transport-sentinel";

fn manager() -> PathBuf {
    env::var_os("HARNESS_ACCEPTANCE_XAI_MANAGER")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_codex-harness")))
        .canonicalize()
        .unwrap()
}

/// A distinct build source: the same image plus a trailing byte, so the
/// selected generation differs while the binary still runs.
fn manager_copy(base: &Path, home: &Path, tag: &str) -> PathBuf {
    let directory = home.join(format!("build-{tag}"));
    fs::create_dir_all(&directory).unwrap();
    let target = directory.join("codex-harness.exe");
    let mut bytes = fs::read(base).unwrap();
    bytes.push(b'x');
    fs::write(&target, bytes).unwrap();
    target
}

fn free_port() -> u16 {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    listener.local_addr().unwrap().port()
}

fn port_free(port: u16) -> bool {
    // The OS listener table is authoritative; a firewall can make a closed
    // port time out silently, so a connection probe alone would be ambiguous.
    loopback_listener_owner(port)
        .map(|owner| owner.is_none())
        .unwrap_or(false)
}

fn port_accepting(port: u16) -> bool {
    TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_millis(500)).is_ok()
}

fn wait_until(mut condition: impl FnMut() -> bool, timeout: Duration, what: &str) {
    let deadline = Instant::now() + timeout;
    while !condition() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        thread::sleep(Duration::from_millis(50));
    }
}

// ---------------------------------------------------------------------------
// Synthetic upstream
// ---------------------------------------------------------------------------

struct Upstream {
    port: u16,
    requests: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

impl Upstream {
    /// Answers each `/responses` request with two delimited chunks around a
    /// `stall`, so one accepted stream can outlive a retirement grace while a
    /// later request still uses the same endpoint.
    fn start(stall: Duration) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&requests);
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = Arc::clone(&stop);
        let worker = thread::spawn(move || {
            while !stopping.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let recorded = Arc::clone(&recorded);
                        thread::spawn(move || handle_upstream(stream, stall, &recorded));
                    }
                    Err(_) => thread::sleep(Duration::from_millis(20)),
                }
            }
        });
        Self {
            port,
            requests,
            stop,
            worker: Some(worker),
        }
    }

    fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    fn saw_sentinel(&self) -> bool {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .any(|request| request.contains("harness-xai-transport-sentinel"))
    }

    fn request_count(&self) -> usize {
        self.requests.lock().unwrap().len()
    }
}

impl Drop for Upstream {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn handle_upstream(mut stream: TcpStream, stall: Duration, recorded: &Mutex<Vec<String>>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(30)));
    let mut request = Vec::new();
    let mut byte = [0u8; 1];
    while !request.windows(4).any(|window| window == b"\r\n\r\n") {
        match stream.read_exact(&mut byte) {
            Ok(()) => request.push(byte[0]),
            Err(_) => return,
        }
        if request.len() > 64 * 1024 {
            return;
        }
    }
    let head = String::from_utf8_lossy(&request).into_owned();
    let length: usize = head
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.trim()
                .eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())?
        })
        .unwrap_or(0);
    let mut body = vec![0u8; length];
    if stream.read_exact(&mut body).is_err() {
        return;
    }
    let mut full = head.clone();
    full.push_str(&String::from_utf8_lossy(&body));
    recorded.lock().unwrap().push(full);

    if !head.starts_with("POST /responses") {
        let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n");
        return;
    }
    let _ = stream.write_all(
        b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
    );
    let first = format!("data: {{\"chunk\":\"first\",\"sentinel\":\"{SENTINEL}\"}}\n\n");
    let last = format!("data: {{\"chunk\":\"last\",\"sentinel\":\"{SENTINEL}\"}}\n\n");
    for (index, chunk) in [first, last].into_iter().enumerate() {
        if index == 1 {
            // The stream stays open across the former retirement grace; the
            // shim must keep it under its original request deadline.
            thread::sleep(stall);
        }
        let framed = format!("{:x}\r\n{chunk}\r\n", chunk.len());
        if stream.write_all(framed.as_bytes()).is_err() {
            return;
        }
        let _ = stream.flush();
    }
    let _ = stream.write_all(b"0\r\n\r\n");
}

// ---------------------------------------------------------------------------
// Foreign listener probe (spoof evidence must never authenticate)
// ---------------------------------------------------------------------------

struct ForeignProbe {
    port: u16,
    seen: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

impl ForeignProbe {
    fn start() -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&seen);
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = Arc::clone(&stop);
        let worker = thread::spawn(move || {
            while !stopping.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                        let mut buffer = [0u8; 4096];
                        let size = stream.read(&mut buffer).unwrap_or(0);
                        recorded
                            .lock()
                            .unwrap()
                            .push(String::from_utf8_lossy(&buffer[..size]).into_owned());
                        // A plausible identity document changes nothing: the
                        // receipt, the OS listener binding and the token are
                        // the evidence, not this self-description.
                        let body = serde_json::json!({
                            "harness": "xai-responses-shim",
                            "schema": 2,
                            "pid": std::process::id(),
                            "port": port,
                            "source": "f".repeat(64),
                        })
                        .to_string();
                        let _ = stream.write_all(
                            format!(
                                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                                body.len()
                            )
                            .as_bytes(),
                        );
                    }
                    Err(_) => thread::sleep(Duration::from_millis(20)),
                }
            }
        });
        Self {
            port,
            seen,
            stop,
            worker: Some(worker),
        }
    }

    fn saw_authorization(&self) -> bool {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .any(|request| request.to_ascii_lowercase().contains("authorization:"))
    }
}

impl Drop for ForeignProbe {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

// ---------------------------------------------------------------------------
// Owned child processes
// ---------------------------------------------------------------------------

struct Owner {
    child: Child,
    port: u16,
    lines: mpsc::Receiver<String>,
}

impl Owner {
    fn ensure(
        home: &Path,
        manager: &Path,
        preferred: u16,
        hold_ms: u64,
        upstream: Option<&str>,
    ) -> Self {
        let mut command = Command::new(env::current_exe().unwrap());
        command
            .args(["--exact", "owner_child", "--ignored", "--nocapture"])
            .env("HARNESS_XAI_TEST_HOME", home)
            .env("HARNESS_XAI_TEST_MANAGER", manager)
            .env("HARNESS_XAI_TEST_PREFERRED", preferred.to_string())
            .env("HARNESS_XAI_TEST_HOLD_MS", hold_ms.to_string())
            // Owned test processes only: the delivered release defaults stay
            // in force for real sessions, and these shorten the observation
            // of owner-release retirement.
            .env("HARNESS_XAI_SHIM_GRACE_MS", "4000")
            .env("HARNESS_XAI_SHIM_POLL_MS", "500")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        if let Some(upstream) = upstream {
            // Loopback-only override; the shim never carries provider
            // credentials anywhere else.
            command.env("HARNESS_XAI_SHIM_UPSTREAM", upstream);
        }
        let mut child = command.spawn().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (sender, lines) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                let _ = sender.send(line);
            }
        });
        let port = loop {
            let line = lines
                .recv_timeout(Duration::from_secs(60))
                .expect("owner child must report its selected endpoint");
            if let Some(port) = line.strip_prefix("port=") {
                break port.parse::<u16>().unwrap();
            }
            if line.contains("panicked") {
                panic!("owner child failed: {line}");
            }
        };
        Self { child, port, lines }
    }

    fn wait_for_exit(&mut self) -> i32 {
        let status = self.child.wait().unwrap();
        status.code().unwrap_or(-1)
    }

    fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    fn drain_lines(&mut self) -> Vec<String> {
        let mut collected = Vec::new();
        while let Ok(line) = self.lines.try_recv() {
            collected.push(line);
        }
        collected
    }
}

/// Child entry point: bind this process as a verified generation's owner and
/// hold it until the parent finishes. Exiting (cleanly or by kill) releases
/// the owner, which retires the generation.
#[test]
#[ignore = "child entry point for owned xAI generation acceptance"]
fn owner_child() {
    let home = PathBuf::from(env::var_os("HARNESS_XAI_TEST_HOME").unwrap());
    let manager = PathBuf::from(env::var_os("HARNESS_XAI_TEST_MANAGER").unwrap());
    let preferred: u16 = env::var("HARNESS_XAI_TEST_PREFERRED")
        .unwrap()
        .parse()
        .unwrap();
    let hold: u64 = env::var("HARNESS_XAI_TEST_HOLD_MS")
        .map(|value| value.parse().unwrap())
        .unwrap_or(5_000);
    let endpoint = ensure_xai_endpoint(&manager, &home, preferred).unwrap();
    println!("port={}", endpoint.port());
    io::stdout().flush().unwrap();
    thread::sleep(Duration::from_millis(hold));
}

/// Child entry point: the pinned route inside a kill-on-close Job. The shim
/// must survive cleanup of that Job, so the parent can still verify and
/// retire it afterwards.
#[test]
#[ignore = "child entry point for the owned pinned-route preflight regression"]
fn pinned_preflight_child() {
    let manager = PathBuf::from(env::var_os("HARNESS_XAI_TEST_MANAGER").unwrap());
    let home = PathBuf::from(env::var_os("HARNESS_XAI_TEST_HOME").unwrap());
    let port: u16 = env::var("HARNESS_XAI_TEST_PREFERRED")
        .unwrap()
        .parse()
        .unwrap();
    let endpoint = ensure_xai_endpoint(&manager, &home, port).unwrap();
    // The pinned route inside this Job must reuse the same verified endpoint.
    ensure_xai_shim_at(&manager, &home, endpoint.port()).unwrap();
    // The parent reads the actually selected endpoint from a file the child
    // alone writes: a pre-chosen port can be stolen by unrelated processes.
    if let Some(path) = env::var_os("HARNESS_XAI_TEST_PORT_FILE") {
        fs::write(path, endpoint.port().to_string()).unwrap();
    }
    println!("port={}", endpoint.port());
    io::stdout().flush().unwrap();
    thread::sleep(Duration::from_secs(30));
}

// ---------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------

fn open_model_stream(port: u16, read_timeout: Duration) -> io::Result<TcpStream> {
    let body = serde_json::json!({
        "model": "grok-synthetic",
        "input": [{"role": "user", "content": [{"type": "input_text", "text": "ping"}]}],
        "stream": true,
    })
    .to_string();
    let mut stream = TcpStream::connect(("127.0.0.1", port))?;
    stream.set_read_timeout(Some(read_timeout))?;
    stream.set_write_timeout(Some(read_timeout))?;
    write!(
        stream,
        "POST /responses HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: {SENTINEL}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )?;
    Ok(stream)
}

fn model_request(port: u16, read_timeout: Duration) -> io::Result<String> {
    let stream = open_model_stream(port, read_timeout)?;
    let mut response = String::new();
    stream.take(1024 * 1024).read_to_string(&mut response)?;
    Ok(response)
}

/// Read only until the first streamed chunk arrives. A synthetic upstream
/// keeps later chunks behind a deliberate stall, so a full read would wait for
/// the whole stream instead of proving the endpoint serves a request.
fn model_first_chunk(port: u16, timeout: Duration) -> io::Result<String> {
    let mut stream = open_model_stream(port, timeout)?;
    let mut received = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(size) => {
                received.extend_from_slice(&chunk[..size]);
                if String::from_utf8_lossy(&received).contains("first") {
                    break;
                }
            }
            Err(error) => {
                if String::from_utf8_lossy(&received).contains("first") {
                    break;
                }
                return Err(error);
            }
        }
    }
    Ok(String::from_utf8_lossy(&received).into_owned())
}

// ---------------------------------------------------------------------------
// Acceptance
// ---------------------------------------------------------------------------

#[test]
fn foreign_and_spoofed_listeners_are_never_trusted_and_never_receive_secrets() {
    let home = tempfile::tempdir().unwrap();
    let manager = manager();
    let upstream = Upstream::start(Duration::ZERO);
    let foreign = ForeignProbe::start();

    // The pinned route fails explicitly and preserves the foreign process.
    let error = ensure_xai_shim_at(&manager, home.path(), foreign.port).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("not a verified harness generation"),
        "{error}"
    );
    assert!(
        !foreign.saw_authorization(),
        "an unverified listener must never see the generation token or a secret"
    );
    assert!(
        port_accepting(foreign.port),
        "the foreign listener is preserved, not terminated"
    );

    // Selecting a route avoids the spoof entirely and serves the session from
    // a verified owned endpoint; the synthetic secret reaches only there.
    let mut owner = Owner::ensure(
        home.path(),
        &manager,
        foreign.port,
        30_000,
        Some(&upstream.url()),
    );
    assert_ne!(owner.port, foreign.port);
    assert!(
        model_request(owner.port, Duration::from_secs(15))
            .unwrap()
            .contains("first")
    );
    assert!(
        upstream.saw_sentinel(),
        "the verified endpoint forwarded the secret"
    );
    assert!(
        !foreign.saw_authorization(),
        "the spoofed listener received no model request"
    );

    // Explicit authenticated recovery releases only this generation.
    assert!(retire_xai_endpoint(home.path(), owner.port).unwrap());
    wait_until(
        || port_free(owner.port),
        Duration::from_secs(10),
        "the retired generation to release its port",
    );
    assert!(
        port_accepting(foreign.port),
        "unrelated foreign listeners survive explicit recovery"
    );
    owner.kill();
}

#[test]
fn generation_retires_when_its_last_owner_releases() {
    let home = tempfile::tempdir().unwrap();
    let manager = manager();
    // The loopback dead-end guarantees no acceptance step could ever reach a
    // production endpoint even by mistake; this test forwards nothing.
    let mut owner = Owner::ensure(
        home.path(),
        &manager,
        free_port(),
        1_500,
        Some("http://127.0.0.1:1"),
    );
    let port = owner.port;
    assert_eq!(owner.wait_for_exit(), 0, "{:?}", owner.drain_lines());
    // The recorded owner is gone; the generation retires after its grace.
    wait_until(
        || port_free(port),
        Duration::from_secs(20),
        "the released generation to retire",
    );
    assert!(
        !retire_xai_endpoint(home.path(), port).unwrap(),
        "an already-retired generation has nothing left to retire"
    );
}

#[test]
fn concurrent_launches_and_an_unclean_exit_reclaim_only_abandoned_generations() {
    let home = tempfile::tempdir().unwrap();
    let manager_a = manager();
    let manager_b = manager_copy(&manager_a, home.path(), "b");
    let shared = free_port();
    let upstream = Upstream::start(Duration::ZERO);

    // Two builds launch concurrently; admission serializes selection so both
    // sessions end up on distinct verified generations without a lost owner.
    let mut owner_a = Owner::ensure(
        home.path(),
        &manager_a,
        shared,
        300_000,
        Some(&upstream.url()),
    );
    let mut owner_b = Owner::ensure(
        home.path(),
        &manager_b,
        shared,
        300_000,
        Some(&upstream.url()),
    );
    assert_ne!(owner_a.port, owner_b.port);
    assert!(
        model_request(owner_a.port, Duration::from_secs(15))
            .unwrap()
            .contains("first")
    );
    assert!(
        model_request(owner_b.port, Duration::from_secs(15))
            .unwrap()
            .contains("first")
    );

    // An unclean owner exit abandons only its own generation.
    owner_b.kill();
    wait_until(
        || port_free(owner_b.port),
        Duration::from_secs(20),
        "the abandoned generation to be reclaimed",
    );
    assert!(
        !retire_xai_endpoint(home.path(), owner_b.port).unwrap(),
        "nothing verified remains at the abandoned endpoint"
    );
    assert!(
        model_request(owner_a.port, Duration::from_secs(15))
            .unwrap()
            .contains("first"),
        "the unrelated live generation stays intact"
    );

    // A rollback launch of the killed build reuses the reclaimed generation
    // slot and works, without touching the first build's live generation.
    let mut owner_b2 = Owner::ensure(
        home.path(),
        &manager_b,
        owner_b.port,
        300_000,
        Some(&upstream.url()),
    );
    assert_eq!(
        owner_b2.port, owner_b.port,
        "the abandoned endpoint is reclaimed"
    );
    assert!(
        model_request(owner_b2.port, Duration::from_secs(15))
            .unwrap()
            .contains("first")
    );
    assert!(
        model_request(owner_a.port, Duration::from_secs(15))
            .unwrap()
            .contains("first"),
        "no generation was retired to reinstate another"
    );
    assert!(retire_xai_endpoint(home.path(), owner_a.port).unwrap());
    owner_a.kill();
    owner_b2.kill();
}

#[test]
fn pinned_route_survives_preflight_job_cleanup_and_verifies_ownership() {
    let home = tempfile::tempdir().unwrap();
    let manager = manager();
    let port_file = home.path().join("preflight-port.txt");
    let job = Job::new(Limits::default()).unwrap();
    let mut command = CommandSpec::new(env::current_exe().unwrap());
    command.args = [
        "--exact",
        "pinned_preflight_child",
        "--ignored",
        "--nocapture",
    ]
    .map(Into::into)
    .to_vec();
    command.env.insert(
        "HARNESS_XAI_TEST_MANAGER".into(),
        Some(manager.clone().into_os_string()),
    );
    command
        .env
        .insert("HARNESS_XAI_TEST_HOME".into(), Some(home.path().into()));
    command.env.insert(
        "HARNESS_XAI_TEST_PREFERRED".into(),
        Some(free_port().to_string().into()),
    );
    command.env.insert(
        "HARNESS_XAI_TEST_PORT_FILE".into(),
        Some(port_file.clone().into_os_string()),
    );
    // Loopback dead-end: the preflight route forwards nothing, and even a
    // mistaken forward could never reach a production endpoint.
    command.env.insert(
        "HARNESS_XAI_SHIM_UPSTREAM".into(),
        Some("http://127.0.0.1:1".into()),
    );
    // Keep the shared generation alive across the Job reap so the parent can
    // still authenticate and explicitly retire it.
    command
        .env
        .insert("HARNESS_XAI_SHIM_GRACE_MS".into(), Some("8000".into()));
    command
        .env
        .insert("HARNESS_XAI_SHIM_POLL_MS".into(), Some("2000".into()));
    let _child = job.spawn(&command).unwrap();
    drop(command);
    // The child reports its actually selected endpoint; unrelated processes
    // can steal a pre-chosen port, so the parent never guesses it.
    let mut port = None;
    wait_until(
        || {
            port = fs::read_to_string(&port_file)
                .ok()
                .and_then(|value| value.trim().parse::<u16>().ok());
            port.is_some()
        },
        Duration::from_secs(60),
        "the preflight child to report its endpoint",
    );
    let port = port.unwrap();
    // Wait until the child's shim accepts, then reap the whole Job: the
    // shared transport must not belong to it.
    wait_until(
        || port_accepting(port),
        Duration::from_secs(20),
        "the pinned shim to accept",
    );
    job.terminate(0, Duration::from_secs(5)).unwrap();
    // The verified generation survived the Job cleanup: retirement must still
    // authenticate and succeed against it.
    assert!(
        retire_xai_endpoint(home.path(), port).unwrap(),
        "the shared pinned generation survives the preflight Job"
    );
    wait_until(
        || port_free(port),
        Duration::from_secs(10),
        "the preflight generation to release its port",
    );
}

#[test]
#[ignore = "long acceptance: a stream outliving the former 60s retirement grace"]
fn update_selects_a_new_endpoint_while_the_old_session_stream_and_requests_survive() {
    // The delivered acceptance uses 70s so the stream outlives the former
    // 60-second retirement grace; a shorter stall is available for debugging
    // iterations of the same interleaving.
    let stall_ms: u64 = env::var("HARNESS_XAI_TEST_STALL_MS")
        .map(|value| value.parse().unwrap())
        .unwrap_or(70_000);
    let stall = Duration::from_millis(stall_ms);
    let home = tempfile::tempdir().unwrap();
    let manager_a = manager();
    let manager_b = manager_copy(&manager_a, home.path(), "b");
    let upstream = Upstream::start(stall);
    let mut owner_a = Owner::ensure(
        home.path(),
        &manager_a,
        free_port(),
        300_000,
        Some(&upstream.url()),
    );
    let port_a = owner_a.port;

    // One accepted stream is in flight when the update starts.
    let mut client = TcpStream::connect(("127.0.0.1", port_a)).unwrap();
    client
        .set_read_timeout(Some(stall + Duration::from_secs(30)))
        .unwrap();
    let body = serde_json::json!({
        "model": "grok-synthetic",
        "input": [{"role": "user", "content": [{"type": "input_text", "text": "long"}]}],
        "stream": true,
    })
    .to_string();
    write!(
        client,
        "POST /responses HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: {SENTINEL}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
    let mut first = [0u8; 4096];
    let read = client.read(&mut first).unwrap();
    let first_text = String::from_utf8_lossy(&first[..read]).into_owned();
    assert!(first_text.contains("first"), "{first_text}");

    // Update: a newer build selects its own verified endpoint; the older
    // generation must not be retired under the accepted stream.
    let updated_at = Instant::now();
    let mut owner_b = Owner::ensure(
        home.path(),
        &manager_b,
        port_a,
        300_000,
        Some(&upstream.url()),
    );
    assert_ne!(owner_b.port, port_a, "an update selects a new endpoint");
    let new_first =
        model_first_chunk(owner_b.port, Duration::from_secs(20)).unwrap_or_else(|error| {
            panic!(
                "a concurrent new session could not use the new verified generation {}: {error:?}",
                owner_b.port
            )
        });
    assert!(
        new_first.contains("first"),
        "a concurrent new session uses the new verified generation"
    );

    // The old stream continues beyond the former retirement grace and under
    // its original deadline.
    let mut rest = String::new();
    client.take(1024 * 1024).read_to_string(&mut rest).unwrap();
    assert!(
        rest.contains("last"),
        "the accepted stream must outlive the update: {rest}"
    );
    assert!(
        updated_at.elapsed() > Duration::from_secs(60),
        "the stream outlived the former 60-second retirement grace"
    );
    // A later request from the old session still uses its compatible endpoint.
    assert!(
        model_first_chunk(port_a, Duration::from_secs(30))
            .unwrap()
            .contains("first"),
        "the old session's next request still works"
    );
    assert!(upstream.request_count() >= 3);
    assert!(retire_xai_endpoint(home.path(), owner_b.port).unwrap());
    assert!(retire_xai_endpoint(home.path(), port_a).unwrap());
    owner_a.kill();
    owner_b.kill();
}
