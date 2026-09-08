//! Bounded structured Codex inspection helper.
//!
//! Native process jobs own the launched prefix and the independent oracle. The
//! helper records separate final JSON and event JSONL evidence and never calls a
//! model itself.

use harness_core::process::{
    Cancellation, CommandSpec, Deadline, Job, Limits, Outcome, StopReason,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const SCHEMA: &str = r#"{
  "type": "object",
  "additionalProperties": false,
  "required": ["run_id", "findings", "unresolved_issues"],
  "properties": {
    "run_id": {"type": "string"},
    "findings": {
      "type": "array",
      "items": {
        "type": "object",
        "additionalProperties": false,
        "required": ["path", "line", "description", "evidence"],
        "properties": {
          "path": {"type": "string"},
          "line": {"type": "integer"},
          "description": {"type": "string"},
          "evidence": {"type": "string"}
        }
      }
    },
    "unresolved_issues": {"type": "array", "items": {"type": "string"}}
  }
}"#;

const CLEANUP: Duration = Duration::from_secs(5);
const GIT_TIMEOUT: Duration = Duration::from_secs(10);
const POLL: Duration = Duration::from_millis(50);
const PROMPT_LIMIT: usize = 262144;
const MEMORY_LIMIT: usize = 512 * 1024 * 1024;

#[derive(Debug)]
pub struct InspectionRequest {
    pub launch: Vec<String>,
    pub cwd: PathBuf,
    pub prompt: String,
    pub oracle: Vec<String>,
    pub model: String,
    pub provider: String,
    pub subscription: String,
    pub inputs: Vec<String>,
    pub timeout: u64,
    pub output_limit: u64,
    pub codex_home: Option<PathBuf>,
}

#[derive(Debug)]
pub struct InspectionResult {
    pub status: String,
    pub value: Value,
}

pub fn run_cli(args: &[OsString]) -> io::Result<i32> {
    let request = parse_args(args)?;
    let result = run_inspection(request);
    println!("{}", serde_json::to_string(&result.value)?);
    Ok(if result.status == "success" { 0 } else { 1 })
}

pub fn run_inspection(request: InspectionRequest) -> InspectionResult {
    match run_inspection_inner(request) {
        Ok(result) => result,
        Err(error) => InspectionResult {
            status: "infrastructure-failure".into(),
            value: json!({
                "run_id": "",
                "status": "infrastructure-failure",
                "evidence_root": "",
                "error_type": "Error",
                "error": error.to_string(),
            }),
        },
    }
}

fn run_inspection_inner(request: InspectionRequest) -> io::Result<InspectionResult> {
    validate_request(&request)?;
    let cwd = fs::canonicalize(&request.cwd)?;
    if !cwd.is_dir() {
        return Err(io::Error::other("cwd must be an existing directory"));
    }
    if fs::canonicalize(std::env::temp_dir())?.starts_with(&cwd) {
        return Err(io::Error::other(
            "Evidence must be outside the inspected directory; select a narrower target checkout.",
        ));
    }
    let evidence = tempfile::Builder::new()
        .prefix("structured-codex-")
        .tempdir()?
        .keep();
    let run_id = unique_run_id();
    let mut result = json!({
        "run_id": run_id,
        "status": "incomplete",
        "evidence_root": path_string(&evidence),
    });
    write_json(&evidence.join("acceptance.json"), &result)?;
    if let Err(error) = execute(&request, &cwd, &evidence, &run_id, &mut result) {
        result["status"] = json!("infrastructure-failure");
        result["error_type"] = json!("Error");
        result["error"] = json!(error.to_string());
    }
    write_json(&evidence.join("acceptance.json"), &result)?;
    Ok(InspectionResult {
        status: result["status"]
            .as_str()
            .unwrap_or("infrastructure-failure")
            .to_owned(),
        value: result,
    })
}

struct Observe<'a> {
    argv: &'a [String],
    cwd: &'a Path,
    timeout: u64,
    output_limit: u64,
    stdin: Option<&'a Path>,
    stdout: &'a Path,
    stderr: &'a Path,
    codex_home: Option<&'a Path>,
    watched_file: Option<&'a Path>,
    limit_marker: Option<&'a Path>,
}

