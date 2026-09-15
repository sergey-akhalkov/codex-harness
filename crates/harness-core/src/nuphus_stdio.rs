//! Native stdio adapter for audited original Nuphus.
//!
//! Handshake and tools/list stay local when the catalogue cache matches the
//! audited binary. Tool calls start one lazy official worker. Browser tools use
//! a private CDP endpoint; desktop tools take the account-wide admission lock.
#![cfg(windows)]

use crate::{
    build_identity,
    cancellable_pipe::anonymous_pipe,
    lazy_stdio::{self, LazyWorker, Rejection, WorkerLaunch},
    mcp_session::{Operation, Session},
    mcp_stdio,
    nuphus_protocol::{
        BrowserReferences, SCREENSHOT_TOOLS, adapt_schema, bound_screenshot_result, bounded_detail,
    },
    process::{Cancellation, CommandSpec, Deadline, Job, Limits, OwnedProcess},
    resource_admission::{self, Resource},
};
use serde_json::{Value, json};
use std::{
    fs::{self, File},
    io::{self, BufRead, BufReader, Read, Write},
    net::{Ipv4Addr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

const REQUEST: Duration = Duration::from_secs(20);
const BROWSER_READY: Duration = Duration::from_secs(15);
const BROWSER_HTTP: Duration = Duration::from_secs(5);
const BROWSER_CLEANUP: Duration = Duration::from_secs(10);
const DESKTOP_ADMISSION: Duration = Duration::from_secs(10);
const CATALOGUE_NAME: &str = "nuphus-catalogue.json";

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

fn other(message: impl Into<String>) -> io::Error {
    io::Error::other(message.into())
}

/// Connection owner supplies every path. The executable must be the audited
/// original payload, not a locally patched companion.
pub struct Configuration {
    pub executable: PathBuf,
    pub expected_digest: String,
    pub codex_home: PathBuf,
    pub account: PathBuf,
    pub source_root: PathBuf,
    pub expected_server: Option<String>,
    pub idle: Option<Duration>,
    pub worker_args: Vec<std::ffi::OsString>,
    pub browser_cdp_url: Option<String>,
}

pub fn verify_original(executable: &Path, expected_digest: &str) -> io::Result<String> {
    if !executable.is_absolute() {
        return Err(invalid("Nuphus executable must be an absolute path"));
    }
    let digest = build_identity::hash_file(executable)?;
    if digest != expected_digest {
        return Err(other(
            "Nuphus original executable has not passed the supported package integrity audit",
        ));
    }
    Ok(digest)
}

fn idle_seconds(source_root: &Path) -> io::Result<Duration> {
    let path = source_root.join("global/tool-resources.json");
    let value: Value = serde_json::from_slice(&fs::read(&path)?)?;
    if value["schema_version"] != 1 {
        return Err(invalid("Unsupported resource policy"));
    }
    let seconds = value["nuphus"]["idle_seconds"].as_u64().unwrap_or(0);
    if !(1..=3600).contains(&seconds) {
        return Err(invalid("Nuphus idle bound is invalid"));
    }
    Ok(Duration::from_secs(seconds))
}

fn atomic_json(path: &Path, value: &Value) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| invalid("Nuphus catalogue cache needs a parent"))?;
    fs::create_dir_all(parent)?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| invalid("Nuphus catalogue cache name is invalid"))?;
    let temp = parent.join(format!("{}.{}.tmp", name, std::process::id()));
    let mut bytes = serde_json::to_vec_pretty(value)?;
    if !bytes.ends_with(b"\n") {
        bytes.push(b'\n');
    }
    fs::write(&temp, &bytes)?;
    fs::rename(&temp, path)?;
    Ok(())
}

