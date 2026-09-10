#![cfg(windows)]
use harness_core::{
    codegraph_transport::Worker,
    process::{Cancellation, CommandSpec, Deadline, StopReason},
};
use serde_json::json;
use std::{io, time::Duration};

fn start(mode: &str, lease: Duration) -> Worker {
    let mut command = CommandSpec::new(env!("CARGO_BIN_EXE_harness-codegraph-fixture"));
    command.args.push(mode.into());
    Worker::start(command, lease, &Cancellation::default()).unwrap()
}
fn deadline() -> Deadline {
    Deadline::after(Duration::from_secs(5)).unwrap()
}

#[test]
fn real_pipe_round_trip_retains_errors_and_reclaims_descendants() {
    for mode in ["normal", "descendant", "error"] {
        let mut worker = start(mode, Duration::from_secs(30));
        worker
            .initialize(deadline(), &Cancellation::default())
            .unwrap();
        let response = worker
            .request(
                "tools/call",
                json!({"name":"codegraph_search","arguments":{"query":"entry"}}),
                deadline(),
                &Cancellation::default(),
            )
            .unwrap();
        if mode == "error" {
            assert_eq!(response["result"]["isError"], true);
        } else {
            assert!(response.to_string().contains("entry"));
        }
        let outcome = worker.close().unwrap().unwrap();
        assert_eq!(outcome.job.active_processes, 0);
        assert_eq!(outcome.job.memory_limit_bytes, 2 * 1024 * 1024 * 1024);
        assert_eq!(outcome.job.cpu_rate, 2500);
    }
}

#[test]
fn failed_and_oversized_frames_are_explicit_and_slot_is_reusable_after_cleanup() {
    for mode in ["malformed", "oversized", "duplicate-key", "wrong-id"] {
        let mut worker = start(mode, Duration::from_secs(30));
        worker
            .initialize(deadline(), &Cancellation::default())
            .unwrap();
        let result = worker.request(
            "tools/call",
            json!({}),
            deadline(),
            &Cancellation::default(),
        );
        assert_eq!(
            result.unwrap_err().kind(),
            io::ErrorKind::InvalidData,
            "{mode}"
        );
        assert_eq!(worker.close().unwrap().unwrap().job.active_processes, 0);
    }
}

#[test]
fn admission_cancellation_and_nonrenewable_lease_bound_work() {
    let mut worker = start("hang", Duration::from_secs(30));
    worker
        .initialize(deadline(), &Cancellation::default())
        .unwrap();
    let contender = Worker::start(
        CommandSpec::new(env!("CARGO_BIN_EXE_harness-codegraph-fixture")),
        Duration::from_secs(30),
        &Cancellation::default(),
    );
    assert!(matches!(contender,Err(ref error) if error.kind()==io::ErrorKind::WouldBlock));
    let cancellation = Cancellation::default();
    let signal = cancellation.clone();
    let cancel_thread = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        signal.cancel();
    });
    assert!(
        worker
            .request("tools/call", json!({}), deadline(), &cancellation)
            .is_err()
    );
    cancel_thread.join().unwrap();
    assert_eq!(worker.close().unwrap().unwrap().job.active_processes, 0);

    let mut worker = start("hang", Duration::from_secs(1));
    worker
        .initialize(deadline(), &Cancellation::default())
        .unwrap();
    assert!(
        worker
            .request(
                "tools/call",
                json!({}),
                deadline(),
                &Cancellation::default()
            )
            .is_err()
    );
    // The supervisor has its own nonrenewable lease. Allow it to observe the
    // expiry before a deliberate close cancellation can win that outcome tie.
    std::thread::sleep(Duration::from_millis(100));
    let outcome = worker.close().unwrap().unwrap();
    assert_eq!(outcome.reason, StopReason::Timeout);
    assert_eq!(outcome.job.active_processes, 0);
}

#[test]
fn asynchronous_refresh_failure_and_storage_pressure_stop_idle_workers() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    let mut worker = start("watch-failure", Duration::from_secs(30));
    worker
        .initialize(deadline(), &Cancellation::default())
        .unwrap();
    let until = deadline();
    while !worker.diagnostics()["failure"].is_string() && !until.expired() {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        worker.diagnostics()["failure"]
            .as_str()
            .unwrap()
            .contains("fixture partial generation")
    );
    assert_eq!(worker.close().unwrap().unwrap().job.active_processes, 0);

    let pressure = Arc::new(AtomicBool::new(false));
    let probe = pressure.clone();
    let mut command = CommandSpec::new(env!("CARGO_BIN_EXE_harness-codegraph-fixture"));
    command.args.push("normal".into());
    let mut worker = Worker::start_monitored(
        command,
        Duration::from_secs(30),
        &Cancellation::default(),
        Some(Arc::new(move || {
            if probe.load(Ordering::SeqCst) {
                Err(io::Error::other("owned allowance reached"))
            } else {
                Ok(())
            }
        })),
    )
    .unwrap();
    worker
        .initialize(deadline(), &Cancellation::default())
        .unwrap();
    pressure.store(true, Ordering::SeqCst);
    let until = deadline();
    while !worker.diagnostics()["failure"].is_string() && !until.expired() {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        worker.diagnostics()["failure"]
            .as_str()
            .unwrap()
            .contains("owned allowance reached")
    );
    assert_eq!(worker.close().unwrap().unwrap().job.active_processes, 0);
}

