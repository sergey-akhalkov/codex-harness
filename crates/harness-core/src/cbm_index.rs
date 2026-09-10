//! Audited CBM indexing and tool calls in bounded, owned process trees.
#![cfg(windows)]

use crate::{
    cbm_configuration, dependency_mcp_probe as native,
    process::{Cancellation, CommandSpec, Deadline, Job, Limits, StopReason},
    registration_native::ReadGuard,
    resource_admission::{Lease, Resource},
};
use serde_json::{Value, json};
use std::{
    fs::{self, File},
    io::{self, Read, Write},
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

pub const AUDITED_BUILD: &str = "b4b403b1d7c4def3785f148b93f345ce8427858f4f5489ce28580c4387a336a6";
const OUTPUT_LIMIT: usize = 16 * 1024 * 1024;
const CLEANUP: Duration = Duration::from_secs(5);

#[derive(Debug)]
struct OperationFailure {
    source: io::Error,
    reclaimed: bool,
    public_message: Option<String>,
}
impl std::fmt::Display for OperationFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.source.fmt(formatter)
    }
}
impl std::error::Error for OperationFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}
fn marked_failure(source: io::Error, reclaimed: bool) -> io::Error {
    io::Error::new(
        source.kind(),
        OperationFailure {
            source,
            reclaimed,
            public_message: None,
        },
    )
}
pub(crate) fn failure_reclaimed(error: &io::Error) -> bool {
    error
        .get_ref()
        .and_then(|error| error.downcast_ref::<OperationFailure>())
        .is_some_and(|error| error.reclaimed)
}

/// Only a verified, reclaimed resource stop can become a normal MCP error.
/// Foreign output and private diagnostic paths never enter the public message.
pub(crate) fn resource_failure_message(error: &io::Error) -> Option<&str> {
    let failure = error.get_ref()?.downcast_ref::<OperationFailure>()?;
    failure.reclaimed.then_some(())?;
    failure.public_message.as_deref()
}

#[cfg(test)]
#[path = "cbm_index_acceptance.rs"]
mod acceptance;

#[derive(Clone, Copy)]
enum Invocation<'a> {
    Index,
    Cli(&'a str),
}

/// Parse CLI argument files without accepting duplicate or ambiguous JSON keys.
pub fn parse_arguments(bytes: &[u8]) -> io::Result<Value> {
    if bytes.len() > 16 * 1024 {
        return Err(failure("CBM arguments exceed their byte limit"));
    }
    let value = native::strict_json(bytes)?;
    if !value.is_object() {
        return Err(failure("CBM requires bounded object arguments"));
    }
    Ok(value)
}

/// Execute an audited non-index tool in a fresh private daemon namespace. The
/// caller authorizes the named operation on the selected cache; some tools
/// mutate graph state. The process tree is bounded and never shared with an
/// adopted daemon. Account-wide persistent broker integration is separate.
pub fn call(
    executable: &Path,
    cache: &Path,
    name: &str,
    arguments: &Value,
    cancellation: &Cancellation,
) -> io::Result<Value> {
    if !matches!(
        name,
        "search_graph"
            | "query_graph"
            | "trace_path"
            | "get_code_snippet"
            | "get_graph_schema"
            | "get_architecture"
            | "search_code"
            | "list_projects"
            | "delete_project"
            | "index_status"
            | "check_index_coverage"
            | "detect_changes"
            | "manage_adr"
            | "ingest_traces"
    ) {
        return Err(failure(
            "CBM tool is unknown or requires the audited indexing path",
        ));
    }
    if !arguments.is_object() || serde_json::to_vec(arguments)?.len() > 16 * 1024 {
        return Err(failure("CBM tool requires bounded object arguments"));
    }
    let deadline = Deadline::after(Duration::from_secs(60))?;
    let (_artifact, cache) = (|| {
        let artifact = native::artifact(executable, AUDITED_BUILD, deadline, cancellation)?;
        let cache = crate::dependency_discovery::local_path(cache)?;
        crate::native_build::ordinary_ancestors(&cache)?;
        if !cbm_configuration::read(&cache)?.bounded_policy_active() {
            return Err(failure("CBM resource policy inactive; no worker started"));
        }
        check_daemon_log_available(&cache)?;
        Ok((artifact, cache))
    })()
    .map_err(|error| marked_failure(error, true))?;
    execute_invocation(
        executable,
        &cache,
        None,
        arguments,
        deadline,
        cancellation,
        Invocation::Cli(name),
    )
}