fn load_catalogue(path: &Path, identity: &str) -> io::Result<Option<Vec<Value>>> {
    if !path.exists() {
        return Ok(None);
    }
    let value: Value = serde_json::from_slice(&fs::read(path)?)?;
    if value["identity"].as_str() != Some(identity) {
        return Ok(None);
    }
    value["tools"]
        .as_array()
        .cloned()
        .ok_or_else(|| invalid("Nuphus catalogue cache is not a tool list"))
        .map(Some)
}

fn adapt_catalogue(tools: Vec<Value>) -> io::Result<Vec<Value>> {
    tools
        .into_iter()
        .map(|tool| adapt_schema(tool).map_err(other))
        .collect()
}

fn cdp_override() -> Option<String> {
    std::env::var("NUPHUS_MCP_BROWSER_CDP_URL")
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

struct DesktopLease {
    directory: PathBuf,
    held: Option<resource_admission::Lease>,
}

impl lazy_stdio::Lease for DesktopLease {
    fn acquire(&mut self, deadline: &Deadline, cancellation: &Cancellation) -> io::Result<()> {
        let bound = DESKTOP_ADMISSION.min(deadline.remaining());
        if bound.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "resource busy; no additional worker started",
            ));
        }
        self.held = Some(resource_admission::Lease::acquire(
            &self.directory,
            Resource::Desktop,
            Deadline::after(bound)?,
            cancellation,
        )?);
        Ok(())
    }

    fn release(&mut self) -> io::Result<()> {
        self.held.take();
        Ok(())
    }
}

struct OwnedBrowser {
    listener: Option<TcpListener>,
    port: u16,
    endpoint: String,
    job: Option<Job>,
    child: Option<OwnedProcess>,
    directory: Option<PathBuf>,
    directory_parent: Option<PathBuf>,
    directory_resolved: Option<PathBuf>,
    started: bool,
    failed: bool,
}

