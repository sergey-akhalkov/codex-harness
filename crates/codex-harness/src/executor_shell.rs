//! Preserve the owner's permitted PowerShell instead of silently selecting 5.1.
use harness_core::process::{Cancellation, CommandSpec, Deadline, Job, Limits, StopReason};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    ffi::OsString,
    fs, io,
    os::windows::process::CommandExt,
    path::Path,
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct PreparedShell {
    pub path: OsString,
    pub executable: PathBuf,
    pub version: String,
    pub sandbox_mode: String,
}

pub(crate) fn prepare(
    launcher: &Path,
    home: &Path,
    profile: &str,
    workspace: &Path,
    sandbox: Option<&str>,
) -> io::Result<PreparedShell> {
    let evidence = tempfile::Builder::new()
        .prefix("executor-shell-")
        .tempdir()?;
    let home = home.canonicalize()?;
    let workspace = workspace.canonicalize()?;
    // CLI 0.155.1 rejects --profile for app-server, and config/read has no
    // profile parameter. Its model-free prompt diagnostic uses the actual
    // runtime configuration loader, including profile/project permissions.
    let mut command = CommandSpec::new(launcher);
    command.args = harness_core::orchestration_config::executor_session_args(profile)?
        .into_iter()
        .map(Into::into)
        .collect();
    if let Some(mode) = sandbox {
        command.args.extend([
            "-c".into(),
            format!("sandbox_mode={}", serde_json::to_string(mode)?).into(),
        ]);
    }
    command.args.extend([
        "-C".into(),
        workspace.as_os_str().into(),
        "debug".into(),
        "prompt-input".into(),
    ]);
    command.current_dir = Some(workspace);
    command
        .env
        .insert("CODEX_HOME".into(), Some(home.into_os_string()));
    let response = capture(
        command,
        evidence.path(),
        "permissions",
        // The model-free diagnostic still loads the effective runtime
        // configuration, including MCP brokers and skills: measured 34-36 s on
        // the owner host against the previous 20 s budget, which fail-closed
        // every dispatch. Keep the bound, but cover the observed startup cost.
        Duration::from_secs(60),
    )
    .and_then(|text| serde_json::from_str::<Value>(&text).map_err(io::Error::other))
    .map_err(|error| {
        let retained = evidence.path().to_owned();
        // Failed native startup evidence belongs to the local TEMP lifecycle.
        let _ = fs::write(retained.join("failure.txt"), error.to_string());
        io::Error::other(format!(
            "executor shell configuration preflight failed: {error}; inspect {}",
            retained.display()
        ))
    });
    let response = match response {
        Ok(response) => response,
        Err(error) => {
            let _ = evidence.keep();
            return Err(error);
        }
    };
    let mode = sandbox_mode(&response)?;
    let inherited = std::env::var_os("PATH")
        .ok_or_else(|| io::Error::other("executor PATH is missing; PowerShell 7 is required"))?;
    let path = permitted_path(inherited, &mode)?;
    let executable = find_powershell(&path)?;
    let version = match probe_version(&executable, evidence.path()) {
        Ok(version) => version,
        Err(error) => {
            let retained = evidence.keep();
            return Err(io::Error::other(format!(
                "executor shell {}: {error}; inspect {}",
                executable.display(),
                retained.display()
            )));
        }
    };
    Ok(PreparedShell {
        path,
        executable,
        version,
        sandbox_mode: mode,
    })
}

fn sandbox_mode(response: &Value) -> io::Result<String> {
    let mut observed = Vec::new();
    for item in response.as_array().into_iter().flatten() {
        if item["type"] != "message" || item["role"] != "developer" {
            continue;
        }
        for content in item["content"].as_array().into_iter().flatten() {
            let Some(text) = content["text"].as_str() else {
                continue;
            };
            if !text.starts_with("<permissions instructions>")
                || !text.trim_end().ends_with("</permissions instructions>")
            {
                continue;
            }
            for mode in ["danger-full-access", "workspace-write", "read-only"] {
                let declaration = format!("`sandbox_mode` is `{mode}`");
                if text.matches(&declaration).count() == 1 {
                    observed.push(mode);
                }
            }
        }
    }
    match observed.as_slice() {
        [mode] => Ok((*mode).into()),
        _ => Err(io::Error::other(
            "native prompt diagnostic did not unambiguously identify executor sandbox_mode; refusing to guess the permitted shell",
        )),
    }
}

fn permitted_path(path: OsString, mode: &str) -> io::Result<OsString> {
    if mode == "danger-full-access" {
        return Ok(path);
    }
    std::env::join_paths(std::env::split_paths(&path).filter(|entry| {
        !entry.components().any(|part| {
            part.as_os_str()
                .to_string_lossy()
                .eq_ignore_ascii_case("WindowsApps")
        })
    }))
    .map_err(io::Error::other)
}

fn find_powershell(path: &std::ffi::OsStr) -> io::Result<PathBuf> {
    std::env::split_paths(path)
        .filter(|entry| entry.is_absolute())
        .map(|entry| entry.join("pwsh.exe"))
        .find(|file| file.is_file())
        .ok_or_else(|| io::Error::other(
            "executor requires the owner's existing PowerShell 7 on its permitted PATH; Windows PowerShell 5.1 fallback is refused. Inspect the effective sandbox policy and owner shell configuration; no shell was installed or permissions changed",
        ))
}

