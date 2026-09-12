//! Rust-controlled Serena startup around the existing guarded entry.
//! The Python seam remains until this boundary proves equivalent isolation.
#![cfg(windows)]

use crate::{
    cancellable_pipe::{CancellablePipe, anonymous_pipe},
    mcp_protocol::{Decoder, Kind, Message, READ_CHUNK},
    process::{Cancellation, CommandSpec, Deadline, Job, Limits, Outcome, OwnedProcess},
};
use serde_json::{Value, json};
use std::{
    fs::{self, File},
    io,
    path::{Path, PathBuf},
    time::Duration,
};

const PACKAGE: &str = "serena-agent";
const VERSION: &str = "1.7.0";
const CLEANUP: Duration = Duration::from_secs(5);

#[derive(Clone, Debug)]
pub struct Launch {
    pub python: PathBuf,
    pub entry: PathBuf,
    pub registry: PathBuf,
    pub project: PathBuf,
    pub home: PathBuf,
}

pub struct Session {
    job: Option<Job>,
    child: OwnedProcess,
    input: Option<CancellablePipe>,
    output: CancellablePipe,
    decoder: Decoder,
    cancel: Cancellation,
    next_id: u64,
    stderr: PathBuf,
}

fn ordinary(path: &Path) -> io::Result<PathBuf> {
    crate::dependency_discovery::local_path(path)
}

pub fn command(launch: &Launch) -> io::Result<CommandSpec> {
    let python = ordinary(&launch.python)?;
    let entry = ordinary(&launch.entry)?;
    let registry = ordinary(&launch.registry)?;
    let project = ordinary(&launch.project)?;
    let home = ordinary(&launch.home)?;
    if !python.is_file() || !entry.is_file() || !registry.is_file() || !project.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "Serena launch inputs are missing",
        ));
    }
    let inventory: Value = serde_json::from_slice(&fs::read(&registry)?)
        .map_err(|_| io::Error::other("Serena registry is not JSON"))?;
    let serena = inventory
        .get("mcp")
        .and_then(Value::as_array)
        .and_then(|items| items.iter().find(|item| item["id"] == "serena"))
        .ok_or_else(|| io::Error::other("Serena is not in the adopted registry"))?;
    if serena["identity"] != PACKAGE || serena["version"] != VERSION {
        return Err(io::Error::other(
            "Reassess the Serena adapter for this package version.",
        ));
    }
    if !matches!(
        serena["status"].as_str(),
        Some("adopted" | "present" | "installed" | "ready" | "verified")
    ) {
        return Err(io::Error::other(
            "Serena dependency is missing or incompatible; explicit provisioning is required.",
        ));
    }
    fs::create_dir_all(home.join("harness/runtime/serena"))?;
    fs::create_dir_all(home.join("serena-home"))?;
    let mut command = CommandSpec::new(python);
    command.current_dir = Some(project.clone());
    command.args = vec![
        "-B".into(),
        "-u".into(),
        entry.into_os_string(),
        "start-mcp-server".into(),
        "--context".into(),
        "codex".into(),
        "--project".into(),
        project.into_os_string(),
        "--enable-web-dashboard".into(),
        "false".into(),
        "--enable-gui-log-window".into(),
        "false".into(),
    ];
    for key in [
        "PIP_REQUIRE_VIRTUALENV",
        "UV_NO_CACHE",
        "HARNESS_CODE_TOOLS_REGISTRY",
        "CODEX_HOME",
        "SERENA_HOME",
        "PYTHONUTF8",
        "PYTHONDONTWRITEBYTECODE",
        "HARNESS_SERENA_SHARED_WORKER",
    ] {
        command.env.insert(key.into(), None);
    }
    command.env.insert(
        "HARNESS_CODE_TOOLS_REGISTRY".into(),
        Some(registry.into_os_string()),
    );
    command
        .env
        .insert("CODEX_HOME".into(), Some(home.clone().into_os_string()));
    command.env.insert(
        "SERENA_HOME".into(),
        Some(home.join("serena-home").into_os_string()),
    );
    command.env.insert("PYTHONUTF8".into(), Some("1".into()));
    command
        .env
        .insert("PYTHONDONTWRITEBYTECODE".into(), Some("1".into()));
    command
        .env
        .insert("PIP_REQUIRE_VIRTUALENV".into(), Some("1".into()));
    command.env.insert("UV_NO_CACHE".into(), Some("1".into()));
    Ok(command)
}

