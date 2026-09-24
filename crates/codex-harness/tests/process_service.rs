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
    // The WMI sibling start inherits no client job, so the service admits itself
    // from its own bootstrap before it does any payload work.
    assert!(
        service.in_shared_cpu_budget(&budget).unwrap(),
        "the WMI sibling start must join the account allowance"
    );
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
    assert!(
        leaf.wait_for_exit(deadline(5)).unwrap(),
        "service cleanup still reclaims its own child"
    );
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

/// Kernel behavior probe for the service creation path: a payload created with
/// a creation-time job list belongs to exactly the listed jobs, so a bounded
/// helper can start a service outside its own job chain without a provider
/// container.
#[test]
#[ignore = "kernel behavior probe, run explicitly"]
fn creation_time_job_list_places_the_payload_outside_the_creators_job_chain() {
    let root = tempfile::tempdir().unwrap();
    let fixture = Path::new(env!("CARGO_BIN_EXE_harness-process-fixture"));
    let creator_job = Job::new(Limits::default()).unwrap();
    let marker = root.path().join("owner.json");
    let mut spec = CommandSpec::new(fixture);
    spec.current_dir = Some(root.path().into());
    spec.args = vec!["owner-running".into(), marker.clone().into_os_string()];
    let creator = creator_job.spawn(&spec).unwrap();
    let report = wait_json(&marker.with_extension("child.json"));
    let child = ServiceProcess::observe(
        report["pid"].as_u64().unwrap() as u32,
        fixture,
        0,
        &current_user().unwrap(),
    )
    .unwrap();
    assert_eq!(report["in_job"], true, "the fixture child has its own job");
    assert!(
        !creator_job.owns(child.identity()).unwrap(),
        "a creation-time job list must not inherit the creator's job chain"
    );
    child.terminate(0).unwrap();
    creator_job.terminate(0, Duration::from_secs(3)).unwrap();
    assert!(creator.wait_for_exit(Duration::from_secs(3)).unwrap());
}

/// Kernel behavior probe for the account allowance: while a member runs, does
/// the object keep its name after the last handle closes? The delivered path
/// lets a bounded helper establish the allowance and exit before the service
/// holds its own handle, so a released name would let a later caller create a
/// second allowance instead of rejoining this one.
#[test]
#[ignore = "kernel behavior probe, run explicitly"]
fn allowance_name_survives_the_last_handle_while_a_member_runs() {
    let root = tempfile::tempdir().unwrap();
    let holder = Job::new(Limits::default()).unwrap();
    let marker = root.path().join("member.json");
    let mut spec = CommandSpec::new(env!("CARGO_BIN_EXE_harness-process-fixture"));
    spec.args = vec!["hold".into(), marker.clone().into_os_string()];
    let member = {
        let budget = SharedCpuBudget::acquire(&account(root.path()), SHARED_CPU_PERCENT).unwrap();
        let member = budget.spawn(&holder, &spec).unwrap();
        drop(budget);
        member
    };
    assert_eq!(wait_json(&marker)["in_job"], true);
    let rejoined = SharedCpuBudget::acquire(&account(root.path()), SHARED_CPU_PERCENT).unwrap();
    let snapshot = rejoined.snapshot().unwrap();
    assert!(
        rejoined.contains(&member).unwrap(),
        "the allowance name was released with its last handle (members={})",
        snapshot.active_processes
    );
    holder.terminate(0, Duration::from_secs(3)).unwrap();
    assert!(member.wait_for_exit(Duration::from_secs(3)).unwrap());
}

/// Owned native oracle for the breakaway probe: independent Win32 declarations
/// rather than the implementation's job-query wrappers.
#[allow(dead_code)]
mod breakaway_probe {
    use std::ffi::c_void;

    pub const JOB_OBJECT_LIMIT_BREAKAWAY_OK: u32 = 0x0000_0800;
    pub const JOB_OBJECT_EXTENDED_LIMIT_INFORMATION: u32 = 9;
    pub const CREATE_SUSPENDED: u32 = 0x0000_0004;
    pub const CREATE_UNICODE_ENVIRONMENT: u32 = 0x0000_0400;
    pub const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
    pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    #[repr(C)]
    pub struct BasicLimitInformation {
        pub per_process_user_time_limit: i64,
        pub per_job_user_time_limit: i64,
        pub limit_flags: u32,
        pub minimum_working_set_size: usize,
        pub maximum_working_set_size: usize,
        pub active_process_limit: u32,
        pub affinity: usize,
        pub priority_class: u32,
        pub scheduling_class: u32,
    }