impl OwnedBrowser {
    fn reserve() -> io::Result<Self> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        let port = listener.local_addr()?.port();
        Ok(Self {
            listener: Some(listener),
            port,
            endpoint: format!("http://127.0.0.1:{port}"),
            job: None,
            child: None,
            directory: None,
            directory_parent: None,
            directory_resolved: None,
            started: false,
            failed: false,
        })
    }

    fn ensure_started(&mut self, runtime: &Path) -> io::Result<()> {
        if self.failed {
            return Err(other(
                "Owned browser exited; start a new MCP session. No shared-browser fallback is allowed.",
            ));
        }
        if self.started {
            if self
                .child
                .as_ref()
                .is_some_and(|child| child.is_running().unwrap_or(false))
            {
                return Ok(());
            }
            return Err(other(
                "Owned browser exited; start a new MCP session. No shared-browser fallback is allowed.",
            ));
        }
        let executable = find_browser().ok_or_else(|| {
            other("An existing Chrome/Chromium/Edge installation is required. Runtime browser installation is disabled.")
        })?;
        fs::create_dir_all(runtime)?;
        let directory = runtime.join(format!(
            "{}_{}",
            std::process::id(),
            crate::broker_endpoint::random_key()
                .unwrap_or_else(|_| format!("{:032x}", std::process::id() as u128))
        ));
        fs::create_dir(&directory)?;
        let directory = std::path::absolute(&directory)?;
        let directory_parent = directory
            .parent()
            .ok_or_else(|| other("owned browser directory has no parent"))?
            .canonicalize()?;
        let directory_resolved = directory.canonicalize()?;
        drop(
            self.listener
                .take()
                .ok_or_else(|| other("owned browser port reservation is missing"))?,
        );
        self.directory = Some(directory.clone());
        self.directory_parent = Some(directory_parent.clone());
        self.directory_resolved = Some(directory_resolved.clone());
        let (read, write) = anonymous_pipe(4096).map_err(io::Error::from)?;
        let mut command = CommandSpec::new(executable);
        command.args = vec![
            "--headless=new".into(),
            "--do-not-de-elevate".into(),
            format!("--remote-debugging-port={}", self.port).into(),
            "--remote-debugging-address=127.0.0.1".into(),
            "--remote-allow-origins=*".into(),
            format!("--user-data-dir={}", directory.join("browser").display()).into(),
            "--no-first-run".into(),
            "--no-default-browser-check".into(),
            "--disable-background-networking".into(),
            "--disable-component-update".into(),
            "about:blank".into(),
        ];
        command.stderr = Some(write);
        let job = Job::new(Limits::default())?;
        let child = job.spawn(&command)?;
        drop(command);
        self.job = Some(job);
        self.child = Some(child);
        let (announced_tx, announced_rx) = std::sync::mpsc::channel();
        if let Err(error) = thread::Builder::new()
            .name("nuphus-browser-stderr".into())
            .spawn(move || {
                let mut reader = BufReader::new(read);
                let mut line = String::new();
                loop {
                    line.clear();
                    match reader.read_line(&mut line) {
                        Ok(0) => break,
                        Ok(_) => {
                            let trimmed = line.trim_end();
                            if let Some((_, rest)) = trimmed.split_once("DevTools listening on ") {
                                let _ = announced_tx.send(Ok(rest.trim().to_owned()));
                            }
                        }
                        Err(error) => {
                            let _ = announced_tx.send(Err(error));
                            break;
                        }
                    }
                }
            })
        {
            let _ = self.reclaim();
            self.failed = true;
            return Err(error);
        }
        let ready = Deadline::after(BROWSER_READY)?;
        let announced = loop {
            if ready.expired() {
                let _ = self.reclaim();
                self.failed = true;
                return Err(other(
                    "Owned browser did not become ready (deadline); shared-browser fallback is disabled",
                ));
            }
            match announced_rx.recv_timeout(Duration::from_millis(50).min(ready.remaining())) {
                Ok(Ok(url)) => break url,
                Ok(Err(error)) => {
                    let _ = self.reclaim();
                    self.failed = true;
                    return Err(other(format!(
                        "Owned browser did not become ready ({error}); shared-browser fallback is disabled"
                    )));
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    let _ = self.reclaim();
                    self.failed = true;
                    return Err(other(
                        "Owned browser did not become ready (stderr closed); shared-browser fallback is disabled",
                    ));
                }
            }
        };
        let mut last_error = String::from("version endpoint unanswered");
        loop {
            if ready.expired() {
                let _ = self.reclaim();
                self.failed = true;
                return Err(other(format!(
                    "Owned browser did not become ready ({last_error}); shared-browser fallback is disabled"
                )));
            }
            match http_json_version(self.port) {
                Ok(actual) if actual == announced => break,
                Ok(_) => {
                    let _ = self.reclaim();
                    self.failed = true;
                    return Err(other("Owned browser endpoint identity mismatch"));
                }
                Err(error) => {
                    last_error = error.to_string();
                    thread::sleep(Duration::from_millis(50).min(ready.remaining()));
                }
            }
        }
        self.started = true;
        Ok(())
    }

    fn reclaim(&mut self) -> io::Result<()> {
        self.listener.take();
        if let (Some(job), Some(child)) = (self.job.take(), self.child.take()) {
            let cancel = Cancellation::default();
            let _ = job.wait(
                &child,
                Deadline::after(BROWSER_CLEANUP)?,
                &cancel,
                BROWSER_CLEANUP,
            );
        }
        if let (Some(directory), Some(parent), Some(resolved)) = (
            self.directory.take(),
            self.directory_parent.take(),
            self.directory_resolved.take(),
        ) && let Err(error) = remove_owned(&directory, &directory, &parent, &resolved)
        {
            eprintln!("Nuphus cleanup pending: {} ({error})", directory.display());
        }
        self.started = false;
        Ok(())
    }
}

impl Drop for OwnedBrowser {
    fn drop(&mut self) {
        let _ = self.reclaim();
    }
}

