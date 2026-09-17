//! Shared Serena broker service and its exchange client.
//!
//! One authenticated broker per CODEX_HOME owns the project pool. The stdio
//! proxy is a thin client: it forwards requests through the bounded broker
//! exchange and keeps its own route; this service owns worker lifetime. The
//! broker source identity covers the manager, adopted interpreter, guarded
//! entry, registry, tool resources and the shared Serena home, so a stale
//! broker is retired instead of silently reused.
#![cfg(windows)]

use crate::{
    broker_launch, broker_rpc, broker_service,
    broker_state::BrokerRoot,
    build_identity,
    process::{Cancellation, Deadline},
    process_service::ServiceGuard,
    serena, serena_route,
    serena_shared::{self, Pool},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs::OpenOptions,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

impl Drop for Admission {
    fn drop(&mut self) {
        use std::os::windows::io::AsRawHandle;
        unsafe {
            windows_sys::Win32::System::Threading::ReleaseMutex(self.0.as_raw_handle());
        }
    }
}

/// Fully resolved shared-broker launch inputs. The manager selects every path;
/// the service discovers nothing.
#[derive(Clone, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct Configuration {
    /// The adopted Serena console entry point.
    pub serena: PathBuf,
    pub registry: PathBuf,
    pub codex_home: PathBuf,
    pub source_root: PathBuf,
}

pub fn source(configuration: &Configuration) -> io::Result<String> {
    let serena_home =
        crate::serena_configuration::prepare(&configuration.registry, &configuration.codex_home)?;
    Ok(build_identity::hash_bytes(
        serde_json::to_string(&json!({
            "protocol":"coding-agents-harness/serena-broker/v1",
            "manager":build_identity::hash_file(&std::env::current_exe()?)?,
            "serena":build_identity::hash_file(&configuration.serena)?,
            "registry":build_identity::hash_file(&configuration.registry)?,
            "resources":build_identity::hash_file(
                &configuration.source_root.join("global/tool-resources.json"),
            )?,
            "config":build_identity::hash_file(&serena_home.join(crate::serena_configuration::CONFIG_NAME))?,
            "serena_home":serena_home,
        }))?
        .as_bytes(),
    ))
}

#[derive(serde::Deserialize, serde::Serialize)]
struct Record {
    owner: String,
    account: String,
    root: PathBuf,
}

const OWNER: &str = "codex-harness-serena-broker";

struct Admission(std::os::windows::io::OwnedHandle);

impl Admission {
    fn acquire(account: &str, deadline: Deadline, cancel: &Cancellation) -> io::Result<Self> {
        use std::os::windows::io::{AsRawHandle, FromRawHandle};
        use windows_sys::Win32::{
            Foundation::{WAIT_ABANDONED, WAIT_OBJECT_0, WAIT_TIMEOUT},
            System::Threading::{CreateMutexW, WaitForSingleObject},
        };
        let name: Vec<_> = format!("Global\\CodingAgentsHarness.SerenaBroker.Location.{account}")
            .encode_utf16()
            .chain([0])
            .collect();
        let raw = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
        if raw.is_null() {
            return Err(io::Error::last_os_error());
        }
        let handle = unsafe { std::os::windows::io::OwnedHandle::from_raw_handle(raw) };
        loop {
            if cancel.is_cancelled() || deadline.expired() {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "Serena broker location admission expired",
                ));
            }
            let waited = unsafe { WaitForSingleObject(handle.as_raw_handle(), 250) };
            match waited {
                WAIT_OBJECT_0 | WAIT_ABANDONED => return Ok(Self(handle)),
                WAIT_TIMEOUT => continue,
                _ => return Err(io::Error::last_os_error()),
            }
        }
    }
}

/// The shared broker root for one CODEX_HOME. The anchor record names a
/// private prepared root; an unused CODEX_HOME creates no service state until
/// the first proxy connects.
pub fn root(codex_home: &Path, deadline: Deadline, cancel: &Cancellation) -> io::Result<PathBuf> {
    let parent = crate::dependency_discovery::local_path(&codex_home.join("harness/runtime"))?;
    std::fs::create_dir_all(&parent)?;
    let account = crate::process_service::current_user()?;
    let _admission = Admission::acquire(&account, deadline, cancel)?;
    let anchor = parent.join("serena-broker.json");
    match crate::registration_native::FileGuard::read_regular(&anchor) {
        Ok((_guard, bytes)) => {
            if bytes.len() > 4096 {
                return Err(io::Error::other(
                    "Serena broker location record exceeds its bound",
                ));
            }
            let record: Record = serde_json::from_slice(&bytes)
                .map_err(|_| io::Error::other("Serena broker location record is invalid"))?;
            if record.owner != OWNER || record.account != account {
                return Err(io::Error::other(
                    "Serena broker location record is not owned; preserving it",
                ));
            }
            Ok(BrokerRoot::open(&record.root)?.path().to_path_buf())
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let prepared = BrokerRoot::prepare()?;
            let record = Record {
                owner: OWNER.to_owned(),
                account,
                root: prepared.root().path().into(),
            };
            crate::registration_native::StagedFile::create(&anchor, &serde_json::to_vec(&record)?)?
                .commit()?;
            Ok(prepared.keep().path().to_path_buf())
        }
        Err(error) => Err(error),
    }
}

