//! Optional verification records for `harness-observe`.
//!
//! A record documents one execution: the declared scope, the actual argv,
//! executable and working directory, bounded Git HEAD/status, and content
//! identities of explicitly declared input files before and after the child
//! ran. It never accepts a task, never reuses a previous pass and never claims
//! coverage of unlisted files or external runtime state.
//!
//! Declared inputs are read-only: a link or junction resolves to its target,
//! and the record keeps both the requested path and the resolved identity.

use harness_core::build_identity;
use serde_json::{Value, json};
use std::ffi::OsStr;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Upper bound of one captured `git status --porcelain` output.
pub const GIT_STATUS_LIMIT: u64 = 256 * 1024;
/// Upper bound on one Git probe.
pub const GIT_TIMEOUT: Duration = Duration::from_secs(5);
/// Upper bound on the captured Git error line.
const GIT_ERROR_LIMIT: u64 = 8 * 1024;
/// Longest accepted scope description.
pub const SCOPE_LIMIT: usize = 200;
/// Stated coverage of every record.
pub const COVERAGE: &str = "Declared inputs only: unlisted files and external runtime state are not covered. This record reports one execution; it does not accept the task and never reuses a previous pass. See this receipt's status and native fields for the actual process outcome.";

/// Validates a declared verification scope.
pub fn validate_scope(scope: Option<&str>) -> io::Result<()> {
    let Some(scope) = scope else {
        return Ok(());
    };
    if scope.trim().is_empty() || scope.len() > SCOPE_LIMIT {
        return Err(invalid(format!(
            "verification scope must be a non-empty description of at most {SCOPE_LIMIT} bytes"
        )));
    }
    Ok(())
}

/// Validates declared inputs before any case state exists. A declared input
/// must be an absolute path resolving to an existing regular file; links and
/// junctions are followed to their target, so a missing or non-file
/// declaration fails before the command is launched.
pub fn validate_inputs(inputs: &[PathBuf]) -> io::Result<()> {
    for path in inputs {
        if !path.is_absolute() {
            return Err(invalid(format!(
                "declared input must be an absolute path: {}",
                path.display()
            )));
        }
        let metadata = fs::metadata(path).map_err(|error| {
            if error.kind() == io::ErrorKind::NotFound {
                invalid(format!(
                    "declared input is missing or links to nothing: {}; declare an existing regular file",
                    path.display()
                ))
            } else {
                error
            }
        })?;
        if !metadata.is_file() {
            return Err(invalid(format!(
                "declared input must be a regular file, but {} is {}",
                path.display(),
                if metadata.is_dir() {
                    "a directory"
                } else {
                    "not a regular file"
                }
            )));
        }
    }
    Ok(())
}

/// An in-flight verification capture: identities recorded before the child
/// started, completed into one receipt object after the child ended.
pub struct Capture {
    scope: Option<String>,
    cwd: PathBuf,
    argv: Vec<String>,
    executable: PathBuf,
    inputs: Vec<PathBuf>,
    before: Snapshot,
}

impl Capture {
    /// Starts a capture when the caller declared a scope or inputs. Requests
    /// without a declaration keep their previous receipt unchanged. The full
    /// argv is captured upfront so it survives an infrastructure failure.
    pub fn begin(
        scope: Option<&str>,
        cwd: &Path,
        argv: &[String],
        inputs: &[PathBuf],
    ) -> io::Result<Option<Self>> {
        if scope.is_none() && inputs.is_empty() {
            return Ok(None);
        }
        validate_scope(scope)?;
        validate_inputs(inputs)?;
        let executable = argv
            .first()
            .ok_or_else(|| invalid("verification capture needs the command argv"))?;
        let cwd = fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_owned());
        let executable = PathBuf::from(executable);
        let inputs = inputs.to_vec();
        let before = Snapshot::capture(&executable, &inputs, &cwd);
        Ok(Some(Self {
            scope: scope.map(str::to_owned),
            cwd,
            argv: argv.to_vec(),
            executable,
            inputs,
            before,
        }))
    }

    /// Completes the record after the observed process ended.
    pub fn finish(self) -> Value {
        let after = Snapshot::capture(&self.executable, &self.inputs, &self.cwd);
        json!({
            "scope": self.scope,
            "cwd": path_string(&self.cwd),
            "argv": self.argv,
            "executable": self.before.executable.merged(&after.executable),
            "inputs": self
                .before
                .inputs
                .iter()
                .zip(&after.inputs)
                .map(|(before, after)| before.merged(after))
                .collect::<Vec<Value>>(),
            "git": self.before.git.merged(&after.git),
            "coverage": COVERAGE,
        })
    }
}

