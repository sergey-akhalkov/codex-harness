//! Owned process observer equivalent to the reproduce-regression process_case helper.
//!
//! A fresh anonymous job is assigned before resume. Cleanup never looks up a
//! process by name or PID. Child stdout and stderr stay in the case root.

use harness_core::inventory;
use harness_core::process::{
    Cancellation, CommandSpec, Deadline, Job, Limits, Outcome, StopReason,
};
use serde_json::{Value, json};
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

const CLEANUP: Duration = Duration::from_secs(5);
const POLL: Duration = Duration::from_millis(50);
const MEMORY_LIMIT: usize = 512 * 1024 * 1024;
const DEFAULT_TIMEOUT: u64 = 10;
const DEFAULT_OUTPUT_LIMIT: u64 = 16 * 1024 * 1024;
const MARKER_LIMIT: u64 = 64;

#[derive(Clone, Debug)]
pub struct CaseRequest {
    pub argv: Vec<String>,
    pub cwd: PathBuf,
    pub timeout: u64,
    pub ready_timeout: Option<u64>,
    pub output_limit: u64,
    pub stdin: Option<PathBuf>,
    pub root: Option<PathBuf>,
    pub cancellation: Cancellation,
}

impl Default for CaseRequest {
    fn default() -> Self {
        Self {
            argv: Vec::new(),
            cwd: PathBuf::from("."),
            timeout: DEFAULT_TIMEOUT,
            ready_timeout: None,
            output_limit: DEFAULT_OUTPUT_LIMIT,
            stdin: None,
            root: None,
            cancellation: Cancellation::default(),
        }
    }
}

pub fn run_cli(args: &[OsString]) -> io::Result<i32> {
    let request = parse_args(args)?;
    let result = run_case(request)?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    let ok = result.get("status") == Some(&json!("exited"))
        && result
            .get("native")
            .and_then(|native| native.get("ExitCode"))
            .and_then(Value::as_u64)
            == Some(0);
    Ok(if ok { 0 } else { 1 })
}

/// Invalid requests return Err. Launch and wait failures stay in the JSON
/// receipt as infrastructure-failure, matching the Python helper.
pub fn run_case(request: CaseRequest) -> io::Result<Value> {
    validate(&request)?;
    let started = Instant::now();
    let root = allocate_root(request.root.as_deref())?;
    let mut result = match execute(&request, &root) {
        Ok(value) => value,
        Err(error) => json!({
            "status": "infrastructure-failure",
            "ready": false,
            "native": Value::Null,
            "error": error.to_string(),
        }),
    };
    finish(&mut result, &root, started, request.output_limit)?;
    Ok(result)
}

fn allocate_root(requested: Option<&Path>) -> io::Result<PathBuf> {
    match requested {
        Some(path) => create_new_root(path),
        None => Ok(tempfile::Builder::new()
            .prefix("harness-process-case-")
            .tempdir()?
            .keep()),
    }
}

fn create_new_root(path: &Path) -> io::Result<PathBuf> {
    if !path.is_absolute() {
        return Err(invalid("case root must be an absolute new directory"));
    }
    inventory::ordinary_parents(path)?;
    fs::create_dir(path).map_err(|error| {
        if error.kind() == io::ErrorKind::AlreadyExists {
            invalid("case root must be a new directory")
        } else {
            error
        }
    })?;
    inventory::ordinary_parents(&path.join("probe"))?;
    fs::canonicalize(path)
}

fn validate(request: &CaseRequest) -> io::Result<()> {
    if request.argv.is_empty()
        || request.argv.iter().any(|arg| arg.contains('\0'))
        || !Path::new(&request.argv[0]).is_absolute()
    {
        return Err(invalid(
            "Require an absolute executable and separately tokenized string arguments",
        ));
    }
    if !(1..=600).contains(&request.timeout) {
        return Err(invalid(
            "Require argv, timeout 1..600 and readiness within execution deadline",
        ));
    }
    if let Some(ready) = request.ready_timeout
        && !(1..=request.timeout).contains(&ready)
    {
        return Err(invalid(
            "Require argv, timeout 1..600 and readiness within execution deadline",
        ));
    }
    if request.output_limit < 1 {
        return Err(invalid("output_limit must be positive"));
    }
    Ok(())
}