fn execute(
    request: &InspectionRequest,
    cwd: &Path,
    evidence: &Path,
    run_id: &str,
    result: &mut Value,
) -> io::Result<()> {
    let schema_path = evidence.join("inspection.schema.json");
    fs::write(&schema_path, SCHEMA.as_bytes())?;
    let before = fingerprint(cwd, &request.inputs)?;
    let git = git_state(cwd)?;
    let final_path = evidence.join("final.json");
    let events_path = evidence.join("events.jsonl");
    let stderr_path = evidence.join("stderr.txt");
    let stdin_path = evidence.join("stdin.txt");
    let limit_marker = evidence.join("output-limit.txt");
    let mut argv = request.launch.clone();
    argv.extend([
        "exec".into(),
        "--model".into(),
        request.model.clone(),
        "-c".into(),
        format!("model_provider={}", json!(request.provider)),
        "-c".into(),
        "approval_policy=\"never\"".into(),
        "--sandbox".into(),
        "read-only".into(),
        "--ephemeral".into(),
        "--json".into(),
        "-C".into(),
        path_string(cwd),
        "--output-schema".into(),
        path_string(&schema_path),
        "--output-last-message".into(),
        path_string(&final_path),
        "-".into(),
    ]);
    let prompt = format!(
        "{}

Return the contracted inspection JSON with run_id exactly {run_id}. Do not modify files or delegate.",
        request.prompt
    );
    fs::write(&stdin_path, prompt.as_bytes())?;
    write_json(
        &evidence.join("contract.json"),
        &json!({
            "run_id": run_id,
            "cwd": path_string(cwd),
            "git": git,
            "inputs": before,
            "model": request.model,
            "provider": request.provider,
            "subscription": request.subscription,
            "allowed_effects": "read-only",
            "argv": argv,
            "timeout": request.timeout,
            "output_limit": request.output_limit,
            "oracle": request.oracle,
            "executable_sha256": hash_file(Path::new(&request.launch[0]))?,
            "schema_sha256": hash_bytes(SCHEMA.as_bytes()),
            "codex_home": request
                .codex_home
                .as_ref()
                .map(|path| path_string(path))
                .unwrap_or_else(|| "inherited".into()),
            "started_at": unix_time(),
        }),
    )?;
    let receipt = observe_command(Observe {
        argv: &argv,
        cwd,
        timeout: request.timeout,
        output_limit: request.output_limit,
        stdin: Some(stdin_path.as_path()),
        stdout: &events_path,
        stderr: &stderr_path,
        codex_home: request.codex_home.as_deref(),
        watched_file: Some(final_path.as_path()),
        limit_marker: Some(limit_marker.as_path()),
    })?;
    write_json(&evidence.join("process.json"), &receipt)?;
    let status = if limit_marker.exists() && receipt.get("status") == Some(&json!("exited")) {
        "output-limit".into()
    } else {
        inspect_result(evidence, run_id, &receipt, request.output_limit)?
            .unwrap_or_else(|| "oracle-pending".into())
    };
    result["status"] = json!(status);
    let after = fingerprint(cwd, &request.inputs)?;
    write_json(&evidence.join("inputs-after.json"), &json!(after))?;
    if before != after {
        result["inputs_changed"] = json!(true);
        if result["status"] == "oracle-pending" {
            result["status"] = json!("inputs-changed");
        }
    }
    if result["status"] == "oracle-pending" {
        let mut oracle = request.oracle.clone();
        oracle.push(path_string(&final_path));
        let oracle_stdout = evidence.join("oracle-stdout.txt");
        let oracle_stderr = evidence.join("oracle-stderr.txt");
        let oracle_receipt = observe_command(Observe {
            argv: &oracle,
            cwd,
            timeout: request.timeout.min(30),
            output_limit: request.output_limit,
            stdin: None,
            stdout: &oracle_stdout,
            stderr: &oracle_stderr,
            codex_home: None,
            watched_file: None,
            limit_marker: None,
        })?;
        write_json(&evidence.join("oracle.json"), &oracle_receipt)?;
        result["status"] = json!(oracle_status(&oracle_receipt));
        let after_oracle = fingerprint(cwd, &request.inputs)?;
        write_json(
            &evidence.join("inputs-after-oracle.json"),
            &json!(after_oracle),
        )?;
        if after_oracle != before {
            result["inputs_changed"] = json!(true);
            if result["status"] == "success" {
                result["status"] = json!("inputs-changed");
            }
        }
    }
    Ok(())
}