pub struct Client {
    configuration: Configuration,
    root: PathBuf,
    source: String,
    client: String,
}

impl Client {
    pub fn new(
        configuration: Configuration,
        deadline: Deadline,
        cancel: &Cancellation,
    ) -> io::Result<Self> {
        let source = source(&configuration)?;
        let root = root(&configuration.codex_home, deadline, cancel)?;
        let key = crate::broker_endpoint::random_key()?;
        // The pool's client identities are 32 hex characters, matching the
        // seam's per-session tokens.
        let client = key[..32].to_owned();
        Ok(Self {
            configuration,
            root,
            source,
            client,
        })
    }

    pub fn identity(&self) -> &str {
        &self.client
    }

    fn invoke(
        &self,
        operation: &str,
        payload: &Value,
        deadline: Deadline,
        cancel: &Cancellation,
    ) -> io::Result<Value> {
        let root = BrokerRoot::open(&self.root)?;
        let mut environment = BTreeMap::new();
        for key in ["SystemRoot", "TEMP", "TMP", "USERPROFILE", "LOCALAPPDATA"] {
            if let Ok(value) = std::env::var(key) {
                environment.insert(key.into(), value);
            }
        }
        let endpoint = broker_launch::ensure(
            &root,
            &std::env::current_exe()?,
            vec![
                "serena".into(),
                self.source.clone(),
                serde_json::to_string(&self.configuration)?,
            ],
            environment,
            &self.source,
            Deadline::after(deadline.remaining().min(Duration::from_secs(30)))?,
            cancel,
        )?;
        match broker_rpc::invoke(&endpoint, operation, payload, deadline, cancel) {
            Ok(value) if value.get("error").is_some() => Err(io::Error::other(
                value["error"]
                    .as_str()
                    .unwrap_or("Serena broker request failed"),
            )),
            Ok(value) => Ok(value),
            Err(error) => {
                eprintln!("serena-client: invoke failed: {error}");
                Err(error)
            }
        }
    }

    pub fn connect(
        &self,
        arguments: &[std::ffi::OsString],
        cwd: &Path,
        initialize: &Value,
        deadline: Deadline,
        cancel: &Cancellation,
    ) -> io::Result<Value> {
        self.invoke(
            "connect",
            &json!({
                "client": self.client,
                "arguments": arguments.iter().map(|value| value.to_string_lossy()).collect::<Vec<_>>(),
                "cwd": cwd,
                "initialize": initialize,
            }),
            deadline,
            cancel,
        )
    }

    pub fn rpc(
        &self,
        method: &str,
        params: &Value,
        route: &Value,
        initialize: &Value,
        deadline: Deadline,
        cancel: &Cancellation,
    ) -> io::Result<Value> {
        self.invoke(
            "rpc",
            &json!({
                "client": self.client,
                "method": method,
                "params": params,
                "route": route,
                "initialize": initialize,
            }),
            deadline,
            cancel,
        )
    }

    pub fn disconnect(&self) -> io::Result<()> {
        let root = BrokerRoot::open(&self.root)?;
        if let crate::broker_endpoint::Observation::Ready { endpoint, .. } =
            crate::broker_endpoint::observe(&root)?
        {
            broker_rpc::invoke(
                &endpoint,
                "disconnect",
                &json!({"client": self.client}),
                Deadline::after(Duration::from_secs(5))?,
                &Cancellation::default(),
            )?;
        }
        Ok(())
    }
}

struct Backend {
    pool: Arc<Mutex<Pool>>,
    stop: Arc<AtomicBool>,
}

impl Backend {
    fn start(policy: serena_route::Policy, configuration: &Configuration) -> io::Result<Arc<Self>> {
        let cancel = Cancellation::default();
        let serena_home = crate::serena_configuration::prepare(
            &configuration.registry,
            &configuration.codex_home,
        )?;
        let launch = serena::Launch {
            serena: configuration.serena.clone(),
            registry: configuration.registry.clone(),
            project: configuration.codex_home.clone(),
            home: configuration.codex_home.clone(),
        };
        let factory = serena_shared::session_factory(launch, cancel)?;
        let pool = Arc::new(Mutex::new(Pool::new(policy, serena_home, factory)?));
        let stop = Arc::new(AtomicBool::new(false));
        {
            let pool = Arc::clone(&pool);
            let stop = Arc::clone(&stop);
            thread::Builder::new()
                .name("serena-pool-reaper".into())
                .spawn(move || {
                    while !stop.load(Ordering::SeqCst) {
                        thread::sleep(Duration::from_secs(1));
                        if let Ok(mut pool) = pool.lock() {
                            pool.reap();
                        }
                    }
                })?;
        }
        Ok(Arc::new(Self { pool, stop }))
    }
}