fn execute(request: &CaseRequest, root: &Path) -> io::Result<Value> {
    #[cfg(not(windows))]
    {
        let _ = (request, root);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Windows 10+ Job Objects are required",
        ))
    }
    #[cfg(windows)]
    {
        execute_windows(request, root)
    }
}

#[cfg(windows)]
fn execute_windows(request: &CaseRequest, root: &Path) -> io::Result<Value> {
    let cwd = fs::canonicalize(&request.cwd)?;
    if !cwd.is_dir() {
        return Err(io::Error::other("Working directory must be a directory."));
    }
    let program = PathBuf::from(&request.argv[0]);
    let stdout_path = root.join("stdout.txt");
    let stderr_path = root.join("stderr.txt");
    let started_path = root.join("started.json");
    write_json(
        &root.join("request.json"),
        &json!({
            "executable": path_string(&program),
            "arguments": request.argv[1..].to_vec(),
            "workingDirectory": path_string(&cwd),
            "stdoutPath": path_string(&stdout_path),
            "stderrPath": path_string(&stderr_path),
            "startedPath": path_string(&started_path),
            "timeoutSeconds": request.timeout,
            "memoryLimitMiB": 512,
            "environment": {"PROCESS_CASE_ROOT": path_string(root)},
            "stdinPath": request.stdin.as_ref().map(|path| path_string(path)),
        }),
    )?;
    let mut spec = CommandSpec::new(&program);
    spec.args = request.argv[1..].iter().map(OsString::from).collect();
    spec.current_dir = Some(cwd);
    spec.env
        .insert("PROCESS_CASE_ROOT".into(), Some(path_string(root).into()));
    if let Some(path) = &request.stdin {
        spec.stdin = Some(File::open(path)?);
    }
    spec.stdout = Some(create_new(&stdout_path)?);
    spec.stderr = Some(create_new(&stderr_path)?);
    let job = Job::new(Limits {
        memory_bytes: Some(MEMORY_LIMIT),
        cpu_percent: None,
    })?;
    let suspended = job.spawn_suspended(&spec)?;
    if !job.contains(suspended.process())? {
        return Err(io::Error::other(
            "created process is outside its required job",
        ));
    }
    let identity = suspended.process().identity();
    write_json(
        &started_path,
        &json!({
            "processId": identity.pid,
            "assignedBeforeResume": true,
        }),
    )?;
    let child = suspended.resume()?;
    drop(spec);
    let cancellation = request.cancellation.clone();
    let stop = Arc::new(AtomicBool::new(false));
    let ready_flag = Arc::new(AtomicBool::new(false));
    let output_flag = Arc::new(AtomicBool::new(false));
    let readiness_timeout = Arc::new(AtomicBool::new(false));
    let user_cancel = Arc::new(AtomicBool::new(false));
    let clock = Instant::now();
    let watcher = {
        let cancellation = cancellation.clone();
        let stop = stop.clone();
        let ready_flag = ready_flag.clone();
        let output_flag = output_flag.clone();
        let readiness_timeout = readiness_timeout.clone();
        let user_cancel = user_cancel.clone();
        let root = root.to_path_buf();
        let stdout_path = stdout_path.clone();
        let stderr_path = stderr_path.clone();
        let output_limit = request.output_limit;
        let ready_timeout = request.ready_timeout;
        std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                if is_ready(&root) {
                    ready_flag.store(true, Ordering::Relaxed);
                }
                if file_len(&stdout_path) > output_limit || file_len(&stderr_path) > output_limit {
                    output_flag.store(true, Ordering::Relaxed);
                    cancellation.cancel();
                    break;
                }
                if is_cancel(&root) {
                    user_cancel.store(true, Ordering::Relaxed);
                    cancellation.cancel();
                    break;
                }
                if let Some(seconds) = ready_timeout
                    && !ready_flag.load(Ordering::Relaxed)
                    && clock.elapsed() >= Duration::from_secs(seconds)
                {
                    readiness_timeout.store(true, Ordering::Relaxed);
                    cancellation.cancel();
                    break;
                }
                std::thread::sleep(POLL);
            }
        })
    };
    let outcome = job.wait(
        &child,
        Deadline::after(Duration::from_secs(request.timeout))?,
        &cancellation,
        CLEANUP,
    );
    stop.store(true, Ordering::Relaxed);
    let _ = watcher.join();
    let outcome = outcome?;
    let ready = ready_flag.load(Ordering::Relaxed) || is_ready(root);
    let limit_hit = output_flag.load(Ordering::Relaxed)
        || file_len(&stdout_path) > request.output_limit
        || file_len(&stderr_path) > request.output_limit;
    let status = classify(
        &outcome,
        ready,
        request.ready_timeout.is_some(),
        limit_hit,
        readiness_timeout.load(Ordering::Relaxed),
        user_cancel.load(Ordering::Relaxed) || request.cancellation.is_cancelled(),
    );
    Ok(json!({
        "status": status,
        "ready": ready,
        "native": {
            "Status": job_status(&outcome),
            "ExitCode": outcome.exit_code,
            "ProcessExitCode": outcome.process_exit_code,
            "ProcessId": identity.pid,
            "AssignedBeforeResume": true,
            "MemoryLimitBytes": outcome.job.memory_limit_bytes,
            "PeakJobMemoryBytes": outcome.job.peak_job_memory_bytes,
        },
        "error": Value::Null,
        "reason": format!("{:?}", outcome.reason),
        "job": {
            "active_processes": outcome.job.active_processes,
            "kill_on_close": outcome.job.kill_on_close,
            "memory_limit_bytes": outcome.job.memory_limit_bytes,
            "peak_job_memory_bytes": outcome.job.peak_job_memory_bytes,
        },
    }))
}

