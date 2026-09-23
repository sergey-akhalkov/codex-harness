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
    broker_state::{BrokerRoot, Generation},
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

fn serena_rpc_recoverable(error: &io::Error) -> bool {
    let text = error.to_string();
    text.contains("tree cleanup was not confirmed") || text.contains("route echo is incomplete")
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
    /// One broker location per delivered build generation; see
    /// [`crate::broker_state::choose_generation`].
    #[serde(default)]
    generations: Vec<Generation>,
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
pub fn root(
    codex_home: &Path,
    source: &str,
    deadline: Deadline,
    cancel: &Cancellation,
) -> io::Result<PathBuf> {
    let parent = crate::dependency_discovery::local_path(&codex_home.join("harness/runtime"))?;
    std::fs::create_dir_all(&parent)?;
    let account = crate::process_service::current_user()?;
    let _admission = Admission::acquire(&account, deadline, cancel)?;
    let anchor = parent.join("serena-broker.json");
    match crate::registration_native::FileGuard::read_regular(&anchor) {
        Ok((guard, bytes)) => {
            if bytes.len() > crate::broker_state::ANCHOR_LIMIT {
                return Err(io::Error::other(
                    "Serena broker location record exceeds its bound",
                ));
            }
            let mut record: Record = serde_json::from_slice(&bytes)
                .map_err(|_| io::Error::other("Serena broker location record is invalid"))?;
            if record.owner != OWNER || record.account != account {
                return Err(io::Error::other(
                    "Serena broker location record is not owned; preserving it",
                ));
            }
            let identity = guard.object_identity()?;
            drop(guard);
            let (location, changed) = crate::broker_state::choose_generation(
                &record.root,
                &mut record.generations,
                source,
                || {
                    let prepared = BrokerRoot::prepare()?;
                    Ok(prepared.keep().path().to_path_buf())
                },
            )?;
            if changed {
                crate::registration_native::FileGuard::replace_regular(
                    &anchor,
                    &identity,
                    &bytes,
                    &serde_json::to_vec(&record)?,
                )?;
            }
            Ok(location)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let prepared = BrokerRoot::prepare()?;
            let root = prepared.keep().path().to_path_buf();
            let record = Record {
                owner: OWNER.to_owned(),
                account,
                root: root.clone(),
                generations: vec![Generation {
                    source: source.to_owned(),
                    root: root.clone(),
                }],
            };
            crate::registration_native::StagedFile::create(&anchor, &serde_json::to_vec(&record)?)?
                .commit()?;
            Ok(root)
        }
        Err(error) => Err(error),
    }
}

pub struct Client {
    configuration: Configuration,
    root: Mutex<PathBuf>,
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
        let root = root(&configuration.codex_home, &source, deadline, cancel)?;
        let key = crate::broker_endpoint::random_key()?;
        // The pool's client identities are 32 hex characters, matching the
        // seam's per-session tokens.
        let client = key[..32].to_owned();
        Ok(Self {
            configuration,
            root: Mutex::new(root),
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
        let mut environment = BTreeMap::new();
        for key in ["SystemRoot", "TEMP", "TMP", "USERPROFILE", "LOCALAPPDATA"] {
            if let Ok(value) = std::env::var(key) {
                environment.insert(key.into(), value);
            }
        }
        let arguments = vec![
            "serena".into(),
            self.source.clone(),
            serde_json::to_string(&self.configuration)?,
        ];
        let startup = Deadline::after(deadline.remaining().min(Duration::from_secs(30)))?;
        let mut location = self.location()?;
        let mut endpoint = match broker_launch::ensure(
            &BrokerRoot::open(&location)?,
            &std::env::current_exe()?,
            arguments.clone(),
            environment.clone(),
            &self.source,
            startup,
            cancel,
        ) {
            Ok(endpoint) => endpoint,
            // Another build generation took the location between resolution
            // and startup; resolve this generation's own location once.
            Err(error) if broker_launch::source_conflict(&error) => {
                let retry = Deadline::after(deadline.remaining().min(Duration::from_secs(30)))?;
                location = root(&self.configuration.codex_home, &self.source, retry, cancel)?;
                self.relocate(&location)?;
                broker_launch::ensure(
                    &BrokerRoot::open(&location)?,
                    &std::env::current_exe()?,
                    arguments.clone(),
                    environment.clone(),
                    &self.source,
                    retry,
                    cancel,
                )?
            }
            Err(error) => return Err(error),
        };
        match Self::rpc_once(&endpoint, operation, payload, deadline, cancel) {
            Ok(value) => Ok(value),
            Err(error) if serena_rpc_recoverable(&error) => {
                let retry = Deadline::after(deadline.remaining().min(Duration::from_secs(30)))?;
                match broker_launch::retire(&BrokerRoot::open(&location)?, retry, cancel)? {
                    broker_launch::Retirement::Pending { pid } => Err(io::Error::other(format!(
                        "Serena broker retirement still pending (pid {pid:?})"
                    ))),
                    broker_launch::Retirement::Absent
                    | broker_launch::Retirement::Exited { .. } => {
                        endpoint = broker_launch::ensure(
                            &BrokerRoot::open(&location)?,
                            &std::env::current_exe()?,
                            arguments,
                            environment,
                            &self.source,
                            retry,
                            cancel,
                        )?;
                        Self::rpc_once(&endpoint, operation, payload, deadline, cancel)
                    }
                }
            }
            Err(error) => Err(error),
        }
    }

    fn location(&self) -> io::Result<PathBuf> {
        self.root
            .lock()
            .map(|root| root.clone())
            .map_err(|_| io::Error::other("Serena broker location lock poisoned"))
    }

    fn relocate(&self, root: &Path) -> io::Result<()> {
        *self
            .root
            .lock()
            .map_err(|_| io::Error::other("Serena broker location lock poisoned"))? =
            root.to_owned();
        Ok(())
    }

    fn rpc_once(
        endpoint: &crate::broker_endpoint::Endpoint,
        operation: &str,
        payload: &Value,
        deadline: Deadline,
        cancel: &Cancellation,
    ) -> io::Result<Value> {
        match broker_rpc::invoke(endpoint, operation, payload, deadline, cancel) {
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
        let root = BrokerRoot::open(&self.location()?)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grown_record_from_repeated_deliveries_is_pruned_and_reused() {
        let home = tempfile::tempdir().unwrap();
        let mut roots = Vec::new();
        let mut generations = Vec::new();
        for seed in 0u32..30 {
            let root = BrokerRoot::prepare().unwrap().keep().path().to_path_buf();
            generations.push(Generation {
                source: format!("{seed:064x}"),
                root: root.clone(),
            });
            roots.push(root);
        }
        let parent =
            crate::dependency_discovery::local_path(&home.path().join("harness/runtime")).unwrap();
        std::fs::create_dir_all(&parent).unwrap();
        let anchor = parent.join("serena-broker.json");
        let record = json!({
            "owner": OWNER,
            "account": crate::process_service::current_user().unwrap(),
            "root": roots[0],
            "generations": generations,
        });
        let bytes = serde_json::to_vec(&record).unwrap();
        assert!(
            bytes.len() > 4096,
            "the fixture must reproduce the grown live record"
        );
        std::fs::write(&anchor, &bytes).unwrap();
        let new_source = u64::MAX;
        let source = format!("{new_source:064x}");
        let deadline = Deadline::after(Duration::from_secs(10)).unwrap();
        let cancel = Cancellation::default();
        let resolved = root(home.path(), &source, deadline, &cancel).unwrap();
        assert!(
            roots.contains(&resolved),
            "a freed location must be reused instead of preparing a new one"
        );
        let rewritten: Value = serde_json::from_slice(&std::fs::read(&anchor).unwrap()).unwrap();
        let retained = rewritten["generations"].as_array().unwrap();
        assert_eq!(retained.len(), 1, "{retained:?}");
        assert_eq!(retained[0]["source"], source);
        assert!(
            std::fs::metadata(&anchor).unwrap().len() < 4096,
            "the rewritten record must be bounded"
        );
        for root in roots {
            drop(BrokerRoot::open(&root).ok());
            std::fs::remove_dir_all(root).unwrap();
        }
    }
}
