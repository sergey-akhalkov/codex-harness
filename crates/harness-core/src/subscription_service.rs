//! Native subscription service host. Task Scheduler launches this process;
//! OpenCodex runs inside an owned 2048 MiB job assigned before resume.
#![cfg(windows)]

use crate::{
    process::{Cancellation, CommandSpec, Deadline, Job, Limits, StopReason},
    subscription_lifecycle,
};
use serde_json::{Value, json};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    net::{Ipv4Addr, TcpStream},
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const MEMORY: usize = 2048 * 1024 * 1024;
const READY: Duration = Duration::from_secs(90);
const RETRY: Duration = Duration::from_secs(60);
const RESTORE: Duration = Duration::from_secs(30);
const CLEANUP: Duration = Duration::from_secs(10);
const MAX_RETRIES: u32 = 3;
const SECRET_LIMIT: usize = 8192;

fn other(message: impl Into<String>) -> io::Error {
    io::Error::other(message.into())
}

pub struct HostOptions {
    pub ready: Duration,
    pub retry_delay: Duration,
    pub memory_bytes: usize,
    pub restore: bool,
}

impl Default for HostOptions {
    fn default() -> Self {
        Self {
            ready: READY,
            retry_delay: RETRY,
            memory_bytes: MEMORY,
            restore: true,
        }
    }
}

/// Task Scheduler / explicit CLI entry. Runtime failures retry at most three
/// times, one minute apart. Ownership and configuration failures do not retry.
pub fn serve(state_path: &Path) -> io::Result<()> {
    serve_with(state_path, HostOptions::default())
}

pub fn serve_with(state_path: &Path, options: HostOptions) -> io::Result<()> {
    let state_path = std::path::absolute(state_path)?;
    let log = HostLog::create(&state_path)?;
    log.stage("entry", None, 0)?;
    let result = (|| {
        let descriptor = read_json(&state_path)?;
        if descriptor["schema_version"] != 1 || descriptor["owner"] != "codex-harness-subscriptions"
        {
            return Err(other(
                "Subscription ownership record mismatch; preserving state.",
            ));
        }
        let source = path_field(&descriptor, "source")?;
        let user = path_field(&descriptor, "user")?;
        let home = path_field(&descriptor, "codex")?;
        let paths = subscription_lifecycle::service_paths(&source, &user, &home)?;
        if state_path != paths.service {
            return Err(other("Subscription service descriptor path mismatch."));
        }
        subscription_lifecycle::assert_owned(&descriptor, &paths)?;
        let config_link = subscription_lifecycle::current_link_target(&paths.config_link)?;
        if config_link.as_deref() != Some(paths.config_source.as_path()) {
            return Err(other(
                "Subscription service configuration link is missing or changed; preserving host state.",
            ));
        }
        let mut last = Ok(());
        for attempt in 1..=MAX_RETRIES + 1 {
            log.stage("service-attempt", None, attempt)?;
            match run_once(&descriptor, &paths, &options, &log) {
                Ok(()) => {
                    log.stage("completed", None, attempt)?;
                    return Ok(());
                }
                Err(error) if attempt <= MAX_RETRIES && is_runtime(&error) => {
                    log.stage("service-retry", Some(&error), attempt)?;
                    last = Err(error);
                    std::thread::sleep(options.retry_delay);
                }
                Err(error) => return Err(error),
            }
        }
        log.stage("service-exhausted", last.as_ref().err(), MAX_RETRIES + 1)?;
        last
    })();
    if let Err(error) = &result {
        let _ = log.stage("failed:service-host", Some(error), 0);
    }
    result
}

fn is_runtime(error: &io::Error) -> bool {
    error.to_string().contains("Subscription runtime failed")
}

