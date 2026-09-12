//! Pinned foreign CLI contracts. No generated JavaScript or package acquisition.
use crate::process::{Cancellation, CommandSpec, Deadline, Job, Limits, Outcome, StopReason};
use serde::Serialize;
use std::{
    fs, io,
    path::{Path, PathBuf},
    time::Duration,
};

const OUTPUT_LIMIT: u64 = 1024 * 1024;
const PACKAGE_NAME: &str = "@bitkyc08/opencodex";
const PACKAGE_VERSION: &str = "2.44.0";

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ValidationStatus {
    Valid,
    Rejected,
    ProcessFailed,
    TimedOut,
    Cancelled,
    MemoryLimit,
    OutputLimit,
}

#[derive(Debug, Serialize)]
pub struct Validation {
    pub status: ValidationStatus,
    pub process: Outcome,
    /// Private raw diagnostics stay outside the source tree and are never echoed.
    pub evidence: PathBuf,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ProbeStatus {
    Ok,
    Failed,
    TimedOut,
    Cancelled,
    MemoryLimit,
    OutputLimit,
}

#[derive(Debug, Serialize)]
pub struct CliProbe {
    pub status: ProbeStatus,
    pub process: Outcome,
    pub evidence: PathBuf,
}

fn adopted_runtime(package_root: &Path) -> io::Result<(PathBuf, PathBuf)> {
    let package_root = package_root.canonicalize()?;
    let metadata_path = package_root.join("package.json");
    if fs::metadata(&metadata_path)?.len() > OUTPUT_LIMIT {
        return Err(io::Error::other("Invalid package metadata size."));
    }
    let metadata: serde_json::Value = serde_json::from_slice(&fs::read(metadata_path)?)
        .map_err(|_| io::Error::other("Invalid package metadata."))?;
    if metadata["name"] != PACKAGE_NAME || metadata["version"] != PACKAGE_VERSION {
        return Err(io::Error::other(
            "Reassess the OpenCodex CLI contract for this package version.",
        ));
    }
    let bun = package_root.join("node_modules/bun/bin/bun.exe");
    let cli = package_root.join("src/cli/index.ts");
    if !bun.is_file() || !cli.is_file() {
        return Err(io::Error::other(
            "The adopted OpenCodex runtime is missing; explicit provisioning is required.",
        ));
    }
    Ok((bun, cli))
}

fn owned_homes(prefix: &str) -> io::Result<(tempfile::TempDir, PathBuf, PathBuf)> {
    let temporary = tempfile::Builder::new().prefix(prefix).tempdir()?;
    let root = temporary.path();
    let codex_home = root.join("codex");
    let ocx_home = root.join("opencodex");
    fs::create_dir(&codex_home)?;
    fs::create_dir(&ocx_home)?;
    fs::write(
        ocx_home.join("config.json"),
        br#"{"codexShimAutoRestore":false}"#,
    )?;
    Ok((temporary, codex_home, ocx_home))
}

fn run_cli(
    bun: PathBuf,
    cli: PathBuf,
    args: Vec<std::ffi::OsString>,
    current_dir: &Path,
    codex_home: PathBuf,
    ocx_home: PathBuf,
    stdin: Option<fs::File>,
    deadline: Deadline,
    cancellation: &Cancellation,
) -> io::Result<(Outcome, PathBuf, PathBuf, PathBuf)> {
    let out_path = current_dir.join("stdout.json");
    let err_path = current_dir.join("stderr.txt");
    let mut command = CommandSpec::new(bun);
    command.args = std::iter::once(cli.into()).chain(args).collect();
    command.current_dir = Some(current_dir.to_owned());
    command
        .env
        .insert("CODEX_HOME".into(), Some(codex_home.into()));
    command
        .env
        .insert("OPENCODEX_HOME".into(), Some(ocx_home.into()));
    command
        .env
        .insert("OPENCODEX_CODEX_SHIM_AUTO_RESTORE".into(), Some("0".into()));
    command.stdin = stdin;
    command.stdout = Some(fs::File::create(&out_path)?);
    command.stderr = Some(fs::File::create(&err_path)?);
    let job = Job::new(Limits {
        memory_bytes: Some(512 * 1024 * 1024),
        cpu_percent: Some(25.0),
    })?;
    let child = job.spawn(&command)?;
    let overflow_cancel = Cancellation::default();
    let mut overflow = false;
    while child.is_running()? && !deadline.expired() && !cancellation.is_cancelled() {
        if fs::metadata(&out_path)?.len() > OUTPUT_LIMIT
            || fs::metadata(&err_path)?.len() > OUTPUT_LIMIT
        {
            overflow = true;
            overflow_cancel.cancel();
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let process = job.wait(
        &child,
        deadline,
        if overflow {
            &overflow_cancel
        } else {
            cancellation
        },
        Duration::from_secs(5),
    )?;
    drop(command);
    Ok((process, out_path, err_path, current_dir.to_path_buf()))
}

fn probe_status(overflow: bool, process: &Outcome) -> ProbeStatus {
    if overflow {
        ProbeStatus::OutputLimit
    } else {
        match process.reason {
            StopReason::Timeout => ProbeStatus::TimedOut,
            StopReason::Cancelled => ProbeStatus::Cancelled,
            StopReason::MemoryLimit => ProbeStatus::MemoryLimit,
            StopReason::Exited => {
                if process.exit_code == 0 {
                    ProbeStatus::Ok
                } else {
                    ProbeStatus::Failed
                }
            }
        }
    }
}

fn private_text(path: &Path) -> io::Result<String> {
    let bytes = fs::read(path)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn contains_secret(haystack: &str, secret: &str) -> bool {
    !secret.is_empty() && haystack.contains(secret)
}

fn overflow(out_path: &Path, err_path: &Path) -> io::Result<bool> {
    Ok(
        fs::metadata(out_path)?.len() > OUTPUT_LIMIT
            || fs::metadata(err_path)?.len() > OUTPUT_LIMIT,
    )
}

/// Validate bytes through the audited upstream CLI in fresh, owned runtime homes.
/// Successful schema validation is distinct from the harness's routing policy.
pub fn validate_candidate(
    package_root: &Path,
    candidate: &[u8],
    deadline: Deadline,
    cancellation: &Cancellation,
) -> io::Result<Validation> {
    if candidate.len() > OUTPUT_LIMIT as usize {
        return Err(io::Error::other(
            "Candidate configuration exceeds the size limit.",
        ));
    }
    let (bun, cli) = adopted_runtime(package_root)?;
    let (temporary, codex_home, ocx_home) = owned_homes("harness-ocx-validate-")?;
    let input = temporary.path().join("candidate.json");
    fs::write(&input, candidate)?;
    let (process, out_path, err_path, _) = run_cli(
        bun,
        cli,
        vec![
            "config".into(),
            "validate".into(),
            input.into(),
            "--json".into(),
        ],
        temporary.path(),
        codex_home,
        ocx_home,
        None,
        deadline,
        cancellation,
    )?;
    let evidence = temporary.keep();
    let overflow = overflow(&out_path, &err_path)?;
    let status = if overflow {
        ValidationStatus::OutputLimit
    } else {
        match process.reason {
            StopReason::Timeout => ValidationStatus::TimedOut,
            StopReason::Cancelled => ValidationStatus::Cancelled,
            StopReason::MemoryLimit => ValidationStatus::MemoryLimit,
            StopReason::Exited => {
                let result =
                    serde_json::from_slice::<serde_json::Value>(&fs::read(&out_path)?).ok();
                match (
                    process.exit_code,
                    result
                        .as_ref()
                        .and_then(|v| v.get("ok"))
                        .and_then(|v| v.as_bool()),
                ) {
                    (0, Some(true)) => ValidationStatus::Valid,
                    (0 | 1, Some(false)) => ValidationStatus::Rejected,
                    _ => ValidationStatus::ProcessFailed,
                }
            }
        }
    };
    let result = Validation {
        status,
        process,
        evidence,
    };
    fs::write(
        result.evidence.join("receipt.json"),
        serde_json::to_vec_pretty(&result)?,
    )?;
    Ok(result)
}

/// Public `ocx restore --json` on owned homes. This command also writes durable
/// desired-state and can restore history; it is not equivalent to skipHistory.
pub fn restore_native(
    package_root: &Path,
    injected_config: Option<&str>,
    deadline: Deadline,
    cancellation: &Cancellation,
) -> io::Result<CliProbe> {
    let (bun, cli) = adopted_runtime(package_root)?;
    let (temporary, codex_home, ocx_home) = owned_homes("harness-ocx-restore-")?;
    if let Some(config) = injected_config {
        fs::write(codex_home.join("config.toml"), config)?;
    }
    let (process, out_path, err_path, _) = run_cli(
        bun,
        cli,
        vec!["restore".into(), "--json".into()],
        temporary.path(),
        codex_home,
        ocx_home,
        None,
        deadline,
        cancellation,
    )?;
    let evidence = temporary.keep();
    let overflow = overflow(&out_path, &err_path)?;
    let stdout = private_text(&out_path)?;
    let parsed = serde_json::from_str::<serde_json::Value>(&stdout).ok();
    let success = parsed
        .as_ref()
        .and_then(|value| value.get("success"))
        .and_then(|value| value.as_bool())
        == Some(true);
    let mut status = probe_status(overflow, &process);
    if status == ProbeStatus::Ok && !success {
        status = ProbeStatus::Failed;
    }
    let result = CliProbe {
        status,
        process,
        evidence,
    };
    fs::write(
        result.evidence.join("receipt.json"),
        serde_json::to_vec_pretty(&result)?,
    )?;
    Ok(result)
}

/// Public `ocx login xai` with closed stdin on owned homes. Stock login still
/// installs a manual-code waiter; closed input is a bounded failure, not a
/// browser-only success path.
pub fn login_xai_closed_stdin(
    package_root: &Path,
    deadline: Deadline,
    cancellation: &Cancellation,
) -> io::Result<CliProbe> {
    let (bun, cli) = adopted_runtime(package_root)?;
    let (temporary, codex_home, ocx_home) = owned_homes("harness-ocx-login-")?;
    fs::write(
        ocx_home.join("config.json"),
        br#"{"codexShimAutoRestore":false,"providers":{"xai":{"adapter":"openai-chat","baseUrl":"https://cli-chat-proxy.grok.com/v1","authMode":"oauth"}}}"#,
    )?;
    let closed = fs::File::open("NUL")?;
    let (process, out_path, err_path, _) = run_cli(
        bun,
        cli,
        vec!["login".into(), "xai".into()],
        temporary.path(),
        codex_home,
        ocx_home,
        Some(closed),
        deadline,
        cancellation,
    )?;
    let evidence = temporary.keep();
    let overflow = overflow(&out_path, &err_path)?;
    let stdout = private_text(&out_path)?;
    let stderr = private_text(&err_path)?;
    if contains_secret(&stdout, "access_token")
        || contains_secret(&stderr, "access_token")
        || contains_secret(&stdout, "refresh_token")
        || contains_secret(&stderr, "refresh_token")
    {
        return Err(io::Error::other(
            "OpenCodex login probe emitted private credential material.",
        ));
    }
    let result = CliProbe {
        status: probe_status(overflow, &process),
        process,
        evidence,
    };
    fs::write(
        result.evidence.join("receipt.json"),
        serde_json::to_vec_pretty(&result)?,
    )?;
    Ok(result)
}