impl Session {
    pub fn start(launch: &Launch, cancel: &Cancellation) -> io::Result<Self> {
        if cancel.is_cancelled() {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "Serena start cancelled",
            ));
        }
        let mut command = command(launch)?;
        let home = ordinary(&launch.home)?;
        let stderr = home.join("harness/runtime/serena/stderr.txt");
        fs::create_dir_all(stderr.parent().unwrap())?;
        let (stdin, write) = anonymous_pipe(4096)?;
        let (read, stdout) = anonymous_pipe(4096)?;
        command.stdin = Some(stdin);
        command.stdout = Some(stdout);
        command.stderr = Some(File::create(&stderr)?);
        let job = Job::new(Limits {
            memory_bytes: Some(2048 * 1024 * 1024),
            cpu_percent: Some(25.0),
        })?;
        let child = job.spawn(&command)?;
        drop(command);
        Ok(Self {
            job: Some(job),
            child,
            input: Some(CancellablePipe::writer(write, cancel.clone())?),
            output: CancellablePipe::reader(read, cancel.clone())?,
            decoder: Decoder::default(),
            cancel: cancel.clone(),
            next_id: 0,
            stderr,
        })
    }

    pub fn identity(&self) -> crate::process::ProcessIdentity {
        self.child.identity()
    }

    pub fn stderr_path(&self) -> &Path {
        &self.stderr
    }

    fn send(&mut self, value: Value, deadline: Deadline) -> io::Result<()> {
        let mut bytes = serde_json::to_vec(&value)?;
        bytes.push(b'\n');
        self.input
            .as_mut()
            .ok_or_else(|| io::Error::other("Serena input closed"))?
            .write_all(&bytes, deadline, &self.cancel)
            .map_err(io::Error::from)
    }

    fn next(&mut self, deadline: Deadline) -> io::Result<Message> {
        loop {
            if let Some(message) = self.decoder.next_message()? {
                return Ok(message);
            }
            let bytes = self
                .output
                .read(READ_CHUNK, deadline, &self.cancel)
                .map_err(io::Error::from)?;
            if bytes.is_empty() {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "Serena closed before its response",
                ));
            }
            self.decoder.push(&bytes)?;
        }
    }

    pub fn request(
        &mut self,
        method: &str,
        params: Value,
        deadline: Deadline,
    ) -> io::Result<Value> {
        self.next_id += 1;
        let id = json!(self.next_id);
        self.send(
            json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}),
            deadline,
        )?;
        loop {
            let message = self.next(deadline)?;
            match message.kind() {
                Kind::Result | Kind::Error if message.id() == Some(&id) => {
                    return Ok(message.into_value());
                }
                Kind::Notification => continue,
                _ => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "unexpected Serena response identity",
                    ));
                }
            }
        }
    }

    pub fn notify(&mut self, method: &str, params: Value, deadline: Deadline) -> io::Result<()> {
        self.send(
            json!({"jsonrpc":"2.0","method":method,"params":params}),
            deadline,
        )
    }

    pub fn initialize(&mut self, deadline: Deadline) -> io::Result<Value> {
        let result = self.request(
            "initialize",
            json!({
                "protocolVersion":"2024-11-05",
                "capabilities":{},
                "clientInfo":{"name":"coding-agents-harness-pack","version":env!("CARGO_PKG_VERSION")}
            }),
            deadline,
        )?;
        if result.get("error").is_some() || result["result"]["serverInfo"]["name"] != "Serena" {
            return Err(io::Error::other("Serena initialization failed"));
        }
        self.notify("notifications/initialized", json!({}), deadline)?;
        Ok(result["result"].clone())
    }

    pub fn tool(&mut self, name: &str, arguments: Value, deadline: Deadline) -> io::Result<Value> {
        let result = self.request(
            "tools/call",
            json!({"name":name,"arguments":arguments}),
            deadline,
        )?;
        let text = result["result"]["content"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|item| item["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n");
        if result.get("error").is_some()
            || result["result"]
                .get("isError")
                .is_some_and(|value| value != false)
            || text.starts_with("Error:")
        {
            return Err(io::Error::other(format!("Serena {name} failed: {text}")));
        }
        Ok(result)
    }

    pub fn tool_text(
        &mut self,
        name: &str,
        arguments: Value,
        deadline: Deadline,
    ) -> io::Result<String> {
        let result = self.tool(name, arguments, deadline)?;
        Ok(result["result"]["content"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|item| item["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"))
    }

    pub fn close(mut self) -> io::Result<Outcome> {
        if let Some(input) = self.input.take() {
            let _ = input.close(Deadline::after(CLEANUP)?);
        }
        let outcome = self.job.take().unwrap().wait(
            &self.child,
            Deadline::after(Duration::from_secs(8))?,
            &self.cancel,
            CLEANUP,
        )?;
        Ok(outcome)
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if let Some(input) = self.input.take() {
            if let Ok(deadline) = Deadline::after(CLEANUP) {
                let _ = input.close(deadline);
            }
        }
        if let Some(job) = self.job.take() {
            if let Ok(deadline) = Deadline::after(Duration::from_secs(8)) {
                let _ = job.wait(&self.child, deadline, &self.cancel, CLEANUP);
            }
        }
    }
}