fn run_once(
    descriptor: &Value,
    paths: &subscription_lifecycle::ServicePaths,
    options: &HostOptions,
    log: &HostLog,
) -> io::Result<()> {
    assert_runtime_owned(paths)?;
    let port = descriptor["port"]
        .as_u64()
        .filter(|port| (1024..=65535).contains(port))
        .ok_or_else(|| other("Subscription service port is invalid"))? as u16;
    assert_port_free(port)?;
    let dependency = descriptor
        .get("dependency")
        .filter(|value| !value.is_null())
        .ok_or_else(|| other("Subscription service has no runtime dependency"))?;
    let bun = path_field(dependency, "bun")?;
    if !bun.is_file() {
        return Err(other("Subscription runtime executable is missing"));
    }
    let fixture = dependency["fixture"] == true;
    if !fixture {
        assert_zai_source(&paths.config_source)?;
        let cli = path_field(dependency, "cli")?;
        if !cli.is_file() {
            return Err(other("Subscription runtime CLI is missing"));
        }
    }
    let mut command = CommandSpec::new(&bun);
    command.current_dir = Some(paths.source.clone());
    if fixture {
        command.args = vec!["start".into(), "--port".into(), port.to_string().into()];
    } else {
        command.args = vec![
            "--no-env-file".into(),
            path_field(dependency, "cli")?.into(),
            "start".into(),
            "--port".into(),
            port.to_string().into(),
        ];
    }
    command
        .env
        .insert("CODEX_HOME".into(), Some(paths.home.as_os_str().to_owned()));
    command.env.insert(
        "OPENCODEX_HOME".into(),
        Some(paths.opencodex.as_os_str().to_owned()),
    );
    command.env.insert("OCX_SERVICE".into(), Some("1".into()));
    if let Some(secret) = read_secret(&paths.zai_key)? {
        command
            .env
            .insert("ZAI_API_KEY".into(), Some(secret.into()));
    }
    if fixture && dependency["fail"] == true {
        command
            .env
            .insert("HARNESS_SUBSCRIPTION_FIXTURE_FAIL".into(), Some("1".into()));
    }
    fs::create_dir_all(&paths.runtime)?;
    let prefix = paths.runtime.join(unique_name());
    command.stdout = Some(File::create_new(prefix.with_extension("stdout"))?);
    command.stderr = Some(File::create_new(prefix.with_extension("stderr"))?);
    let started_path = prefix.with_extension("started.json");
    write_json(
        &prefix.with_extension("request.json"),
        &json!({
            "executable": bun,
            "secretFiles": secret_files(paths),
            "memoryLimitMiB": options.memory_bytes / (1024 * 1024)
        }),
    )?;
    remove_stopped_markers(paths)?;
    let job = Job::new(Limits {
        memory_bytes: Some(options.memory_bytes),
        cpu_percent: None,
    })?;
    let suspended = job.spawn_suspended(&command)?;
    let pid = suspended.process().identity().pid;
    write_json(
        &started_path,
        &json!({"processId": pid, "assignedBeforeResume": true}),
    )?;
    write_json(
        &paths.runtime.join("active-run.json"),
        &json!({"started": started_path}),
    )?;
    let child = suspended.resume()?;
    let snapshot = job.snapshot()?;
    if snapshot.memory_limit_bytes != options.memory_bytes || !snapshot.kill_on_close {
        let _ = job.terminate(126, CLEANUP);
        return Err(other("Windows job limits changed unexpectedly"));
    }
    let ready_until = Instant::now() + options.ready;
    let mut ready = false;
    let pending = paths.pending.exists();
    while child.is_running()? {
        if !ready {
            if readyz(port, pid)? {
                if !pending {
                    publish_role(paths)?;
                }
                ready = true;
                log.stage("ready", None, 0)?;
            } else if Instant::now() >= ready_until {
                let _ = job.wait(
                    &child,
                    Deadline::after(CLEANUP)?,
                    &Cancellation::default(),
                    CLEANUP,
                );
                return Err(other(
                    "Subscription runtime failed: startup did not become ready within 90 seconds",
                ));
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let outcome = job.wait(
        &child,
        Deadline::after(CLEANUP)?,
        &Cancellation::default(),
        CLEANUP,
    )?;
    let restore_error = if options.restore && !fixture {
        restore_native(paths, dependency).err()
    } else {
        None
    };
    let role_error = withdraw_role(paths).err();
    if !ready || outcome.reason != StopReason::Exited || outcome.exit_code != 0 {
        return Err(other(format!(
            "Subscription runtime failed ({:?}, exit {})",
            outcome.reason, outcome.exit_code
        )));
    }
    if let Some(error) = restore_error.or(role_error) {
        write_json(
            &paths.runtime.join("recovery-required.json"),
            &json!({"message": error.to_string(), "action": "Run Recover, then Install."}),
        )?;
        return Err(error);
    }
    Ok(())
}

fn restore_native(
    paths: &subscription_lifecycle::ServicePaths,
    dependency: &Value,
) -> io::Result<()> {
    let bun = path_field(dependency, "bun")?;
    let script = paths.source.join("tools/opencodex-native-restore.mjs");
    if !script.is_file() {
        return Ok(());
    }
    let mut command = CommandSpec::new(bun);
    command.args = vec![
        "--no-env-file".into(),
        script.into(),
        path_field(dependency, "root")?.into(),
    ];
    command
        .env
        .insert("CODEX_HOME".into(), Some(paths.home.as_os_str().to_owned()));
    command.env.insert(
        "OPENCODEX_HOME".into(),
        Some(paths.opencodex.as_os_str().to_owned()),
    );
    let job = Job::new(Limits {
        memory_bytes: Some(768 * 1024 * 1024),
        cpu_percent: None,
    })?;
    let child = job.spawn(&command)?;
    let outcome = job.wait(
        &child,
        Deadline::after(RESTORE)?,
        &Cancellation::default(),
        CLEANUP,
    )?;
    if outcome.reason != StopReason::Exited || outcome.exit_code != 0 {
        return Err(other(format!(
            "Native routing restore failed ({:?}, exit {})",
            outcome.reason, outcome.exit_code
        )));
    }
    Ok(())
}

fn publish_role(paths: &subscription_lifecycle::ServicePaths) -> io::Result<()> {
    let state = read_json(&paths.state)?;
    subscription_lifecycle::assert_owned(&state, paths)?;
    let recorded = state["links"]["roleLink"]
        .as_str()
        .map(PathBuf::from)
        .ok_or_else(|| other("Subscription service role source changed; run Install."))?;
    if subscription_lifecycle::path_display(&recorded)?
        != subscription_lifecycle::path_display(&paths.role_source)?
    {
        return Err(other(
            "Subscription service role source changed; run Install.",
        ));
    }
    let current = subscription_lifecycle::current_link_target(&paths.role_link)?;
    if let Some(actual) = current.as_deref()
        && actual != paths.role_source.as_path()
    {
        return Err(other(
            "Foreign subscription role preserved during service restart.",
        ));
    }
    subscription_lifecycle::set_owned_link(
        &paths.role_link,
        Some(&paths.role_source),
        current.as_deref(),
    )
}

fn withdraw_role(paths: &subscription_lifecycle::ServicePaths) -> io::Result<()> {
    let current = subscription_lifecycle::current_link_target(&paths.role_link)?;
    if current.as_deref() == Some(paths.role_source.as_path()) {
        subscription_lifecycle::set_owned_link(&paths.role_link, None, current.as_deref())?;
    }
    Ok(())
}

fn assert_runtime_owned(paths: &subscription_lifecycle::ServicePaths) -> io::Result<()> {
    let Some(runtime) = read_json_optional(&paths.opencodex.join("runtime-port.json"))? else {
        return Ok(());
    };
    let started = started_receipt(paths)?;
    if started.and_then(|value| value["processId"].as_u64()) != runtime["pid"].as_u64() {
        return Err(other(
            "Another OpenCodex runtime owns this home; native routing is preserved.",
        ));
    }
    Ok(())
}

fn started_receipt(paths: &subscription_lifecycle::ServicePaths) -> io::Result<Option<Value>> {
    let Some(active) = read_json_optional(&paths.runtime.join("active-run.json"))? else {
        return Ok(None);
    };
    let Some(started) = active["started"].as_str() else {
        return Ok(None);
    };
    let started = PathBuf::from(started);
    if started.parent() != Some(paths.runtime.as_path()) {
        return Err(other(
            "Foreign subscription process receipt path preserved.",
        ));
    }
    read_json_optional(&started)
}

fn remove_stopped_markers(paths: &subscription_lifecycle::ServicePaths) -> io::Result<()> {
    for name in ["runtime-port.json", "ocx.pid"] {
        let path = paths.opencodex.join(name);
        if !path.exists() {
            continue;
        }
        let pid = if name == "runtime-port.json" {
            read_json(&path)?["pid"].as_u64()
        } else {
            fs::read_to_string(&path)?.trim().parse().ok()
        };
        let Some(pid) = pid.filter(|pid| *pid > 0) else {
            return Err(other("Invalid OpenCodex PID marker preserved."));
        };
        if process_alive(pid as u32) {
            return Err(other(
                "Subscription process is still running; preserving its runtime markers.",
            ));
        }
        fs::remove_file(&path)?;
    }
    Ok(())
}

fn process_alive(pid: u32) -> bool {
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        return false;
    }
    let mut code = 0;
    let alive = unsafe { GetExitCodeProcess(handle, &mut code) } != 0 && code == 259;
    unsafe {
        windows_sys::Win32::Foundation::CloseHandle(handle);
    }
    alive
}

fn assert_port_free(port: u16) -> io::Result<()> {
    drop(std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, port))?);
    Ok(())
}

