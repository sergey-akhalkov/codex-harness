//! Pinned foreign CLI contracts. No generated JavaScript or package acquisition.
use crate::process::{Cancellation, CommandSpec, Deadline, Job, Limits, Outcome, StopReason};
use serde::Serialize;
use std::{
    fs, io,
    path::{Path, PathBuf},
    time::Duration,
};

const OUTPUT_LIMIT: u64 = 1024 * 1024;

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
    let package_root = package_root.canonicalize()?;
    let metadata_path = package_root.join("package.json");
    if fs::metadata(&metadata_path)?.len() > OUTPUT_LIMIT {
        return Err(io::Error::other("Invalid package metadata size."));
    }
    let metadata: serde_json::Value = serde_json::from_slice(&fs::read(metadata_path)?)
        .map_err(|_| io::Error::other("Invalid package metadata."))?;
    if metadata["name"] != "@bitkyc08/opencodex" || metadata["version"] != "2.44.0" {
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
    let temporary = tempfile::Builder::new()
        .prefix("harness-ocx-validate-")
        .tempdir()?;
    let root = temporary.path();
    let codex_home = root.join("codex");
    let ocx_home = root.join("opencodex");
    fs::create_dir(&codex_home)?;
    fs::create_dir(&ocx_home)?;
    fs::write(
        ocx_home.join("config.json"),
        br#"{"codexShimAutoRestore":false}"#,
    )?;
    let input = root.join("candidate.json");
    fs::write(&input, candidate)?;
    let out_path = root.join("stdout.json");
    let err_path = root.join("stderr.txt");
    let mut command = CommandSpec::new(bun);
    command.args = vec![
        cli.into(),
        "config".into(),
        "validate".into(),
        input.into(),
        "--json".into(),
    ];
    command.current_dir = Some(root.to_owned());
    command
        .env
        .insert("CODEX_HOME".into(), Some(codex_home.into()));
    command
        .env
        .insert("OPENCODEX_HOME".into(), Some(ocx_home.into()));
    command
        .env
        .insert("OPENCODEX_CODEX_SHIM_AUTO_RESTORE".into(), Some("0".into()));
    command.stdout = Some(fs::File::create(&out_path)?);
    command.stderr = Some(fs::File::create(&err_path)?);
    // Retain failed/partial executions too. Nothing in this directory is global.
    let evidence = temporary.keep();
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
    overflow |= fs::metadata(&out_path)?.len() > OUTPUT_LIMIT
        || fs::metadata(&err_path)?.len() > OUTPUT_LIMIT;
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
                    // Audited 2.44.0 dispatch can return exit 0 even after the
                    // validate handler sets exitCode=1. Its explicit JSON
                    // rejection must never become a successful validation.
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