fn probe_version(executable: &Path, evidence: &Path) -> io::Result<String> {
    // Packaged pwsh rejects creation with PROC_THREAD_ATTRIBUTE_JOB_LIST.
    // This version-only process runs no command/profile or descendants and
    // uses ordinary native creation, matching Codex's shell entry point.
    let output = evidence.join("powershell.stdout");
    let mut child = Command::new(executable)
        .args(["-NoLogo", "-NoProfile", "--version"])
        .stdin(Stdio::null())
        .stdout(fs::File::create(&output)?)
        .stderr(fs::File::create(evidence.join("powershell.stderr"))?)
        .creation_flags(0x0800_0000)
        .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.try_wait()? {
            if !status.success() {
                return Err(io::Error::other(format!(
                    "PowerShell version exited {status}"
                )));
            }
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "PowerShell version probe timed out",
            ));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let version = fs::read_to_string(output)?.trim().to_owned();
    if !version.starts_with("PowerShell 7.") {
        return Err(io::Error::other(
            "executor shell is not PowerShell 7; refusing native fallback",
        ));
    }
    Ok(version)
}

fn capture(
    mut command: CommandSpec,
    evidence: &Path,
    name: &str,
    timeout: Duration,
) -> io::Result<String> {
    let output = evidence.join(format!("{name}.stdout"));
    let error_output = evidence.join(format!("{name}.stderr"));
    command.stdout = Some(fs::File::create(&output)?);
    command.stderr = Some(fs::File::create(&error_output)?);
    let job = Job::new(Limits::default())?;
    let child = job.spawn(&command)?;
    let outcome = job.wait(
        &child,
        Deadline::after(timeout)?,
        &Cancellation::default(),
        Duration::from_secs(1),
    )?;
    drop(command);
    if outcome.reason != StopReason::Exited || outcome.exit_code != 0 {
        return Err(io::Error::other(format!(
            "executor {name} preflight failed: {:?}, exit {}; inspect {}",
            outcome.reason,
            outcome.exit_code,
            error_output.display()
        )));
    }
    if fs::metadata(&output)?.len() > 8 * 1024 * 1024 {
        return Err(io::Error::other("executor preflight output exceeds 8 MiB"));
    }
    fs::read_to_string(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn packaged_shell_remains_available_only_without_sandbox() {
        let original = std::env::join_paths([
            r"C:\Program Files\WindowsApps\Microsoft.PowerShell",
            r"C:\Tools\PowerShell\7",
            r"C:\Windows\System32",
        ])
        .unwrap();
        assert_eq!(
            permitted_path(original.clone(), "danger-full-access").unwrap(),
            original
        );
        for mode in ["workspace-write", "read-only"] {
            let filtered = permitted_path(original.clone(), mode).unwrap();
            let paths: Vec<_> = std::env::split_paths(&filtered).collect();
            assert_eq!(
                paths,
                [
                    PathBuf::from(r"C:\Tools\PowerShell\7"),
                    PathBuf::from(r"C:\Windows\System32")
                ]
            );
        }
    }

    #[test]
    fn missing_shell_and_unknown_policy_fail_closed() {
        let root = tempfile::tempdir().unwrap();
        let error = find_powershell(root.path().as_os_str()).unwrap_err();
        assert!(error.to_string().contains("5.1 fallback is refused"));
        assert!(sandbox_mode(&json!([])).is_err());
        let block = "<permissions instructions>\n`sandbox_mode` is `danger-full-access`\n</permissions instructions>";
        let item = json!({"type":"message","role":"developer","content":[{"type":"input_text","text":block}]});
        assert_eq!(
            sandbox_mode(&json!([item.clone()])).unwrap(),
            "danger-full-access"
        );
        assert!(sandbox_mode(&json!([item.clone(), item])).is_err());
        assert!(
            sandbox_mode(&json!([{"type":"message","role":"user","content":[{"text":block}]}]))
                .is_err()
        );
    }

    #[test]
    #[ignore = "model-free read of the explicitly supplied live Codex home/profile and owner PowerShell"]
    fn installed_shell_preflight() {
        let home =
            PathBuf::from(std::env::var_os("HARNESS_LIVE_CODEX_HOME").expect("live Codex home"));
        let profile = std::env::var("HARNESS_LIVE_EXECUTOR_PROFILE").expect("live profile");
        let shell = prepare(
            &home.join("harness/bin/codex.exe"),
            &home,
            &profile,
            &std::env::current_dir().unwrap(),
            None,
        )
        .unwrap();
        println!(
            "{}: {} (policy {})",
            shell.executable.display(),
            shell.version,
            shell.sandbox_mode
        );
        assert!(shell.version.starts_with("PowerShell 7."));
        let sandboxed = prepare(
            &home.join("harness/bin/codex.exe"),
            &home,
            &profile,
            &std::env::current_dir().unwrap(),
            Some("read-only"),
        );
        let filtered = permitted_path(std::env::var_os("PATH").unwrap(), "read-only").unwrap();
        if find_powershell(&filtered).is_err() {
            assert!(
                sandboxed
                    .unwrap_err()
                    .to_string()
                    .contains("5.1 fallback is refused")
            );
        } else {
            assert_eq!(sandboxed.unwrap().sandbox_mode, "read-only");
        }
    }
}