fn inspect_result(
    evidence: &Path,
    run_id: &str,
    receipt: &Value,
    output_limit: u64,
) -> io::Result<Option<String>> {
    let stderr = if evidence.join("stderr.txt").is_file() {
        let mut text = String::new();
        let _ = File::open(evidence.join("stderr.txt"))?
            .take(64 * 1024)
            .read_to_string(&mut text);
        text
    } else {
        String::new()
    };
    if let Some(failure) = process_status(receipt, &stderr) {
        return Ok(Some(failure));
    }
    let paths = [
        evidence.join("final.json"),
        evidence.join("events.jsonl"),
        evidence.join("stderr.txt"),
    ];
    if evidence.join("output-limit.txt").exists()
        || paths
            .iter()
            .any(|path| path.is_file() && file_len(path).unwrap_or(0) > output_limit)
    {
        return Ok(Some("output-limit".into()));
    }
    let final_path = evidence.join("final.json");
    if !final_path.is_file() {
        return Ok(Some("missing-json".into()));
    }
    let value = match read_json(&final_path) {
        Ok(value) => value,
        Err(_) => return Ok(Some("malformed-json".into())),
    };
    if !validate_inspection(&value) {
        return Ok(Some("schema-invalid".into()));
    }
    if value.get("run_id").and_then(Value::as_str) != Some(run_id) {
        return Ok(Some("stale-json".into()));
    }
    let events = match parse_events(&evidence.join("events.jsonl")) {
        Ok(events) => events,
        Err(_) => return Ok(Some("malformed-events".into())),
    };
    if events.is_empty() || !events.iter().all(Value::is_object) {
        return Ok(Some("task-incomplete".into()));
    }
    if events.iter().any(|event| {
        matches!(
            event.get("type").and_then(Value::as_str),
            Some("error" | "turn.failed")
        )
    }) {
        return Ok(Some("task-failure".into()));
    }
    if events
        .last()
        .and_then(|event| event.get("type"))
        .and_then(Value::as_str)
        != Some("turn.completed")
    {
        return Ok(Some("task-incomplete".into()));
    }
    if value
        .get("unresolved_issues")
        .and_then(Value::as_array)
        .is_some_and(|issues| !issues.is_empty())
    {
        return Ok(Some("unresolved-issues".into()));
    }
    Ok(None)
}

fn process_status(receipt: &Value, stderr: &str) -> Option<String> {
    let status = receipt.get("status").and_then(Value::as_str)?;
    if status != "exited" {
        return Some(status.to_owned());
    }
    let code = receipt
        .get("native")
        .and_then(|native| native.get("ExitCode"))
        .and_then(Value::as_u64)?;
    if code == 0 {
        return None;
    }
    if code >= 0x8000_0000 {
        return Some("terminated".into());
    }
    if auth_failure(stderr) {
        return Some("auth-failure".into());
    }
    Some("process-failure".into())
}

fn oracle_status(receipt: &Value) -> String {
    if receipt.get("status").and_then(Value::as_str) != Some("exited") {
        return "oracle-failure".into();
    }
    match receipt
        .get("native")
        .and_then(|native| native.get("ExitCode"))
        .and_then(Value::as_u64)
    {
        Some(0) => "success".into(),
        Some(1) => "wrong-answer".into(),
        _ => "oracle-failure".into(),
    }
}