fn find_browser() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    for base in [
        std::env::var_os("PROGRAMFILES"),
        std::env::var_os("PROGRAMFILES(X86)"),
        std::env::var_os("LOCALAPPDATA"),
    ]
    .into_iter()
    .flatten()
    {
        for suffix in [
            "Google/Chrome/Application/chrome.exe",
            "Microsoft/Edge/Application/msedge.exe",
            "Chromium/Application/chrome.exe",
        ] {
            candidates.push(PathBuf::from(&base).join(suffix));
        }
    }
    if let Some(path) = std::env::var_os("PATH") {
        for directory in std::env::split_paths(&path) {
            for name in ["chrome.exe", "msedge.exe", "chromium.exe"] {
                candidates.push(directory.join(name));
            }
        }
    }
    candidates.into_iter().find(|path| path.is_file())
}

fn http_json_version(port: u16) -> io::Result<String> {
    let deadline = Deadline::after(BROWSER_HTTP)?;
    let address = std::net::SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let mut stream = TcpStream::connect_timeout(&address, deadline.remaining())?;
    stream.set_read_timeout(Some(Duration::from_millis(200)))?;
    stream.set_write_timeout(Some(Duration::from_millis(200)))?;
    let request = format!(
        "GET /json/version HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAccept: application/json\r\nConnection: close\r\n\r\n"
    );
    stream.write_all(request.as_bytes())?;
    let mut raw = Vec::new();
    loop {
        if deadline.expired() {
            return Err(other(format!(
                "owned browser version endpoint timed out ({})",
                bounded_http_detail(&raw)
            )));
        }
        let mut buffer = [0u8; 1024];
        match stream.read(&mut buffer) {
            Ok(0) => {
                break;
            }
            Ok(count) => {
                raw.extend_from_slice(&buffer[..count]);
                if raw.len() > 64 * 1024 {
                    return Err(other("owned browser version endpoint exceeded its bound"));
                }
                if http_body(&raw).is_some() {
                    break;
                }
            }
            Err(error)
                if error.kind() == io::ErrorKind::WouldBlock
                    || error.kind() == io::ErrorKind::TimedOut =>
            {
                thread::sleep(Duration::from_millis(20).min(deadline.remaining()));
            }
            Err(error) => return Err(error),
        }
    }
    let body = http_body(&raw).ok_or_else(|| {
        other(format!(
            "owned browser version endpoint closed early ({})",
            bounded_http_detail(&raw)
        ))
    })?;
    let value: Value = serde_json::from_str(body.trim())?;
    value["webSocketDebuggerUrl"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| other("owned browser version endpoint omitted the debugger URL"))
}

fn http_body(raw: &[u8]) -> Option<&str> {
    let text = std::str::from_utf8(raw).ok()?;
    let (head, body) = text.split_once("\r\n\r\n")?;
    let _ = head;
    extract_json_object(body)
}

fn extract_json_object(text: &str) -> Option<&str> {
    let start = text.find('{')?;
    let rest = &text[start..];
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escape = false;
    for (index, ch) in rest.char_indices() {
        if in_string {
            if escape {
                escape = false;
            } else if ch == '\\' {
                escape = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&rest[..=index]);
                }
            }
            _ => {}
        }
    }
    None
}

fn bounded_http_detail(raw: &[u8]) -> String {
    String::from_utf8_lossy(raw).chars().take(240).collect()
}

fn is_reparse(path: &Path) -> io::Result<bool> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Ok(true);
    }
    use std::os::windows::fs::MetadataExt;
    Ok(metadata.file_attributes() & 0x400 != 0)
}

