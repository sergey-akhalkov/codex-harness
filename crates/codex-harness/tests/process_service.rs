#![cfg(windows)]
use harness_core::{
    cancellable_pipe::{CancellablePipe, PipeIoError, anonymous_pipe},
    process::{
        Cancellation, CommandSpec, Deadline, Job, Limits, SHARED_CPU_PERCENT, SharedCpuBudget,
    },
    process_service::{
        ServiceProcess, SharedCpuCoverage, creation_clock, current_user, shared_cpu_coverage,
    },
};
use serde_json::Value;
use std::{
    fs::{self, File},
    path::{Path, PathBuf},
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

/// Synthetic account allowance for one test. The client conveys it to the
/// service, and the test verifies membership against the same directory, so no
/// test touches the machine's real account budget.
fn account(root: &Path) -> PathBuf {
    root.join("cpu-account")
}

fn start(root: &Path, mode: &str) -> (Job, CancellablePipe, ServiceProcess, u64) {
    start_with_account(root, mode, Some(&account(root)))
}

/// `account` is the CPU allowance the service joins through the client's
/// environment. `None` withholds every account location, which is how a service
/// outside a usable allowance (an older installation or missing storage) is
/// reproduced.
fn start_with_account(
    root: &Path,
    mode: &str,
    account: Option<&Path>,
) -> (Job, CancellablePipe, ServiceProcess, u64) {
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
    match account {
        Some(path) => {
            command.env.insert(
                "CODEX_HARNESS_CPU_ACCOUNT".into(),
                Some(path.as_os_str().to_owned()),
            );
        }
        None => {
            command.env.insert("CODEX_HARNESS_CPU_ACCOUNT".into(), None);
            command.env.insert("LOCALAPPDATA".into(), None);
        }
    }
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

fn launch_ordinary_outside_caller_job(account: &Path, marker: &Path) -> u32 {
    let dir = marker.parent().unwrap();
    let script = dir.join("launch-ordinary.ps1");
    let exe = std::env::current_exe().unwrap();
    let system_root = std::env::var("SystemRoot").unwrap();
    let temp = std::env::var("TEMP").unwrap_or_else(|_| dir.display().to_string());
    let body = format!(
        "$startup = ([wmiclass]'Win32_ProcessStartup').CreateInstance()\r\n$startup.CreateFlags = 150995968\r\n$startup.ShowWindow = 0\r\n$startup.EnvironmentVariables = @('SystemRoot={system_root}','TEMP={temp}','HARNESS_ORDINARY_ACCOUNT={account}','HARNESS_ORDINARY_MARKER={marker}')\r\n$exe = '{exe}'\r\n$cmd = [char]34 + $exe + [char]34 + ' ordinary_chain_worker --exact --test-threads=1'\r\n$result = ([wmiclass]'Win32_Process').Create($cmd, '{dir}', $startup)\r\nif ($result.ReturnValue -ne 0) {{ exit $result.ReturnValue }}\r\nWrite-Output $result.ProcessId\r\n",
        system_root = system_root,
        temp = temp,
        account = account.display(),
        marker = marker.display(),
        exe = exe.display(),
        dir = dir.display(),
    );
    fs::write(&script, &body).unwrap();
    let output = std::process::Command::new("pwsh")
        .args(["-NoProfile", "-File"])
        .arg(&script)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "outside-job ordinary host failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .unwrap()
}
#[test]
fn ordinary_chain_worker() {
    let Ok(account) = std::env::var("HARNESS_ORDINARY_ACCOUNT") else {
        return;
    };
    let marker = PathBuf::from(std::env::var("HARNESS_ORDINARY_MARKER").unwrap());
    let done = marker.with_extension("done");
    let receipt_path = marker.with_extension("worker.json");
    let budget = SharedCpuBudget::acquire(Path::new(&account), SHARED_CPU_PERCENT).unwrap();
    let job = Job::new(Limits::default()).unwrap();
    let mut spec = CommandSpec::new(env!("CARGO_BIN_EXE_harness-process-fixture"));
    spec.args = vec!["hold".into(), marker.into_os_string()];
    let member = budget
        .spawn(&job, &spec)
        .expect("ordinary spawn outside the caller job");
    assert!(budget.contains(&member).unwrap());
    fs::write(
        &receipt_path,
        format!(
            "{{\"pid\":{},\"creation_time\":{},\"budget\":{},\"rate\":{}}}",
            member.identity().pid,
            member.identity().creation_time,
            serde_json::to_string(budget.name()).unwrap(),
            budget.snapshot().unwrap().cpu_rate
        ),
    )
    .unwrap();
    let until = Instant::now() + Duration::from_secs(40);
    while !done.exists() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(20));
    }
    let _ = job.terminate(0, Duration::from_secs(3));
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
    // The requested 25% of host is expressed against the verified 75% account
    // allowance (33.33% of the parent), so its host-relative meaning is kept.
    assert_eq!(report["job"]["cpu_rate"], 3333);
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

#[test]
fn admitted_service_and_backend_keep_the_account_allowance_after_their_client_exits() {
    let root = tempfile::Builder::new()
        .prefix("shared allowance Русский-日本-")
        .tempdir()
        .unwrap();
    let _stop = StopService(root.path());
    let budget = SharedCpuBudget::acquire(&account(root.path()), SHARED_CPU_PERCENT).unwrap();
    let snapshot = budget.snapshot().unwrap();
    assert_eq!(snapshot.cpu_rate, 7500);
    assert!(snapshot.cpu_hard_cap && !snapshot.kill_on_close);
    let (job, input, service, began) = start(root.path(), "serve");
    let report = wait_json(&root.path().join("endpoint.json"));
    assert_eq!(report["pid"], service.identity().pid);
    // The sibling start inherits no client job, so the service admits itself
    // from its own bootstrap before it does any payload work.
    assert!(
        service.in_shared_cpu_budget(&budget).unwrap(),
        "the sibling start must join the account allowance"
    );
    // Service-first: an ordinary-chain participant started after the service
    // joins the same allowance object, including its 7500 readback. This test
    // itself runs inside the machine account budget, so the ordinary spawn is
    // hosted by a sibling process that is not in that chain.
    let ordinary_marker = root.path().join("ordinary.json");
    let worker = launch_ordinary_outside_caller_job(&account(root.path()), &ordinary_marker);
    let receipt = wait_json(&ordinary_marker.with_extension("worker.json"));
    assert_eq!(receipt["budget"], budget.name());
    assert_eq!(receipt["rate"], 7500);
    let ordinary_member = ServiceProcess::observe(
        receipt["pid"].as_u64().unwrap() as u32,
        Path::new(env!("CARGO_BIN_EXE_harness-process-fixture")),
        0,
        &current_user().unwrap(),
    )
    .unwrap();
    assert!(
        ordinary_member.in_shared_cpu_budget(&budget).unwrap(),
        "an ordinary-chain participant must join the allowance the service anchored"
    );
    assert_eq!(budget.snapshot().unwrap().cpu_rate, 7500);
    // Its own lower ceiling is expressed against the verified parent rate, so
    // 25% of host stays 25% of host inside the 75% allowance.
    assert_eq!(report["job"]["cpu_rate"], 3333);
    assert_eq!(report["job"]["memory_limit_bytes"], 256 * 1024 * 1024);
    assert_eq!(report["job"]["kill_on_close"], true);
    let leaf = ServiceProcess::observe(
        report["child"].as_u64().unwrap() as u32,
        Path::new(FIXTURE),
        began,
        &current_user().unwrap(),
    )
    .unwrap();
    assert!(
        leaf.in_shared_cpu_budget(&budget).unwrap(),
        "a service descendant inherits the allowance"
    );
    let starter = fs::read_to_string(root.path().join("starter.stderr")).unwrap();
    assert!(
        !starter.contains("shared CPU allowance"),
        "an admitted service must not be reported degraded: {starter}"
    );
    // Real retained calls flow through the admitted service.
    exchange(root.path(), "first client");
    // One client exits: its Job is gone while the backend keeps working and
    // keeps its place in the one account allowance.
    assert_eq!(
        job.terminate(130, Duration::from_secs(3))
            .unwrap()
            .active_processes,
        0
    );
    drop(input);
    assert!(service.is_running().unwrap() && leaf.is_running().unwrap());
    assert!(service.in_shared_cpu_budget(&budget).unwrap());
    assert!(leaf.in_shared_cpu_budget(&budget).unwrap());
    assert_eq!(budget.snapshot().unwrap().cpu_rate, 7500);
    exchange(root.path(), "second client 日本");
    match shared_cpu_coverage(&service, Some(&account(root.path()))) {
        SharedCpuCoverage::Covered { job: name, rate } => {
            assert_eq!(rate, 7500);
            assert!(name.starts_with("CodingAgentsHarness.SharedCpu."), "{name}");
        }
        other => panic!("admitted service reported as {other:?}"),
    }
    fs::write(root.path().join("stop"), []).unwrap();
    assert!(service.wait_for_exit(deadline(8)).unwrap());
    fs::write(ordinary_marker.with_extension("done"), []).unwrap();
    let _worker = ServiceProcess::observe(worker, &std::env::current_exe().unwrap(), 0, &current_user().unwrap()).unwrap();
    assert!(
        leaf.wait_for_exit(deadline(5)).unwrap(),
        "service cleanup still reclaims its own child"
    );
}

#[test]
fn service_joins_allowance_already_anchored_by_ordinary_participant() {
    let root = tempfile::Builder::new()
        .prefix("anchored allowance Русский-日本-")
        .tempdir()
        .unwrap();
    let _stop = StopService(root.path());
    let budget = SharedCpuBudget::acquire(&account(root.path()), SHARED_CPU_PERCENT).unwrap();
    assert_eq!(budget.snapshot().unwrap().cpu_rate, 7500);
    let marker = root.path().join("ordinary.json");
    let ordinary_job = Job::new(Limits::default()).unwrap();
    let mut ordinary = CommandSpec::new(env!("CARGO_BIN_EXE_harness-process-fixture"));
    ordinary.args = vec!["hold".into(), marker.clone().into_os_string()];
    let member = budget.spawn(&ordinary_job, &ordinary).unwrap();
    assert_eq!(wait_json(&marker)["in_job"], true);
    assert!(budget.contains(&member).unwrap());
    let (job, _input, service, _began) = start(root.path(), "serve");
    assert!(
        service.in_shared_cpu_budget(&budget).unwrap(),
        "a sibling service must join the allowance an ordinary participant anchored"
    );
    assert_eq!(budget.snapshot().unwrap().cpu_rate, 7500);
    assert_eq!(
        SharedCpuBudget::acquire(&account(root.path()), SHARED_CPU_PERCENT)
            .unwrap()
            .name(),
        budget.name()
    );
    exchange(root.path(), "anchored client 日本");
    let starter = fs::read_to_string(root.path().join("starter.stderr")).unwrap();
    assert!(
        !starter.contains("shared CPU allowance"),
        "an admitted service must not be reported degraded: {starter}"
    );
    job.terminate(130, Duration::from_secs(3)).unwrap();
    assert!(service.is_running().unwrap());
    assert!(service.in_shared_cpu_budget(&budget).unwrap());
    ordinary_job.terminate(0, Duration::from_secs(3)).unwrap();
    fs::write(root.path().join("stop"), []).unwrap();
    assert!(service.wait_for_exit(deadline(8)).unwrap());
}

#[test]
fn service_without_account_storage_starts_degraded_with_a_visible_warning() {
    let root = tempfile::tempdir().unwrap();
    let _stop = StopService(root.path());
    let (job, input, service, _began) = start_with_account(root.path(), "serve", None);
    let report = wait_json(&root.path().join("endpoint.json"));
    // Fail-open: the requested payload is unchanged, and an unadmitted service
    // keeps its own host-relative ceiling instead of claiming the 75% one.
    assert_eq!(report["job"]["cpu_rate"], 2500);
    // Real work still reaches the retained service.
    exchange(root.path(), "degraded client");
    // The service reports the failed allowance on its own established channel.
    let log = fs::read_to_string(root.path().join("service.log")).unwrap();
    assert!(
        log.contains("outside the shared 75% CPU allowance"),
        "{log}"
    );
    assert!(log.contains("LOCALAPPDATA"), "{log}");
    // The starting client reports the same uncovered state, never a cap.
    let starter = fs::read_to_string(root.path().join("starter.stderr")).unwrap();
    assert!(
        starter.contains("shared CPU allowance unverified"),
        "{starter}"
    );
    assert!(starter.contains("LOCALAPPDATA is not set"), "{starter}");
    assert!(
        !starter.contains("inside the shared CPU allowance"),
        "{starter}"
    );
    // Independent verification: the process is not a member of a usable account
    // allowance established by the verifying client.
    let verifying = account(root.path());
    match shared_cpu_coverage(&service, Some(&verifying)) {
        SharedCpuCoverage::Unadmitted { cause } => {
            assert!(cause.contains("not a member"), "{cause}")
        }
        other => panic!("unadmitted service reported as {other:?}"),
    }
    // An account allowance established at another rate cannot verify membership:
    // it is reported unverified with its cause, never as capped.
    let conflicting = root.path().join("other-account");
    let _other = SharedCpuBudget::acquire(&conflicting, 1.0).unwrap();
    match shared_cpu_coverage(&service, Some(&conflicting)) {
        SharedCpuCoverage::Unverified { cause } => {
            assert!(
                cause.contains("already established at 1 percent"),
                "{cause}"
            )
        }
        other => panic!("conflicting allowance reported as {other:?}"),
    }
    assert!(service.is_running().unwrap());
    assert_eq!(
        job.terminate(130, Duration::from_secs(3))
            .unwrap()
            .active_processes,
        0
    );
    drop(input);
    assert!(
        service.is_running().unwrap(),
        "fail-open keeps the service available"
    );
    fs::write(root.path().join("stop"), []).unwrap();
    assert!(service.wait_for_exit(deadline(8)).unwrap());
}
