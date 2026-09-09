//! One explicit native outcome attempt. Correctness remains the caller's oracle.
#[path = "outcome_events.rs"]
mod events;

use harness_core::build_identity::{hash_file, ordinary};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

const MODEL: &str = "gpt-6-astra";
const EFFORT: &str = "xhigh";
const INPUT_LIMIT: u64 = 4 * 1024 * 1024;
const PROMPT_LIMIT: usize = 1024 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    case_root: PathBuf,
    codex_home: PathBuf,
    /// The caller selects the native linked launcher, never a PATH search.
    launcher: PathBuf,
    prompt: String,
    #[serde(default = "default_timeout")]
    timeout: u64,
    #[serde(default = "default_output_limit")]
    output_limit: u64,
    #[serde(default)]
    extra_config: BTreeMap<String, Value>,
    useful_command_pattern: Option<String>,
    cancel_file: Option<PathBuf>,
}
fn default_timeout() -> u64 {
    600
}
fn default_output_limit() -> u64 {
    128 * 1024 * 1024
}

pub fn run(args: &[OsString]) -> io::Result<i32> {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!(
            "codex-harness outcome-run --request PATH --run-model-probes\nRuns one explicitly selected native launcher in an isolated temporary case/home. Private evidence is retained; correctness requires a separate oracle."
        );
        return Ok(0);
    }
    let mut request = None;
    let mut opted_in = false;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--request" && request.is_none() {
            request = Some(PathBuf::from(iter.next().ok_or_else(invalid)?));
        } else if arg == "--run-model-probes" && !opted_in {
            opted_in = true;
        } else {
            return Err(invalid());
        }
    }
    // No request read, discovery, model or evidence-directory creation by default.
    if !opted_in {
        println!(
            "{}",
            json!({"status":"skipped","reason":"explicit --run-model-probes required","model_calls":0})
        );
        return Ok(0);
    }
    let path = request.ok_or_else(invalid)?;
    let bytes = bounded_read(&path, INPUT_LIMIT).map_err(|_| invalid())?;
    let request: Request = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
    let result = attempt(request)?;
    let code = if result["status"] == "completed" {
        0
    } else {
        1
    };
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(code)
}

fn attempt(request: Request) -> io::Result<Value> {
    let private = tempfile::Builder::new()
        .prefix("codex-outcome-native-")
        .tempdir()?;
    let root = private.path().canonicalize()?;
    if root.starts_with(repository()) {
        return Err(invalid());
    }
    // Preserve every started attempt, including validation/launch failure.
    let _ = private.keep();
    let started = events::now();
    let mut result = json!({"evidence_root":root,"status":"incomplete",
        "started_at":started,"ended_at":null,"thread_id":null,"children":[],
        "rollout_paths":[],"first_useful_signal":null,
        "usage":{"status":"unknown","total_tokens":null},
        "final_path":root.join("final.txt"),"result_path":root.join("result.json")});
    write_new(&root.join("native-started.json"), &result)?;
    if let Err(error) = execute(&request, &root, &mut result) {
        let phase = result
            .as_object_mut()
            .expect("owned result")
            .remove("failure_phase")
            .unwrap_or(json!("validation"));
        result["status"] = json!("failed");
        result["error_type"] = json!("native_outcome_failure");
        // Neither parser text, prompt nor command arguments enter public errors.
        write_new(
            &root.join("failure.json"),
            &json!({"phase":phase,"kind":format!("{:?}",error.kind()),"raw_os_error":error.raw_os_error(),"message":error.to_string()}),
        )?;
    }
    result["ended_at"] = json!(events::now());
    result["elapsed_seconds"] = json!(events::now() - started);
    write_new(&root.join("native.json"), &result)?;
    Ok(result)
}

fn validate(request: &Request) -> io::Result<(PathBuf, PathBuf, Vec<String>)> {
    if !(1..=604800).contains(&request.timeout)
        || !(1024..=512 * 1024 * 1024).contains(&request.output_limit)
        || request.prompt.len() > PROMPT_LIMIT
        || request.prompt.contains('\0')
        || !request.launcher.is_absolute()
        || !request.launcher.is_file()
        || !request
            .launcher
            .extension()
            .is_some_and(|s| s.eq_ignore_ascii_case("exe"))
        || request
            .cancel_file
            .as_ref()
            .is_some_and(|p| !p.is_absolute())
        || request
            .useful_command_pattern
            .as_ref()
            .is_some_and(|s| s.len() > 4096)
    {
        return Err(invalid());
    }
    let case = isolated(&request.case_root)?;
    let home = isolated(&request.codex_home)?;
    if case.starts_with(&home) || home.starts_with(&case) {
        return Err(invalid());
    }
    if let Some(pattern) = &request.useful_command_pattern {
        regex::RegexBuilder::new(pattern)
            .size_limit(1024 * 1024)
            .build()
            .map_err(|_| invalid())?;
    }
    Ok((case, home, config_arguments(&request.extra_config)?))
}

