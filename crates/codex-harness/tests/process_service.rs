#![cfg(windows)]
use harness_core::{
    cancellable_pipe::{CancellablePipe, PipeIoError, anonymous_pipe},
    process::{Cancellation, CommandSpec, Deadline, Job, Limits},
    process_service::{ServiceProcess, creation_clock, current_user},
};
use serde_json::Value;
use std::{
    fs::{self, File},
    path::Path,
    time::{Duration, Instant},
};

const FIXTURE: &str = env!("CARGO_BIN_EXE_harness-service-fixture");
fn deadline(seconds: u64) -> Deadline {
    Deadline::after(Duration::from_secs(seconds)).unwrap()
}

struct StopService<'a>(&'a Path);
impl Drop for StopService<'_> {
    fn drop(&mut self) {
        let _ = fs::write(self.0.join("stop"), []);
    }
}

fn wait_json(path: &Path) -> Value {
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(bytes) = fs::read(path)
            && let Ok(value) = serde_json::from_slice(&bytes)
        {
            return value;
        }
        assert!(
            Instant::now() < end,
            "missing owned fixture report: {}",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn start(root: &Path, mode: &str) -> (Job, CancellablePipe, ServiceProcess, u64) {
    let (stdin, write) = anonymous_pipe(4096).unwrap();
    let (read, stdout) = anonymous_pipe(4096).unwrap();
    let mut command = CommandSpec::new(FIXTURE);
    command.current_dir = Some(root.into());
    command.args = vec![
        "--start".into(),
        root.into(),
        mode.into(),
        "Русский 日本".into(),
        "with \"quotes\"\\".into(),
        "".into(),
    ];
    command.env.insert(
        "HARNESS_SERVICE_AMBIENT_ONLY".into(),
        Some("must-not-inherit".into()),
    );
    command.stdin = Some(stdin);
    command.stdout = Some(stdout);
    command.stderr = Some(File::create(root.join("starter.stderr")).unwrap());
    let job = Job::new(Limits {
        memory_bytes: Some(512 * 1024 * 1024),
        cpu_percent: Some(25.0),
    })
    .unwrap();
    let began = creation_clock();
    let child = job.spawn(&command).unwrap();
    drop(command);
    let cancel = Cancellation::default();
    let input = CancellablePipe::writer(write, cancel.clone()).unwrap();
    let mut output = CancellablePipe::reader(read, cancel.clone()).unwrap();
    let mut bytes = Vec::new();
    let end = deadline(40);
    while !bytes.ends_with(b"\n") {
        let part = output.read(1024, end, &cancel).unwrap_or_else(|error| {
            panic!(
                "starter pipe: {error}; exit={:?}; stderr={}",
                child.exit_code(),
                fs::read_to_string(root.join("starter.stderr")).unwrap()
            )
        });
        assert!(
            !part.is_empty(),
            "starter failed, exit={:?}, stderr={}",
            child.exit_code(),
            fs::read_to_string(root.join("starter.stderr")).unwrap()
        );
        bytes.extend(part);
        assert!(bytes.len() < 4096);
    }
    output.close(deadline(3)).unwrap();
    let receipt: Value = serde_json::from_slice(&bytes).unwrap();
    let service = ServiceProcess::observe(
        receipt["pid"].as_u64().unwrap() as u32,
        Path::new(FIXTURE),
        began,
        &current_user().unwrap(),
    )
    .unwrap();
    assert_eq!(receipt["creation_time"], service.identity().creation_time);
    assert!(
        !job.owns(service.identity()).unwrap(),
        "service still belongs to first client Job"
    );
    (job, input, service, began)
}

fn exchange(root: &Path, value: &str) {
    fs::write(root.join("request"), value).unwrap();
    let end = Instant::now() + Duration::from_secs(3);
    loop {
        if fs::read_to_string(root.join("response")).is_ok_and(|reply| reply == value) {
            return;
        }
        assert!(Instant::now() < end, "service did not handle owned request");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
#[ignore = "actual local WMI launch, owned native fixture only"]
fn service_survives_first_client_job_and_reclaims_its_child() {
    let root = tempfile::Builder::new()
        .prefix("native service Русский-日本-")
        .tempdir()
        .unwrap();
    let _stop = StopService(root.path());
    let (job, input, service, began) = start(root.path(), "serve");
    let report = wait_json(&root.path().join("endpoint.json"));
    assert_eq!(report["pid"], service.identity().pid);
    assert_eq!(
        report["arguments"],
        serde_json::json!(["Русский 日本", "with \"quotes\"\\", ""])
    );
    assert_eq!(report["sentinel"], "owned-private-значение-日本");
    assert!(report["ambient"].is_null());
    assert_eq!(
        std::path::PathBuf::from(report["cwd"].as_str().unwrap())
            .canonicalize()
            .unwrap(),
        root.path().canonicalize().unwrap()
    );
    assert_eq!(report["job"]["active_processes"], 2);
    assert_eq!(report["job"]["memory_limit_bytes"], 256 * 1024 * 1024);
    assert_eq!(report["job"]["cpu_rate"], 2500);
    assert_eq!(report["job"]["kill_on_close"], true);
    assert_eq!(report["job"]["handle_inheritable"], false);
    let leaf = ServiceProcess::observe(
        report["child"].as_u64().unwrap() as u32,
        Path::new(FIXTURE),
        began,
        &current_user().unwrap(),
    )
    .unwrap();
    exchange(root.path(), "first client");
    assert_eq!(
        job.terminate(130, Duration::from_secs(3))
            .unwrap()
            .active_processes,
        0
    );
    drop(input);
    assert!(service.is_running().unwrap());
    assert!(leaf.is_running().unwrap());
    exchange(root.path(), "second client 日本");
    // Deliberately mismatched user, executable and creation time confer no authority.
    assert!(
        ServiceProcess::observe(
            service.identity().pid,
            Path::new(FIXTURE),
            began,
            "wrong-user"
        )
        .is_err()
    );
    assert!(
        ServiceProcess::observe(
            service.identity().pid,
            &std::env::current_exe().unwrap(),
            began,
            &current_user().unwrap()
        )
        .is_err()
    );
    assert!(
        ServiceProcess::observe(
            service.identity().pid,
            Path::new(FIXTURE),
            creation_clock() + 1,
            &current_user().unwrap()
        )
        .is_err()
    );
    assert!(service.is_running().unwrap());
    fs::write(root.path().join("stop"), []).unwrap();
    assert!(service.wait_for_exit(deadline(5)).unwrap());
    assert_eq!(service.exit_code().unwrap(), Some(0));
    assert!(
        leaf.wait_for_exit(deadline(5)).unwrap(),
        "service-owned child survived owner exit"
    );
    let log = fs::read_to_string(root.path().join("service.log")).unwrap();
    assert!(log.contains("Owned service Unicode log: проверка 日本"));
    assert!(log.contains("Owned service stderr: проверка 日本"));
}

#[test]
#[ignore = "actual local WMI launch, owned native fixture only"]
fn service_without_readiness_exits_after_startup_deadline() {
    readiness_timeout("never-ready", true);
}

#[test]
#[ignore = "actual local WMI launch, owned native fixture only"]
fn locked_stderr_cannot_block_the_readiness_watchdog() {
    readiness_timeout("never-ready-locked-stderr", false);
}

fn readiness_timeout(mode: &str, require_log: bool) {
    let root = tempfile::tempdir().unwrap();
    let _stop = StopService(root.path());
    let (job, input, service, began) = start(root.path(), mode);
    let report = wait_json(&root.path().join("service.json"));
    let leaf = ServiceProcess::observe(
        report["child"].as_u64().unwrap() as u32,
        Path::new(FIXTURE),
        began,
        &current_user().unwrap(),
    )
    .unwrap();
    assert_eq!(
        job.terminate(130, Duration::from_secs(3))
            .unwrap()
            .active_processes,
        0
    );
    drop(input);
    assert!(service.wait_for_exit(deadline(12)).unwrap());
    assert_eq!(service.exit_code().unwrap(), Some(124));
    assert!(leaf.wait_for_exit(deadline(3)).unwrap());
    assert!(
        !require_log
            || fs::read_to_string(root.path().join("service.log"))
                .unwrap()
                .contains("startup deadline elapsed")
    );
    assert!(!root.path().join("endpoint.json").exists());
}

#[test]
#[ignore = "actual local WMI launch, owned native fixture only"]
fn crash_before_readiness_reclaims_the_service_child() {
    let root = tempfile::tempdir().unwrap();
    let _stop = StopService(root.path());
    let (job, input, service, began) = start(root.path(), "crash");
    let report = wait_json(&root.path().join("service.json"));
    let leaf = ServiceProcess::observe(
        report["child"].as_u64().unwrap() as u32,
        Path::new(FIXTURE),
        began,
        &current_user().unwrap(),
    )
    .unwrap();
    fs::write(root.path().join("crash-now"), []).unwrap();
    assert!(service.wait_for_exit(deadline(5)).unwrap());
    assert_eq!(
        service.exit_code().unwrap(),
        Some(101),
        "startup panic exit status was lost"
    );
    assert!(leaf.wait_for_exit(deadline(3)).unwrap());
    assert!(!root.path().join("endpoint.json").exists());
    assert!(
        fs::read_to_string(root.path().join("service.log"))
            .unwrap()
            .contains("owned startup crash before readiness")
    );
    assert_eq!(
        job.terminate(130, Duration::from_secs(3))
            .unwrap()
            .active_processes,
        0
    );
    drop(input);
}

#[test]
fn late_bootstrap_exits_before_service_work() {
    let root = tempfile::tempdir().unwrap();
    let mut command = CommandSpec::new(FIXTURE);
    command.current_dir = Some(root.path().into());
    command.args = vec![
        harness_core::process_service::RUN_ARGUMENT.into(),
        "1".into(),
        current_user().unwrap().into(),
        "serve".into(),
    ];
    let job = Job::new(Limits::default()).unwrap();
    let child = job.spawn(&command).unwrap();
    let outcome = job
        .wait(
            &child,
            deadline(5),
            &Cancellation::default(),
            Duration::from_secs(3),
        )
        .unwrap();
    assert_eq!(outcome.exit_code, 124);
    assert_eq!(outcome.job.active_processes, 0);
    assert!(!root.path().join("endpoint.json").exists());
}

#[test]
fn helper_rejects_closed_malformed_and_duplicate_input_without_starting_service() {
    for bytes in [
        b"".as_slice(),
        b"{",
        br#"{"directory":"private-sentinel","directory":"second"}"#,
    ] {
        let root = tempfile::tempdir().unwrap();
        let (stdin, write) = anonymous_pipe(4096).unwrap();
        let (read, stdout) = anonymous_pipe(4096).unwrap();
        let mut command = CommandSpec::new(FIXTURE);
        command.current_dir = Some(root.path().into());
        command
            .args
            .push(harness_core::process_service::CREATE_ARGUMENT.into());
        command.stdin = Some(stdin);
        command.stdout = Some(stdout);
        let job = Job::new(Limits::default()).unwrap();
        let child = job.spawn(&command).unwrap();
        drop(command);
        let cancel = Cancellation::default();
        let mut writer = CancellablePipe::writer(write, cancel.clone()).unwrap();
        if !bytes.is_empty() {
            writer.write_all(bytes, deadline(3), &cancel).unwrap();
        }
        writer.close(deadline(3)).unwrap();
        let mut reader = CancellablePipe::reader(read, cancel.clone()).unwrap();
        let mut output = Vec::new();
        loop {
            match reader.read(1024, deadline(3), &cancel) {
                Ok(part) => {
                    output.extend(part);
                    assert!(output.len() < 1024);
                }
                Err(PipeIoError::EndOfFile) => break,
                Err(error) => panic!("helper reply failed: {error}"),
            }
        }
        reader.close(deadline(3)).unwrap();
        let outcome = job
            .wait(&child, deadline(3), &cancel, Duration::from_secs(3))
            .unwrap();
        assert_eq!(outcome.exit_code, 2);
        assert_eq!(outcome.job.active_processes, 0);
        let result: Value = serde_json::from_slice(&output).unwrap();
        assert_eq!(result["error"], "invalid service request JSON");
        assert!(!String::from_utf8_lossy(&output).contains("private-sentinel"));
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
    }
}

#[test]
fn blocked_helper_is_bounded_and_cancelled_without_detaching_its_process() {
    for cancel_operation in [false, true] {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("hold-helper"), []).unwrap();
        let path = root.path().to_owned();
        let cancel = Cancellation::default();
        let call_cancel = cancel.clone();
        let environment = std::collections::BTreeMap::from([(
            "SystemRoot".into(),
            std::env::var("SystemRoot").unwrap(),
        )]);
        let began = creation_clock();
        let thread = std::thread::spawn(move || {
            harness_core::process_service::spawn(
                Path::new(FIXTURE),
                &path,
                vec!["serve".into()],
                environment,
                deadline(if cancel_operation { 20 } else { 2 }),
                &call_cancel,
            )
        });
        let marker = root.path().join("helper-started");
        let end = Instant::now() + Duration::from_secs(5);
        while !marker.exists() {
            assert!(Instant::now() < end, "owned helper never started");
            std::thread::sleep(Duration::from_millis(20));
        }
        let pid: u32 = fs::read_to_string(&marker).unwrap().parse().unwrap();
        let helper =
            ServiceProcess::observe(pid, Path::new(FIXTURE), began, &current_user().unwrap())
                .unwrap();
        if cancel_operation {
            cancel.cancel();
        }
        assert!(thread.join().unwrap().is_err());
        assert!(
            helper.wait_for_exit(deadline(3)).unwrap(),
            "blocked helper outlived owned cleanup"
        );
        assert!(!root.path().join("endpoint.json").exists());
    }
}