    #[repr(C)]
    pub struct IoCounters {
        pub read_operation_count: u64,
        pub write_operation_count: u64,
        pub other_operation_count: u64,
        pub read_transfer_count: u64,
        pub write_transfer_count: u64,
        pub other_transfer_count: u64,
    }

    #[repr(C)]
    pub struct ExtendedLimitInformation {
        pub basic: BasicLimitInformation,
        pub io: IoCounters,
        pub process_memory_limit: usize,
        pub job_memory_limit: usize,
        pub peak_process_memory_used: usize,
        pub peak_job_memory_used: usize,
    }

    #[repr(C)]
    pub struct StartupInfo {
        pub cb: u32,
        pub reserved: *mut u16,
        pub desktop: *mut u16,
        pub title: *mut u16,
        pub x: u32,
        pub y: u32,
        pub x_size: u32,
        pub y_size: u32,
        pub x_count_chars: u32,
        pub y_count_chars: u32,
        pub fill_attribute: u32,
        pub flags: u32,
        pub show_window: u16,
        pub reserved_units: u16,
        pub reserved_data: *mut u8,
        pub std_input: *mut c_void,
        pub std_output: *mut c_void,
        pub std_error: *mut c_void,
    }

    #[repr(C)]
    pub struct ProcessInformation {
        pub process: *mut c_void,
        pub thread: *mut c_void,
        pub process_id: u32,
        pub thread_id: u32,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        #[link_name = "GetCurrentProcess"]
        pub fn current_process() -> *mut c_void;
        #[link_name = "CreateJobObjectW"]
        pub fn create_job(attributes: *const c_void, name: *const u16) -> *mut c_void;
        #[link_name = "SetInformationJobObject"]
        pub fn set_job(job: *mut c_void, class: u32, value: *const c_void, bytes: u32) -> i32;
        #[link_name = "AssignProcessToJobObject"]
        pub fn assign_process(job: *mut c_void, process: *mut c_void) -> i32;
        #[link_name = "IsProcessInJob"]
        pub fn in_job(process: *mut c_void, job: *mut c_void, result: *mut i32) -> i32;
        #[link_name = "CreateProcessW"]
        #[allow(clippy::too_many_arguments)]
        pub fn create_process(
            application: *const u16,
            line: *mut u16,
            process_attributes: *const c_void,
            thread_attributes: *const c_void,
            inherit: i32,
            flags: u32,
            environment: *const c_void,
            directory: *const u16,
            startup: *const StartupInfo,
            info: *mut ProcessInformation,
        ) -> i32;
        #[link_name = "ResumeThread"]
        pub fn resume_thread(thread: *mut c_void) -> u32;
        #[link_name = "TerminateProcess"]
        pub fn terminate_process(process: *mut c_void, code: u32) -> i32;
        #[link_name = "WaitForSingleObject"]
        pub fn wait_for_process(handle: *mut c_void, milliseconds: u32) -> u32;
        #[link_name = "GetExitCodeProcess"]
        pub fn exit_code(process: *mut c_void, code: *mut u32) -> i32;
        #[link_name = "CloseHandle"]
        pub fn close_handle(handle: *mut c_void) -> i32;
    }
}

/// Owned native oracle for job-list creation: independent Win32 declarations.
#[allow(dead_code)]
mod job_list_probe {
    use std::ffi::c_void;

    pub const JOB_OBJECT_LIMIT_BREAKAWAY_OK: u32 = 0x0000_0800;
    pub const JOB_OBJECT_EXTENDED_LIMIT_INFORMATION: u32 = 9;
    pub const CREATE_SUSPENDED: u32 = 0x0000_0004;
    pub const CREATE_UNICODE_ENVIRONMENT: u32 = 0x0000_0400;
    pub const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
    pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    pub const EXTENDED_STARTUPINFO_PRESENT: u32 = 0x0008_0000;
    pub const PROC_THREAD_ATTRIBUTE_JOB_LIST: usize = 0x0002_000D;