/// The bounded identities of one execution instant.
struct Snapshot {
    executable: Identity,
    inputs: Vec<Identity>,
    git: GitSnapshot,
}

impl Snapshot {
    fn capture(executable: &Path, inputs: &[PathBuf], cwd: &Path) -> Self {
        Self {
            executable: Identity::capture(executable),
            inputs: inputs.iter().map(|path| Identity::capture(path)).collect(),
            git: GitSnapshot::capture(cwd),
        }
    }
}

/// Content identity of one file; unavailable identity is explicit, never zero.
struct Identity {
    path: PathBuf,
    resolved: Option<String>,
    sha256: Option<String>,
    bytes: Option<u64>,
    unavailable: Option<String>,
}

impl Identity {
    fn capture(path: &Path) -> Self {
        match resolve_and_hash(path) {
            Ok((resolved, sha256, bytes)) => Self {
                path: path.to_owned(),
                resolved: Some(resolved),
                sha256: Some(sha256),
                bytes: Some(bytes),
                unavailable: None,
            },
            Err(error) => Self {
                path: path.to_owned(),
                resolved: None,
                sha256: None,
                bytes: None,
                unavailable: Some(error.to_string()),
            },
        }
    }

    /// Before/after identities with an explicit unchanged, changed or
    /// unavailable state. A repointed link changes the resolved identity even
    /// when both targets hold equal content.
    fn merged(&self, after: &Self) -> Value {
        let state = match (&self.sha256, &after.sha256, &self.resolved, &after.resolved) {
            (Some(before), Some(after), Some(resolved_before), Some(resolved_after))
                if before == after && resolved_before == resolved_after =>
            {
                "unchanged"
            }
            (Some(_), Some(_), Some(_), Some(_)) => "changed",
            _ => "unavailable",
        };
        json!({
            "path": path_string(&self.path),
            "resolved_before": self.resolved.as_deref(),
            "resolved_after": after.resolved.as_deref(),
            "bytes_before": self.bytes,
            "bytes_after": after.bytes,
            "sha256_before": self.sha256.as_deref(),
            "sha256_after": after.sha256.as_deref(),
            "state": state,
            "unavailable": self.unavailable.as_deref().or(after.unavailable.as_deref()),
        })
    }
}

fn resolve_and_hash(path: &Path) -> io::Result<(String, String, u64)> {
    let resolved = fs::canonicalize(path)?;
    let sha256 = build_identity::hash_file(&resolved)?;
    let bytes = fs::metadata(&resolved)?.len();
    Ok((path_string(&resolved), sha256, bytes))
}

/// Bounded Git identity of the working directory.
#[derive(Clone, Debug)]
struct GitSnapshot {
    available: bool,
    reason: Option<String>,
    head: Option<String>,
    status_sha256: Option<String>,
    status_entries: Option<usize>,
    status_truncated: bool,
}

impl GitSnapshot {
    fn unavailable(reason: String) -> Self {
        Self {
            available: false,
            reason: Some(reason),
            head: None,
            status_sha256: None,
            status_entries: None,
            status_truncated: false,
        }
    }