fn observe_command(request: Observe<'_>) -> io::Result<Value> {
    let started = SystemTime::now();
    command(request.argv)?;
    let program = PathBuf::from(&request.argv[0]);
    if !program.is_file()
        || !program
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
    {
        return Err(io::Error::other(
            "Native CLI prefix must be an actual EXE; keep an explicit provided shell prefix when a script launcher is required",
        ));
    }
    let mut spec = CommandSpec::new(&program);
    spec.args = request.argv[1..].iter().map(OsString::from).collect();
    spec.current_dir = Some(request.cwd.to_path_buf());
    if let Some(home) = request.codex_home {
        spec.env
            .insert("CODEX_HOME".into(), Some(path_string(home).into()));
    }
    if let Some(path) = request.stdin {
        spec.stdin = Some(File::open(path)?);
    }
    spec.stdout = Some(create_file(request.stdout)?);
    spec.stderr = Some(create_file(request.stderr)?);
    let job = Job::new(Limits {
        memory_bytes: Some(MEMORY_LIMIT),
        cpu_percent: None,
    })?;
    let child = job.spawn(&spec)?;
    drop(spec);
    let cancellation = Cancellation::default();
    let stop = Arc::new(AtomicBool::new(false));
    let watcher = {
        let cancellation = cancellation.clone();
        let stop = stop.clone();
        let stdout = request.stdout.to_path_buf();
        let stderr = request.stderr.to_path_buf();
        let watched = request.watched_file.map(Path::to_path_buf);
        let marker = request.limit_marker.map(Path::to_path_buf);
        let output_limit = request.output_limit;
        std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                let over = file_len(&stdout).unwrap_or(0) > output_limit
                    || file_len(&stderr).unwrap_or(0) > output_limit
                    || watched.as_deref().is_some_and(|path| {
                        path.is_file() && file_len(path).unwrap_or(0) > output_limit
                    });
                if over {
                    if let Some(path) = &marker {
                        let _ = fs::write(path, b"final output limit");
                    }
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
    let limit_hit = request.limit_marker.is_some_and(Path::exists)
        || file_len(request.stdout).unwrap_or(0) > request.output_limit
        || file_len(request.stderr).unwrap_or(0) > request.output_limit
        || request.watched_file.is_some_and(|path| {
            path.is_file() && file_len(path).unwrap_or(0) > request.output_limit
        });
    let mut receipt = json!({
        "status": receipt_status(&outcome, limit_hit),
        "native": {
            "ExitCode": outcome.exit_code,
            "ProcessExitCode": outcome.process_exit_code,
        },
        "reason": format!("{:?}", outcome.reason),
        "elapsed_seconds": elapsed(started),
        "streams": {
            "stdout": {"path": path_string(request.stdout), "bytes": file_len(request.stdout).ok()},
            "stderr": {"path": path_string(request.stderr), "bytes": file_len(request.stderr).ok()},
        },
        "output_limit_reached": limit_hit,
        "job": {
            "active_processes": outcome.job.active_processes,
            "peak_job_memory_bytes": outcome.job.peak_job_memory_bytes,
        }
    });
    receipt["outcome"] = serde_json::to_value(outcome).map_err(io::Error::other)?;
    Ok(receipt)
}

fn receipt_status(outcome: &Outcome, limit_hit: bool) -> &'static str {
    if limit_hit {
        return "output-limit";
    }
    match outcome.reason {
        StopReason::Exited => "exited",
        StopReason::Timeout => "timeout",
        StopReason::Cancelled => "cancelled",
        StopReason::MemoryLimit => "memory-limit",
    }
}

fn parse_args(args: &[OsString]) -> io::Result<InspectionRequest> {
    let mut values: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let name = arg
            .to_str()
            .ok_or_else(|| io::Error::other("non-utf8 argument"))?;
        if !name.starts_with("--") {
            return Err(io::Error::other(format!("unknown argument {name}")));
        }
        let value = iter
            .next()
            .and_then(|v| v.to_str().map(str::to_owned))
            .ok_or_else(|| io::Error::other(format!("missing value for {name}")))?;
        values.entry(name.to_owned()).or_default().push(value);
    }
    let required = |name: &str| {
        values
            .get(name)
            .and_then(|v| v.last())
            .cloned()
            .ok_or_else(|| io::Error::other(format!("{name} is required")))
    };
    let timeout = values
        .get("--timeout")
        .and_then(|v| v.last())
        .map(|v| parse_u64(v, "--timeout"))
        .transpose()?
        .unwrap_or(180);
    let output_limit = values
        .get("--output-limit")
        .and_then(|v| v.last())
        .map(|v| parse_u64(v, "--output-limit"))
        .transpose()?
        .unwrap_or(1_048_576);
    let mut prompt = fs::read_to_string(required("--prompt-file")?)?;
    if let Some(rest) = prompt.strip_prefix('\u{feff}') {
        prompt = rest.to_owned();
    }
    Ok(InspectionRequest {
        launch: parse_json_array(&required("--command-json")?)?,
        cwd: PathBuf::from(required("--cwd")?),
        prompt,
        oracle: parse_json_array(&required("--oracle-json")?)?,
        model: required("--model")?,
        provider: required("--provider")?,
        subscription: required("--subscription")?,
        inputs: values.get("--input").cloned().unwrap_or_default(),
        timeout,
        output_limit,
        codex_home: values
            .get("--codex-home")
            .and_then(|v| v.last())
            .map(PathBuf::from),
    })
}

