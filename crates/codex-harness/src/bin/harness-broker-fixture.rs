//! Owned Windows WMI broker fixture for integration tests.
//!
//! Argument dispatch:
//! - `--harness-service-create`: native WMI helper bootstrap. An owned
//!   `hold-helper` marker in the current directory pauses before COM.
//! - `--harness-service-run <until> <user> <source> <idle-ms>`: enter the
//!   service Job first, then prepare root/log/backend and run the broker.
//!   A fixture-only watchdog exits 99 after 20 seconds so a stuck service
//!   cannot outlive the test.
//! - `--client <root> <source> <idle-ms> <operation> [json] [deadline-ms]`:
//!   `broker_launch::ensure(current_exe, [source, idle-ms], SystemRoot/TEMP/TMP)`
//!   then `broker_http::exchange`. Stdout is one JSON object with `result` and
//!   `identity` (pid/creation_time/source/port). The bearer is not printed.
//!   After the JSON line, the process waits on stdin so the test can keep the
//!   client Job alive. Optional `deadline-ms` bounds the client exchange.
//! - `--leaf`: backend child that lives until the service Job exits.

#[cfg(windows)]
use harness_core::{
    broker_http, broker_launch, broker_service,
    broker_state::BrokerRoot,
    process::{Cancellation, Deadline, JobSnapshot, Limits},
    process_service::{self, CREATE_ARGUMENT, RUN_ARGUMENT, ServiceGuard},
};
#[cfg(windows)]
use serde_json::{Value, json};
#[cfg(windows)]
use std::{
    collections::BTreeMap,
    fs::OpenOptions,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

#[cfg(windows)]
fn run() -> io::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some(CREATE_ARGUMENT) if args.len() == 1 => {
            // Owned test marker simulates a stuck native helper before COM.
            // The production helper has no test modes or ambient overrides.
            if std::env::current_dir()?.join("hold-helper").is_file() {
                std::fs::write("helper-started", std::process::id().to_string())?;
                std::thread::sleep(Duration::from_secs(120));
            }
            process_service::create_helper_entry()
        }
        Some(RUN_ARGUMENT) if args.len() >= 5 => {
            let until = args[1]
                .parse()
                .map_err(|_| io::Error::other("invalid fixture deadline"))?;
            let mut guard = ServiceGuard::enter(
                until,
                &args[2],
                Limits {
                    memory_bytes: Some(256 * 1024 * 1024),
                    cpu_percent: Some(25.0),
                },
            )?;
            std::thread::Builder::new()
                .name("fixture-watchdog".into())
                .spawn(|| {
                    std::thread::sleep(Duration::from_secs(20));
                    std::process::exit(99);
                })?;
            let source = args[3].clone();
            let idle_ms: u64 = args[4]
                .parse()
                .map_err(|_| io::Error::other("invalid fixture idle timeout"))?;
            let root = BrokerRoot::open(&std::env::current_dir()?)?;
            let log = OpenOptions::new()
                .create(true)
                .append(true)
                .open(root.path().join("service.log"))?;
            guard.redirect_standard_streams(&log)?;
            println!("Owned broker Unicode log: проверка 日本");
            eprintln!("Owned broker stderr: проверка 日本");
            let mut empty = [0u8; 1];
            assert_eq!(io::stdin().read(&mut empty)?, 0);
            let backend = FixtureBackend::prepare(&guard)?;
            broker_service::run(
                &root,
                guard,
                broker_service::Options {
                    source,
                    idle_timeout: Duration::from_millis(idle_ms),
                    request_timeout: Duration::from_secs(8),
                    max_connections: 8,
                },
                backend,
            )
        }
        Some("--leaf") => {
            std::thread::sleep(Duration::from_secs(120));
            Ok(())
        }
        Some("--client") if (5..=7).contains(&args.len()) => client(&args),
        _ => Err(io::Error::other("invalid broker fixture arguments")),
    }
}

#[cfg(windows)]
fn client(args: &[String]) -> io::Result<()> {
    let root = BrokerRoot::open(&PathBuf::from(&args[1]))
        .map_err(|error| io::Error::new(error.kind(), format!("fixture root open: {error}")))?;
    let source = &args[2];
    let idle_ms = &args[3];
    let operation = &args[4];
    let payload: Value = if args.len() >= 6 {
        serde_json::from_str(&args[5])
            .map_err(|_| io::Error::other("invalid fixture payload JSON"))?
    } else {
        json!({})
    };
    if !payload.is_object() {
        return Err(io::Error::other("fixture payload must be a JSON object"));
    }
    let exchange_ms: u64 = if args.len() == 7 {
        args[6]
            .parse()
            .map_err(|_| io::Error::other("invalid fixture client deadline"))?
    } else {
        12_000
    };
    if exchange_ms == 0 {
        return Err(io::Error::other("fixture client deadline must be positive"));
    }
    let mut environment = BTreeMap::new();
    for name in ["SystemRoot", "TEMP", "TMP"] {
        if let Ok(value) = std::env::var(name) {
            environment.insert(name.into(), value);
        }
    }
    let program = std::env::current_exe()?;
    let endpoint = broker_launch::ensure(
        &root,
        &program,
        vec![source.clone(), idle_ms.clone()],
        environment,
        source,
        Deadline::after(Duration::from_secs(30))?,
        &Cancellation::default(),
    )
    .map_err(|error| io::Error::new(error.kind(), format!("fixture ensure: {error}")))?;
    let result = broker_http::exchange(
        endpoint.port,
        endpoint.token(),
        operation,
        &payload,
        Deadline::after(Duration::from_millis(exchange_ms))?,
        &Cancellation::default(),
    )
    .map_err(|error| io::Error::new(error.kind(), format!("fixture exchange: {error}")))?;
    println!(
        "{}",
        json!({
            "result": result,
            "identity": {
                "pid": endpoint.pid,
                "creation_time": endpoint.creation_time,
                "source": endpoint.source,
                "port": endpoint.port,
            }
        })
    );
    io::stdout().flush()?;
    let mut input = Vec::new();
    io::stdin().take(1).read_to_end(&mut input)?;
    Ok(())
}