fn assert_zai_source(config: &Path) -> io::Result<()> {
    let value = read_json(config)?;
    let zai = &value["providers"]["zai"];
    if zai["adapter"] != "openai-chat"
        || zai["baseUrl"] != "https://api.z.ai/api/coding/paas/v4"
        || zai["authMode"] != "key"
        || zai["apiKey"] != "${ZAI_API_KEY}"
        || zai["selectedModels"] != json!(["glm-5.3"])
    {
        return Err(other("Z.AI provider must use the Coding Plan Chat route."));
    }
    Ok(())
}

fn readyz(port: u16, pid: u32) -> io::Result<bool> {
    let mut stream = match TcpStream::connect_timeout(
        &(std::net::SocketAddr::from((Ipv4Addr::LOCALHOST, port))),
        Duration::from_secs(2),
    ) {
        Ok(stream) => stream,
        Err(_) => return Ok(false),
    };
    stream.set_read_timeout(Some(Duration::from_millis(500)))?;
    stream.set_write_timeout(Some(Duration::from_millis(500)))?;
    let request =
        format!("GET /readyz HTTP/1.0\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
    let _ = stream.write_all(request.as_bytes());
    let mut raw = Vec::new();
    let mut buffer = [0u8; 1024];
    for _ in 0..32 {
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => {
                raw.extend_from_slice(&buffer[..count]);
                if raw.len() > 16 * 1024 {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    let text = String::from_utf8_lossy(&raw);
    let Some(body) = text.split("\r\n\r\n").nth(1) else {
        return Ok(false);
    };
    let Ok(value) = serde_json::from_str::<Value>(body.trim()) else {
        return Ok(false);
    };
    Ok(value["status"] == "ready"
        && value["service"] == "opencodex"
        && value["pid"] == pid
        && value["port"] == port)
}

fn read_secret(path: &Path) -> io::Result<Option<String>> {
    match fs::read(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
        Ok(bytes) => {
            if bytes.is_empty() || bytes.len() > SECRET_LIMIT || bytes.contains(&0) {
                return Err(other("Secret file must be between 1 and 8192 bytes."));
            }
            let text = String::from_utf8(bytes)
                .map_err(|_| other("Secret file must be UTF-8"))?
                .trim()
                .to_owned();
            if text.is_empty() || text.contains(['\r', '\n']) {
                return Err(other("Secret file must be a single non-empty line."));
            }
            Ok(Some(text))
        }
    }
}

fn secret_files(paths: &subscription_lifecycle::ServicePaths) -> Value {
    if paths.zai_key.is_file() {
        json!({"ZAI_API_KEY": paths.zai_key})
    } else {
        json!({})
    }
}

fn path_field(value: &Value, name: &str) -> io::Result<PathBuf> {
    value[name]
        .as_str()
        .map(PathBuf::from)
        .ok_or_else(|| other(format!("Subscription field {name} is missing")))
}

fn read_json(path: &Path) -> io::Result<Value> {
    serde_json::from_slice(&fs::read(path)?).map_err(|_| other("subscription JSON is invalid"))
}

fn read_json_optional(path: &Path) -> io::Result<Option<Value>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(
            serde_json::from_slice(&bytes).map_err(|_| other("subscription JSON is invalid"))?,
        )),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn write_json(path: &Path, value: &Value) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut bytes = serde_json::to_vec_pretty(value)?;
    if !bytes.ends_with(b"\n") {
        bytes.push(b'\n');
    }
    fs::write(path, bytes)
}

fn unique_name() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_nanos())
        .unwrap_or(0);
    format!("{}-{}", nanos, std::process::id())
}