    #[repr(C)]
    pub struct BasicLimitInformation {
        pub per_process_user_time_limit: i64,
        pub per_job_user_time_limit: i64,
        pub limit_flags: u32,
        pub minimum_working_set_size: usize,
        pub maximum_working_set_size: usize,
        pub active_process_limit: u32,
        pub affinity: usize,
        pub priority_class: u32,
        pub scheduling_class: u32,
    }

    #[repr(C)]
    pub struct IoCounters {
        pub read_operation_count: u64,
        pub write_operation_count: u64,
        pub other_operation_count: u64,
        pub read_transfer_count: u64,
        pub write_transfer_count: u64,
        pub other_transfer_count: u64,
    }

    #[repr(C)]
    pub struct ExtendedLimitInformation {
        pub basic: BasicLimitInformation,
        pub io: IoCounters,
        pub process_memory_limit: usize,
        pub job_memory_limit: usize,
        pub peak_process_memory_used: usize,
        pub peak_job_memory_used: usize,
    }

    #[repr(C)]
    pub struct StartupInfo {
        pub cb: u32,
        pub reserved: *mut u16,
        pub desktop: *mut u16,
        pub title: *mut u16,
        pub x: u32,
        pub y: u32,
        pub x_size: u32,
        pub y_size: u32,
        pub x_count_chars: u32,
        pub y_count_chars: u32,
        pub fill_attribute: u32,
        pub flags: u32,
        pub show_window: u16,
        pub reserved_units: u16,
        pub reserved_data: *mut u8,
        pub std_input: *mut c_void,
        pub std_output: *mut c_void,
        pub std_error: *mut c_void,
    }

    #[repr(C)]
    pub struct StartupInfoEx {
        pub info: StartupInfo,
        pub attribute_list: *mut c_void,
    }

    #[repr(C)]
    pub struct ProcessInformation {
        pub process: *mut c_void,
        pub thread: *mut c_void,
        pub process_id: u32,
        pub thread_id: u32,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        #[link_name = "GetCurrentProcess"]
        pub fn current_process() -> *mut c_void;
        #[link_name = "CreateJobObjectW"]
        pub fn create_job(attributes: *const c_void, name: *const u16) -> *mut c_void;
        #[link_name = "SetInformationJobObject"]
        pub fn set_job(job: *mut c_void, class: u32, value: *const c_void, bytes: u32) -> i32;
        #[link_name = "AssignProcessToJobObject"]
        pub fn assign_process(job: *mut c_void, process: *mut c_void) -> i32;
        #[link_name = "IsProcessInJob"]
        pub fn in_job(process: *mut c_void, job: *mut c_void, result: *mut i32) -> i32;
        #[link_name = "InitializeProcThreadAttributeList"]
        pub fn init_attributes(
            list: *mut c_void,
            count: u32,
            flags: u32,
            size: *mut usize,
        ) -> i32;
        #[link_name = "UpdateProcThreadAttribute"]
        #[allow(clippy::too_many_arguments)]
        pub fn update_attribute(
            list: *mut c_void,
            flags: u32,
            attribute: usize,
            value: *const c_void,
            size: usize,
            previous: *mut c_void,
            returned: *mut usize,
        ) -> i32;
        #[link_name = "DeleteProcThreadAttributeList"]
        pub fn delete_attributes(list: *mut c_void);
        #[link_name = "CreateProcessW"]
        #[allow(clippy::too_many_arguments)]
        pub fn create_process(
            application: *const u16,
            line: *mut u16,
            process_attributes: *const c_void,
            thread_attributes: *const c_void,
            inherit: i32,
            flags: u32,
            environment: *const c_void,
            directory: *const u16,
            startup: *const StartupInfo,
            info: *mut ProcessInformation,
        ) -> i32;
        #[link_name = "TerminateProcess"]
        pub fn terminate_process(process: *mut c_void, code: u32) -> i32;
        #[link_name = "CloseHandle"]
        pub fn close_handle(handle: *mut c_void) -> i32;
    }
}