fn classify(
    outcome: &Outcome,
    ready: bool,
    ready_required: bool,
    limit_hit: bool,
    readiness_timeout: bool,
    cancelled: bool,
) -> &'static str {
    if limit_hit {
        return "output-limit";
    }
    if readiness_timeout {
        return "readiness-timeout";
    }
    if cancelled && matches!(outcome.reason, StopReason::Cancelled) {
        return "cancelled";
    }
    match outcome.reason {
        StopReason::Exited if ready_required && !ready => "readiness-failure",
        StopReason::Exited => "exited",
        StopReason::Timeout => "timeout",
        StopReason::Cancelled => "cancelled",
        StopReason::MemoryLimit => "memory-limit",
    }
}

fn job_status(outcome: &Outcome) -> &'static str {
    match outcome.reason {
        StopReason::Exited => "exited",
        StopReason::Timeout => "timeout",
        StopReason::Cancelled => "cancelled",
        StopReason::MemoryLimit => "memory-limit",
    }
}

fn finish(result: &mut Value, root: &Path, started: Instant, output_limit: u64) -> io::Result<()> {
    let stdout = root.join("stdout.txt");
    let stderr = root.join("stderr.txt");
    let ready = result
        .get("ready")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || is_ready(root);
    result["ready"] = json!(ready);
    result["root"] = json!(path_string(root));
    result["elapsed_seconds"] = json!(started.elapsed().as_secs_f64());
    result["streams"] = json!({
        "stdout": {"path": path_string(&stdout), "bytes": file_len_opt(&stdout)},
        "stderr": {"path": path_string(&stderr), "bytes": file_len_opt(&stderr)},
    });
    let over = file_len(&stdout) > output_limit || file_len(&stderr) > output_limit;
    result["output_limit_reached"] = json!(over);
    if over && result.get("status") == Some(&json!("exited")) {
        result["status"] = json!("output-limit");
    }
    write_json(&root.join("observed.json"), result)?;
    write_json(&root.join("report.json"), result)?;
    Ok(())
}

