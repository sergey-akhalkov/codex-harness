//! Managed candidate connection. Installation supplies verified package paths.
#![cfg(windows)]
use crate::{
    codegraph_catalogue,
    codegraph_response::Responses,
    codegraph_runtime::Runtime,
    dependency_discovery::local_path,
    mcp_session::{Operation, Session},
    mcp_stdio,
    process::{Cancellation, CommandSpec, Deadline},
};
use serde_json::{Value, json};
use std::{
    fs::File,
    io,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

/// Construct only from a checksum-validated published package. Configuration
/// inspection itself neither starts a process nor modifies project state.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Configuration {
    pub node: PathBuf,
    pub entry: PathBuf,
    pub project: PathBuf,
    pub data_name: String,
}

impl Configuration {
    /// Build only published noninteractive CLI commands. The caller prepares an
    /// owned active/staging database and validates completion before committing.
    pub fn cli_command(&self, operation: &str) -> io::Result<CommandSpec> {
        if !matches!(operation, "index" | "sync") {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "unsupported CodeGraph command",
            ));
        }
        let mut command = self.command()?;
        command.args = vec![
            self.entry.clone().into_os_string(),
            operation.into(),
            self.project.clone().into_os_string(),
            "--quiet".into(),
        ];
        command
            .env
            .insert("CODEGRAPH_FORCE_WATCH".into(), Some("0".into()));
        Ok(command)
    }

    pub fn command(&self) -> io::Result<CommandSpec> {
        self.command_policy(true)
    }

    pub(crate) fn command_policy(&self, require_index: bool) -> io::Result<CommandSpec> {
        let root = local_path(&self.project)?;
        if !root.is_dir()
            || !self.data_name.starts_with(".codegraph-")
            || self.data_name.contains(['/', '\\', ':'])
            || self.data_name.contains("..")
            || self.data_name.len() > 80
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid CodeGraph project/data directory",
            ));
        }
        // Avoid the upstream ancestor-root fallback. Unindexed roots remain
        // explicit until deliberate managed indexing is wired into this owner.
        let database = root.join(&self.data_name).join("codegraph.db");
        if require_index && (!database.is_file() || local_path(&database)? != database) {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "selected CodeGraph root is unindexed or its database is redirected",
            ));
        }
        let node = local_path(&self.node)?;
        let entry = local_path(&self.entry)?;
        if !node.is_file() || !entry.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "CodeGraph package entry is missing",
            ));
        }
        let mut command = CommandSpec::new(node);
        command.current_dir = Some(root.clone());
        command.args = vec![
            entry.into_os_string(),
            "serve".into(),
            "--mcp".into(),
            "--path".into(),
            root.into_os_string(),
        ];
        // The owned child has a fixed policy. Remove ambient upstream tuning,
        // Node preloads and daemon/watch overrides without changing user state.
        for (key, _) in std::env::vars_os() {
            let upper = key.to_string_lossy().to_ascii_uppercase();
            if upper.starts_with("CODEGRAPH_")
                || ["NODE_OPTIONS", "NODE_PATH", "NODE_COMPILE_CACHE"].contains(&upper.as_str())
            {
                command.env.insert(key, None);
            }
        }
        for (key, value) in [
            ("CODEGRAPH_DIR", self.data_name.as_str()),
            ("CODEGRAPH_NO_DAEMON", "1"),
            ("CODEGRAPH_PARSE_WORKERS", "1"),
            ("CODEGRAPH_RESOLVE_WORKERS", "1"),
            ("CODEGRAPH_FORCE_WATCH", "1"),
            ("CODEGRAPH_WATCH_DEBOUNCE_MS", "500"),
            ("CODEGRAPH_EXPLORE_DEDUP", "0"),
            ("CODEGRAPH_TELEMETRY", "0"),
            ("CODEGRAPH_NO_UPDATE_CHECK", "1"),
            ("DO_NOT_TRACK", "1"),
            ("NO_COLOR", "1"),
        ] {
            command.env.insert(key.into(), Some(value.into()));
        }
        Ok(command)
    }
}

struct Client {
    configuration: Configuration,
    shared: Option<crate::codegraph_broker::Client>,
    runtime: Runtime,
    responses: Responses,
    generation: String,
    failed: bool,
}

impl Client {
    fn identity(&self) -> Value {
        json!({"provider":"colbymchenry/codegraph","version":"1.6.0","root":self.configuration.project,
            "generation":null,"worker_lease":self.generation,"freshness":if self.failed {"failed"} else {"unverified"},
            "coverage":"Candidate edges and upstream language/size/ignore limits apply. Native shared connections observe each active indexed root and schedule finite refresh episodes. Use current source for pending or exact claims."})
    }

