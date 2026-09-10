//! Shared direct CodeGraph worker using the existing authenticated native broker.
//! Responses/details stay in each stdio client; this service retains no answers.
#![cfg(windows)]
use crate::{
    broker_launch, broker_rpc, broker_service,
    broker_state::BrokerRoot,
    build_identity,
    codegraph_scheduler::Scheduler,
    codegraph_stdio::Configuration,
    process::{Cancellation, Deadline},
    process_service::ServiceGuard,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs::OpenOptions,
    io::{self, Write},
    path::PathBuf,
    sync::Arc,
    time::Duration,
};

pub struct Client {
    configuration: Configuration,
    root: PathBuf,
    source: String,
    client: String,
}
impl Client {
    pub fn new(configuration: Configuration, root: PathBuf) -> io::Result<Self> {
        let source = source(&configuration)?;
        Ok(Self {
            configuration,
            root,
            source,
            client: crate::broker_endpoint::random_key()?,
        })
    }
    pub fn connect(&self, deadline: Deadline, cancel: &Cancellation) -> io::Result<()> {
        let process = crate::process_service::ServiceProcess::observe(
            std::process::id(),
            &std::env::current_exe()?,
            0,
            &crate::process_service::current_user()?,
        )?;
        let identity = process.identity();
        let result = self.invoke("client/open", &json!({"client":self.client,
            "project":self.configuration.project,"pid":identity.pid,"created":identity.creation_time}), deadline, cancel)?;
        if result["connected"] != true {
            return Err(io::Error::other(format!(
                "CodeGraph connection failed: {result}"
            )));
        }
        Ok(())
    }
    pub fn disconnect(&self) -> io::Result<()> {
        let root = BrokerRoot::open(&self.root)?;
        if let crate::broker_endpoint::Observation::Ready { endpoint, .. } =
            crate::broker_endpoint::observe(&root)?
        {
            broker_rpc::invoke(
                &endpoint,
                "client/close",
                &json!({"client":self.client}),
                Deadline::after(Duration::from_secs(5))?,
                &Cancellation::default(),
            )?;
        }
        Ok(())
    }
    pub fn call(
        &self,
        name: &str,
        arguments: &Value,
        deadline: Deadline,
        cancel: &Cancellation,
    ) -> io::Result<Value> {
        self.invoke(
            name,
            &json!({"client":self.client,"arguments":arguments}),
            deadline,
            cancel,
        )
    }
    fn invoke(
        &self,
        name: &str,
        arguments: &Value,
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
                "codegraph".into(),
                self.source.clone(),
                serde_json::to_string(&self.configuration)?,
            ],
            environment,
            &self.source,
            Deadline::after(deadline.remaining().min(Duration::from_secs(30)))?,
            cancel,
        )?;
        broker_rpc::invoke(&endpoint, name, arguments, deadline, cancel)
    }
}

fn source(configuration: &Configuration) -> io::Result<String> {
    Ok(build_identity::hash_bytes(serde_json::to_string(&json!({
        "protocol":"coding-agents-harness/codegraph-broker/v2","data_name":configuration.data_name,
        "manager":build_identity::hash_file(&std::env::current_exe()?)?,
        "node":build_identity::hash_file(&configuration.node)?,"entry":build_identity::hash_file(&configuration.entry)?
    }))?.as_bytes()))
}

struct Backend {
    state: Scheduler,
}
impl broker_service::Backend for Backend {
    fn call(
        &self,
        name: &str,
        payload: &Value,
        deadline: Deadline,
        cancel: &Cancellation,
    ) -> io::Result<Value> {
        self.state.call(name, payload, deadline, cancel)
    }
    fn keep_alive(&self) -> bool {
        self.state.keep_alive()
    }
    fn is_idle(&self) -> bool {
        self.state.is_idle()
    }
    fn status(&self) -> Value {
        self.state.status()
    }
    fn shutdown(&self, _deadline: Deadline, _cancel: &Cancellation) -> io::Result<()> {
        self.state.shutdown()
    }
}

pub fn serve(
    mut guard: ServiceGuard,
    expected: &str,
    configuration: Configuration,
) -> io::Result<()> {
    if source(&configuration)? != expected {
        return Err(io::Error::other(
            "CodeGraph broker source changed before startup",
        ));
    }
    let root = BrokerRoot::open(&std::env::current_dir()?)?;
    // Startup admission has established that no previous service owns this
    // root. Reuse its one bounded log only if the previous header is ours.
    let header = b"coding-agents-harness CodeGraph shared worker\n";
    let log_path = root.path().join("service.log");
    match root.read_private("service.log", 65536) {
        Ok(before) => {
            if !before.starts_with(header) {
                return Err(io::Error::other(
                    "CodeGraph service log is not owned; preserving it",
                ));
            }
            let (guard, actual) = crate::registration_native::FileGuard::read_regular(&log_path)?;
            let identity = guard.object_identity()?;
            drop(guard);
            if actual != before {
                return Err(io::Error::other("CodeGraph service log changed"));
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
    broker_service::run(
        &root,
        guard,
        broker_service::Options {
            source: expected.into(),
            idle_timeout: Duration::from_secs(60),
            request_timeout: Duration::from_secs(600),
            max_connections: 8,
        },
        Arc::new(Backend {
            state: Scheduler::start(configuration)?,
        }),
    )
}
