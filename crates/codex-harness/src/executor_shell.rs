//! Preserve the owner's permitted PowerShell instead of silently selecting 5.1.
//! The selected installation's `harness/bin` is prefixed for the model-free
//! preflight and the hosted child. Missing or changed commands are refused by
//! the runtime owner before a measured submission, not by relocating unrelated
//! tools or writing a command program.
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
    let inherited = std::env::var_os("PATH")
        .ok_or_else(|| io::Error::other("executor PATH is missing; PowerShell 7 is required"))?;
    prepare_with_path(launcher, home, profile, workspace, sandbox, &inherited)
}

fn prepare_with_path(
    launcher: &Path,
    home: &Path,
    profile: &str,
    workspace: &Path,
    sandbox: Option<&str>,
    ambient: &std::ffi::OsStr,
) -> io::Result<PreparedShell> {
    let evidence = tempfile::Builder::new()
        .prefix("executor-shell-")
        .tempdir()?;
    let home = home.canonicalize()?;
    let workspace = workspace.canonicalize()?;
    // Bind before the diagnostic runs. Installation publishes each arm onto
    // this process PATH, so an unbound preflight would resolve a sibling tool.
    let preflight_path = bind_selected_commands(&home, ambient, "danger-full-access")?;
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
    command.current_dir = Some(workspace.clone());
    command
        .env
        .insert("CODEX_HOME".into(), Some(home.clone().into_os_string()));
    command.env.insert("PATH".into(), Some(preflight_path));
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
    let path = bind_selected_commands(&home, ambient, &mode)?;
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

/// Child PATH for one selected installation.
///
/// The selected `harness/bin` is first, ahead of the entries `permitted_path`
/// already allowed for this sandbox. That prefix is sufficient while the
/// selected command inventory is complete and verified by the runtime owner.
/// It does not relocate unrelated tools, write a command program, or classify
/// remote, relative, or unreadable entries. The process, user, and registry
/// PATH are unchanged.
fn bind_selected_commands(
    home: &Path,
    ambient: &std::ffi::OsStr,
    mode: &str,
) -> io::Result<OsString> {
    let permitted = permitted_path(ambient.to_os_string(), mode)?;
    let selected = ordinary_absolute(&home.join("harness").join("bin"))?;
    let mut entries = vec![selected.clone()];
    for entry in std::env::split_paths(&permitted) {
        if entry.as_os_str().is_empty() || !same_dir(&entry, &selected) {
            entries.push(entry);
        }
    }
    let path = std::env::join_paths(&entries).map_err(io::Error::other)?;
    if let Some(text) = path.to_str() {
        harness_core::path_plan::check_precedence(text, &selected).map_err(|error| {
            io::Error::other(format!(
                "selected installation {} does not precede other managed codex commands: {error}",
                selected.display()
            ))
        })?;
    }
    Ok(path)
}

fn ordinary_absolute(path: &Path) -> io::Result<PathBuf> {
    Ok(ordinary_path(&std::path::absolute(path)?))
}

fn ordinary_path(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    if let Some(rest) = text.strip_prefix(r"\\?\") {
        return PathBuf::from(rest);
    }
    path.to_path_buf()
}

fn ordinary_key(path: &Path) -> String {
    ordinary_path(path)
        .to_string_lossy()
        .trim_end_matches(['\\', '/'])
        .to_ascii_lowercase()
}

fn same_dir(left: &Path, right: &Path) -> bool {
    ordinary_key(left) == ordinary_key(right)
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

    /// Real command resolution, not a constructed PATH string. The preflight
    /// child and a later PowerShell invocation both consume files in the
    /// selected harness/bin when ambient PATH lists conflicting copies first.
    /// Ordinary and explicit .exe names are both executed. Unrelated tools
    /// keep their original directory and can read data beside that directory.
    /// Remote and relative entries stay. Process PATH is not changed.
    #[test]
    fn selected_installation_commands_consume_that_installation() {
        let root = tempfile::tempdir().unwrap();
        let launcher = compile_tool(root.path(), "launcher", LAUNCHER_SOURCE);
        let selected_tool =
            compile_tool(root.path(), "selected-tool", &identity_source("selected"));
        let selected_rtk = compile_tool(
            root.path(),
            "selected-rtk",
            &identity_source("selected-rtk"),
        );
        let sibling_tool = compile_tool(root.path(), "sibling-tool", &identity_source("sibling"));
        let foreign_tool = compile_tool(root.path(), "foreign-tool", &identity_source("foreign"));
        let foreign_rtk = compile_tool(root.path(), "foreign-rtk", &identity_source("foreign-rtk"));
        let side_tool = compile_tool(
            root.path(),
            "side-tool",
            &relative_source("mixed", "side-data.txt"),
        );
        let unrelated_tool = compile_tool(
            root.path(),
            "unrelated-tool",
            &relative_source("unrelated", "marker-data.txt"),
        );

        let home = root.path().join("home");
        let bin = home.join("harness").join("bin");
        fs::create_dir_all(&bin).unwrap();
        fs::copy(&launcher, bin.join("codex.exe")).unwrap();
        fs::copy(&selected_tool, bin.join("codex-harness.exe")).unwrap();
        fs::copy(&selected_rtk, bin.join("harness-rtk.exe")).unwrap();
        let sibling = root.path().join("sibling");
        let foreign = root.path().join("foreign");
        let mixed = root.path().join("mixed");
        let unrelated = root.path().join("unrelated");
        for directory in [&sibling, &foreign, &mixed, &unrelated] {
            fs::create_dir_all(directory).unwrap();
        }
        fs::copy(&sibling_tool, sibling.join("codex-harness.exe")).unwrap();
        fs::copy(&foreign_tool, foreign.join("codex-harness.exe")).unwrap();
        fs::copy(&foreign_rtk, foreign.join("harness-rtk.exe")).unwrap();
        fs::copy(&foreign_rtk, mixed.join("harness-rtk.exe")).unwrap();
        fs::copy(&side_tool, mixed.join("side-tool.exe")).unwrap();
        fs::write(mixed.join("side-data.txt"), "mixed-sibling-sentinel").unwrap();
        fs::copy(&unrelated_tool, unrelated.join("marker-tool.exe")).unwrap();
        fs::write(
            unrelated.join("marker-data.txt"),
            "unrelated-sibling-sentinel",
        )
        .unwrap();
        let workspace = root.path().join("workspace");
        fs::create_dir_all(&workspace).unwrap();

        let mut entries = vec![
            sibling.clone(),
            foreign.clone(),
            mixed.clone(),
            unrelated.clone(),
            PathBuf::from("relative-tools"),
            PathBuf::from(r"\\server\share\harness-tools"),
        ];
        if let Some(path) = std::env::var_os("PATH") {
            entries.extend(std::env::split_paths(&path));
        }
        let ambient = std::env::join_paths(&entries).unwrap();
        let before = std::env::var_os("PATH");
        let owner_shell = Command::new("pwsh")
            .args([
                "-NoLogo",
                "-NoProfile",
                "-Command",
                "(Get-Command pwsh.exe).Source",
            ])
            .output()
            .unwrap_or_else(|error| panic!("owner PowerShell did not start: {error}"));
        assert!(
            owner_shell.status.success(),
            "owner PowerShell lookup failed: {}",
            text_output(&owner_shell)
        );
        let owner_pwsh = String::from_utf8_lossy(&owner_shell.stdout)
            .trim()
            .to_owned();
        let shell = prepare_with_path(
            &bin.join("codex.exe"),
            &home,
            "default",
            &workspace,
            None,
            &ambient,
        )
        .unwrap_or_else(|error| panic!("selected shell binding failed: {error}"));
        assert_eq!(
            before,
            std::env::var_os("PATH"),
            "binding must not mutate process PATH"
        );
        assert!(
            shell.version.starts_with("PowerShell 7."),
            "{}",
            shell.version
        );
        assert!(
            same_dir(Path::new(&owner_pwsh), &shell.executable),
            "binding relocated PowerShell from {owner_pwsh} to {}",
            shell.executable.display()
        );
        assert!(
            !home
                .join("harness")
                .join("executor-command-binding")
                .exists()
        );
        assert!(!home.join("harness").join("path-view").exists());

        let recorded = fs::read_to_string(workspace.join("preflight-path.txt"))
            .unwrap_or_else(|error| panic!("preflight did not record PATH: {error}"));
        let recorded = recorded.trim_end_matches(['\r', '\n']);
        assert_eq!(
            recorded,
            shell.path.to_string_lossy(),
            "the model-free preflight must use the same bound PATH the host returns"
        );
        let bound_entries: Vec<String> = std::env::split_paths(&shell.path)
            .map(|entry| entry.to_string_lossy().to_ascii_lowercase())
            .collect();
        let expected_bin =
            ordinary_absolute(&home.canonicalize().unwrap().join("harness").join("bin")).unwrap();
        let first = std::env::split_paths(&shell.path)
            .next()
            .unwrap_or_else(|| panic!("bound PATH is empty: {bound_entries:?}"));
        assert!(
            same_dir(&first, &expected_bin),
            "selected bin must be the first PATH entry: {first:?} expected {expected_bin:?}"
        );
        for kept in [&sibling, &foreign, &mixed, &unrelated] {
            let kept = kept.to_string_lossy().to_ascii_lowercase();
            assert!(
                bound_entries.iter().any(|entry| entry == &kept),
                "an unrelated tool directory was removed from PATH: {kept} in {bound_entries:?}"
            );
        }
        assert!(
            bound_entries.iter().any(|entry| entry == "relative-tools"),
            "relative PATH entries must be preserved: {bound_entries:?}"
        );
        assert!(
            bound_entries
                .iter()
                .any(|entry| entry == r"\\server\share\harness-tools"),
            "remote PATH entries must be preserved: {bound_entries:?}"
        );

        assert_identity(
            &shell,
            "codex-harness",
            "selected",
            &root.path().join("selected-name.txt"),
        );
        assert_identity(
            &shell,
            "codex-harness.exe",
            "selected",
            &root.path().join("selected-exe.txt"),
        );
        assert_identity(
            &shell,
            "harness-rtk",
            "selected-rtk",
            &root.path().join("selected-rtk-name.txt"),
        );
        assert_identity(
            &shell,
            "harness-rtk.exe",
            "selected-rtk",
            &root.path().join("selected-rtk-exe.txt"),
        );
        assert_relative(
            &shell,
            "side-tool",
            "mixed",
            "mixed-sibling-sentinel",
            &mixed,
        );
        assert_relative(
            &shell,
            "marker-tool",
            "unrelated",
            "unrelated-sibling-sentinel",
            &unrelated,
        );

        let shell_run = Command::new("pwsh")
            .args([
                "-NoLogo",
                "-NoProfile",
                "-Command",
                "$PSVersionTable.PSVersion.ToString(); (Get-Command pwsh.exe).Source",
            ])
            .env("PATH", &shell.path)
            .output()
            .unwrap_or_else(|error| {
                panic!("owner shell did not start from the bound PATH: {error}")
            });
        let shell_text = text_output(&shell_run);
        assert!(
            shell_run.status.success() && shell_text.contains("7."),
            "the owner PowerShell must remain usable: {shell_text}"
        );
        assert!(
            shell_text
                .to_ascii_lowercase()
                .contains(&owner_pwsh.to_ascii_lowercase()),
            "the bound child resolved a different PowerShell: {shell_text}"
        );
    }

    fn run_bound(shell: &PreparedShell, command: &str, marker: &Path) -> std::process::Output {
        Command::new(&shell.executable)
            .args(["-NoLogo", "-NoProfile", "-Command", command])
            .env("PATH", &shell.path)
            .env("ARM_TOOL_MARKER", marker)
            .output()
            .unwrap_or_else(|error| panic!("PowerShell did not start: {error}"))
    }

    fn assert_identity(shell: &PreparedShell, command: &str, identity: &str, marker: &Path) {
        let run = run_bound(shell, command, marker);
        let text = text_output(&run);
        assert!(run.status.success(), "{command} failed: {text}");
        let expected = format!("arm-tool-identity:{identity}");
        assert!(
            text.contains(&expected),
            "{command} did not consume the selected identity: {text}"
        );
        assert_eq!(fs::read_to_string(marker).unwrap().trim(), expected);
        let executed = text
            .lines()
            .find_map(|line| line.trim().strip_prefix("executed-from:"))
            .unwrap_or("");
        assert!(
            executed.to_ascii_lowercase().contains("harness\\bin")
                || executed.to_ascii_lowercase().contains("harness/bin"),
            "{command} did not execute from the selected installation: {text}"
        );
    }

    fn assert_relative(
        shell: &PreparedShell,
        command: &str,
        identity: &str,
        sentinel: &str,
        original: &Path,
    ) {
        let marker = original.join("ran.txt");
        let run = run_bound(shell, command, &marker);
        let text = text_output(&run);
        assert!(run.status.success(), "{command} failed: {text}");
        assert!(
            text.contains(&format!("arm-tool-identity:{identity}")),
            "{command} did not consume the original tool: {text}"
        );
        assert!(
            text.contains(&format!("relative-data:{sentinel}")),
            "{command} did not read data beside its original directory: {text}"
        );
        let executed = text
            .lines()
            .find_map(|line| line.trim().strip_prefix("executed-from:"))
            .unwrap_or("");
        assert!(
            same_dir(
                Path::new(executed).parent().unwrap_or(Path::new(executed)),
                original
            ),
            "{command} did not run from its original directory {original:?}: {text}"
        );
        assert!(
            !executed.to_ascii_lowercase().contains("path-view"),
            "{command} was relocated into a path view: {text}"
        );
    }

    fn text_output(output: &std::process::Output) -> String {
        format!(
            "exit {:?}\nstdout {}\nstderr {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    }

    fn compile_tool(root: &Path, name: &str, source: &str) -> PathBuf {
        let source_path = root.join(format!("{name}.rs"));
        fs::write(&source_path, source).unwrap();
        let executable = root.join(format!("{name}.exe"));
        let output = Command::new("rustc")
            .arg(&source_path)
            .arg("--edition=2024")
            .arg("-o")
            .arg(&executable)
            .output()
            .unwrap_or_else(|error| {
                panic!("rustc is required to build the command fixture: {error}")
            });
        assert!(
            output.status.success(),
            "fixture compile failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        executable
    }

    fn identity_source(id: &str) -> String {
        let source = r#"fn main() {
    let exe = std::env::current_exe().expect("exe");
    let id = "arm-tool-identity:ID";
    if let Ok(path) = std::env::var("ARM_TOOL_MARKER") {
        let _ = std::fs::write(path, id);
    }
    println!("{id}");
    println!("executed-from:{}", exe.display());
}
"#;
        source.replace("ID", id)
    }

    fn relative_source(id: &str, data_name: &str) -> String {
        let source = r#"fn main() {
    let exe = std::env::current_exe().expect("exe");
    let data = exe.parent().expect("parent").join("DATA");
    let body = std::fs::read_to_string(&data).unwrap_or_else(|error| format!("missing:{error}"));
    println!("arm-tool-identity:ID");
    println!("relative-data:{body}");
    println!("executed-from:{}", exe.display());
}
"#;
        source.replace("ID", id).replace("DATA", data_name)
    }

    const LAUNCHER_SOURCE: &str = r#"
fn main() {
    let path = std::env::var("PATH").unwrap_or_else(|_| "missing".to_owned());
    let directory = std::env::current_dir().expect("workspace");
    std::fs::write(directory.join("preflight-path.txt"), &path).expect("record preflight PATH");
    println!(
        "[{{\"type\":\"message\",\"role\":\"developer\",\"content\":[{{\"type\":\"input_text\",\"text\":\"<permissions instructions>\\nFilesystem sandboxing defines which files can be read or written. `sandbox_mode` is `danger-full-access`: No filesystem sandboxing - all commands are permitted.\\nApproval policy is currently never.\\n</permissions instructions>\"}}]}}]"
    );
}
"#;
}