#[test]
fn deliberate_commands_preserve_exit_and_output_under_the_same_job_policy() {
    for (mode, exit_code) in [("cli-success", 0), ("cli-failure", 7), ("--linger", 124)] {
        let mut command = CommandSpec::new(env!("CARGO_BIN_EXE_harness-codegraph-fixture"));
        command.args.push(mode.into());
        let result = harness_core::codegraph_transport::run_command(
            command,
            Deadline::after(Duration::from_secs(1)).unwrap(),
            &Cancellation::default(),
            None,
        )
        .unwrap();
        assert_eq!(result.outcome.exit_code, exit_code, "{mode}");
        assert_eq!(result.outcome.job.active_processes, 0);
        assert_eq!(
            result.outcome.job.memory_limit_bytes,
            2 * 1024 * 1024 * 1024
        );
        assert_eq!(result.outcome.job.cpu_rate, 2500);
        if mode == "cli-success" {
            assert!(result.stdout.contains("fixture completed"));
            assert!(result.stderr.contains("fixture warning retained"));
        }
        if mode == "cli-failure" {
            assert!(result.stderr.contains("fixture partial write failed"));
        }
    }
}

#[test]
#[ignore = "requires explicitly selected published package and an owned indexed project"]
fn published_direct_worker_search_and_watcher() {
    use std::{fs, path::PathBuf};
    let package = PathBuf::from(
        std::env::var_os("CODEGRAPH_ACCEPTANCE_PACKAGE").expect("explicit published package"),
    );
    let project = PathBuf::from(
        std::env::var_os("CODEGRAPH_ACCEPTANCE_PROJECT").expect("explicit owned project"),
    );
    let data = std::env::var("CODEGRAPH_ACCEPTANCE_DATA_NAME").expect("explicit data directory");
    assert!(data.starts_with(".codegraph-"));
    assert!(project.join(&data).join("codegraph.db").is_file());
    let mut command = CommandSpec::new(package.join("node.exe"));
    command.current_dir = Some(project.clone());
    command.args = vec![
        package.join("lib/dist/bin/codegraph.js").into_os_string(),
        "serve".into(),
        "--mcp".into(),
        "--path".into(),
        project.clone().into_os_string(),
    ];
    for (key, value) in [
        ("CODEGRAPH_DIR", data.as_str()),
        ("CODEGRAPH_NO_DAEMON", "1"),
        ("DO_NOT_TRACK", "1"),
        ("CODEGRAPH_NO_UPDATE_CHECK", "1"),
        ("CODEGRAPH_PARSE_WORKERS", "1"),
        ("CODEGRAPH_RESOLVE_WORKERS", "1"),
        ("CODEGRAPH_FORCE_WATCH", "1"),
        ("CODEGRAPH_WATCH_DEBOUNCE_MS", "100"),
        ("CODEGRAPH_EXPLORE_DEDUP", "0"),
    ] {
        command.env.insert(key.into(), Some(value.into()));
    }
    let mut worker =
        Worker::start(command, Duration::from_secs(60), &Cancellation::default()).unwrap();
    let cancel = Cancellation::default();
    let deadline = || Deadline::after(Duration::from_secs(15)).unwrap();
    let initialized = worker.initialize(deadline(), &cancel).unwrap();
    assert_eq!(initialized["serverInfo"]["name"], "codegraph");
    let query =
        json!({"name":"codegraph_search","arguments":{"query":"evaluation_target","limit":5}});
    let first = worker
        .request("tools/call", query, deadline(), &cancel)
        .unwrap();
    assert!(first.to_string().contains("evaluation_target"), "{first}");
    let recovery = tempfile::tempdir().unwrap();
    let source_database = project.join(&data).join("codegraph.db");
    let before = harness_core::codegraph_store::counts(&source_database).unwrap();
    assert!(before["files"].as_u64().unwrap() > 0);
    let snapshot = recovery.path().join("committed.db");
    let copied = harness_core::codegraph_store::snapshot(
        &source_database,
        &snapshot,
        1024 * 1024 * 1024,
        5 * 1024 * 1024 * 1024,
        deadline(),
        &cancel,
    )
    .unwrap();
    assert!(copied > 0);
    assert_eq!(
        harness_core::codegraph_store::counts(&snapshot).unwrap(),
        before
    );
    // Pressure is simulated with a tiny allowance; no drive-filling fixture.
    assert!(
        harness_core::codegraph_store::snapshot(
            &source_database,
            &recovery.path().join("denied.db"),
            1,
            0,
            deadline(),
            &cancel
        )
        .is_err()
    );
    assert!(!recovery.path().join("denied.db").exists());
    let addition = project.join("src/harness_owned_watch_probe.rs");
    assert!(
        !addition.exists(),
        "owned probe must not overwrite product source"
    );
    fs::write(
        &addition,
        "pub fn harness_owned_watch_probe() -> usize { 7 }\n",
    )
    .unwrap();
    let until = std::time::Instant::now() + Duration::from_secs(10);
    let mut found = false;
    while std::time::Instant::now() < until {
        std::thread::sleep(Duration::from_millis(300));
        let answer = worker.request("tools/call",json!({"name":"codegraph_search","arguments":{"query":"harness_owned_watch_probe","limit":5}}),deadline(),&cancel).unwrap();
        if answer.to_string().contains("harness_owned_watch_probe.rs") {
            found = true;
            break;
        }
    }
    // Only this test's create-new path is removed. Preserve an unexpected edit.
    if fs::read(&addition).unwrap() == b"pub fn harness_owned_watch_probe() -> usize { 7 }\n" {
        fs::remove_file(&addition).unwrap();
    }
    let outcome = worker.close().unwrap().unwrap();
    assert_eq!(outcome.job.active_processes, 0);
    assert!(
        found,
        "watcher did not make the addition searchable; {}",
        worker.diagnostics()
    );
    assert_eq!(
        harness_core::codegraph_store::counts(&snapshot).unwrap(),
        before
    );
    println!(
        "published worker search/watcher passed; peak Job bytes={}",
        outcome.job.peak_job_memory_bytes
    );
}