fn validate_request(request: &InspectionRequest) -> io::Result<()> {
    command(&request.launch)?;
    command(&request.oracle)?;
    if !(1..=600).contains(&request.timeout) || request.output_limit < 1 {
        return Err(io::Error::other(
            "Require timeout 1..600 and positive output limit",
        ));
    }
    for (name, value) in [
        ("model", &request.model),
        ("provider", &request.provider),
        ("subscription", &request.subscription),
        ("prompt", &request.prompt),
    ] {
        if value.trim().is_empty() {
            return Err(io::Error::other(format!("{name} must be nonempty")));
        }
    }
    if request.prompt.len() > PROMPT_LIMIT {
        return Err(io::Error::other("Prompt exceeds 256 KiB"));
    }
    if request.inputs.is_empty() {
        return Err(io::Error::other(
            "Record at least one relevant source input",
        ));
    }
    Ok(())
}

fn command(value: &[String]) -> io::Result<()> {
    if value.is_empty()
        || value.iter().any(|arg| arg.contains(' '))
        || !Path::new(&value[0]).is_absolute()
    {
        return Err(io::Error::other(
            "Require an absolute executable and tokenized argument array",
        ));
    }
    Ok(())
}

fn fingerprint(cwd: &Path, inputs: &[String]) -> io::Result<BTreeMap<String, String>> {
    let mut result = BTreeMap::new();
    for name in inputs {
        let resolved = fs::canonicalize(cwd.join(name))?;
        if !resolved.starts_with(cwd) || !resolved.is_file() {
            return Err(io::Error::other(
                "Inputs must be files inside the target checkout",
            ));
        }
        result.insert(name.clone(), hash_file(&resolved)?);
    }
    Ok(result)
}

fn git_state(cwd: &Path) -> io::Result<Value> {
    let git = resolve_exe("git")?;
    let head = git_output(&git, cwd, &["rev-parse", "HEAD"])?;
    let dirty = git_output(
        &git,
        cwd,
        &["status", "--porcelain=v1", "--untracked-files=all"],
    )?;
    Ok(json!({"head": head, "dirty": dirty}))
}

fn git_output(git: &Path, cwd: &Path, args: &[&str]) -> io::Result<String> {
    let stdout = tempfile::NamedTempFile::new()?;
    let stderr = tempfile::NamedTempFile::new()?;
    let mut spec = CommandSpec::new(git);
    spec.args = args.iter().map(OsString::from).collect();
    spec.current_dir = Some(cwd.to_path_buf());
    spec.stdout = Some(stdout.reopen()?);
    spec.stderr = Some(stderr.reopen()?);
    let job = Job::new(Limits::default())?;
    let child = job.spawn(&spec)?;
    drop(spec);
    let outcome = job.wait(
        &child,
        Deadline::after(GIT_TIMEOUT)?,
        &Cancellation::default(),
        CLEANUP,
    )?;
    if outcome.reason != StopReason::Exited || outcome.exit_code != 0 {
        return Err(io::Error::other("git identity command failed"));
    }
    let mut text = String::new();
    File::open(stdout.path())?.read_to_string(&mut text)?;
    Ok(text.trim().to_owned())
}