struct HostLog {
    file: std::sync::Mutex<File>,
}

impl HostLog {
    fn create(state_path: &Path) -> io::Result<Self> {
        let directory = state_path
            .parent()
            .ok_or_else(|| other("service descriptor has no parent"))?
            .join("runs");
        fs::create_dir_all(&directory)?;
        let path = directory.join(format!("{}.host.jsonl", unique_name()));
        Ok(Self {
            file: std::sync::Mutex::new(
                OpenOptions::new().create_new(true).write(true).open(path)?,
            ),
        })
    }

    fn stage(&self, stage: &str, failure: Option<&io::Error>, attempt: u32) -> io::Result<()> {
        let mut record = json!({
            "time": epoch_stamp(),
            "processId": std::process::id(),
            "stage": stage
        });
        if attempt > 0 {
            record["attempt"] = json!(attempt);
            record["maxRetries"] = json!(MAX_RETRIES);
        }
        if let Some(error) = failure {
            record["exceptionType"] = json!("std::io::Error");
            record["kind"] = json!(format!("{:?}", error.kind()));
        }
        let mut line = serde_json::to_vec(&record)?;
        line.push(b'\n');
        let mut file = self.file.lock().unwrap();
        file.write_all(&line)?;
        file.flush()
    }
}

fn epoch_stamp() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}.{:03}Z", now.as_secs(), now.subsec_millis())
}