impl broker_service::Backend for Backend {
    fn call(
        &self,
        operation: &str,
        payload: &Value,
        deadline: Deadline,
        _cancel: &Cancellation,
    ) -> io::Result<Value> {
        // Lock poisoning is the only fatal condition; per-request failures
        // answer as broker error envelopes and keep the service serving,
        // matching the seam's dispatch contract.
        let result = (|| -> io::Result<Value> {
            let mut pool = self
                .pool
                .lock()
                .map_err(|_| io::Error::other("Serena pool lock poisoned"))?;
            let client = payload["client"]
                .as_str()
                .ok_or_else(|| invalid("Serena broker request lacks a client identity"))?;
            match operation {
                "connect" => {
                    let arguments: Vec<std::ffi::OsString> = payload["arguments"]
                        .as_array()
                        .ok_or_else(|| invalid("Serena connect lacks forwarded arguments"))?
                        .iter()
                        .map(|value| std::ffi::OsString::from(value.as_str().unwrap_or_default()))
                        .collect();
                    let cwd = PathBuf::from(
                        payload["cwd"]
                            .as_str()
                            .ok_or_else(|| invalid("Serena connect lacks a working directory"))?,
                    );
                    pool.connect(
                        client,
                        &arguments,
                        &cwd,
                        payload["initialize"].clone(),
                        deadline,
                    )
                }
                "rpc" => {
                    let method = payload["method"]
                        .as_str()
                        .ok_or_else(|| invalid("Serena rpc lacks a method"))?;
                    let route = serena_route::Route::from_json(&payload["route"])?;
                    pool.rpc(
                        client,
                        method,
                        payload["params"].clone(),
                        route,
                        payload["initialize"].clone(),
                        deadline,
                    )
                }
                "disconnect" => Ok(json!(pool.disconnect(client))),
                _ => Err(invalid("Unknown Serena broker operation")),
            }
        })();
        match result {
            Ok(value) => Ok(value),
            Err(error) => {
                eprintln!("serena-broker: {operation} failed: {error}");
                Ok(json!({"error": error.to_string()}))
            }
        }
    }

    fn is_idle(&self) -> bool {
        self.pool.lock().map(|pool| pool.is_idle()).unwrap_or(false)
    }

    fn status(&self) -> Value {
        self.pool
            .lock()
            .map(|pool| pool.status())
            .unwrap_or_else(|_| json!({"poisoned": true}))
    }

    fn shutdown(&self, _deadline: Deadline, _cancel: &Cancellation) -> io::Result<()> {
        self.stop.store(true, Ordering::SeqCst);
        let mut pool = self
            .pool
            .lock()
            .map_err(|_| io::Error::other("Serena pool lock poisoned"))?;
        pool.close()
    }
}

pub fn serve(
    mut guard: ServiceGuard,
    expected: &str,
    configuration: Configuration,
) -> io::Result<()> {
    if source(&configuration)? != expected {
        return Err(io::Error::other(
            "Serena broker source changed before startup",
        ));
    }
    let policy = serena_route::policy(&configuration.source_root)?;
    let root = BrokerRoot::open(&std::env::current_dir()?)?;
    let header = b"coding-agents-harness Serena shared broker\n";
    let log_path = root.path().join("service.log");
    match root.read_private("service.log", 65536) {
        Ok(before) => {
            if !before.starts_with(header) {
                return Err(io::Error::other(
                    "Serena service log is not owned; preserving it",
                ));
            }
            let (guard, actual) = crate::registration_native::FileGuard::read_regular(&log_path)?;
            let identity = guard.object_identity()?;
            drop(guard);
            if actual != before {
                return Err(io::Error::other("Serena service log changed"));
            }
            crate::registration_native::FileGuard::replace_regular(
                &log_path, &identity, &before, header,
            )?;
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            crate::registration_native::StagedFile::create(&log_path, header)?.commit()?;
        }
        Err(error) => return Err(error),
    }
    let mut log = OpenOptions::new().append(true).open(&log_path)?;
    log.flush()?;
    guard.redirect_standard_streams(&log)?;
    let backend = Backend::start(policy, &configuration)?;
    broker_service::run(
        &root,
        guard,
        broker_service::Options {
            source: expected.into(),
            idle_timeout: Duration::from_secs(policy.idle_seconds.max(60)),
            request_timeout: Duration::from_secs(240),
            max_connections: 8,
        },
        backend,
    )
}