#[cfg(windows)]
struct FixtureBackend {
    child: Mutex<std::process::Child>,
    child_pid: u32,
    job: JobSnapshot,
    slow: AtomicBool,
    hanging: AtomicBool,
}

#[cfg(windows)]
impl FixtureBackend {
    fn prepare(guard: &ServiceGuard) -> io::Result<Arc<Self>> {
        let child = std::process::Command::new(std::env::current_exe()?)
            .arg("--leaf")
            .current_dir(std::env::current_dir()?)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        let child_pid = child.id();
        let job = guard.job().snapshot()?;
        Ok(Arc::new(Self {
            child: Mutex::new(child),
            child_pid,
            job,
            slow: AtomicBool::new(false),
            hanging: AtomicBool::new(false),
        }))
    }

    fn wait(deadline: Deadline, cancel: &Cancellation, root: &Path) -> io::Result<()> {
        loop {
            if cancel.is_cancelled() {
                let _ = std::fs::write(root.join("cancel-observed"), []);
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "broker request cancelled",
                ));
            }
            if deadline.expired() {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "broker request deadline elapsed",
                ));
            }
            if root.join("slow-continue").is_file() {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(20).min(deadline.remaining()));
        }
    }
}

#[cfg(windows)]
impl broker_service::Backend for FixtureBackend {
    fn call(
        &self,
        operation: &str,
        payload: &Value,
        deadline: Deadline,
        cancel: &Cancellation,
    ) -> io::Result<Value> {
        match operation {
            "echo" => Ok(json!({ "payload": payload })),
            "tool-error" => Ok(json!({"isError":true,"content":[]})),
            "fail-call" => Err(io::Error::other("owned-private-call-sentinel")),
            "slow" | "slow-cleanup" => {
                self.slow.store(true, Ordering::Release);
                std::fs::write("slow-admitted", [])?;
                let result = Self::wait(deadline, cancel, &std::env::current_dir()?);
                if operation == "slow-cleanup" && result.is_err() {
                    std::fs::write("cleanup-pending", [])?;
                    let cleanup = Deadline::after(Duration::from_secs(3))?;
                    while !Path::new("cleanup-continue").is_file() {
                        if cleanup.expired() {
                            return Err(io::Error::other("fixture cleanup was not released"));
                        }
                        std::thread::sleep(Duration::from_millis(20));
                    }
                    std::fs::write("cleanup-finished", [])?;
                }
                self.slow.store(false, Ordering::Release);
                match result {
                    Ok(()) => Ok(json!({"released":true})),
                    Err(error)
                        if matches!(
                            error.kind(),
                            io::ErrorKind::Interrupted | io::ErrorKind::TimedOut
                        ) =>
                    {
                        Ok(json!({"isError":true,"content":[]}))
                    }
                    Err(error) => Err(error),
                }
            }
            "hang" => {
                self.hanging.store(true, Ordering::Release);
                std::fs::write("hang-admitted", [])?;
                // Ignore deadline/cancel and hold stderr so a blocked fatal
                // eprintln cannot hide a leaked service from the watchdog.
                let _stderr = io::stderr().lock();
                std::thread::sleep(Duration::from_secs(30));
                self.hanging.store(false, Ordering::Release);
                Ok(json!({}))
            }
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid broker request",
            )),
        }
    }

    fn is_idle(&self) -> bool {
        !self.slow.load(Ordering::Acquire) && !self.hanging.load(Ordering::Acquire)
    }

    fn status(&self) -> Value {
        json!({
            "child": self.child_pid,
            "job": {
                "active_processes": self.job.active_processes,
                "memory_limit_bytes": self.job.memory_limit_bytes,
                "cpu_rate": self.job.cpu_rate,
                "kill_on_close": self.job.kill_on_close,
                "handle_inheritable": self.job.handle_inheritable,
            },
            "slow": self.slow.load(Ordering::Acquire),
            "hanging": self.hanging.load(Ordering::Acquire),
        })
    }

    fn shutdown(&self, deadline: Deadline, cancel: &Cancellation) -> io::Result<()> {
        if std::path::Path::new("fail-shutdown").is_file() {
            return Err(io::Error::other("owned-private-shutdown-sentinel"));
        }
        while self.slow.load(Ordering::Acquire) || self.hanging.load(Ordering::Acquire) {
            if cancel.is_cancelled() || deadline.expired() {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "broker backend did not finish shutdown",
                ));
            }
            std::thread::sleep(Duration::from_millis(20).min(deadline.remaining()));
        }
        let _ = self
            .child
            .lock()
            .map_err(|_| io::Error::other("broker leaf lock poisoned"))?
            .try_wait();
        Ok(())
    }
}

fn main() {
    #[cfg(windows)]
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(91);
    }
    #[cfg(not(windows))]
    std::process::exit(2);
}
