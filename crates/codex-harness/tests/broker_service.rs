#![cfg(windows)]
use harness_core::{
    broker_endpoint::{self, Endpoint, Observation},
    broker_http, broker_launch, broker_rpc,
    broker_state::BrokerRoot,
    cancellable_pipe::{CancellablePipe, anonymous_pipe},
    process::{Cancellation, CommandSpec, Deadline, Job, Limits, OwnedProcess, ProcessIdentity},
    process_service::{ServiceProcess, current_user},
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
    fn start(root: &Path, args: Vec<std::ffi::OsString>, stderr_name: &str) -> Self {
        let (stdin, write) = anonymous_pipe(4096).unwrap();
        let (read, stdout) = anonymous_pipe(4096).unwrap();
        let stderr = root.join(stderr_name);
        let mut command = CommandSpec::new(FIXTURE);
        command.current_dir = Some(root.into());
        command.args = args;
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
        Self::start(root, args, stderr_name)
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
    assert_eq!(status["backend"]["job"]["cpu_rate"], 2500);
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
