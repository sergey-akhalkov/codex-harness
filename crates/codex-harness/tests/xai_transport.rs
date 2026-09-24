//! Real shim lifecycle on an owned port; no OAuth or provider requests.
#![cfg(windows)]

use harness_core::{
    cancellable_pipe::{CancellablePipe, PipeIoError, anonymous_pipe},
    native_launcher::ensure_xai_shim,
    process::{Cancellation, CommandSpec, Deadline, Job, Limits, StopReason},
    xai_responses_shim::{IDENTITY_PATH, RETIRE_PATH},
};
use serde_json::Value;
use std::{
    env, io,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};

fn request(port: u16, method: &str, path: &str) -> io::Result<Value> {
    let mut stream =
        TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_secs(2))?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    )?;
    let mut response = String::new();
    stream.take(16_384).read_to_string(&mut response)?;
    let (head, body) = response
        .split_once("\r\n\r\n")
        .ok_or_else(|| io::Error::other("missing shim response head"))?;
    if !head.starts_with("HTTP/1.1 200 ") {
        return Err(io::Error::other("shim control request failed"));
    }
    serde_json::from_str(body).map_err(io::Error::other)
}

struct Shim {
    port: u16,
    manager: PathBuf,
}

#[test]
#[ignore = "child entry point for the owned preflight Job regression"]
fn shim_preflight_child() {
    let manager = PathBuf::from(env::var_os("HARNESS_XAI_TEST_MANAGER").unwrap());
    let port = env::var("HARNESS_XAI_TEST_PORT").unwrap().parse().unwrap();
    ensure_xai_shim(&manager, port).unwrap();
}

#[test]
fn xai_shim_outlives_the_preflight_job_that_started_it() {
    let manager = env::var_os("HARNESS_ACCEPTANCE_XAI_MANAGER")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_codex-harness")))
        .canonicalize()
        .unwrap();
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let shim = Shim {
        port: listener.local_addr().unwrap().port(),
        manager,
    };
    drop(listener);
    let job = Job::new(Limits::default()).unwrap();
    let mut command = CommandSpec::new(env::current_exe().unwrap());
    command.args = [
        "--exact",
        "shim_preflight_child",
        "--ignored",
        "--nocapture",
    ]
    .map(Into::into)
    .to_vec();
    command.env.insert(
        "HARNESS_XAI_TEST_MANAGER".into(),
        Some(shim.manager.clone().into_os_string()),
    );
    command.env.insert(
        "HARNESS_XAI_TEST_PORT".into(),
        Some(shim.port.to_string().into()),
    );
    let (reader, writer) = anonymous_pipe(4096).unwrap();
    command.stdout = Some(writer);
    let child = job.spawn(&command).unwrap();
    drop(command);
    let outcome = job
        .wait(
            &child,
            Deadline::after(Duration::from_secs(30)).unwrap(),
            &Cancellation::default(),
            Duration::from_secs(2),
        )
        .unwrap();
    assert_eq!(outcome.reason, StopReason::Exited);
    assert_eq!(outcome.exit_code, 0, "preflight must complete successfully");
    let cancel = Cancellation::default();
    let mut output = CancellablePipe::reader(reader, cancel.clone()).unwrap();
    let deadline = Deadline::after(Duration::from_secs(3)).unwrap();
    let mut size = 0;
    loop {
        match output.read(4096, deadline, &cancel) {
            Ok(bytes) if bytes.is_empty() => break,
            Err(PipeIoError::EndOfFile) => break,
            Ok(bytes) => {
                size += bytes.len();
                assert!(size <= 16_384, "preflight output exceeds its bound");
            }
            Err(error) => panic!("shared shim retained the preflight output pipe: {error}"),
        }
    }
    output
        .close(Deadline::after(Duration::from_secs(2)).unwrap())
        .unwrap();
    let identity = request(shim.port, "GET", IDENTITY_PATH)
        .expect("shared shim must survive cleanup of the preflight Job");
    assert_eq!(
        PathBuf::from(identity["exe"].as_str().unwrap()),
        shim.manager
    );
}

impl Drop for Shim {
    fn drop(&mut self) {
        // Cleanup is restricted to this test's port and verified manager.
        if let Ok(identity) = request(self.port, "GET", IDENTITY_PATH)
            && identity["exe"].as_str().map(PathBuf::from).as_ref() == Some(&self.manager)
        {
            let _ = request(self.port, "POST", RETIRE_PATH);
        }
    }
}

#[test]
fn cold_xai_shim_is_ready_reused_and_survives_session_cleanup() {
    // An explicit installed binary permits the same check outside the checkout.
    let manager = env::var_os("HARNESS_ACCEPTANCE_XAI_MANAGER")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_codex-harness")))
        .canonicalize()
        .unwrap();
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let shim = Shim {
        port: listener.local_addr().unwrap().port(),
        manager,
    };
    drop(listener);
    let ready = std::sync::Barrier::new(2);
    thread::scope(|scope| {
        let start = || {
            ready.wait();
            ensure_xai_shim(&shim.manager, shim.port).unwrap();
        };
        let peer = scope.spawn(start);
        start();
        peer.join().unwrap();
    });
    let identity = request(shim.port, "GET", IDENTITY_PATH).unwrap();
    assert_eq!(identity["harness"], "xai-responses-shim");
    assert_eq!(
        PathBuf::from(identity["exe"].as_str().unwrap()),
        shim.manager
    );
    assert!(identity["pid"].as_u64().is_some());

    let job = Job::new(Limits::default()).unwrap();
    let mut command = CommandSpec::new(env!("CARGO_BIN_EXE_harness-executor-fixture"));
    command
        .env
        .insert("HARNESS_EXECUTOR_FIXTURE_MODE".into(), Some("hang".into()));
    let child = job.spawn(&command).unwrap();
    assert!(job.contains(&child).unwrap());

    ensure_xai_shim(&shim.manager, shim.port).unwrap();
    assert_eq!(request(shim.port, "GET", IDENTITY_PATH).unwrap(), identity);
    job.terminate(0, Duration::from_secs(5)).unwrap();
    assert_eq!(request(shim.port, "GET", IDENTITY_PATH).unwrap(), identity);

    let port = shim.port;
    drop(shim);
    let until = Instant::now() + Duration::from_secs(5);
    while TcpStream::connect(("127.0.0.1", port)).is_ok() {
        assert!(Instant::now() < until, "owned shim must release its port");
        thread::sleep(Duration::from_millis(20));
    }
}