/// Kernel measurement of where a creation-time job list and a breakaway child
/// land relative to the creator's chain. Independent Win32 oracle; run with
/// `--ignored`.
#[test]
#[ignore = "kernel behavior probe, run explicitly"]
fn kernel_job_list_and_breakaway_landing_rule() {
    use job_list_probe::*;
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;

    fn named_job(name: &str, flags: u32) -> *mut c_void {
        let name: Vec<u16> = name.encode_utf16().chain([0]).collect();
        let job = unsafe { create_job(std::ptr::null(), name.as_ptr()) };
        assert!(!job.is_null(), "probe job: {}", std::io::Error::last_os_error());
        let mut limits: ExtendedLimitInformation = unsafe { std::mem::zeroed() };
        limits.basic.limit_flags = flags;
        if flags != 0 {
            assert_ne!(
                unsafe {
                    set_job(
                        job,
                        JOB_OBJECT_EXTENDED_LIMIT_INFORMATION,
                        (&limits as *const ExtendedLimitInformation).cast(),
                        size_of::<ExtendedLimitInformation>() as u32,
                    )
                },
                0,
                "probe limits: {}",
                std::io::Error::last_os_error()
            );
        }
        job
    }

    fn member(process: *mut c_void, job: *mut c_void) -> bool {
        let mut value = 0;
        assert_ne!(unsafe { in_job(process, job, &mut value) }, 0);
        value != 0
    }

    struct Child {
        process: *mut c_void,
        thread: *mut c_void,
    }
    impl Drop for Child {
        fn drop(&mut self) {
            unsafe {
                terminate_process(self.process, 0);
                close_handle(self.thread);
                close_handle(self.process);
            }
        }
    }

    fn spawn(fixture: &Path, directory: &Path, flags: u32, jobs: &[*mut c_void]) -> Option<Child> {
        let mut list_size = 0usize;
        let count = if jobs.is_empty() { 0 } else { 1 };
        unsafe {
            init_attributes(std::ptr::null_mut(), count, 0, &mut list_size);
        }
        let mut list = vec![0u8; list_size.max(1)];
        assert_eq!(
            unsafe { init_attributes(list.as_mut_ptr().cast(), count, 0, &mut list_size) },
            1,
            "probe attribute list: {}",
            std::io::Error::last_os_error()
        );
        if !jobs.is_empty() {
            assert_ne!(
                unsafe {
                    update_attribute(
                        list.as_mut_ptr().cast(),
                        0,
                        PROC_THREAD_ATTRIBUTE_JOB_LIST,
                        jobs.as_ptr().cast(),
                        size_of_val(jobs),
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                    )
                },
                0,
                "probe job list attribute: {}",
                std::io::Error::last_os_error()
            );
        }
        let marker = directory.join("landing.json");
        let mut line: Vec<u16> = std::iter::once(34u16)
            .chain(fixture.as_os_str().encode_wide())
            .chain(" hold \"".encode_utf16())
            .chain(marker.as_os_str().encode_wide())
            .chain([34, 0])
            .collect();
        let application: Vec<u16> = fixture.as_os_str().encode_wide().chain([0]).collect();
        let directory_wide: Vec<u16> = directory.as_os_str().encode_wide().chain([0]).collect();
        let mut startup: StartupInfoEx = unsafe { std::mem::zeroed() };
        startup.info.cb = size_of::<StartupInfoEx>() as u32;
        startup.attribute_list = list.as_mut_ptr().cast();
        let mut info: ProcessInformation = unsafe { std::mem::zeroed() };
        let created = unsafe {
            create_process(
                application.as_ptr(),
                line.as_mut_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                flags
                    | CREATE_SUSPENDED
                    | CREATE_NO_WINDOW
                    | CREATE_UNICODE_ENVIRONMENT
                    | EXTENDED_STARTUPINFO_PRESENT,
                std::ptr::null(),
                directory_wide.as_ptr(),
                &startup.info,
                &mut info,
            )
        };
        unsafe { delete_attributes(list.as_mut_ptr().cast()) };
        if created == 0 {
            println!(
                "  creation refused: {}",
                std::io::Error::last_os_error()
            );
            return None;
        }
        Some(Child {
            process: info.process,
            thread: info.thread,
        })
    }

    let root = tempfile::tempdir().unwrap();
    let fixture = Path::new(env!("CARGO_BIN_EXE_harness-process-fixture"));
    let budget = named_job("CodingAgentsHarness.Probe.Landing.Budget", 0);
    let session = named_job("CodingAgentsHarness.Probe.Landing.Session", 0);
    let allowed = named_job("CodingAgentsHarness.Probe.Landing.Allowed", JOB_OBJECT_LIMIT_BREAKAWAY_OK);
    let fresh = named_job("CodingAgentsHarness.Probe.Landing.Fresh", 0);
    assert_ne!(unsafe { assign_process(budget, current_process()) }, 0);
    assert_ne!(unsafe { assign_process(session, current_process()) }, 0);
    assert_ne!(unsafe { assign_process(allowed, current_process()) }, 0);

    // Chain: [.., budget, session, allowed].
    let plain = spawn(fixture, root.path(), 0, &[]).expect("plain creation");
    println!(
        "plain: budget={} session={} allowed={} any={}",
        member(plain.process, budget),
        member(plain.process, session),
        member(plain.process, allowed),
        member(plain.process, std::ptr::null_mut()),
    );
    drop(plain);

    let listed = spawn(fixture, root.path(), 0, &[fresh]).expect("job list creation");
    println!(
        "job list [fresh]: fresh={} budget={} session={} allowed={} any={}",
        member(listed.process, fresh),
        member(listed.process, budget),
        member(listed.process, session),
        member(listed.process, allowed),
        member(listed.process, std::ptr::null_mut()),
    );
    drop(listed);

    let ancestor = spawn(fixture, root.path(), 0, &[budget]);
    match &ancestor {
        Some(child) => println!(
            "job list [budget]: budget={} session={} allowed={} any={}",
            member(child.process, budget),
            member(child.process, session),
            member(child.process, allowed),
            member(child.process, std::ptr::null_mut()),
        ),
        None => println!("job list [budget]: refused"),
    }
    drop(ancestor);

    let middle = spawn(fixture, root.path(), 0, &[session]);
    match &middle {
        Some(child) => println!(
            "job list [session]: budget={} session={} allowed={} any={}",
            member(child.process, budget),
            member(child.process, session),
            member(child.process, allowed),
            member(child.process, std::ptr::null_mut()),
        ),
        None => println!("job list [session]: refused"),
    }
    drop(middle);

    let escaped = spawn(
        fixture,
        root.path(),
        job_list_probe::CREATE_BREAKAWAY_FROM_JOB,
        &[],
    );
    match &escaped {
        Some(child) => println!(
            "breakaway: budget={} session={} allowed={} any={}",
            member(child.process, budget),
            member(child.process, session),
            member(child.process, allowed),
            member(child.process, std::ptr::null_mut()),
        ),
        None => println!("breakaway: refused"),
    }
    drop(escaped);

    // Membership semantics: a job that is not in the process's chain, but is
    // in the same hierarchy, must answer false for that process.
    let outer = named_job("CodingAgentsHarness.Probe.Landing.Outer", 0);
    let inner = named_job("CodingAgentsHarness.Probe.Landing.Inner", 0);
    assert_ne!(unsafe { assign_process(outer, current_process()) }, 0);
    let sibling_free = spawn(fixture, root.path(), 0, &[]).expect("outer child");
    assert_ne!(unsafe { assign_process(inner, current_process()) }, 0);
    println!(
        "sibling semantics: outer={} inner={}",
        member(sibling_free.process, outer),
        member(sibling_free.process, inner),
    );
    drop(sibling_free);

    // Ordered list whose root is an ancestor of the creator, plus a fresh job.
    let deep = spawn(fixture, root.path(), 0, &[budget, fresh]);
    match &deep {
        Some(child) => println!(
            "job list [budget, fresh]: fresh={} budget={} session={} allowed={} any={}",
            member(child.process, fresh),
            member(child.process, budget),
            member(child.process, session),
            member(child.process, allowed),
            member(child.process, std::ptr::null_mut()),
        ),
        None => println!("job list [budget, fresh]: refused"),
    }
    drop(deep);

    // Ordered list whose root is the creator's own innermost job.
    let own = spawn(fixture, root.path(), 0, &[inner, fresh]);
    match &own {
        Some(child) => println!(
            "job list [inner, fresh]: fresh={} inner={} outer={} any={}",
            member(child.process, fresh),
            member(child.process, inner),
            member(child.process, outer),
            member(child.process, std::ptr::null_mut()),
        ),
        None => println!("job list [inner, fresh]: refused"),
    }
    drop(own);
    unsafe {
        close_handle(inner);
        close_handle(outer);
    }

    unsafe {
        close_handle(fresh);
        close_handle(allowed);
        close_handle(session);
        close_handle(budget);
    }
}