fn remove_owned(
    path: &Path,
    root: &Path,
    parent_resolved: &Path,
    root_resolved: &Path,
) -> io::Result<()> {
    let absolute = std::path::absolute(path)?;
    if !absolute.starts_with(root) {
        return Err(other("cleanup target escaped the exact owned directory"));
    }
    let current_parent = root
        .parent()
        .ok_or_else(|| other("owned browser directory has no parent"))?
        .canonicalize()?;
    if current_parent != *parent_resolved {
        return Err(other("cleanup parent identity changed"));
    }
    if absolute != *root {
        let parent = absolute
            .parent()
            .ok_or_else(|| other("cleanup child has no parent"))?
            .canonicalize()?;
        if !parent.starts_with(root_resolved) {
            return Err(other("cleanup child parent escaped the owned directory"));
        }
    }
    if !path.exists() && !is_reparse(path).unwrap_or(false) {
        return Ok(());
    }
    if is_reparse(path)? {
        let metadata = fs::symlink_metadata(path)?;
        if metadata.is_dir() {
            fs::remove_dir(path)
        } else {
            fs::remove_file(path)
        }
    } else if path.is_dir() {
        if !absolute.canonicalize()?.starts_with(root_resolved) {
            return Err(other("cleanup resolved directory escaped ownership"));
        }
        for child in fs::read_dir(path)? {
            remove_owned(&child?.path(), root, parent_resolved, root_resolved)?;
        }
        fs::remove_dir(path)
    } else {
        fs::remove_file(path)
    }
}

fn tool_error(message: impl AsRef<str>) -> Value {
    json!({
        "content":[{"type":"text","text":message.as_ref()}],
        "isError":true
    })
}

fn worker_command(
    executable: &Path,
    args: &[std::ffi::OsString],
    cdp: &str,
    codex_home: &Path,
) -> CommandSpec {
    let mut command = CommandSpec::new(executable);
    command.args = args.to_vec();
    command
        .env
        .insert("NUPHUS_MCP_NO_MODEL_DOWNLOAD".into(), Some("1".into()));
    command
        .env
        .insert("NUPHUS_MCP_CONFIRM_WRITE".into(), Some("0".into()));
    command
        .env
        .insert("NUPHUS_MCP_BROWSER_CDP_URL".into(), Some(cdp.into()));
    command
        .env
        .insert("CODEX_HOME".into(), Some(codex_home.as_os_str().to_owned()));
    command
        .env
        .insert("NUPHUS_MCP_ALLOW_PRIVATE_NAV".into(), Some("1".into()));
    command
}