fn resolve_exe(name: &str) -> io::Result<PathBuf> {
    if let Some(paths) = std::env::var_os("PATH") {
        for directory in std::env::split_paths(&paths).filter(|p| p.is_absolute()) {
            let mut candidate = directory.join(name);
            candidate.set_extension("exe");
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }
    Err(io::Error::other(format!(
        "{name}.exe is required to record checkout identity"
    )))
}

fn parse_events(path: &Path) -> io::Result<Vec<Value>> {
    let text = fs::read_to_string(path)?;
    let mut events = Vec::new();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        events.push(serde_json::from_str(line).map_err(io::Error::other)?);
    }
    Ok(events)
}

fn validate_inspection(value: &Value) -> bool {
    validate_schema(
        value,
        &serde_json::from_str(SCHEMA).expect("bundled schema"),
    )
}

fn validate_schema(value: &Value, schema: &Value) -> bool {
    match schema.get("type").and_then(Value::as_str).unwrap_or("") {
        "object" => {
            let Some(object) = value.as_object() else {
                return false;
            };
            let required: Vec<_> = schema
                .get("required")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect();
            let properties = schema
                .get("properties")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            let names: Vec<_> = object.keys().cloned().collect();
            let property_names: Vec<_> = properties.keys().cloned().collect();
            if !(set_eq(&names, &required) && set_eq(&names, &property_names)) {
                return false;
            }
            properties.iter().all(|(key, child)| {
                object
                    .get(key)
                    .is_some_and(|item| validate_schema(item, child))
            })
        }
        "array" => {
            let Some(items) = value.as_array() else {
                return false;
            };
            let Some(child) = schema.get("items") else {
                return false;
            };
            items.iter().all(|item| validate_schema(item, child))
        }
        "string" => value.is_string(),
        "integer" => value.is_i64(),
        _ => false,
    }
}

fn set_eq(left: &[String], right: &[String]) -> bool {
    let mut a = left.to_vec();
    let mut b = right.to_vec();
    a.sort();
    b.sort();
    a == b
}

fn auth_failure(stderr: &str) -> bool {
    let lower = stderr.to_ascii_lowercase();
    [
        "unauthorized",
        "authentication failed",
        "invalid api key",
        "401",
    ]
    .iter()
    .any(|needle| contains_word(&lower, needle))
}

fn contains_word(haystack: &str, needle: &str) -> bool {
    let Some(index) = haystack.find(needle) else {
        return false;
    };
    let before = haystack[..index]
        .chars()
        .next_back()
        .is_none_or(|ch| !ch.is_ascii_alphanumeric());
    let after = haystack[index + needle.len()..]
        .chars()
        .next()
        .is_none_or(|ch| !ch.is_ascii_alphanumeric());
    before && after
}

fn parse_json_array(text: &str) -> io::Result<Vec<String>> {
    let value: Value = serde_json::from_str(text).map_err(io::Error::other)?;
    value
        .as_array()
        .ok_or_else(|| io::Error::other("JSON array required"))?
        .iter()
        .map(|item| {
            item.as_str()
                .map(str::to_owned)
                .ok_or_else(|| io::Error::other("JSON array of strings required"))
        })
        .collect()
}

fn parse_u64(value: &str, name: &str) -> io::Result<u64> {
    value
        .parse()
        .map_err(|_| io::Error::other(format!("{name} must be an integer")))
}

fn write_json(path: &Path, value: &Value) -> io::Result<()> {
    fs::write(path, serde_json::to_vec_pretty(value)?)
}

fn read_json(path: &Path) -> io::Result<Value> {
    let text = fs::read_to_string(path)?;
    serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(io::Error::other)
}

fn create_file(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)
}

fn file_len(path: &Path) -> io::Result<u64> {
    Ok(if path.exists() {
        fs::metadata(path)?.len()
    } else {
        0
    })
}

fn hash_file(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut digest = Sha256::new();
    io::copy(&mut file, &mut digest)?;
    Ok(format!("{:x}", digest.finalize()))
}

fn hash_bytes(bytes: &[u8]) -> String {
    let mut digest = Sha256::new();
    digest.update(bytes);
    format!("{:x}", digest.finalize())
}

fn unique_run_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    hash_bytes(format!("{nanos}-{}", std::process::id()).as_bytes())[..32].to_owned()
}

fn unix_time() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

fn elapsed(started: SystemTime) -> f64 {
    started.elapsed().unwrap_or_default().as_secs_f64()
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}
