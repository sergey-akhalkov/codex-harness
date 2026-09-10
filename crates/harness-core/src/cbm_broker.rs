//! Explicit shared CBM connection using the audited native worker boundary.
#![cfg(windows)]

use crate::{
    broker_launch, broker_rpc, broker_service,
    broker_state::BrokerRoot,
    build_identity, cbm_index, cbm_stdio,
    mcp_session::Operation,
    process::{Cancellation, Deadline},
    process_service::ServiceGuard,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs::OpenOptions,
    io::{self, Write},
    path::PathBuf,
    sync::{Arc, Mutex, TryLockError},
    time::Duration,
};

pub struct Client {
    configuration: cbm_stdio::Configuration,
    root: PathBuf,
    source: String,
}

impl Client {
    /// Local configuration/identity reads only; neither a broker nor CBM starts.
    pub fn new(configuration: cbm_stdio::Configuration, root: PathBuf) -> io::Result<Self> {
        let source = source(&configuration)?;
        Ok(Self {
            configuration,
            root,
            source,
        })
    }

    pub fn call(&self, operation: Operation) -> io::Result<Value> {
        if operation.cancellation.is_cancelled() || operation.deadline.expired() {
            return Ok(cancelled());
        }
        let root = BrokerRoot::open(&self.root)?;
        let mut environment = BTreeMap::new();
        for name in ["SystemRoot", "TEMP", "TMP"] {
            if let Ok(value) = std::env::var(name) {
                environment.insert(name.into(), value);
            }
        }
        // Native worker preparation needs a private-state parent. Keep it in
        // this owned service root; do not inherit the user's ambient profile.
        environment.insert(
            "LOCALAPPDATA".into(),
            root.path()
                .to_str()
                .ok_or_else(|| io::Error::other("CBM broker root is not valid Unicode"))?
                .into(),
        );
        let encoded = serde_json::to_string(&self.configuration)
            .map_err(|_| io::Error::other("CBM broker configuration could not be encoded"))?;
        let endpoint = broker_launch::ensure(
            &root,
            &std::env::current_exe()?,
            vec!["codebase-memory".into(), self.source.clone(), encoded],
            environment,
            &self.source,
            Deadline::after(operation.deadline.remaining().min(Duration::from_secs(30)))?,
            &operation.cancellation,
        )?;
        broker_rpc::invoke(
            &endpoint,
            &operation.name,
            &operation.arguments,
            operation.deadline,
            &operation.cancellation,
        )
    }
}

fn source(configuration: &cbm_stdio::Configuration) -> io::Result<String> {
    let identity = json!({
        "protocol":"codex-harness/native-cbm-broker/v1",
        "manager":build_identity::hash_file(&std::env::current_exe()?)?,
        "configuration":configuration,
        "catalogue":build_identity::hash_file(&configuration.catalogue)?,
        "cbm":cbm_index::AUDITED_BUILD,
    });
    Ok(build_identity::hash_bytes(identity.to_string().as_bytes()))
}

fn cancelled() -> Value {
    json!({"content":[],"isError":true})
}

struct Backend {
    configuration: cbm_stdio::Configuration,
    // CBM 0.10.8 permits one daemon log writer for a selected cache. Keep the
    // actual worker cycle serialized across clients until its cleanup returns.
    worker: Mutex<()>,
}

impl broker_service::Backend for Backend {
    fn call(
        &self,
        name: &str,
        payload: &Value,
        deadline: Deadline,
        cancel: &Cancellation,
    ) -> io::Result<Value> {
        let _worker = loop {
            if cancel.is_cancelled() || deadline.expired() {
                return Ok(cancelled());
            }
            match self.worker.try_lock() {
                Ok(guard) => break guard,
                Err(TryLockError::WouldBlock) => {
                    std::thread::sleep(Duration::from_millis(20).min(deadline.remaining()))
                }
                Err(TryLockError::Poisoned(_)) => {
                    return Err(io::Error::other("CBM broker worker state poisoned"));
                }
            }
        };
        self.configuration.call(Operation {
            id: Value::Null,
            name: name.into(),
            arguments: payload.clone(),
            deadline,
            cancellation: cancel.clone(),
        })
    }
    fn is_idle(&self) -> bool {
        self.worker.try_lock().is_ok()
    }
    fn status(&self) -> Value {
        json!({"provider":"codebase-memory", "worker_busy":self.worker.try_lock().is_err()})
    }
    fn shutdown(&self, _deadline: Deadline, _cancel: &Cancellation) -> io::Result<()> {
        match self.worker.try_lock() {
            Ok(_) => Ok(()),
            Err(_) => Err(io::Error::other("CBM worker cleanup was not confirmed")),
        }
    }
}

/// Initial explicit path uses one fresh private root per broker lifetime.
/// Its log is created once and never adopts/truncates an existing file. The
/// installation lifecycle will provide root/log retirement for global routing.
pub fn serve(
    mut guard: ServiceGuard,
    expected: &str,
    configuration: cbm_stdio::Configuration,
) -> io::Result<()> {
    if source(&configuration)? != expected {
        return Err(io::Error::other("CBM broker source changed before startup"));
    }
    let root = BrokerRoot::open(&std::env::current_dir()?)?;
    let mut log = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(root.path().join("service.log"))?;
    writeln!(log, "codex-harness native CBM shared service")?;
    guard.redirect_standard_streams(&log)?;
    broker_service::run(
        &root,
        guard,
        broker_service::Options {
            source: expected.into(),
            idle_timeout: Duration::from_secs(300),
            request_timeout: Duration::from_secs(600),
            max_connections: 8,
        },
        Arc::new(Backend {
            configuration,
            worker: Mutex::new(()),
        }),
    )
}