/// Serve one Nuphus stdio connection. The connection deadline is an explicit
/// maximum chosen by its owner. Live global registration remains a later task.
pub fn serve(
    configuration: Configuration,
    input: File,
    output: File,
    cancellation: &Cancellation,
    deadline: Deadline,
) -> io::Result<()> {
    let digest = verify_original(&configuration.executable, &configuration.expected_digest)?;
    fs::create_dir_all(&configuration.account)?;
    let idle = match configuration.idle {
        Some(idle) if !idle.is_zero() => idle,
        Some(_) => return Err(invalid("Nuphus idle bound is invalid")),
        None => idle_seconds(&configuration.source_root)?,
    };
    let cache = configuration.account.join(CATALOGUE_NAME);
    let runtime = configuration.codex_home.join("harness/runtime/nuphus");
    let cdp_url = configuration.browser_cdp_url.clone().or_else(cdp_override);
    let owned = match cdp_url.as_deref() {
        Some(_) => None,
        None => Some(Arc::new(Mutex::new(OwnedBrowser::reserve()?))),
    };
    let cdp = match cdp_url {
        Some(url) => url,
        None => owned
            .as_ref()
            .expect("owned browser reserved")
            .lock()
            .unwrap()
            .endpoint
            .clone(),
    };
    let references = Arc::new(Mutex::new(BrowserReferences::new()));
    let references_before = Arc::clone(&references);
    let owned_before = owned.clone();
    let runtime_before = runtime.clone();
    let before = Box::new(move |name: &str, arguments: &Value| {
        if !name.starts_with("browser_") {
            return Ok(Ok(None));
        }
        let rewritten = references_before
            .lock()
            .unwrap()
            .arguments(arguments.clone())
            .map_err(Rejection::new);
        let rewritten = match rewritten {
            Ok(value) => value,
            Err(rejection) => return Ok(Err(rejection)),
        };
        if let Some(browser) = &owned_before {
            browser.lock().unwrap().ensure_started(&runtime_before)?;
        }
        Ok(Ok(Some(rewritten)))
    });
    let references_after = Arc::clone(&references);
    let after = Box::new(move |name: &str, arguments: &Value, result: Value| {
        if name == "browser_snapshot" {
            return references_after
                .lock()
                .unwrap()
                .snapshot(result)
                .map_err(other);
        }
        if name == "browser_navigate" || name == "browser_close" {
            references_after.lock().unwrap().expire();
            return Ok(result);
        }
        if SCREENSHOT_TOOLS.contains(&name) {
            return match bound_screenshot_result(result, arguments) {
                Ok(value) => Ok(value),
                Err(error) => Ok(tool_error(bounded_detail(&error.message))),
            };
        }
        Ok(result)
    });
    let lease_account = configuration.account.clone();
    let lease_for = Box::new(move |name: &str, _arguments: &Value| {
        if name.starts_with("browser_") {
            None
        } else {
            Some(Box::new(DesktopLease {
                directory: lease_account.clone(),
                held: None,
            }) as Box<dyn lazy_stdio::Lease>)
        }
    });
    let launch = WorkerLaunch {
        command: worker_command(
            &configuration.executable,
            &configuration.worker_args,
            &cdp,
            &configuration.codex_home,
        ),
        stderr: runtime.join("worker-stderr.txt"),
        limits: Limits::default(),
        initialize: json!({
            "protocolVersion":"2024-11-05",
            "capabilities":{},
            "clientInfo":{"name":"harness-nuphus","version":env!("CARGO_PKG_VERSION")}
        }),
        expected_server: configuration.expected_server.clone(),
        startup: REQUEST,
    };
    let worker = LazyWorker::new(launch, idle, Some(lease_for), Some(before), Some(after))?;
    {
        let references_idle = Arc::clone(&references);
        let owned_idle = owned.clone();
        worker.set_on_idle(Box::new(move || {
            references_idle.lock().unwrap().expire();
            if let Some(browser) = &owned_idle {
                let mut browser = browser.lock().unwrap();
                if browser.started {
                    browser.reclaim()?;
                    *browser = OwnedBrowser::reserve()?;
                    return Ok(vec![(
                        "NUPHUS_MCP_BROWSER_CDP_URL".into(),
                        Some(browser.endpoint.clone().into()),
                    )]);
                }
            }
            Ok(Vec::new())
        }));
    }
    let tools = match load_catalogue(&cache, &digest)? {
        Some(tools) => tools,
        None => {
            let listed = worker.list_tools(Deadline::after(REQUEST)?, cancellation)?;
            atomic_json(
                &cache,
                &json!({"identity": digest, "tools": listed.clone()}),
            )?;
            listed
        }
    };
    let definitions = adapt_catalogue(tools)?;
    let session = Session::new("harness-nuphus", definitions)?;
    let served = mcp_stdio::serve_fallible(session, input, output, cancellation, deadline, {
        let worker = Arc::clone(&worker);
        move |operation: Operation| {
            worker.call(
                &operation.name,
                operation.arguments.clone(),
                operation.deadline,
                &operation.cancellation,
            )
        }
    });
    let closed = worker.close();
    if let Some(browser) = owned {
        let _ = browser.lock().unwrap().reclaim();
    }
    closed?;
    served
}

#[cfg(test)]
mod tests {
    #[test]
    fn audited_original_digest_is_required() {
        assert_eq!(
            crate::nuphus_protocol::audited_digest("0.2.2"),
            Some(crate::nuphus_protocol::AUDITED_ORIGINALS[0].1)
        );
        assert!(crate::nuphus_protocol::audited_digest("9.9.9").is_none());
    }
}