fn parse_args(args: &[OsString]) -> io::Result<CaseRequest> {
    let mut request = CaseRequest {
        cwd: std::env::current_dir()?,
        ..CaseRequest::default()
    };
    let mut iter = args.iter().peekable();
    while let Some(arg) = iter.next() {
        let name = arg.to_str().ok_or_else(|| invalid("non-utf8 argument"))?;
        if name == "--help" {
            return Err(invalid(
                "harness-observe --cwd DIR --timeout SECONDS [--ready-timeout SECONDS] [--output-limit BYTES] [--stdin FILE] [--root DIR] -- <absolute-exe> [args...]",
            ));
        }
        if name == "--" {
            request.argv = rest(&mut iter)?;
            break;
        }
        if !name.starts_with("--") {
            request.argv.push(name.to_owned());
            request.argv.extend(rest(&mut iter)?);
            break;
        }
        let value = iter
            .next()
            .and_then(|v| v.to_str().map(str::to_owned))
            .ok_or_else(|| invalid(&format!("missing value for {name}")))?;
        match name {
            "--cwd" => request.cwd = PathBuf::from(value),
            "--timeout" => request.timeout = parse_u64(&value, "--timeout")?,
            "--ready-timeout" => {
                request.ready_timeout = Some(parse_u64(&value, "--ready-timeout")?)
            }
            "--output-limit" => request.output_limit = parse_u64(&value, "--output-limit")?,
            "--stdin" => request.stdin = Some(PathBuf::from(value)),
            "--root" => request.root = Some(PathBuf::from(value)),
            other => return Err(invalid(&format!("unknown argument {other}"))),
        }
    }
    if request.argv.is_empty() {
        return Err(invalid(
            "Require an absolute executable and separately tokenized string arguments",
        ));
    }
    Ok(request)
}

fn rest(iter: &mut std::iter::Peekable<std::slice::Iter<'_, OsString>>) -> io::Result<Vec<String>> {
    let mut argv = Vec::new();
    for arg in iter {
        argv.push(
            arg.to_str()
                .ok_or_else(|| invalid("non-utf8 argument"))?
                .to_owned(),
        );
    }
    Ok(argv)
}

fn parse_u64(value: &str, name: &str) -> io::Result<u64> {
    value
        .parse()
        .map_err(|_| invalid(&format!("{name} must be an integer")))
}

fn is_ready(root: &Path) -> bool {
    marker_is(root, "ready.txt", "READY")
}

fn is_cancel(root: &Path) -> bool {
    marker_is(root, "cancel.txt", "CANCEL")
}

fn marker_is(root: &Path, name: &str, expected: &str) -> bool {
    bounded_marker(&root.join(name)).is_ok_and(|text| text.as_deref() == Some(expected))
}

fn bounded_marker(path: &Path) -> io::Result<Option<String>> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let mut bytes = Vec::new();
    file.take(MARKER_LIMIT + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MARKER_LIMIT {
        return Ok(None);
    }
    Ok(String::from_utf8(bytes)
        .ok()
        .map(|text| text.trim().to_owned()))
}

fn create_new(path: &Path) -> io::Result<File> {
    OpenOptions::new().write(true).create_new(true).open(path)
}

fn write_json(path: &Path, value: &Value) -> io::Result<()> {
    inventory::ordinary_parents(path)?;
    let mut file = create_new(path)?;
    file.write_all(&serde_json::to_vec_pretty(value)?)?;
    file.flush()
}

fn file_len(path: &Path) -> u64 {
    fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

fn file_len_opt(path: &Path) -> Option<u64> {
    fs::metadata(path).ok().map(|m| m.len())
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}