pub(crate) fn isolated(path: &Path) -> io::Result<PathBuf> {
    if !path.is_absolute() {
        return Err(invalid());
    }
    let path = path.canonicalize()?;
    let temp = std::env::temp_dir().canonicalize()?;
    if path == temp || !path.is_dir() || !path.starts_with(temp) || path.starts_with(repository()) {
        return Err(invalid());
    }
    Ok(path)
}

pub(crate) fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")))
}

pub(crate) fn config_arguments(config: &BTreeMap<String, Value>) -> io::Result<Vec<String>> {
    let mut args = Vec::new();
    for (key, value) in config {
        if key.is_empty()
            || key.len() > 256
            || key.split('.').any(|part| {
                part.is_empty()
                    || !part
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
            })
            || [
                "model",
                "profile",
                "credential",
                "auth",
                "forced_login",
                "service_tier",
                "openai_base_url",
                "chatgpt_base_url",
                "cli_auth_credentials_store",
                "oss_provider",
            ]
            .iter()
            .any(|prefix| key.starts_with(prefix))
        {
            return Err(invalid());
        }
        let text = format!("{key}={}", toml_value(value)?);
        toml::from_str::<toml::Table>(&text).map_err(|_| invalid())?;
        args.extend(["-c".into(), text]);
    }
    Ok(args)
}

fn toml_value(value: &Value) -> io::Result<String> {
    Ok(match value {
        Value::Null => return Err(invalid()),
        Value::Bool(_) | Value::Number(_) | Value::String(_) => value.to_string(),
        Value::Array(values) => format!(
            "[{}]",
            values
                .iter()
                .map(toml_value)
                .collect::<io::Result<Vec<_>>>()?
                .join(",")
        ),
        Value::Object(values) => format!(
            "{{{}}}",
            values
                .iter()
                .map(|(k, v)| Ok(format!("{}={}", json!(k), toml_value(v)?)))
                .collect::<io::Result<Vec<_>>>()?
                .join(",")
        ),
    })
}