/// CBM 0.10.8 holds its cache-wide operation log open with FILE_SHARE_READ.
/// A second daemon cannot start even in another rendezvous namespace. Reject
/// an observed conflict before spawn; a later race still fails in upstream.
/// This observation is not a lease or authority over an existing daemon.
fn check_daemon_log_available(cache: &Path) -> io::Result<()> {
    match ReadGuard::open(&cache.join("logs/cbm-daemon.log")) {
        Ok(guard) => {
            drop(guard); // The child must be able to open its own log for writing.
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) if error.raw_os_error() == Some(32) => Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            "CBM cache is in use by another daemon; no worker started",
        )),
        Err(_) => Err(failure("CBM daemon log is unavailable; no worker started")),
    }
}

fn failure(message: &'static str) -> io::Error {
    io::Error::other(message)
}

/// Read definitions without opening an installed graph. Account admission stays
/// held until the private catalogue process and its owned descendants stop.
pub fn catalogue(
    executable: &Path,
    account: &Path,
    cancellation: &Cancellation,
) -> io::Result<Value> {
    let _admission = Lease::acquire(
        account,
        Resource::CodebaseCatalogue,
        Deadline::after(Duration::from_secs(20))?,
        cancellation,
    )?;
    native::catalogue(
        executable,
        native::ProbeKind::CodebaseMemory,
        AUDITED_BUILD,
        cancellation,
    )
}

/// Explicit operation: writes the requested graph in the supplied CBM cache.
/// Admission is account-wide and remains held through complete worker cleanup.
/// The caller owns repository authorization and the selected cache/runtime paths.
pub fn index(
    executable: &Path,
    cache: &Path,
    runtime: &Path,
    account: &Path,
    arguments: &Value,
    cancellation: &Cancellation,
) -> io::Result<Value> {
    let deadline = Deadline::after(Duration::from_secs(600))?;
    if !arguments.is_object() || serde_json::to_vec(arguments)?.len() > 16 * 1024 {
        return Err(failure("CBM indexing requires bounded object arguments"));
    }
    let (_artifact, _admission) = (|| {
        let artifact = native::artifact(executable, AUDITED_BUILD, deadline, cancellation)?;
        let admission = Lease::acquire(
            account,
            Resource::CodebaseIndex,
            Deadline::after(Duration::from_secs(2).min(deadline.remaining()))?,
            cancellation,
        )?;
        crate::native_build::ordinary_ancestors(cache)?;
        crate::native_build::ordinary_ancestors(runtime)?;
        // Settings are checked after queueing and before each operation. Drift is
        // never silently repaired and no native process starts on a failed check.
        if !cbm_configuration::read(cache)?.bounded_policy_active() {
            return Err(failure("CBM resource policy inactive; no worker started"));
        }
        Ok((artifact, admission))
    })()
    .map_err(|error| marked_failure(error, true))?;
    execute(
        executable,
        cache,
        runtime,
        arguments,
        deadline,
        cancellation,
    )
}

fn capture(mut stream: File, stop: Cancellation) -> io::Result<Vec<u8>> {
    let mut output = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        let count = match stream.read(&mut buffer) {
            Ok(count) => count,
            Err(_) => {
                stop.cancel();
                return Err(failure("CBM output pipe failed"));
            }
        };
        if count == 0 {
            return Ok(output);
        }
        if output.len().saturating_add(count) > OUTPUT_LIMIT {
            stop.cancel();
            return Err(failure("CBM output exceeded its capture limit"));
        }
        output.extend_from_slice(&buffer[..count]);
    }
}