    fn capture(cwd: &Path) -> Self {
        let status = match run_bounded(
            OsStr::new("git"),
            &[
                OsStr::new("--no-optional-locks"),
                OsStr::new("-C"),
                cwd.as_os_str(),
                OsStr::new("status"),
                OsStr::new("--porcelain"),
            ],
            GIT_STATUS_LIMIT,
            GIT_TIMEOUT,
        ) {
            Ok(probe) if probe.timed_out => {
                return Self::unavailable("git status timed out".to_owned());
            }
            Ok(probe) if probe.exit != Some(0) => {
                let reason = probe
                    .error_line
                    .unwrap_or_else(|| format!("git status exited with {:?}", probe.exit));
                return Self::unavailable(format!("no usable Git identity: {reason}"));
            }
            Ok(probe) => probe,
            Err(error) => {
                return Self::unavailable(format!("git could not run: {error}"));
            }
        };
        let head = run_bounded(
            OsStr::new("git"),
            &[
                OsStr::new("--no-optional-locks"),
                OsStr::new("-C"),
                cwd.as_os_str(),
                OsStr::new("rev-parse"),
                OsStr::new("--verify"),
                OsStr::new("HEAD"),
            ],
            GIT_STATUS_LIMIT,
            GIT_TIMEOUT,
        )
        .ok()
        .filter(|probe| !probe.timed_out && probe.exit == Some(0))
        .and_then(|probe| String::from_utf8(probe.stdout).ok())
        .map(|text| text.trim().to_owned())
        .filter(|head| !head.is_empty());
        Self {
            available: true,
            reason: None,
            head,
            status_sha256: Some(build_identity::hash_bytes(&status.stdout)),
            status_entries: Some(
                String::from_utf8_lossy(&status.stdout)
                    .lines()
                    .filter(|line| !line.trim().is_empty())
                    .count(),
            ),
            status_truncated: status.truncated,
        }
    }

    /// Before/after Git identity with an explicit worktree state.
    fn merged(&self, after: &Self) -> Value {
        let state = match (&self.status_sha256, &after.status_sha256) {
            (Some(before), Some(after)) if before == after => "unchanged",
            (Some(_), Some(_)) => "changed",
            _ => "unavailable",
        };
        json!({
            "available": self.available && after.available,
            "reason": self.reason.clone().or_else(|| after.reason.clone()),
            "head_before": self.head.clone(),
            "head_after": after.head.clone(),
            "status_sha256_before": self.status_sha256.clone(),
            "status_sha256_after": after.status_sha256.clone(),
            "status_entries_before": self.status_entries,
            "status_entries_after": after.status_entries,
            "status_truncated": self.status_truncated || after.status_truncated,
            "worktree_state": state,
        })
    }
}

/// One bounded child probe result.
struct Probe {
    exit: Option<i32>,
    stdout: Vec<u8>,
    error_line: Option<String>,
    truncated: bool,
    timed_out: bool,
}

/// Runs one bounded helper process without a shell and without dumping the
/// environment: stdout is capped, stderr contributes one bounded line, and a
/// hanging helper is killed at `timeout`.
fn run_bounded(
    program: &OsStr,
    args: &[&OsStr],
    limit: u64,
    timeout: Duration,
) -> io::Result<Probe> {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = command.spawn()?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(stream) = stdout {
            let _ = stream.take(limit + 1).read_to_end(&mut bytes);
        }
        bytes
    });
    let error_reader = std::thread::spawn(move || error_line(stderr));
    let started = Instant::now();
    let mut exit = None;
    let mut timed_out = false;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                exit = status.code();
                break;
            }
            Ok(None) if started.elapsed() < timeout => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Ok(None) => {
                timed_out = true;
                let _ = child.kill();
                break;
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        }
    }
    let _ = child.wait();
    let stdout = reader.join().unwrap_or_default();
    let truncated = stdout.len() as u64 > limit;
    Ok(Probe {
        exit,
        stdout,
        error_line: error_reader.join().unwrap_or(None),
        truncated,
        timed_out,
    })
}

/// First non-empty stderr line, bounded for the receipt.
fn error_line(stream: Option<impl Read>) -> Option<String> {
    let mut bytes = Vec::new();
    if let Some(stream) = stream {
        let _ = stream.take(GIT_ERROR_LIMIT).read_to_end(&mut bytes);
    }
    String::from_utf8_lossy(&bytes)
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(|line| line.chars().take(200).collect())
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}