    fn call(&mut self, operation: Operation) -> io::Result<Value> {
        let request = match codegraph_catalogue::request(&operation.name,operation.arguments) {
            Ok(request) => request,
            Err(error) => return Ok(self.responses.shape(json!({"isError":true,"error":{"category":"invalid_arguments","message":error.to_string()}}),&self.identity(),4096)),
        };
        if operation.name == "codegraph_detail" {
            return Ok(self.responses.detail(
                request.arguments["id"].as_str().unwrap(),
                request.arguments["offset"].as_u64().unwrap() as usize,
                request.max_bytes,
            ));
        }
        let deadline = Deadline::after(operation.deadline.remaining().min(request.timeout))?;
        let result = (|| -> io::Result<Value> {
            if self.failed
                && !matches!(
                    operation.name.as_str(),
                    "codegraph_index" | "codegraph_sync" | "codegraph_status"
                )
            {
                return Err(io::Error::other(
                    "CodeGraph worker failed; reconnect deliberately after inspecting the cause",
                ));
            }
            if let Some(shared) = &self.shared {
                return shared.call(
                    &operation.name,
                    &request.arguments,
                    deadline,
                    &operation.cancellation,
                );
            }
            self.runtime.call(
                &operation.name,
                request.arguments,
                deadline,
                &operation.cancellation,
            )
        })();
        match result {
            Ok(value) => {
                self.failed = value["freshness"] == "failed";
                let mut identity = self.identity();
                for key in ["generation", "freshness", "coverage"] {
                    if let Some(field) = value.get(key) {
                        identity[key] = field.clone();
                    }
                }
                if value["managed_ambiguity"] == true && value["isError"] != true {
                    return Ok(self.responses.refuse(value,&identity,"Ambiguous symbol: choose an exact repository-relative file. Upstream applies its limit per definition, so this response does not enumerate a misleading aggregate list.",request.max_bytes));
                }
                Ok(self.responses.shape(value, &identity, request.max_bytes))
            }
            Err(error) => {
                self.failed = true;
                let outcome = self.runtime.close()?;
                Ok(self.responses.shape(json!({"isError":true,"error":{"category":format!("{:?}",error.kind()),"message":error.to_string()},"cleanup":outcome}),&self.identity(),request.max_bytes))
            }
        }
    }
}

pub fn serve(
    configuration: Configuration,
    input: File,
    output: File,
    cancel: &Cancellation,
    deadline: Deadline,
) -> io::Result<()> {
    serve_inner(configuration, None, input, output, cancel, deadline)
}

pub fn serve_shared(
    configuration: Configuration,
    root: PathBuf,
    input: File,
    output: File,
    cancel: &Cancellation,
    deadline: Deadline,
) -> io::Result<()> {
    let shared = crate::codegraph_broker::Client::new(configuration.clone(), root)?;
    serve_inner(configuration, Some(shared), input, output, cancel, deadline)
}

fn serve_inner(
    configuration: Configuration,
    shared: Option<crate::codegraph_broker::Client>,
    input: File,
    output: File,
    cancel: &Cancellation,
    deadline: Deadline,
) -> io::Result<()> {
    let project = local_path(&configuration.project)?;
    if project != configuration.project {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "CodeGraph root must be canonical",
        ));
    }
    // A connection ID is a lease identity, not a claim of a current source hash.
    let generation = crate::broker_endpoint::random_key()?;
    if let Some(shared) = &shared {
        shared.connect(Deadline::after(std::time::Duration::from_secs(30))?, cancel)?;
    }
    let client = Arc::new(Mutex::new(Client {
        runtime: Runtime::new(configuration.clone())?,
        configuration,
        shared,
        responses: Responses::new(),
        generation,
        failed: false,
    }));
    let session = Session::new("codegraph", codegraph_catalogue::tools())?
        .with_instructions(codegraph_catalogue::INSTRUCTIONS)?;
    let handler = client.clone();
    let result =
        mcp_stdio::serve_fallible(session, input, output, cancel, deadline, move |operation| {
            handler
                .lock()
                .map_err(|_| io::Error::other("CodeGraph client state poisoned"))?
                .call(operation)
        });
    let mut state = client
        .lock()
        .map_err(|_| io::Error::other("CodeGraph client cleanup state poisoned"))?;
    state.runtime.close()?;
    if let Some(shared) = &state.shared {
        shared.disconnect()?;
    }
    result
}

/// Read-only canonical identity helper for a validated package consumer.
pub fn configuration(
    node: &Path,
    entry: &Path,
    project: &Path,
    data_name: String,
) -> io::Result<Configuration> {
    Ok(Configuration {
        node: local_path(node)?,
        entry: local_path(entry)?,
        project: local_path(project)?,
        data_name,
    })
}