#[cfg(windows)]
fn execute(request: &Request, root: &Path, result: &mut Value) -> io::Result<()> {
    use harness_core::process::{Cancellation, CommandSpec, Deadline, Job, Limits, StopReason};
    use std::{
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        time::Duration,
    };
    result["failure_phase"] = json!("validation");
    let (case, home, extra) = validate(request)?;
    if root.starts_with(&case) || root.starts_with(&home) {
        return Err(invalid());
    }
    result["failure_phase"] = json!("preparation");
    let final_path = root.join("final.txt");
    let stdout = root.join("events.jsonl");
    let stderr = root.join("stderr.txt");
    let stdin = root.join("stdin.txt");
    let args: Vec<String> = [
        "exec",
        "--strict-config",
        "--skip-git-repo-check",
        "--json",
        "-C",
    ]
    .into_iter()
    .map(str::to_owned)
    .chain([
        case.to_string_lossy().into_owned(),
        "-m".into(),
        MODEL.into(),
        "-c".into(),
        format!("model_reasoning_effort={}", json!(EFFORT)),
    ])
    .chain(extra)
    .chain([
        "--output-last-message".into(),
        final_path.to_string_lossy().into_owned(),
        "-".into(),
    ])
    .collect();
    create(&stdin)?.write_all(request.prompt.as_bytes())?;
    let mut spec = CommandSpec::new(&request.launcher);
    spec.args = args.iter().map(OsString::from).collect();
    spec.current_dir = Some(case.clone());
    spec.env
        .insert("CODEX_HOME".into(), Some(home.as_os_str().into()));
    spec.stdin = Some(File::open(&stdin)?);
    spec.stdout = Some(create(&stdout)?);
    spec.stderr = Some(create(&stderr)?);
    result["executable_sha256"] = json!(hash_file(&request.launcher)?);
    result["model"] = json!(MODEL);
    result["effort"] = json!(EFFORT);
    write_new(
        &root.join("request.json"),
        &json!({"executable":request.launcher,"arguments":args,
        "workingDirectory":case,"stdoutPath":stdout,"stderrPath":stderr,"stdinPath":stdin,
        "memoryLimitMiB":2048,"timeoutSeconds":request.timeout,"outputLimitBytes":request.output_limit,
        "environment":{"CODEX_HOME":home}}),
    )?;
    let mut telemetry = events::Events::open(
        &stdout,
        &root.join("observed.jsonl"),
        request.useful_command_pattern.as_deref(),
    )?;
    let job = Job::new(Limits {
        memory_bytes: Some(2048 * 1024 * 1024),
        cpu_percent: None,
    })?;
    result["failure_phase"] = json!("launch");
    let suspended = job.spawn_suspended(&spec)?;
    if !job.contains(suspended.process())? {
        return Err(invalid());
    }
    let identity = suspended.process().identity();
    write_new(
        &root.join("started.json"),
        &json!({"processId":identity.pid,"assignedBeforeResume":true}),
    )?;
    let child = suspended.resume()?;
    drop(spec);
    let deadline = Deadline::after(Duration::from_secs(request.timeout))?;
    let cancellation = Cancellation::default();
    let stop = Arc::new(AtomicBool::new(false));
    let limit = Arc::new(AtomicBool::new(false));
    let watcher = {
        let stop = stop.clone();
        let limit = limit.clone();
        let cancellation = cancellation.clone();
        let output_limit = request.output_limit;
        let cancel = request.cancel_file.clone();
        let paths = [stdout.clone(), stderr.clone(), final_path.clone()];
        std::thread::spawn(move || -> io::Result<events::Events> {
            let observed = (|| -> io::Result<()> {
                loop {
                    if paths
                        .iter()
                        .any(|path| fs::metadata(path).is_ok_and(|m| m.len() > output_limit))
                    {
                        limit.store(true, Ordering::Relaxed);
                        cancellation.cancel();
                    }
                    if cancel.as_ref().is_some_and(|p| p.is_file()) {
                        cancellation.cancel();
                    }
                    let finished = stop.load(Ordering::Relaxed);
                    telemetry.poll(finished)?;
                    if finished {
                        return Ok(());
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
            })();
            if observed.is_err() {
                cancellation.cancel();
            }
            observed.map(|_| telemetry)
        })
    };
    result["failure_phase"] = json!("observation");
    let outcome = job.wait(&child, deadline, &cancellation, Duration::from_secs(5));
    stop.store(true, Ordering::Relaxed);
    let telemetry = watcher.join().map_err(|_| invalid())?;
    let outcome = outcome?;
    let status = if limit.load(Ordering::Relaxed) {
        "output-limit"
    } else {
        match outcome.reason {
            StopReason::Exited => "exited",
            StopReason::Timeout => "timeout",
            StopReason::Cancelled => "cancelled",
            StopReason::MemoryLimit => "memory-limit",
        }
    };
    let receipt = json!({"Status":status,"ExitCode":outcome.exit_code,"ProcessExitCode":outcome.process_exit_code,
        "ProcessId":identity.pid,"AssignedBeforeResume":true,"MemoryLimitBytes":outcome.job.memory_limit_bytes,
        "PeakJobMemoryBytes":outcome.job.peak_job_memory_bytes,"job":outcome.job});
    write_new(&root.join("result.json"), &receipt)?;
    result["process"] = receipt;
    result["status"] = json!(match status {
        "exited" if outcome.exit_code == 0 => "completed",
        "timeout" => "timeout",
        "cancelled" => "incomplete",
        _ => "failed",
    });
    let telemetry = telemetry?;
    for (key, value) in telemetry.value.as_object().expect("owned telemetry") {
        result[key] = value.clone();
    }
    let mut errors = telemetry.errors;
    if result["status"] == "completed" && result["turn_completed"] != true {
        result["status"] = json!("incomplete");
    }
    if errors.contains("native_error_event") && result["status"] == "completed" {
        result["status"] = json!("failed");
    }
    result["failure_phase"] = json!("usage");
    let paths = rollout_paths(&home, result, &mut errors)?;
    result["rollout_paths"] = json!(paths);
    result["usage"] = usage(&paths);
    let threads = result["usage"]["threads"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let observed: Vec<Value> = threads
        .iter()
        .map(|t| {
            let mut row = serde_json::Map::new();
            for key in ["id", "parent_id", "model", "reasoning", "provider"] {
                row.insert(key.into(), t[key].clone());
            }
            Value::Object(row)
        })
        .collect();
    let expected: std::collections::BTreeSet<String> = result["thread_id"]
        .as_str()
        .into_iter()
        .chain(
            result["children"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str),
        )
        .map(str::to_owned)
        .collect();
    let actual: std::collections::BTreeSet<String> = observed
        .iter()
        .filter_map(|row| row["id"].as_str())
        .map(str::to_owned)
        .collect();
    let identities_match = !expected.is_empty() && expected == actual;
    if !identities_match {
        errors.insert("rollout_identity_mismatch".into());
    }
    if !actual.is_subset(&expected) || actual.len() != observed.len() {
        // A filename match cannot attribute another thread's usage to this run.
        write_new(&root.join("rejected-usage.json"), &result["usage"])?;
        result["usage"] = usage(&[]);
    } else if !identities_match && !paths.is_empty() {
        result["usage"]["partial"] = json!(true);
        result["usage"]["status"] = json!("partial");
    }
    let verified = identities_match
        && errors.is_empty()
        && observed.iter().all(|t| {
            (t["model"] == MODEL || t["model"] == format!("openai/{MODEL}"))
                && t["reasoning"] == EFFORT
                && t["provider"] == "OpenAI"
        });
    if !verified {
        errors.insert("observed_model_policy_unverified".into());
    }
    // This is observed model/effort metadata, not endpoint or authentication proof.
    result["observed_model_metadata_verified"] = json!(verified);
    result["observed_threads"] = json!(observed);
    if result["children"].as_array().is_some_and(|a| !a.is_empty()) {
        errors.insert("unexpected_delegation".into());
    }
    if result["thread_id"].is_null() {
        errors.insert("missing_thread".into());
    }
    if !errors.is_empty() {
        result["evidence_errors"] = json!(errors);
    }
    result
        .as_object_mut()
        .expect("owned result")
        .remove("failure_phase");
    Ok(())
}

#[cfg(not(windows))]
fn execute(_: &Request, _: &Path, _: &mut Value) -> io::Result<()> {
    Err(invalid())
}

fn rollout_paths(
    home: &Path,
    result: &Value,
    errors: &mut std::collections::BTreeSet<String>,
) -> io::Result<Vec<PathBuf>> {
    let mut ids = Vec::new();
    if let Some(id) = result["thread_id"].as_str() {
        ids.push(id.to_owned());
    }
    for id in result["children"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        ids.push(id.to_owned());
    }
    ids.sort();
    ids.dedup();
    ids.retain(|id| id.len() == 36 && id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-'));
    let mut found: BTreeMap<String, Vec<PathBuf>> =
        ids.iter().map(|id| (id.clone(), Vec::new())).collect();
    let sessions = home.join("sessions");
    let mut pending = if sessions.is_dir() {
        vec![sessions]
    } else {
        Vec::new()
    };
    let mut entries = 0_usize;
    while let Some(dir) = pending.pop() {
        ordinary(&dir)?;
        for entry in fs::read_dir(&dir)? {
            entries += 1;
            if entries > 100_000 {
                errors.insert("rollout_discovery_limit".into());
                return Ok(Vec::new());
            }
            let entry = entry?;
            let path = entry.path();
            if ordinary(&path).is_err() {
                errors.insert("rollout_link_skipped".into());
                continue;
            }
            if path.is_dir() {
                pending.push(path);
            } else if let Some(name) = path.file_name().and_then(|s| s.to_str())
                && let Some(stem) = name.strip_suffix(".jsonl")
                && let Some(suffix) = stem.get(stem.len().saturating_sub(36)..)
                && let Some(paths) = found.get_mut(suffix)
            {
                paths.push(path);
            }
        }
    }
    let mut paths = Vec::new();
    for (id, mut matches) in found {
        if matches.len() == 1 {
            paths.push(matches.pop().expect("one match"));
        } else {
            errors.insert(format!("missing_or_ambiguous_rollout:{id}"));
        }
    }
    Ok(paths)
}

fn usage(paths: &[PathBuf]) -> Value {
    let (mut value, _sources) = crate::delegation_usage::summarize(paths);
    if paths.is_empty() {
        for field in [
            "input_tokens",
            "cached_input_tokens",
            "output_tokens",
            "reasoning_output_tokens",
            "total_tokens",
        ] {
            value["totals"][field] = Value::Null;
        }
    }
    value["status"] = json!(if paths.is_empty() {
        "unknown"
    } else if value["partial"] == true {
        "partial"
    } else {
        "known"
    });
    value
}

pub(crate) fn bounded_read(path: &Path, limit: u64) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)?.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(invalid());
    }
    Ok(bytes)
}
pub(crate) fn create(path: &Path) -> io::Result<File> {
    OpenOptions::new().write(true).create_new(true).open(path)
}
pub(crate) fn write_new(path: &Path, value: &Value) -> io::Result<()> {
    let mut file = create(path)?;
    serde_json::to_writer_pretty(&mut file, value)?;
    file.write_all(b"\n")?;
    file.flush()
}
fn invalid() -> io::Error {
    io::Error::other("Invalid native outcome request.")
}
