#![cfg(windows)]
use harness_core::{
    broker_endpoint::{self, Endpoint, Observation},
    broker_http, broker_launch, broker_rpc,
    broker_state::BrokerRoot,
    cancellable_pipe::{CancellablePipe, anonymous_pipe},
    process::{
        Cancellation, CommandSpec, Deadline, Job, Limits, OwnedProcess, ProcessIdentity,
        SHARED_CPU_PERCENT, SharedCpuBudget,
    },
    process_service::{ServiceProcess, SharedCpuCoverage, current_user, shared_cpu_coverage},
};
use serde_json::{Value, json};
use std::{
    fs::{self, File},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const FIXTURE: &str = env!("CARGO_BIN_EXE_harness-broker-fixture");
const SOURCE_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const SOURCE_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const UNICODE: &str = "проверка 日本";
const IDLE_MS: &str = "2500";
const IDLE_SHORT_MS: &str = "400";

fn deadline(seconds: u64) -> Deadline {
    Deadline::after(Duration::from_secs(seconds)).unwrap()
}

fn fixture_path() -> PathBuf {
    PathBuf::from(FIXTURE)
}

/// Synthetic account allowance directory for one test root; see
/// `Prepared::account` and `Client::start`.
fn account(root: &Path) -> PathBuf {
    root.join("cpu-account")
}

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

    fn path(&self) -> &Path {
        self.root().path()
    }

    /// Synthetic account allowance for this test: the clients convey it to the
    /// service, and the test verifies membership against the same directory, so
    /// the machine's real account budget is never touched.
    fn account(&self) -> PathBuf {
        self.path().join("cpu-account")
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
    input: Option<CancellablePipe>,
    child: OwnedProcess,
    output: Option<CancellablePipe>,
    stderr: PathBuf,
}

impl Client {
    /// `account` is the CPU allowance the client (and through it the service)
    /// joins. `None` withholds every account location, which is how a service
    /// outside a usable allowance is reproduced.
    fn start_with_account(
        root: &Path,
        args: Vec<std::ffi::OsString>,
        stderr_name: &str,
        account: Option<&Path>,
    ) -> Self {
        let (stdin, write) = anonymous_pipe(4096).unwrap();
        let (read, stdout) = anonymous_pipe(4096).unwrap();
        let stderr = root.join(stderr_name);
        let mut command = CommandSpec::new(FIXTURE);
        command.current_dir = Some(root.into());
        command.args = args;
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
        command.stderr = Some(File::create(&stderr).unwrap());
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
            input: Some(CancellablePipe::writer(write, cancel.clone()).unwrap()),
            child,
            output: Some(CancellablePipe::reader(read, cancel).unwrap()),
            stderr,
        }
    }

    fn connect(
        root: &Path,
        source: &str,
        idle_ms: &str,
        operation: &str,
        payload: Option<&str>,
        deadline_ms: Option<&str>,
        stderr_name: &str,
    ) -> Self {
        Self::connect_in_account(
            root,
            source,
            idle_ms,
            operation,
            payload,
            deadline_ms,
            stderr_name,
            Some(&account(root)),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn connect_in_account(
        root: &Path,
        source: &str,
        idle_ms: &str,
        operation: &str,
        payload: Option<&str>,
        deadline_ms: Option<&str>,
        stderr_name: &str,
        account: Option<&Path>,
    ) -> Self {
        let mut args = vec![
            "--client".into(),
            root.into(),
            source.into(),
            idle_ms.into(),
            operation.into(),
        ];
        if let Some(payload) = payload {
            args.push(payload.into());
        } else if deadline_ms.is_some() {
            args.push("{}".into());
        }
        if let Some(deadline_ms) = deadline_ms {
            args.push(deadline_ms.into());
        }
        Self::start_with_account(root, args, stderr_name, account)
    }

    fn read_json(&mut self, seconds: u64) -> Value {
        let cancel = Cancellation::default();
        let end = deadline(seconds);
        let mut bytes = Vec::new();
        while !bytes.ends_with(b"\n") {
            let part = self
                .output
                .as_mut()
                .unwrap()
                .read(1024, end, &cancel)
                .unwrap_or_else(|error| {
                    panic!(
                        "client pipe: {error}; log={}; exit={:?}; stderr={}",
                        self.stderr.display(),
                        self.child.exit_code(),
                        fs::read_to_string(&self.stderr).unwrap_or_default()
                    )
                });
            assert!(
                !part.is_empty(),
                "client failed, exit={:?}, stderr={}",
                self.child.exit_code(),
                fs::read_to_string(&self.stderr).unwrap_or_default()
            );
            bytes.extend(part);
            assert!(bytes.len() < 4096);
        }
        serde_json::from_slice(&bytes).unwrap()
    }

    fn terminate(mut self) {
        if let Some(job) = self.job.take() {
            assert_eq!(
                job.terminate(130, Duration::from_secs(3))
                    .unwrap()
                    .active_processes,
                0
            );
        }
        if let Some(input) = self.input.take() {
            let _ = input.close(deadline(3));
        }
        if let Some(output) = self.output.take() {
            let _ = output.close(deadline(3));
        }
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        if let Some(job) = self.job.take() {
            let _ = job.terminate(130, Duration::from_secs(3));
        }
        if let Some(input) = self.input.take() {
            let _ = input.close(deadline(3));
        }
        if let Some(output) = self.output.take() {
            let _ = output.close(deadline(3));
        }
    }
}

fn observe_service(pid: u32, creation_time: u64) -> ServiceProcess {
    ServiceProcess::inspect(
        ProcessIdentity { pid, creation_time },
        &fixture_path(),
        &current_user().unwrap(),
    )
    .unwrap()
    .expect("owned broker identity missing")
}

fn wait_exit(process: &ServiceProcess, seconds: u64) {
    assert!(
        process.wait_for_exit(deadline(seconds)).unwrap(),
        "owned process still running"
    );
}

fn secret_free(value: &Value) {
    let text = value.to_string();
    assert!(!text.to_ascii_lowercase().contains("bearer"));
    assert!(
        !text.contains("\"token\""),
        "status/result leaked a token field"
    );
}

fn endpoint_gone(root: &BrokerRoot) {
    match broker_endpoint::observe(root).unwrap() {
        Observation::Absent => {}
        Observation::Stale => panic!("clean shutdown left a stale endpoint"),
        Observation::Ready { .. } => panic!("broker endpoint still has a live owner"),
    }
}

fn instance_free(root: &BrokerRoot) {
    let lease = root.try_instance().unwrap();
    assert!(lease.is_some(), "instance lock still held after owned exit");
    drop(lease);
}

fn assert_no_secret_files(root: &Path) {
    if let Ok(log) = fs::read_to_string(root.join("service.log")) {
        let lower = log.to_ascii_lowercase();
        assert!(!lower.contains("bearer"));
        assert!(!log.contains("token"));
    }
}

fn start_reserved_broker(prepared: &Prepared) -> (Client, Endpoint, ServiceProcess) {
    let mut client = Client::connect(
        prepared.path(),
        SOURCE_A,
        IDLE_MS,
        "echo",
        None,
        None,
        "reserved-start.stderr",
    );
    let _started = client.read_json(40);
    let Observation::Ready { endpoint, owner } = broker_endpoint::observe(prepared.root()).unwrap()
    else {
        panic!("ready endpoint missing")
    };
    (client, endpoint, owner)
}

fn control(endpoint: &Endpoint, operation: &str, payload: &Value) -> Value {
    broker_http::exchange(
        endpoint.port,
        endpoint.token(),
        operation,
        payload,
        deadline(2),
        &Cancellation::default(),
    )
    .unwrap()
}

fn wait_marker(path: &Path) {
    let until = deadline(3);
    while !path.is_file() && !until.expired() {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        path.is_file(),
        "owned fixture marker missing: {}",
        path.display()
    );
}

fn wait_json_file(path: &Path) -> Value {
    let until = Instant::now() + Duration::from_secs(20);
    loop {
        if let Ok(bytes) = fs::read(path)
            && let Ok(value) = serde_json::from_slice::<Value>(&bytes)
        {
            return value;
        }
        assert!(Instant::now() < until, "missing {}", path.display());
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn launch_ordinary_outside_caller_job(account: &Path, marker: &Path) -> u32 {
    let dir = marker.parent().unwrap();
    let script = dir.join("launch-ordinary.ps1");
    let exe = std::env::current_exe().unwrap();
    let body = format!(
        "$startup = ([wmiclass]'Win32_ProcessStartup').CreateInstance()\r\n$startup.CreateFlags = 150995968\r\n$startup.ShowWindow = 0\r\n$startup.EnvironmentVariables = @('SystemRoot={system_root}','TEMP={temp}','HARNESS_ORDINARY_ACCOUNT={account}','HARNESS_ORDINARY_MARKER={marker}')\r\n$exe = '{exe}'\r\n$cmd = [char]34 + $exe + [char]34 + ' ordinary_chain_worker --exact --test-threads=1'\r\n$result = ([wmiclass]'Win32_Process').Create($cmd, '{dir}', $startup)\r\nif ($result.ReturnValue -ne 0) {{ exit $result.ReturnValue }}\r\nWrite-Output $result.ProcessId\r\n",
        system_root = std::env::var("SystemRoot").unwrap(),
        temp = std::env::var("TEMP").unwrap_or_else(|_| dir.display().to_string()),
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
#[ignore = "creates owned Windows WMI service fixtures"]
fn reservation_prevents_late_and_duplicate_invocation() {
    let prepared = Prepared::new();
    let (client, endpoint, owner) = start_reserved_broker(&prepared);
    let key = control(&endpoint, "request/reserve", &json!({}));
    assert_eq!(
        control(&endpoint, "request/cancel", &key)["reclaimed"],
        true
    );
    let invocation = json!({"key":key["key"],"operation":"echo","payload":{"text":UNICODE}});
    let invoke = |value: &Value| {
        broker_http::exchange(
            endpoint.port,
            endpoint.token(),
            "request/invoke",
            value,
            deadline(2),
            &Cancellation::default(),
        )
    };
    assert!(invoke(&invocation).is_err());
    assert_eq!(
        control(&endpoint, "request/release", &key)["released"],
        true
    );
    assert!(invoke(&invocation).is_err());
    assert_eq!(
        control(&endpoint, "request/cancel", &key)["reclaimed"],
        true
    );
    let key = control(&endpoint, "request/reserve", &json!({}));
    let invocation = json!({"key":key["key"],"operation":"echo","payload":{"text":UNICODE}});
    assert_eq!(invoke(&invocation).unwrap()["payload"]["text"], UNICODE);
    assert!(invoke(&invocation).is_err());
    assert_eq!(
        control(&endpoint, "request/cancel", &key)["reclaimed"],
        true
    );
    assert_eq!(
        control(&endpoint, "request/release", &key)["released"],
        true
    );
    assert!(owner.is_running().unwrap());
    assert_eq!(control(&endpoint, "status", &json!({}))["active"], 0);
    client.terminate();
}

#[test]
#[ignore = "creates owned Windows WMI service fixtures"]
fn cancelled_rpc_waits_for_confirmed_cleanup_before_returning() {
    let prepared = Prepared::new();
    let (client, endpoint, owner) = start_reserved_broker(&prepared);
    let cancel = Cancellation::default();
    std::thread::scope(|scope| {
        let request = scope.spawn(|| {
            broker_rpc::invoke(&endpoint, "slow-cleanup", &json!({}), deadline(8), &cancel)
        });
        wait_marker(&prepared.path().join("slow-admitted"));
        cancel.cancel();
        wait_marker(&prepared.path().join("cleanup-pending"));
        // The backend has observed cancellation but still owns request resources.
        std::thread::sleep(Duration::from_millis(100));
        assert!(
            !request.is_finished(),
            "client acknowledged cancellation before cleanup"
        );
        assert_eq!(control(&endpoint, "status", &json!({}))["active"], 1);
        fs::write(prepared.path().join("cleanup-continue"), []).unwrap();
        assert_eq!(request.join().unwrap().unwrap()["isError"], true);
    });
    assert!(prepared.path().join("cleanup-finished").is_file());
    assert!(owner.is_running().unwrap());
    assert_eq!(
        broker_rpc::invoke(
            &endpoint,
            "echo",
            &json!({"text":UNICODE}),
            deadline(2),
            &Cancellation::default()
        )
        .unwrap()["payload"]["text"],
        UNICODE
    );
    client.terminate();
}

#[test]
#[ignore = "creates owned Windows WMI service fixtures"]
fn expired_rpc_reclaims_cooperative_work_and_preserves_owner() {
    let prepared = Prepared::new();
    let (client, endpoint, owner) = start_reserved_broker(&prepared);
    let result = broker_rpc::invoke(
        &endpoint,
        "slow",
        &json!({}),
        Deadline::after(Duration::from_millis(300)).unwrap(),
        &Cancellation::default(),
    )
    .unwrap();
    assert_eq!(result["isError"], true);
    assert_eq!(control(&endpoint, "status", &json!({}))["active"], 0);
    assert!(owner.is_running().unwrap());
    client.terminate();
}

#[test]
#[ignore = "creates owned Windows WMI service fixtures"]
fn cancelled_rpc_with_unconfirmed_cleanup_fails_and_reclaims_service_job() {
    let prepared = Prepared::new();
    let (client, endpoint, owner) = start_reserved_broker(&prepared);
    let status = control(&endpoint, "status", &json!({}));
    let child_pid = status["backend"]["child"].as_u64().unwrap() as u32;
    let leaf =
        ServiceProcess::observe(child_pid, &fixture_path(), 0, &current_user().unwrap()).unwrap();
    let cancel = Cancellation::default();
    std::thread::scope(|scope| {
        let request =
            scope.spawn(|| broker_rpc::invoke(&endpoint, "hang", &json!({}), deadline(8), &cancel));
        wait_marker(&prepared.path().join("hang-admitted"));
        cancel.cancel();
        assert!(
            request.join().unwrap().is_err(),
            "unconfirmed cleanup became a normal result"
        );
    });
    wait_exit(&owner, 3);
    assert_eq!(owner.exit_code().unwrap(), Some(2));
    wait_exit(&leaf, 3);
    client.terminate();
}

#[test]
#[ignore = "creates owned Windows WMI service fixtures"]
fn two_clients_join_same_pid_and_child_after_first_job_exit() {
    let prepared = Prepared::new();
    let payload = json!({ "text": UNICODE }).to_string();
    let mut first = Client::connect(
        prepared.path(),
        SOURCE_A,
        IDLE_MS,
        "echo",
        Some(&payload),
        None,
        "first.stderr",
    );
    let first_json = first.read_json(40);
    secret_free(&first_json);
    let identity = &first_json["identity"];
    let pid = identity["pid"].as_u64().unwrap() as u32;
    let creation_time = identity["creation_time"].as_u64().unwrap();
    let service = observe_service(pid, creation_time);
    assert!(
        !first
            .job
            .as_ref()
            .unwrap()
            .owns(service.identity())
            .unwrap()
    );
    assert_eq!(first_json["result"]["payload"]["text"], UNICODE);
    let Observation::Ready { endpoint, owner } = broker_endpoint::observe(prepared.root()).unwrap()
    else {
        panic!("ready endpoint missing");
    };
    assert_eq!(owner.identity().pid, pid);
    let cancel = Cancellation::default();
    let status = broker_http::exchange(
        endpoint.port,
        endpoint.token(),
        "status",
        &json!({}),
        deadline(5),
        &cancel,
    )
    .unwrap();
    secret_free(&status);
    assert_eq!(status["pid"], pid);
    assert_eq!(status["creation_time"], creation_time);
    assert_eq!(status["source"], SOURCE_A);
    assert_eq!(status["retiring"], false);
    assert_eq!(
        status["backend"]["job"]["memory_limit_bytes"],
        256 * 1024 * 1024
    );
    // The requested 25% of host is expressed against the verified 75% account
    // allowance (33.33% of the parent), so its host-relative meaning is kept.
    assert_eq!(status["backend"]["job"]["cpu_rate"], 3333);
    let child_pid = status["backend"]["child"].as_u64().unwrap() as u32;
    let leaf =
        ServiceProcess::observe(child_pid, &fixture_path(), 0, &current_user().unwrap()).unwrap();
    first.terminate();
    assert!(service.is_running().unwrap());
    assert!(leaf.is_running().unwrap());
    let mut second = Client::connect(
        prepared.path(),
        SOURCE_A,
        IDLE_MS,
        "echo",
        Some(&json!({ "text": "second 日本" }).to_string()),
        None,
        "second.stderr",
    );
    let second_json = second.read_json(20);
    secret_free(&second_json);
    assert_eq!(second_json["identity"]["pid"], pid);
    assert_eq!(second_json["identity"]["creation_time"], creation_time);
    assert_eq!(second_json["result"]["payload"]["text"], "second 日本");
    let joined = broker_http::exchange(
        endpoint.port,
        endpoint.token(),
        "status",
        &json!({}),
        deadline(5),
        &cancel,
    )
    .unwrap();
    assert_eq!(joined["backend"]["child"], child_pid);
    second.terminate();
    let log = fs::read_to_string(prepared.path().join("service.log")).unwrap();
    assert!(log.contains("Owned broker Unicode log: проверка 日本"));
    assert_no_secret_files(prepared.path());
    let _ = owner;
}

#[test]
#[ignore = "creates owned Windows WMI service fixtures"]
fn wrong_bearer_is_rejected_and_source_mismatch_preserves_owner() {
    let prepared = Prepared::new();
    let mut first = Client::connect(
        prepared.path(),
        SOURCE_A,
        IDLE_MS,
        "echo",
        Some(&json!({ "ok": true }).to_string()),
        None,
        "auth.stderr",
    );
    let first_json = first.read_json(40);
    let pid = first_json["identity"]["pid"].as_u64().unwrap() as u32;
    let creation_time = first_json["identity"]["creation_time"].as_u64().unwrap();
    let service = observe_service(pid, creation_time);
    let Observation::Ready {
        endpoint,
        owner: _owner,
    } = broker_endpoint::observe(prepared.root()).unwrap()
    else {
        panic!("ready endpoint missing");
    };
    let cancel = Cancellation::default();
    let wrong = "0".repeat(64);
    let rejected = broker_http::exchange(
        endpoint.port,
        &wrong,
        "echo",
        &json!({}),
        deadline(3),
        &cancel,
    );
    assert!(rejected.is_err(), "wrong bearer was accepted");
    assert!(service.is_running().unwrap());
    let status = broker_http::exchange(
        endpoint.port,
        endpoint.token(),
        "status",
        &json!({}),
        deadline(3),
        &cancel,
    )
    .unwrap();
    assert_eq!(status["pid"], pid);
    let mismatch = broker_launch::ensure(
        prepared.root(),
        &fixture_path(),
        vec![SOURCE_B.into(), IDLE_MS.into()],
        {
            let mut environment = std::collections::BTreeMap::new();
            for name in ["SystemRoot", "TEMP", "TMP"] {
                if let Ok(value) = std::env::var(name) {
                    environment.insert(name.into(), value);
                }
            }
            environment
        },
        SOURCE_B,
        deadline(5),
        &cancel,
    );
    let error = mismatch.unwrap_err();
    assert!(
        error.to_string().contains("older source") || error.to_string().contains("source"),
        "{error}"
    );
    assert!(service.is_running().unwrap());
    assert_eq!(
        broker_endpoint::observe(prepared.root())
            .ok()
            .and_then(|observation| match observation {
                Observation::Ready { endpoint, .. } => Some(endpoint.pid),
                _ => None,
            }),
        Some(pid)
    );
    first.terminate();
}

#[test]
#[ignore = "creates owned Windows WMI service fixtures"]
fn authenticated_retire_drains_slow_call_and_reclaims_leaf() {
    let prepared = Prepared::new();
    let mut starter = Client::connect(
        prepared.path(),
        SOURCE_A,
        IDLE_MS,
        "echo",
        Some(&json!({}).to_string()),
        None,
        "retire-start.stderr",
    );
    let started = starter.read_json(40);
    let pid = started["identity"]["pid"].as_u64().unwrap() as u32;
    let creation_time = started["identity"]["creation_time"].as_u64().unwrap();
    let service = observe_service(pid, creation_time);
    let Observation::Ready {
        endpoint,
        owner: _owner,
    } = broker_endpoint::observe(prepared.root()).unwrap()
    else {
        panic!("ready endpoint missing");
    };
    let cancel = Cancellation::default();
    let status = broker_http::exchange(
        endpoint.port,
        endpoint.token(),
        "status",
        &json!({}),
        deadline(3),
        &cancel,
    )
    .unwrap();
    let leaf = ServiceProcess::observe(
        status["backend"]["child"].as_u64().unwrap() as u32,
        &fixture_path(),
        0,
        &current_user().unwrap(),
    )
    .unwrap();
    let slow_cancel = Cancellation::default();
    let token = endpoint.token().to_string();
    let port = endpoint.port;
    let slow = std::thread::Builder::new()
        .name("slow-call".into())
        .spawn(move || {
            broker_http::exchange(
                port,
                &token,
                "slow",
                &json!({}),
                Deadline::after(Duration::from_secs(8)).unwrap(),
                &slow_cancel,
            )
        })
        .unwrap();
    let admitted = Instant::now() + Duration::from_secs(3);
    while Instant::now() < admitted {
        if let Ok(status) = broker_http::exchange(
            endpoint.port,
            endpoint.token(),
            "status",
            &json!({}),
            deadline(2),
            &cancel,
        ) && status["backend"]["slow"].as_bool() == Some(true)
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let retiring = broker_http::exchange(
        endpoint.port,
        endpoint.token(),
        "retire",
        &json!({}),
        deadline(5),
        &cancel,
    )
    .unwrap();
    assert_eq!(retiring["retiring"], true);
    let rejected = broker_http::exchange(
        endpoint.port,
        endpoint.token(),
        "echo",
        &json!({ "late": true }),
        deadline(3),
        &cancel,
    );
    assert!(rejected.is_err(), "new work accepted after retire");
    fs::write(prepared.path().join("slow-continue"), []).unwrap();
    let slow_result = slow.join().expect("slow worker panicked");
    assert!(
        slow_result.is_ok(),
        "admitted slow call was dropped: {slow_result:?}"
    );
    wait_exit(&service, 8);
    assert_eq!(service.exit_code().unwrap(), Some(0));
    wait_exit(&leaf, 3);
    endpoint_gone(prepared.root());
    instance_free(prepared.root());
    starter.terminate();
}

#[test]
#[ignore = "creates owned Windows WMI service fixtures"]
fn short_idle_terminates_despite_status_polling() {
    let prepared = Prepared::new();
    let mut starter = Client::connect(
        prepared.path(),
        SOURCE_A,
        IDLE_SHORT_MS,
        "echo",
        Some(&json!({ "idle": true }).to_string()),
        None,
        "idle.stderr",
    );
    let started = starter.read_json(40);
    let pid = started["identity"]["pid"].as_u64().unwrap() as u32;
    let creation_time = started["identity"]["creation_time"].as_u64().unwrap();
    let service = observe_service(pid, creation_time);
    let Observation::Ready {
        endpoint,
        owner: _owner,
    } = broker_endpoint::observe(prepared.root()).unwrap()
    else {
        panic!("ready endpoint missing");
    };
    let cancel = Cancellation::default();
    let status = broker_http::exchange(
        endpoint.port,
        endpoint.token(),
        "status",
        &json!({}),
        deadline(3),
        &cancel,
    )
    .unwrap();
    let leaf = ServiceProcess::observe(
        status["backend"]["child"].as_u64().unwrap() as u32,
        &fixture_path(),
        0,
        &current_user().unwrap(),
    )
    .unwrap();
    let poll_until = Instant::now() + Duration::from_secs(3);
    while Instant::now() < poll_until && service.is_running().unwrap() {
        let _ = broker_http::exchange(
            endpoint.port,
            endpoint.token(),
            "status",
            &json!({}),
            deadline(1),
            &cancel,
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    wait_exit(&service, 6);
    assert_eq!(service.exit_code().unwrap(), Some(0));
    wait_exit(&leaf, 3);
    endpoint_gone(prepared.root());
    instance_free(prepared.root());
    starter.terminate();
}

#[test]
#[ignore = "creates owned Windows WMI service fixtures"]
fn uncooperative_hang_is_reclaimed_and_stale_receipt_restarts() {
    let prepared = Prepared::new();
    let mut starter = Client::connect(
        prepared.path(),
        SOURCE_A,
        IDLE_MS,
        "echo",
        Some(&json!({}).to_string()),
        None,
        "hang-start.stderr",
    );
    let started = starter.read_json(40);
    let pid = started["identity"]["pid"].as_u64().unwrap() as u32;
    let creation_time = started["identity"]["creation_time"].as_u64().unwrap();
    let service = observe_service(pid, creation_time);
    let Observation::Ready {
        endpoint,
        owner: _owner,
    } = broker_endpoint::observe(prepared.root()).unwrap()
    else {
        panic!("ready endpoint missing");
    };
    let cancel = Cancellation::default();
    let status = broker_http::exchange(
        endpoint.port,
        endpoint.token(),
        "status",
        &json!({}),
        deadline(3),
        &cancel,
    )
    .unwrap();
    let leaf = ServiceProcess::observe(
        status["backend"]["child"].as_u64().unwrap() as u32,
        &fixture_path(),
        0,
        &current_user().unwrap(),
    )
    .unwrap();
    let token = endpoint.token().to_string();
    let port = endpoint.port;
    let hang = std::thread::Builder::new()
        .name("hang-call".into())
        .spawn(move || {
            broker_http::exchange(
                port,
                &token,
                "hang",
                &json!({}),
                Deadline::after(Duration::from_millis(200)).unwrap(),
                &Cancellation::default(),
            )
        })
        .unwrap();
    let began = Instant::now();
    wait_exit(&service, 18);
    assert!(
        prepared.path().join("hang-admitted").is_file(),
        "hung operation never reached its backend"
    );
    assert!(
        began.elapsed() < Duration::from_secs(18),
        "service leaked past fixture watchdog"
    );
    let code = service.exit_code().unwrap();
    assert_eq!(code, Some(2), "expected supervisor exit 2, got {code:?}");
    wait_exit(&leaf, 3);
    let _ = hang.join();
    match broker_endpoint::observe(prepared.root()).unwrap() {
        Observation::Stale | Observation::Absent => {}
        Observation::Ready { .. } => panic!("hung owner still published as ready"),
    }
    starter.terminate();
    let mut restart = Client::connect(
        prepared.path(),
        SOURCE_A,
        IDLE_MS,
        "echo",
        Some(&json!({ "after": UNICODE }).to_string()),
        None,
        "hang-restart.stderr",
    );
    let restarted = restart.read_json(40);
    secret_free(&restarted);
    assert_eq!(restarted["result"]["payload"]["after"], UNICODE);
    let new_pid = restarted["identity"]["pid"].as_u64().unwrap() as u32;
    let new_creation = restarted["identity"]["creation_time"].as_u64().unwrap();
    assert_ne!((new_pid, new_creation), (pid, creation_time));
    let replacement = observe_service(new_pid, new_creation);
    assert!(replacement.is_running().unwrap());
    restart.terminate();
}

#[test]
#[ignore = "creates owned Windows WMI service fixtures"]
fn closing_the_client_cancels_an_admitted_slow_call() {
    let prepared = Prepared::new();
    let mut starter = Client::connect(
        prepared.path(),
        SOURCE_A,
        IDLE_MS,
        "echo",
        Some(&json!({}).to_string()),
        None,
        "cancel-start.stderr",
    );
    let started = starter.read_json(40);
    let pid = started["identity"]["pid"].as_u64().unwrap() as u32;
    let creation_time = started["identity"]["creation_time"].as_u64().unwrap();
    let service = observe_service(pid, creation_time);
    let Observation::Ready {
        endpoint,
        owner: _owner,
    } = broker_endpoint::observe(prepared.root()).unwrap()
    else {
        panic!("ready endpoint missing");
    };
    let cancel = Cancellation::default();
    let slow = Client::connect(
        prepared.path(),
        SOURCE_A,
        IDLE_MS,
        "slow",
        Some(&json!({}).to_string()),
        Some("8000"),
        "cancel-slow.stderr",
    );
    let admitted = Instant::now() + Duration::from_secs(3);
    while Instant::now() < admitted {
        if let Ok(status) = broker_http::exchange(
            endpoint.port,
            endpoint.token(),
            "status",
            &json!({}),
            deadline(2),
            &cancel,
        ) && status["backend"]["slow"].as_bool() == Some(true)
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    slow.terminate();
    let seen = Instant::now() + Duration::from_secs(6);
    while Instant::now() < seen {
        if prepared.path().join("cancel-observed").is_file() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        prepared.path().join("cancel-observed").is_file(),
        "backend did not observe cancellation after the client Job closed"
    );
    assert!(service.is_running().unwrap());
    starter.terminate();
}

#[test]
#[ignore = "creates owned Windows WMI service fixtures"]
fn backend_shutdown_failure_is_private_and_reclaims_its_child() {
    let prepared = Prepared::new();
    fs::write(prepared.path().join("fail-shutdown"), []).unwrap();
    let mut starter = Client::connect(
        prepared.path(),
        SOURCE_A,
        IDLE_MS,
        "status",
        Some("{}"),
        None,
        "shutdown-failure.stderr",
    );
    let result = starter.read_json(40);
    let pid = result["identity"]["pid"].as_u64().unwrap() as u32;
    let leaf = ServiceProcess::observe(
        result["result"]["backend"]["child"].as_u64().unwrap() as u32,
        &fixture_path(),
        0,
        &current_user().unwrap(),
    )
    .unwrap();
    let outcome =
        broker_launch::retire(prepared.root(), deadline(5), &Cancellation::default()).unwrap();
    assert!(
        matches!(outcome, broker_launch::Retirement::Exited {pid: observed, exit_code:2} if observed == pid)
    );
    wait_exit(&leaf, 3);
    let log = fs::read_to_string(prepared.path().join("service.log")).unwrap();
    assert!(!log.contains("owned-private-shutdown-sentinel"));
    assert!(matches!(
        broker_endpoint::observe(prepared.root()).unwrap(),
        Observation::Stale
    ));
    instance_free(prepared.root());
    starter.terminate();
}

#[test]
#[ignore = "creates owned Windows WMI service fixtures"]
fn tool_errors_preserve_owner_but_unconfirmed_cleanup_stops_it() {
    let prepared = Prepared::new();
    let mut starter = Client::connect(
        prepared.path(),
        SOURCE_A,
        IDLE_MS,
        "status",
        Some("{}"),
        None,
        "call-failure.stderr",
    );
    let started = starter.read_json(40);
    let service = observe_service(
        started["identity"]["pid"].as_u64().unwrap() as u32,
        started["identity"]["creation_time"].as_u64().unwrap(),
    );
    let leaf = ServiceProcess::observe(
        started["result"]["backend"]["child"].as_u64().unwrap() as u32,
        &fixture_path(),
        0,
        &current_user().unwrap(),
    )
    .unwrap();
    let Observation::Ready { endpoint, .. } = broker_endpoint::observe(prepared.root()).unwrap()
    else {
        panic!("ready endpoint missing")
    };
    let result = broker_http::exchange(
        endpoint.port,
        endpoint.token(),
        "tool-error",
        &json!({}),
        deadline(3),
        &Cancellation::default(),
    )
    .unwrap();
    assert_eq!(result["isError"], true);
    assert!(service.is_running().unwrap());
    assert!(
        broker_http::exchange(
            endpoint.port,
            endpoint.token(),
            "fail-call",
            &json!({}),
            deadline(3),
            &Cancellation::default()
        )
        .is_err()
    );
    wait_exit(&service, 5);
    assert_eq!(service.exit_code().unwrap(), Some(2));
    wait_exit(&leaf, 3);
    assert!(
        !fs::read_to_string(prepared.path().join("service.log"))
            .unwrap()
            .contains("owned-private-call-sentinel")
    );
    starter.terminate();
}

#[test]
fn admitted_broker_and_backend_keep_the_account_allowance_after_one_client_exits() {
    let prepared = Prepared::new();
    let budget = SharedCpuBudget::acquire(&prepared.account(), SHARED_CPU_PERCENT).unwrap();
    let snapshot = budget.snapshot().unwrap();
    assert_eq!(snapshot.cpu_rate, 7500);
    assert!(snapshot.cpu_hard_cap && !snapshot.kill_on_close);
    let mut first = Client::connect(
        prepared.path(),
        SOURCE_A,
        IDLE_MS,
        "echo",
        Some(&json!({ "text": UNICODE }).to_string()),
        None,
        "allowance-first.stderr",
    );
    let first_json = first.read_json(40);
    secret_free(&first_json);
    let pid = first_json["identity"]["pid"].as_u64().unwrap() as u32;
    let creation_time = first_json["identity"]["creation_time"].as_u64().unwrap();
    let service = observe_service(pid, creation_time);
    // The sibling start admits itself before the service does payload work.
    assert!(
        service.in_shared_cpu_budget(&budget).unwrap(),
        "the sibling start must join the account allowance"
    );
    let Observation::Ready { endpoint, owner } = broker_endpoint::observe(prepared.root()).unwrap()
    else {
        panic!("ready endpoint missing")
    };
    assert_eq!(owner.identity().pid, pid);
    assert!(owner.in_shared_cpu_budget(&budget).unwrap());
    // Real retained MCP calls flow through the admitted broker.
    assert_eq!(
        broker_rpc::invoke(
            &endpoint,
            "echo",
            &json!({ "text": UNICODE }),
            deadline(3),
            &Cancellation::default()
        )
        .unwrap()["payload"]["text"],
        UNICODE
    );
    let status = control(&endpoint, "status", &json!({}));
    secret_free(&status);
    // The backend's own lower ceiling is expressed against the verified parent
    // rate, so 25% of host stays 25% of host inside the 75% allowance.
    assert_eq!(status["backend"]["job"]["cpu_rate"], 3333);
    assert_eq!(
        status["backend"]["job"]["memory_limit_bytes"],
        256 * 1024 * 1024
    );
    assert_eq!(status["backend"]["job"]["kill_on_close"], true);
    let child_pid = status["backend"]["child"].as_u64().unwrap() as u32;
    let leaf =
        ServiceProcess::observe(child_pid, &fixture_path(), 0, &current_user().unwrap()).unwrap();
    assert!(
        leaf.in_shared_cpu_budget(&budget).unwrap(),
        "a backend child inherits the allowance"
    );
    // One of two clients exits: its Job is gone, the backend and its child stay
    // alive, keep their place in the one allowance and keep answering.
    first.terminate();
    assert!(service.is_running().unwrap() && leaf.is_running().unwrap());
    assert!(owner.in_shared_cpu_budget(&budget).unwrap());
    assert!(leaf.in_shared_cpu_budget(&budget).unwrap());
    let surviving = budget.snapshot().unwrap();
    assert_eq!(surviving.cpu_rate, 7500);
    assert!(surviving.cpu_hard_cap && !surviving.kill_on_close);
    // A later client joins the same object rather than a second allowance.
    assert_eq!(
        SharedCpuBudget::acquire(&prepared.account(), SHARED_CPU_PERCENT)
            .unwrap()
            .name(),
        budget.name()
    );
    let mut second = Client::connect(
        prepared.path(),
        SOURCE_A,
        IDLE_MS,
        "echo",
        Some(&json!({ "text": "second 日本" }).to_string()),
        None,
        "allowance-second.stderr",
    );
    let second_json = second.read_json(20);
    secret_free(&second_json);
    assert_eq!(second_json["identity"]["pid"], pid);
    assert_eq!(second_json["identity"]["creation_time"], creation_time);
    assert_eq!(second_json["result"]["payload"]["text"], "second 日本");
    assert!(matches!(
        shared_cpu_coverage(&owner, Some(&prepared.account())),
        SharedCpuCoverage::Covered { rate: 7500, .. }
    ));
    second.terminate();
    let retired =
        broker_launch::retire(prepared.root(), deadline(8), &Cancellation::default()).unwrap();
    assert!(
        matches!(retired, broker_launch::Retirement::Exited { pid: exited, .. } if exited == pid)
    );
    wait_exit(&leaf, 5);
}

#[test]
fn ordinary_participant_joins_allowance_anchored_by_sibling_service() {
    let prepared = Prepared::new();
    let budget = SharedCpuBudget::acquire(&prepared.account(), SHARED_CPU_PERCENT).unwrap();
    let mut client = Client::connect(
        prepared.path(),
        SOURCE_A,
        IDLE_MS,
        "echo",
        Some(&json!({ "text": UNICODE }).to_string()),
        None,
        "foreign-chain.stderr",
    );
    let started = client.read_json(40);
    secret_free(&started);
    let pid = started["identity"]["pid"].as_u64().unwrap() as u32;
    let service = observe_service(pid, started["identity"]["creation_time"].as_u64().unwrap());
    assert!(service.in_shared_cpu_budget(&budget).unwrap());
    // Service-first: an ordinary-chain participant joins the same allowance
    // object after the sibling service anchored it. The ordinary spawn is hosted
    // outside this test's machine-account job, which cannot cross into that chain.
    let marker = prepared.path().join("ordinary-member.json");
    let worker = launch_ordinary_outside_caller_job(&prepared.account(), &marker);
    let receipt = wait_json_file(&marker.with_extension("worker.json"));
    assert_eq!(receipt["budget"], budget.name());
    assert_eq!(receipt["rate"], 7500);
    let member = ServiceProcess::observe(
        receipt["pid"].as_u64().unwrap() as u32,
        Path::new(env!("CARGO_BIN_EXE_harness-process-fixture")),
        0,
        &current_user().unwrap(),
    )
    .unwrap();
    assert!(member.in_shared_cpu_budget(&budget).unwrap());
    assert_eq!(budget.snapshot().unwrap().cpu_rate, 7500);
    fs::write(marker.with_extension("done"), []).unwrap();
    let _worker = ServiceProcess::observe(worker, &std::env::current_exe().unwrap(), 0, &current_user().unwrap()).unwrap();
    let snapshot = budget.snapshot().unwrap();
    assert_eq!(snapshot.cpu_rate, 7500);
    assert!(snapshot.cpu_hard_cap && !snapshot.kill_on_close);
    assert!(service.is_running().unwrap());
    assert!(service.in_shared_cpu_budget(&budget).unwrap());
    assert_eq!(
        broker_rpc::invoke(
            &broker_endpoint::observe(prepared.root())
                .ok()
                .and_then(|observation| match observation {
                    Observation::Ready { endpoint, .. } => Some(endpoint),
                    _ => None,
                })
                .expect("ready endpoint missing"),
            "echo",
            &json!({ "text": "still serving" }),
            deadline(3),
            &Cancellation::default()
        )
        .unwrap()["payload"]["text"],
        "still serving"
    );
    client.terminate();
}

#[test]
fn sibling_service_joins_allowance_anchored_by_ordinary_participant() {
    let prepared = Prepared::new();
    let budget = SharedCpuBudget::acquire(&prepared.account(), SHARED_CPU_PERCENT).unwrap();
    assert_eq!(budget.snapshot().unwrap().cpu_rate, 7500);
    let marker = prepared.path().join("ordinary-first.json");
    let ordinary_job = Job::new(Limits::default()).unwrap();
    let mut spec = CommandSpec::new(env!("CARGO_BIN_EXE_harness-process-fixture"));
    spec.args = vec!["hold".into(), marker.clone().into_os_string()];
    let member = budget.spawn(&ordinary_job, &spec).unwrap();
    wait_marker(&marker);
    assert_eq!(
        serde_json::from_slice::<Value>(&fs::read(&marker).unwrap()).unwrap()["in_job"],
        true
    );
    assert!(budget.contains(&member).unwrap());
    let mut client = Client::connect(
        prepared.path(),
        SOURCE_A,
        IDLE_MS,
        "echo",
        Some(&json!({ "text": UNICODE }).to_string()),
        None,
        "session-first.stderr",
    );
    let started = client.read_json(40);
    secret_free(&started);
    assert_eq!(started["result"]["payload"]["text"], UNICODE);
    let service = observe_service(
        started["identity"]["pid"].as_u64().unwrap() as u32,
        started["identity"]["creation_time"].as_u64().unwrap(),
    );
    assert!(
        service.in_shared_cpu_budget(&budget).unwrap(),
        "the sibling service must join the allowance the ordinary participant anchored"
    );
    assert_eq!(budget.snapshot().unwrap().cpu_rate, 7500);
    assert_eq!(
        SharedCpuBudget::acquire(&prepared.account(), SHARED_CPU_PERCENT)
            .unwrap()
            .name(),
        budget.name()
    );
    let starter = fs::read_to_string(prepared.path().join("session-first.stderr")).unwrap();
    assert!(
        !starter.contains("shared CPU allowance"),
        "an admitted service must not be reported degraded: {starter}"
    );
    client.terminate();
    assert!(service.is_running().unwrap());
    assert!(service.in_shared_cpu_budget(&budget).unwrap());
    ordinary_job.terminate(0, Duration::from_secs(3)).unwrap();
}
#[test]
fn unadmitted_pre_existing_broker_is_reported_with_restart_guidance() {
    let prepared = Prepared::new();
    // A service started without any account location cannot join the allowance.
    // It must still start, serve and be reported instead of passing as capped.
    let mut starter = Client::connect_in_account(
        prepared.path(),
        SOURCE_A,
        "8000",
        "echo",
        Some(&json!({ "text": UNICODE }).to_string()),
        None,
        "unadmitted-start.stderr",
        None,
    );
    let started = starter.read_json(40);
    secret_free(&started);
    let pid = started["identity"]["pid"].as_u64().unwrap() as u32;
    let creation_time = started["identity"]["creation_time"].as_u64().unwrap();
    let service = observe_service(pid, creation_time);
    assert!(service.is_running().unwrap());
    // The service reports the failed allowance on its own established channel.
    let log = fs::read_to_string(prepared.path().join("service.log")).unwrap();
    assert!(
        log.contains("outside the shared 75% CPU allowance"),
        "{log}"
    );
    // The starting client reports the uncovered state, never a cap, and real
    // work still reaches the service.
    let starter_log = fs::read_to_string(prepared.path().join("unadmitted-start.stderr")).unwrap();
    assert!(
        starter_log.contains("shared CPU allowance unverified"),
        "{starter_log}"
    );
    assert!(
        !starter_log.contains("inside the shared CPU allowance"),
        "{starter_log}"
    );
    assert_eq!(started["result"]["payload"]["text"], UNICODE);
    // A later client with a usable allowance finds the pre-existing owner
    // outside it: it keeps serving, is preserved rather than adopted, and the
    // diagnostic names the restart boundary.
    let mut joining = Client::connect(
        prepared.path(),
        SOURCE_A,
        "8000",
        "echo",
        Some(&json!({ "text": "joined 日本" }).to_string()),
        None,
        "unadmitted-join.stderr",
    );
    let joined = joining.read_json(20);
    secret_free(&joined);
    assert_eq!(
        joined["identity"]["pid"], pid,
        "the pre-existing owner must be preserved, not replaced"
    );
    assert_eq!(joined["result"]["payload"]["text"], "joined 日本");
    let join_log = fs::read_to_string(prepared.path().join("unadmitted-join.stderr")).unwrap();
    assert!(
        join_log.contains("outside the shared CPU allowance"),
        "{join_log}"
    );
    assert!(join_log.contains("restarted"), "{join_log}");
    assert!(service.is_running().unwrap());
    let Observation::Ready { owner, .. } = broker_endpoint::observe(prepared.root()).unwrap()
    else {
        panic!("ready endpoint missing")
    };
    match shared_cpu_coverage(&owner, Some(&prepared.account())) {
        SharedCpuCoverage::Unadmitted { cause } => {
            assert!(cause.contains("not a member"), "{cause}")
        }
        other => panic!("pre-existing owner reported as {other:?}"),
    }
    starter.terminate();
    joining.terminate();
}