/// Kernel behavior probe for the delivered sibling creation path: a payload
/// created with `CREATE_BREAKAWAY_FROM_JOB` by a member of a breakaway-enabled
/// job that is itself nested inside a chain without that limit (here the
/// shared heavy-command job) must belong to no job at all. Only a job-free
/// payload can join the account allowance unconditionally in either anchoring
/// order.
#[test]
#[ignore = "kernel behavior probe, run explicitly"]
fn breakaway_payload_escapes_the_whole_nested_chain() {
    use breakaway_probe::*;
    use std::os::windows::ffi::OsStrExt;
    let root = tempfile::tempdir().unwrap();
    let marker = root.path().join("payload.json");
    let fixture = Path::new(env!("CARGO_BIN_EXE_harness-process-fixture"));
    let name: Vec<u16> = "CodingAgentsHarness.Probe.Breakaway"
        .encode_utf16()
        .chain([0])
        .collect();
    let job = unsafe { create_job(std::ptr::null(), name.as_ptr()) };
    assert!(
        !job.is_null(),
        "probe job creation: {}",
        std::io::Error::last_os_error()
    );
    let mut limits: ExtendedLimitInformation = unsafe { std::mem::zeroed() };
    limits.basic.limit_flags = JOB_OBJECT_LIMIT_BREAKAWAY_OK;
    assert_ne!(
        unsafe {
            set_job(
                job,
                JOB_OBJECT_EXTENDED_LIMIT_INFORMATION,
                (&limits as *const ExtendedLimitInformation).cast(),
                size_of::<ExtendedLimitInformation>() as u32,
            )
        },
        0,
        "probe job limits: {}",
        std::io::Error::last_os_error()
    );
    assert_ne!(
        unsafe { assign_process(job, current_process()) },
        0,
        "probe membership: {}",
        std::io::Error::last_os_error()
    );
    let mut member = 0;
    assert_ne!(unsafe { in_job(current_process(), job, &mut member) }, 0);
    assert_ne!(member, 0, "the probe process must be in its breakaway job");

    let mut line: Vec<u16> = std::iter::once(34u16)
        .chain(fixture.as_os_str().encode_wide())
        .chain(" hold \"".encode_utf16())
        .chain(marker.as_os_str().encode_wide())
        .chain([34, 0])
        .collect();
    let application: Vec<u16> = fixture.as_os_str().encode_wide().chain([0]).collect();
    let directory: Vec<u16> = root.path().as_os_str().encode_wide().chain([0]).collect();
    let mut startup: StartupInfo = unsafe { std::mem::zeroed() };
    startup.cb = size_of::<StartupInfo>() as u32;
    let mut info: ProcessInformation = unsafe { std::mem::zeroed() };
    let created = unsafe {
        create_process(
            application.as_ptr(),
            line.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            CREATE_BREAKAWAY_FROM_JOB
                | CREATE_NO_WINDOW
                | CREATE_UNICODE_ENVIRONMENT
                | CREATE_SUSPENDED,
            std::ptr::null(),
            directory.as_ptr(),
            &startup,
            &mut info,
        )
    };
    assert_ne!(
        created,
        0,
        "breakaway creation was refused: {}",
        std::io::Error::last_os_error()
    );
    let mut inside = 1;
    assert_ne!(unsafe { in_job(info.process, job, &mut inside) }, 0);
    assert_eq!(
        inside, 0,
        "breakaway kept the payload inside the requesting job"
    );
    // ResumeThread reports the previous suspend count, so a successful resume
    // returns the one suspension this probe requested.
    assert_eq!(unsafe { resume_thread(info.thread) }, 1);
    let report = wait_json(&marker);
    let any_job = report["in_job"].as_bool().unwrap();
    unsafe {
        terminate_process(info.process, 0);
        close_handle(info.thread);
        close_handle(info.process);
        close_handle(job);
    }
    assert!(
        !any_job,
        "the breakaway payload is still inside an inherited job"
    );
}