fn retain_failure(
    state: tempfile::TempDir,
    stdout: &io::Result<Vec<u8>>,
    stderr: &io::Result<Vec<u8>>,
    mut evidence: Value,
) -> (std::path::PathBuf, bool) {
    // Keep first: diagnostic I/O failure must not destroy the original cause.
    let root = state.keep();
    let write = |name: &str, bytes: &[u8]| -> io::Result<()> {
        File::create_new(root.join(name))?.write_all(bytes)
    };
    let mut incomplete = false;
    let response = root.join("response.json");
    match ReadGuard::open(&response) {
        Ok(mut guard) => {
            let mut prefix = Vec::new();
            if (&mut guard.file)
                .take(OUTPUT_LIMIT as u64)
                .read_to_end(&mut prefix)
                .is_err()
                || write("response.bin", &prefix).is_err()
            {
                incomplete = true;
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => (),
        Err(_) => incomplete = true,
    }
    // The upstream output was a polled file, so its full size is not a retention
    // budget. Remove that owned leaf even if saving the bounded prefix failed.
    let response_removed = match fs::remove_file(&response) {
        Ok(()) => true,
        Err(error) if error.kind() == io::ErrorKind::NotFound => true,
        Err(_) => false,
    };
    incomplete |= !response_removed;
    for (name, stream) in [("stdout.bin", stdout), ("stderr.bin", stderr)] {
        if let Ok(bytes) = stream {
            incomplete |= write(name, bytes).is_err();
        }
    }
    evidence["diagnostics_incomplete"] = json!(incomplete);
    evidence["original_response_removed"] = json!(response_removed);
    incomplete |= serde_json::to_vec_pretty(&evidence)
        .and_then(|bytes| write("process.json", &bytes).map_err(serde_json::Error::io))
        .is_err();
    (root, incomplete)
}

fn execute(
    executable: &Path,
    cache: &Path,
    runtime: &Path,
    arguments: &Value,
    deadline: Deadline,
    caller: &Cancellation,
) -> io::Result<Value> {
    execute_invocation(
        executable,
        cache,
        Some(runtime),
        arguments,
        deadline,
        caller,
        Invocation::Index,
    )
}

fn execute_invocation(
    executable: &Path,
    cache: &Path,
    runtime: Option<&Path>,
    arguments: &Value,
    deadline: Deadline,
    caller: &Cancellation,
    invocation: Invocation<'_>,
) -> io::Result<Value> {
    let mut spawn_attempted = false;
    execute_attempt(
        executable,
        cache,
        runtime,
        arguments,
        deadline,
        caller,
        (invocation, &mut spawn_attempted),
    )
    .map_err(|error| {
        if spawn_attempted {
            error
        } else {
            marked_failure(error, true)
        }
    })
}

fn execute_attempt(
    executable: &Path,
    cache: &Path,
    runtime: Option<&Path>,
    arguments: &Value,
    deadline: Deadline,
    caller: &Cancellation,
    (invocation, spawn_attempted): (Invocation<'_>, &mut bool),
) -> io::Result<Value> {
    if matches!(invocation, Invocation::Cli(_)) && runtime.is_some() {
        return Err(failure("CBM CLI runtime must remain private"));
    }
    let state = native::private_directory()?;
    let response = state.path().join("response.json");
    let mut command = CommandSpec::new(executable);
    command.current_dir = Some(state.path().to_owned());
    native::environment(&mut command, state.path())?;
    if matches!(invocation, Invocation::Cli(_)) {
        let expected_runtime = state.path().join("ipc");
        let runtime_text = expected_runtime
            .to_str()
            .filter(|path| !path.is_empty() && path.len() < 4096)
            .ok_or_else(|| {
                failure("CBM private runtime path cannot fit its native environment contract")
            })?;
        if command
            .env
            .get(std::ffi::OsStr::new("CBM_RUNTIME_DIR"))
            .and_then(Option::as_ref)
            .and_then(|path| path.to_str())
            != Some(runtime_text)
        {
            return Err(failure("CBM private runtime environment is inconsistent"));
        }
        crate::native_build::ordinary_ancestors(&expected_runtime)?;
    }
    command
        .env
        .insert("CBM_CACHE_DIR".into(), Some(cache.as_os_str().to_owned()));
    if let Some(runtime) = runtime {
        command.env.insert(
            "CBM_RUNTIME_DIR".into(),
            Some(runtime.as_os_str().to_owned()),
        );
    }
    for (key, value) in [
        ("CBM_WORKERS", "2"),
        ("CBM_MEM_BUDGET_MB", "1024"),
        ("CBM_RETAIN_TOTAL_MB", "8"),
        ("CBM_RETAIN_PER_FILE_MB", "1"),
        ("CBM_LOG_LEVEL", "info"),
    ] {
        command.env.insert(key.into(), Some(value.into()));
    }
    command.args = match invocation {
        Invocation::Index => vec![
            "cli".into(),
            "--index-worker".into(),
            "--index-worker-build".into(),
            AUDITED_BUILD.into(),
            "index_repository".into(),
            serde_json::to_string(arguments)?.into(),
            "--response-out".into(),
            response.clone().into_os_string(),
            "--index-worker-memory-budget-bytes".into(),
            "1073741824".into(),
        ],
        Invocation::Cli(name) => {
            let arguments_path = state.path().join("arguments.json");
            fs::write(&arguments_path, serde_json::to_vec(arguments)?)?;
            vec![
                "cli".into(),
                "--json".into(),
                name.into(),
                "--args-file".into(),
                arguments_path.into_os_string(),
            ]
        }
    };
    let (stdout, out) = native::pipe()?;
    let (stderr, err) = native::pipe()?;
    command.stdout = Some(out);
    command.stderr = Some(err);
    let job = Job::new(Limits {
        memory_bytes: Some(2 * 1024 * 1024 * 1024),
        cpu_percent: Some(25.0),
    })?;
    if caller.is_cancelled() || deadline.expired() {
        return Err(failure("CBM indexing cancelled or expired before launch"));
    }
    *spawn_attempted = true;
    let child = job.spawn(&command)?;
    drop(command);
    let stop = Cancellation::default();
    let reader = |stream, name: &str| {
        let stop = stop.clone();
        std::thread::Builder::new()
            .name(name.into())
            .spawn(move || capture(stream, stop))
    };
    let stdout = match reader(stdout, "cbm-index-stdout") {
        Ok(thread) => thread,
        Err(_) => {
            job.terminate(130, CLEANUP)?;
            return Err(failure("CBM output reader unavailable"));
        }
    };
    let stderr = match reader(stderr, "cbm-index-stderr") {
        Ok(thread) => thread,
        Err(_) => {
            let cleanup = job.terminate(130, CLEANUP);
            let _ = stdout.join();
            cleanup?;
            return Err(failure("CBM error reader unavailable"));
        }
    };
    let finished = Arc::new(AtomicBool::new(false));
    let monitor = {
        let finished = finished.clone();
        let stop = stop.clone();
        let caller = caller.clone();
        let response = response.clone();
        std::thread::Builder::new()
            .name("cbm-index-budget".into())
            .spawn(move || {
                while !finished.load(Ordering::Acquire) {
                    if caller.is_cancelled() {
                        stop.cancel();
                        return Ok(());
                    }
                    match fs::symlink_metadata(&response) {
                        Ok(metadata)
                            if !metadata.is_file() || metadata.len() > OUTPUT_LIMIT as u64 =>
                        {
                            stop.cancel();
                            return Err(failure(
                                "CBM response exceeded its capture limit or changed type",
                            ));
                        }
                        Err(error) if error.kind() != io::ErrorKind::NotFound => {
                            stop.cancel();
                            return Err(failure("CBM response observation failed"));
                        }
                        _ => (),
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                Ok(())
            })
    };
    if monitor.is_err() {
        stop.cancel();
    }
    let outcome = job.wait(&child, deadline, &stop, CLEANUP);
    finished.store(true, Ordering::Release);
    let observed = match monitor {
        Ok(thread) => thread.join().map_err(|_| failure("CBM monitor failed"))?,
        Err(_) => Err(failure("CBM monitor unavailable")),
    };
    // Job.wait has reclaimed every inherited pipe endpoint, including children
    // left behind by an immediately exiting parent, before these joins/reads.
    let stdout = stdout
        .join()
        .map_err(|_| failure("CBM output reader failed"))?;
    let stderr = stderr
        .join()
        .map_err(|_| failure("CBM error reader failed"))?;
    // Both reader threads have joined. Only the retained Job outcome can
    // certify that process cleanup finished; error text/paths cannot do so.
    let reclaimed = outcome
        .as_ref()
        .is_ok_and(|value| value.job.active_processes == 0);
    let evidence = json!({
        "outcome":outcome.as_ref().ok(),
        "monitor_error":observed.as_ref().err().map(ToString::to_string),
        "stdout_error":stdout.as_ref().err().map(ToString::to_string),
        "stderr_error":stderr.as_ref().err().map(ToString::to_string),
        "caller_cancelled":caller.is_cancelled(),"automatic_retry":false
    });
    let public_message = outcome.as_ref().ok().and_then(|outcome| match outcome.reason {
        StopReason::MemoryLimit => Some(format!(
            "CBM worker exceeded its {} MiB memory limit; owned workers reclaimed. Private diagnostics retained.",
            outcome.job.memory_limit_bytes / (1024 * 1024)
        )),
        StopReason::Timeout => Some(
            "CBM worker exceeded its deadline; owned workers reclaimed. Private diagnostics retained."
                .to_owned(),
        ),
        _ => None,
    });
    let completed = (|| -> io::Result<Value> {
        let outcome = outcome?;
        observed?;
        let stdout_bytes = stdout
            .as_ref()
            .map_err(|e| io::Error::new(e.kind(), e.to_string()))?
            .len();
        let stderr_bytes = stderr
            .as_ref()
            .map_err(|e| io::Error::new(e.kind(), e.to_string()))?
            .len();
        if caller.is_cancelled() {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "CBM indexing cancelled",
            ));
        }
        if outcome.reason == StopReason::Timeout {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "CBM indexing deadline exceeded",
            ));
        }
        if outcome.reason != StopReason::Exited
            || match invocation {
                Invocation::Index => outcome.exit_code != 0,
                Invocation::Cli(_) => outcome.exit_code > 1,
            }
        {
            return Err(failure("CBM bounded worker did not finish successfully"));
        }
        let mut file_bytes = Vec::new();
        let bytes = match invocation {
            Invocation::Index => {
                let mut file = ReadGuard::open(&response)?;
                (&mut file.file)
                    .take(OUTPUT_LIMIT as u64 + 1)
                    .read_to_end(&mut file_bytes)?;
                file_bytes.as_slice()
            }
            Invocation::Cli(_) => stdout
                .as_ref()
                .map_err(|e| io::Error::new(e.kind(), e.to_string()))?
                .as_slice(),
        };
        if bytes.len() > OUTPUT_LIMIT {
            return Err(failure("CBM response exceeded its capture limit"));
        }
        if matches!(invocation, Invocation::Cli(_)) && outcome.exit_code == 1 && bytes.is_empty() {
            return Err(failure("CBM CLI failed before returning a tool result"));
        }
        let result = native::strict_json(bytes)?;
        if !result.is_object()
            || !result.get("content").is_some_and(Value::is_array)
            || result.get("isError").is_some_and(|v| !v.is_boolean())
        {
            return Err(failure("CBM response is not a tool result"));
        }
        if matches!(invocation, Invocation::Cli(_))
            && outcome.exit_code != u32::from(result["isError"].as_bool().unwrap_or(false))
        {
            return Err(failure(
                "CBM CLI exit status disagrees with its tool result",
            ));
        }
        let operation = match invocation {
            Invocation::Index => "codebase-memory-index",
            Invocation::Cli(_) => "codebase-memory-tool",
        };
        let response_source = match invocation {
            Invocation::Index => "response-file",
            Invocation::Cli(_) => "stdout",
        };
        let daemon_scope = match invocation {
            Invocation::Index => "no-daemon-worker",
            Invocation::Cli(_) => "fresh-private-runtime",
        };
        Ok(json!({
            "schema_version":1,"operation":operation, "result":result,
            "response_source":response_source,
            "daemon_scope":daemon_scope,
            "artifact_sha256":AUDITED_BUILD,"outcome":outcome,
            "stdout_bytes":stdout_bytes,"stderr_bytes":stderr_bytes,"response_bytes":bytes.len(),
            "automatic_retry":false,"packages_acquired":false,"owned_tree_stopped":outcome.job.active_processes==0,
            "temporary_state_removed":true,
            "capture_limits":{"stdout_bytes":OUTPUT_LIMIT,"stderr_bytes":OUTPUT_LIMIT,
                "response_bytes":OUTPUT_LIMIT,"response_size_poll_ms":10}
        }))
    })();
    match completed {
        Ok(report) => {
            let path = state.path().to_owned();
            state.close().map_err(|_| {
                io::Error::other(format!(
                    "CBM private state cleanup incomplete; private evidence: {}",
                    path.display()
                ))
            })?;
            Ok(report)
        }
        Err(error) => {
            let (retained, incomplete) = retain_failure(state, &stdout, &stderr, evidence);
            let detail = if incomplete {
                "; private diagnostics incomplete"
            } else {
                ""
            };
            Err(io::Error::new(
                error.kind(),
                OperationFailure {
                    source: io::Error::new(
                        error.kind(),
                        format!("{error}{detail}; private evidence: {}", retained.display()),
                    ),
                    reclaimed,
                    public_message: (!incomplete && reclaimed)
                        .then_some(public_message)
                        .flatten(),
                },
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{path::PathBuf, process::Command, sync::OnceLock};

    fn fixture() -> &'static Path {
        static FIXTURE: OnceLock<PathBuf> = OnceLock::new();
        FIXTURE.get_or_init(|| {
            let root = tempfile::tempdir().unwrap().keep();
            let executable = root.join("cbm-worker.exe");
            let result = Command::new("rustc")
                .args(["--edition=2024", "-o"])
                .arg(&executable)
                .arg(
                    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake_cbm_worker.rs"),
                )
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
            executable
        })
    }

    fn run(mode: &str, duration: Duration, cancellation: &Cancellation) -> io::Result<Value> {
        run_invocation(mode, duration, cancellation, Invocation::Index)
    }

    fn run_invocation(
        mode: &str,
        duration: Duration,
        cancellation: &Cancellation,
        invocation: Invocation<'_>,
    ) -> io::Result<Value> {
        let root = tempfile::tempdir().unwrap();
        let cache = root.path().join("cache");
        let runtime = root.path().join("runtime");
        fs::create_dir(&cache).unwrap();
        fs::create_dir(&runtime).unwrap();
        // Compilation is setup, outside the operation's deadline.
        let executable = fixture();
        let result = execute_invocation(
            executable,
            &cache,
            match invocation {
                Invocation::Index => Some(runtime.as_path()),
                Invocation::Cli(_) => None,
            },
            &json!({"mode":mode,"payload":"Привет 日本"}),
            Deadline::after(duration).unwrap(),
            cancellation,
            invocation,
        );
        if let Err(error) = &result
            && let Some((_, locator)) = error.to_string().rsplit_once("; private evidence: ")
        {
            let retained = PathBuf::from(locator);
            assert_eq!(
                retained.parent().unwrap().canonicalize().unwrap(),
                PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap())
                    .canonicalize()
                    .unwrap()
            );
            assert!(
                retained
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("hmp-")
            );
            assert!(!retained.join("response.json").exists());
            for name in ["stdout.bin", "stderr.bin", "response.bin"] {
                if let Ok(metadata) = fs::metadata(retained.join(name)) {
                    assert!(metadata.len() <= OUTPUT_LIMIT as u64);
                }
            }
            if mode == "diagnostic-failure" {
                assert!(error.to_string().contains("private diagnostics incomplete"));
            } else {
                assert!(retained.join("process.json").is_file());
            }
            fs::remove_dir_all(&retained).unwrap();
            assert!(!retained.exists());
        }
        result
    }

    #[test]
    fn cli_tool_results_preserve_nonzero_tool_errors_and_reject_contradictions() {
        for mode in ["valid", "tool-error"] {
            let report = run_invocation(
                mode,
                Duration::from_secs(5),
                &Cancellation::default(),
                Invocation::Cli("list_projects"),
            )
            .unwrap();
            assert_eq!(report["operation"], "codebase-memory-tool");
            assert_eq!(report["response_source"], "stdout");
            assert_eq!(
                report["outcome"]["exit_code"],
                if mode == "tool-error" { 1 } else { 0 }
            );
            assert_eq!(
                report["result"]["isError"].as_bool().unwrap_or(false),
                mode == "tool-error"
            );
            assert_eq!(report["owned_tree_stopped"], true);
            assert_eq!(report["temporary_state_removed"], true);
        }
        for mode in [
            "exit-mismatch",
            "duplicate",
            "malformed",
            "exit",
            "stdout-flood",
            "cli-empty-failure",
        ] {
            let caller = Cancellation::default();
            let error = run_invocation(
                mode,
                Duration::from_secs(5),
                &caller,
                Invocation::Cli("list_projects"),
            )
            .unwrap_err();
            assert!(error.to_string().contains("private evidence"), "{error}");
            if mode == "cli-empty-failure" {
                assert!(
                    error
                        .to_string()
                        .starts_with("CBM CLI failed before returning a tool result")
                );
                assert!(!error.to_string().contains("private upstream"));
            }
            assert!(!caller.is_cancelled());
        }
    }

    #[test]
    fn native_worker_transports_unicode_and_preserves_tool_errors() {
        let report = run("valid", Duration::from_secs(5), &Cancellation::default()).unwrap();
        assert_eq!(report["result"]["content"][0]["text"], "α 日本 😀");
        assert_eq!(
            report["outcome"]["job"]["memory_limit_bytes"],
            2_u64 * 1024 * 1024 * 1024
        );
        assert_eq!(report["outcome"]["job"]["cpu_rate"], 2500);
        assert_eq!(report["owned_tree_stopped"], true);
        assert_eq!(report["temporary_state_removed"], true);
        let error = run(
            "tool-error",
            Duration::from_secs(5),
            &Cancellation::default(),
        )
        .unwrap();
        assert_eq!(error["result"]["isError"], true);
        assert_eq!(error["outcome"]["exit_code"], 0);
    }

    #[test]
    fn diagnostic_io_does_not_change_success_or_hide_failure_and_retention_is_bounded() {
        assert!(
            run(
                "diagnostic-success",
                Duration::from_secs(5),
                &Cancellation::default()
            )
            .is_ok()
        );
        let error = run(
            "diagnostic-failure",
            Duration::from_secs(5),
            &Cancellation::default(),
        )
        .unwrap_err();
        assert!(error.to_string().starts_with("CBM bounded worker did not finish successfully; private diagnostics incomplete; private evidence: "));
        assert!(
            run(
                "response-flood",
                Duration::from_secs(5),
                &Cancellation::default()
            )
            .is_err()
        );
    }

    #[test]
    fn native_worker_rejects_nonzero_malformed_and_duplicate_response() {
        for mode in ["exit", "malformed", "duplicate"] {
            assert!(
                run(mode, Duration::from_secs(5), &Cancellation::default()).is_err(),
                "{mode}"
            );
        }
    }

    #[test]
    fn native_worker_reclaims_flood_and_deadline() {
        let error = run("flood", Duration::from_secs(5), &Cancellation::default()).unwrap_err();
        assert!(
            error
                .to_string()
                .starts_with("CBM output exceeded its capture limit; private evidence: ")
        );
        let error = run("hang", Duration::from_millis(200), &Cancellation::default()).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert_eq!(
            resource_failure_message(&error),
            Some(
                "CBM worker exceeded its deadline; owned workers reclaimed. Private diagnostics retained."
            )
        );
    }

    #[test]
    fn public_resource_errors_require_confirmed_reclamation_and_typed_cause() {
        let message = "CBM worker exceeded its 2048 MiB memory limit; owned workers reclaimed.";
        for reclaimed in [false, true] {
            let error = io::Error::other(OperationFailure {
                source: io::Error::other("private sentinel must stay out of MCP"),
                reclaimed,
                public_message: Some(message.to_owned()),
            });
            assert_eq!(
                resource_failure_message(&error),
                reclaimed.then_some(message)
            );
        }
        assert!(
            resource_failure_message(&marked_failure(io::Error::other(message), true)).is_none()
        );
        assert!(resource_failure_message(&io::Error::other(message)).is_none());
    }

    #[test]
    fn native_worker_reclaims_cancelled_request_and_late_child() {
        let _ = fixture();
        for invocation in [Invocation::Index, Invocation::Cli("list_projects")] {
            let cancellation = Cancellation::default();
            let signal = cancellation.clone();
            let cancel = std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(200));
                signal.cancel();
            });
            let result = run_invocation("hang", Duration::from_secs(5), &cancellation, invocation);
            cancel.join().unwrap();
            let error = result.unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::Interrupted);
            assert!(
                failure_reclaimed(&error),
                "cancelled worker lacks cleanup evidence: {error}"
            );
            let report = run_invocation(
                "late",
                Duration::from_secs(5),
                &Cancellation::default(),
                invocation,
            )
            .unwrap();
            assert_eq!(report["owned_tree_stopped"], true);
            assert_eq!(report["result"]["content"][0]["text"], "α 日本 😀");
        }
    }

    #[test]
    #[ignore = "explicit audited CBM artifact required; indexes only an owned private repository/cache"]
    fn actual_audited_worker_indexes_owned_repository_and_blocks_policy_drift() {
        let executable = PathBuf::from(
            std::env::var_os("HARNESS_CBM_EXECUTABLE").expect("explicit CBM artifact required"),
        );
        // CBM checks ancestor ACLs for its identity/cache root. The host's
        // ordinary Temp ancestor permits another identity to mutate files.
        // Reuse the already verified private local-app-data allocator.
        let root = native::private_directory().unwrap().keep();
        let cache = root.join("cache");
        let runtime = root.join("runtime");
        let account = root.join("account");
        let repository = root.join("репозиторий-日本");
        for path in [&cache, &runtime, &account, &repository] {
            fs::create_dir(path).unwrap();
        }
        crate::cbm_configuration::tests::policy_fixture(&cache, false);
        fs::write(
            repository.join("sample.rs"),
            b"pub fn alpha() -> usize { beta() }\nfn beta() -> usize { 7 }\n",
        )
        .unwrap();
        let arguments =
            json!({"repo_path":repository,"name":"native-cbm-worker-owned","mode":"fast"});
        let report = index(
            &executable,
            &cache,
            &runtime,
            &account,
            &arguments,
            &Cancellation::default(),
        )
        .unwrap();
        fs::write(
            root.join("index-report.json"),
            serde_json::to_vec_pretty(&report).unwrap(),
        )
        .unwrap();
        assert_ne!(report["result"]["isError"], true, "{}", report["result"]);
        assert_eq!(report["owned_tree_stopped"], true);
        assert_eq!(report["temporary_state_removed"], true);
        assert_eq!(report["automatic_retry"], false);
        // A drifted, separate owned configuration is rejected before any worker.
        let drift = root.join("drift");
        fs::create_dir(&drift).unwrap();
        crate::cbm_configuration::tests::policy_fixture(&drift, true);
        let before = fs::read(drift.join("_config.db")).unwrap();
        let failed = index(
            &executable,
            &drift,
            &runtime,
            &account,
            &arguments,
            &Cancellation::default(),
        )
        .unwrap_err();
        assert_eq!(
            failed.to_string(),
            "CBM resource policy inactive; no worker started"
        );
        assert_eq!(fs::read(drift.join("_config.db")).unwrap(), before);
        assert_eq!(fs::read_dir(&drift).unwrap().count(), 2);
        println!("Owned actual worker evidence: {}", root.display());
    }
}