/// Temporary diagnostic: what does the kernel actually do with a
/// CREATE_BREAKAWAY_FROM_JOB payload created by a member of a breakaway-enabled
/// job nested inside a chain without that limit?
#[test]
#[ignore = "kernel behavior probe, run explicitly"]
fn breakaway_diagnostic() {
    use breakaway_probe::*;
    use std::os::windows::ffi::OsStrExt;
    let fixture = Path::new(env!("CARGO_BIN_EXE_harness-process-fixture"));
    let name: Vec<u16> = "CodingAgentsHarness.Probe.Breakaway.Diag"
        .encode_utf16()
        .chain([0])
        .collect();
    let job = unsafe { create_job(std::ptr::null(), name.as_ptr()) };
    assert!(!job.is_null(), "probe job: {}", std::io::Error::last_os_error());
    let mut limits: ExtendedLimitInformation = unsafe { std::mem::zeroed() };
    limits.basic.limit_flags = JOB_OBJECT_LIMIT_BREAKAWAY_OK;
    assert_ne!(
        unsafe {
            set_job(
                job,
                JOB_OBJECT_EXTENDED_LIMIT_INFORMATION,
                (&limits as *const ExtendedLimitInformation).cast(),
                size_of::<ExtendedLimitInformation>() as u32,
            )
        },
        0
    );
    assert_ne!(unsafe { assign_process(job, current_process()) }, 0);
    for (case, flags) in [
        ("plain", CREATE_NO_WINDOW | CREATE_UNICODE_ENVIRONMENT),
        (
            "breakaway",
            CREATE_BREAKAWAY_FROM_JOB | CREATE_NO_WINDOW | CREATE_UNICODE_ENVIRONMENT,
        ),
    ] {
        let root = tempfile::tempdir().unwrap();
        let marker = root.path().join(format!("{case}.json"));
        let mut line: Vec<u16> = std::iter::once(34u16)
            .chain(fixture.as_os_str().encode_wide())
            .chain(" report \"".encode_utf16())
            .chain(marker.as_os_str().encode_wide())
            .chain([34, 0])
            .collect();
        let application: Vec<u16> = fixture.as_os_str().encode_wide().chain([0]).collect();
        let directory: Vec<u16> = root.path().as_os_str().encode_wide().chain([0]).collect();
        let mut startup: StartupInfo = unsafe { std::mem::zeroed() };
        startup.cb = size_of::<StartupInfo>() as u32;
        let mut info: ProcessInformation = unsafe { std::mem::zeroed() };
        let created = unsafe {
            create_process(
                application.as_ptr(),
                line.as_mut_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                flags,
                std::ptr::null(),
                directory.as_ptr(),
                &startup,
                &mut info,
            )
        };
        if created == 0 {
            println!("{case}: create refused: {}", std::io::Error::last_os_error());
            continue;
        }
        let mut in_probe = 0;
        let mut in_any = 0;
        unsafe {
            in_job(info.process, job, &mut in_probe);
            in_job(info.process, std::ptr::null_mut(), &mut in_any);
        }
        let waited = unsafe { wait_for_process(info.process, 8000) };
        let mut code = 0u32;
        unsafe { exit_code(info.process, &mut code) };
        println!(
            "{case}: probe_job={in_probe} any_job={in_any} wait={waited} exit={code} marker={:?} body={:?}",
            marker.exists(),
            std::fs::read_to_string(&marker).ok()
        );
        unsafe {
            close_handle(info.thread);
            close_handle(info.process);
        }
    }
    unsafe { close_handle(job) };
}
