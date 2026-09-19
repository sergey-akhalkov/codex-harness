//! Dispatch a configured executor through native `codex --profile`.
#![cfg(windows)]

use harness_core::orchestration_config::{
    self, ProfileBinding, executor_profile, load, profile_args,
};
use harness_core::task_view;
use serde_json::json;
use std::{
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
    thread,
    time::Duration,
};

use harness_core::process::{CommandSpec, suppress_loader_dialogs};
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};

const USAGE: &str = "codex-harness executor spawn --source CHECKOUT --codex-home DIRECTORY --workspace DIRECTORY [--profile ID] --exec PROMPT\ncodex-harness executor steer --thread ID --worktree DIRECTORY --text TEXT [--out FILE]\ncodex-harness executor run LAUNCHER [ARG...]\nSpawn opens a tab in the current Windows terminal when WT_SESSION is set, otherwise a visible TUI, and returns so the lead can keep working. The prompt is prefixed with /goal unless it already starts with a slash command, and a terminal tab hosts the session through `executor run`, which closes the tab on any exit. Assignments live on the beads board; executors set lead_review when done. Steer delivers visible turn/start with no status polling.";
const STARTUP: Duration = Duration::from_secs(20);
/// Runtime identity of the dispatching session must not leak into the
/// executor: an inherited session/thread id makes the child attach to the
/// lead's conversation instead of the assignment.
const INHERITED_SESSION_ENV: [&str; 3] = ["CODEX_SESSION_ID", "CODEX_THREAD_ID", "CODEX_CI"];

pub fn run(args: &[OsString]) -> io::Result<i32> {
    if args.first().is_some_and(|arg| arg == "--help") {
        println!("{USAGE}");
        return Ok(0);
    }
    match args.first().and_then(|arg| arg.to_str()) {
        Some("spawn") => spawn(&args[1..]),
        Some("steer") => steer(&args[1..]),
        Some("run") => run_exec(&args[1..]),
        _ => Err(invalid("invalid native executor options")),
    }
}

fn spawn(args: &[OsString]) -> io::Result<i32> {
    let mut source = None;
    let mut codex_home = None;
    let mut workspace = None;
    let mut profile = None;
    let mut prompt = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let key = arg
            .to_str()
            .ok_or_else(|| invalid("invalid native executor options"))?;
        let value = iter
            .next()
            .ok_or_else(|| invalid("invalid native executor options"))?;
        match key {
            "--source" => source = Some(PathBuf::from(value)),
            "--codex-home" => codex_home = Some(PathBuf::from(value)),
            "--workspace" => workspace = Some(PathBuf::from(value)),
            "--profile" => {
                profile = Some(
                    value
                        .to_str()
                        .ok_or_else(|| invalid("invalid native executor options"))?
                        .to_owned(),
                )
            }
            "--exec" => {
                prompt = Some(
                    value
                        .to_str()
                        .ok_or_else(|| invalid("invalid native executor options"))?
                        .to_owned(),
                )
            }
            _ => return Err(invalid("invalid native executor options")),
        }
    }
    let source = required(source, "--source")?;
    let codex_home = required(codex_home, "--codex-home")?;
    let workspace = required(workspace, "--workspace")?;
    let prompt = prompt
        .ok_or_else(|| invalid("--exec is required"))
        .map(|prompt| goal_prompt(&prompt))?;
    if !source.is_absolute() || !codex_home.is_absolute() || !workspace.is_absolute() {
        return Err(invalid("executor spawn paths must be absolute"));
    }
    let config = load(&source)?;
    let profile = executor_profile(&config, profile.as_deref())?.to_owned();
    dispatch(&codex_home, &workspace, &profile, &prompt)
}

fn dispatch(codex_home: &Path, workspace: &Path, profile: &str, prompt: &str) -> io::Result<i32> {
    let bound = orchestration_config::binding(codex_home, profile)?;
    fs::create_dir_all(workspace)?;
    ensure_workspace_trust(codex_home, workspace)?;
    let isolation = harness_core::task_worktree::exec_isolation_args(codex_home, workspace)?;
    let launcher = codex_home.join("harness/bin/codex.exe");
    if !launcher.is_file() {
        return Err(invalid("installed Codex launcher is missing"));
    }
    let args = tui_args(profile, workspace, prompt, &isolation)?;
    let title = format!("Codex executor ({profile})");
    let session = std::env::var_os("WT_SESSION");
    let client = windows_terminal_client();
    if prefers_terminal_tab(session.as_deref(), client.as_deref()) {
        dispatch_terminal_tab(
            client.as_ref().expect("terminal client"),
            &launcher,
            workspace,
            profile,
            &title,
            &args,
            &bound,
            codex_home,
        )
    } else {
        dispatch_owned_console(&launcher, workspace, profile, &args, &bound, codex_home)
    }
}

/// Executor assignments are bounded outcomes: carry them as a goal so a
/// mid-work stop does not silently drop the assignment. An explicit slash
/// command from the caller keeps native precedence.
fn goal_prompt(prompt: &str) -> String {
    if prompt.starts_with('/') {
        prompt.to_owned()
    } else {
        format!("/goal {prompt}")
    }
}

/// Tab host: forward argv to the launcher and exit successfully regardless of
/// the child outcome, so Windows Terminal closes the tab on any exit instead
/// of leaving a dead tab that someone must remember to close.
fn run_exec(args: &[OsString]) -> io::Result<i32> {
    let Some((launcher, rest)) = args.split_first() else {
        return Err(invalid("executor run requires the launcher path"));
    };
    if !Path::new(launcher).is_absolute() {
        return Err(invalid("executor run launcher must be absolute"));
    }
    Command::new(launcher).args(rest).status()?;
    Ok(0)
}

fn prefers_terminal_tab(session: Option<&std::ffi::OsStr>, client: Option<&Path>) -> bool {
    session.is_some() && client.is_some()
}

fn windows_terminal_client() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        candidates.push(PathBuf::from(local).join(r"Microsoft\WindowsApps\wt.exe"));
    }
    if let Some(pf) = std::env::var_os("ProgramFiles") {
        if let Ok(entries) = fs::read_dir(PathBuf::from(pf).join("WindowsApps")) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                if name
                    .to_string_lossy()
                    .starts_with("Microsoft.WindowsTerminal_")
                {
                    candidates.push(entry.path().join("wt.exe"));
                }
            }
        }
    }
    candidates.into_iter().find(|path| path.is_file())
}

fn escape_wt_commandline(arg: &str) -> String {
    arg.replace(';', r"\;")
}

fn terminal_tab_args(
    title: &str,
    workspace: &Path,
    wrapper: &Path,
    launcher: &Path,
    tui: &[String],
) -> io::Result<Vec<String>> {
    let mut args = vec![
        "-w".into(),
        "0".into(),
        "new-tab".into(),
        "--title".into(),
        title.to_owned(),
        "--suppressApplicationTitle".into(),
        "-d".into(),
        unicode(workspace)?,
        unicode(wrapper)?,
        "run".into(),
        unicode(launcher)?,
    ];
    args.extend(tui.iter().map(|arg| escape_wt_commandline(arg)));
    if args.iter().any(|arg| {
        arg == "--focus"
            || arg == "-f"
            || arg == "--maximized"
            || arg == "-M"
            || arg == "--fullscreen"
            || arg == "-F"
    }) {
        return Err(invalid("terminal tab spawn must not steal focus"));
    }
    Ok(args)
}

fn apply_executor_env(spec: &mut CommandSpec, codex_home: &Path) {
    spec.env
        .insert("CODEX_HOME".into(), Some(codex_home.as_os_str().to_owned()));
    if let Some(path) = filtered_path() {
        spec.env.insert("PATH".into(), Some(path));
    }
    for name in INHERITED_SESSION_ENV {
        spec.env.insert(name.into(), None);
    }
}

/// Codex blocks an untrusted project directory behind an interactive prompt
/// the executor cannot answer. Trust the explicitly dispatched workspace the
/// same way the interactive approval would, using codex's own config format.
fn ensure_workspace_trust(codex_home: &Path, workspace: &Path) -> io::Result<()> {
    let config = codex_home.join("config.toml");
    let text = fs::read_to_string(&config).unwrap_or_default();
    let section = format!("[projects.'{}']", unicode(workspace)?.to_ascii_lowercase());
    if text
        .lines()
        .any(|line| line.trim().eq_ignore_ascii_case(&section))
    {
        return Ok(());
    }
    let mut updated = text;
    if !updated.is_empty() && !updated.ends_with('\n') {
        updated.push('\n');
    }
    updated.push_str(&format!("\n{section}\ntrust_level = \"trusted\"\n"));
    fs::write(&config, updated)
}

fn dispatch_terminal_tab(
    wt: &Path,
    launcher: &Path,
    workspace: &Path,
    profile: &str,
    title: &str,
    tui: &[String],
    bound: &ProfileBinding,
    codex_home: &Path,
) -> io::Result<i32> {
    let wrapper = std::env::current_exe()
        .map_err(|error| io::Error::other(format!("executor wrapper path: {error}")))?;
    let args = terminal_tab_args(title, workspace, &wrapper, launcher, tui)?;
    save_receipt(
        workspace,
        profile,
        tui,
        bound,
        None,
        "windows-terminal-tab",
        Some(&args),
    )?;
    task_view::preserve_foreground(|| {
        suppress_loader_dialogs();
        let mut cmd = Command::new(wt);
        cmd.args(&args)
            .current_dir(workspace)
            .env("CODEX_HOME", codex_home)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(0x0800_0000);
        if let Some(path) = filtered_path() {
            cmd.env("PATH", path);
        }
        for name in INHERITED_SESSION_ENV {
            cmd.env_remove(name);
        }
        let status = cmd.status()?;
        if !status.success() {
            return Err(invalid("windows terminal tab spawn failed"));
        }
        Ok(())
    })?;
    println!(
        "{}",
        spawn_summary(profile, bound, title, workspace, "windows-terminal-tab",)
    );
    Ok(0)
}

fn dispatch_owned_console(
    launcher: &Path,
    workspace: &Path,
    profile: &str,
    args: &[String],
    bound: &ProfileBinding,
    codex_home: &Path,
) -> io::Result<i32> {
    save_receipt(workspace, profile, args, bound, None, "owned-console", None)?;
    println!(
        "{}",
        spawn_summary(
            profile,
            bound,
            &format!("Codex executor ({profile})"),
            workspace,
            "owned-console",
        )
    );
    let view = task_view::preserve_foreground(|| {
        let mut spec = CommandSpec::new(launcher);
        spec.args = args.iter().map(OsString::from).collect();
        spec.current_dir = Some(workspace.to_path_buf());
        spec.new_console = Some(format!("Opening Codex executor ({profile})").into());
        apply_executor_env(&mut spec, codex_home);
        let placements = task_view::layout(1)?;
        let bounds = placements
            .first()
            .copied()
            .ok_or_else(|| invalid("executor window layout is empty"))?;
        let view = task_view::View::spawn(&spec, bounds, STARTUP)?;
        let _ = view.snapshot()?;
        Ok(view)
    })?;
    let snapshot = view.snapshot()?;
    save_receipt(
        workspace,
        profile,
        args,
        bound,
        Some(&snapshot),
        "owned-console",
        None,
    )?;
    while view.is_running()? {
        thread::sleep(Duration::from_millis(200));
    }
    Ok(view.exit_code()?.unwrap_or(1) as i32)
}

fn spawn_summary(
    profile: &str,
    bound: &ProfileBinding,
    title: &str,
    workspace: &Path,
    host: &str,
) -> String {
    let model = bound.model.as_deref().unwrap_or("unknown");
    let provider = bound.model_provider.as_deref().unwrap_or("unknown");
    let effort = bound.reasoning_effort.as_deref().unwrap_or("default");
    format!(
        "executor started: profile={profile} model={model} provider={provider} effort={effort} host={host} title=\"{title}\"\nreceipt: {}",
        workspace.join("executor-spawn.json").display()
    )
}

fn tui_args(
    profile: &str,
    workspace: &Path,
    prompt: &str,
    isolation: &[String],
) -> io::Result<Vec<String>> {
    let mut args = isolation.to_vec();
    args.extend(profile_args(profile)?);
    args.extend(["-C".into(), unicode(workspace)?, prompt.to_owned()]);
    if args.iter().any(|arg| arg == "--remote") && args.iter().any(|arg| arg == "--worktree") {
        return Err(invalid(
            "native CLI rejects --worktree with --remote; attach the view to the managed cwd",
        ));
    }
    if args.iter().any(|arg| arg == "exec" || arg == "--json") {
        return Err(invalid(
            "executor spawn must open a visible TUI, not headless exec",
        ));
    }
    Ok(args)
}

fn save_receipt(
    workspace: &Path,
    profile: &str,
    args: &[String],
    bound: &ProfileBinding,
    window: Option<&task_view::Snapshot>,
    host: &str,
    terminal: Option<&[String]>,
) -> io::Result<()> {
    let window = match window {
        Some(snapshot) => serde_json::to_value(snapshot)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?,
        None => json!(null),
    };
    fs::write(
        workspace.join("executor-spawn.json"),
        serde_json::to_vec_pretty(&json!({
            "schema": 1,
            "profile": profile,
            "args": args,
            "visible": true,
            "host": host,
            "terminal": terminal,
            "isolation": args.iter().any(|arg| arg == "--worktree"),
            "model": bound.model,
            "modelProvider": bound.model_provider,
            "reasoningEffort": bound.reasoning_effort,
            "window": window,
        }))?,
    )
}

fn filtered_path() -> Option<std::ffi::OsString> {
    filter_windowsapps_path(std::env::var_os("PATH")?)
}

fn filter_windowsapps_path(path: std::ffi::OsString) -> Option<std::ffi::OsString> {
    std::env::join_paths(std::env::split_paths(&path).filter(|entry| {
        !entry
            .to_string_lossy()
            .to_ascii_lowercase()
            .contains("windowsapps")
    }))
    .ok()
}

fn required(value: Option<PathBuf>, name: &str) -> io::Result<PathBuf> {
    value.ok_or_else(|| invalid(&format!("{name} is required")))
}

fn unicode(path: &Path) -> io::Result<String> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| invalid("executor workspace path must be unicode"))
}

fn steer(args: &[OsString]) -> io::Result<i32> {
    let mut thread = None;
    let mut worktree = None;
    let mut text = None;
    let mut out = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let key = arg
            .to_str()
            .ok_or_else(|| invalid("invalid native executor options"))?;
        let value = iter
            .next()
            .ok_or_else(|| invalid("invalid native executor options"))?;
        match key {
            "--thread" => thread = Some(value.to_string_lossy().into_owned()),
            "--worktree" => worktree = Some(PathBuf::from(value)),
            "--text" => text = Some(value.to_string_lossy().into_owned()),
            "--out" => out = Some(PathBuf::from(value)),
            _ => return Err(invalid("invalid native executor options")),
        }
    }
    let thread = thread.ok_or_else(|| invalid("--thread is required"))?;
    let worktree = worktree.ok_or_else(|| invalid("--worktree is required"))?;
    let text = text.ok_or_else(|| invalid("--text is required"))?;
    let payload = harness_core::task_orchestrate::steer(&thread, &text, &worktree);
    if payload["hiddenModelCall"] != false || payload["statusPoll"] != false {
        return Err(invalid("steering must not hide model calls or poll status"));
    }
    if let Some(path) = out {
        fs::write(path, serde_json::to_vec_pretty(&payload)?)?;
    } else {
        println!("{}", serde_json::to_string_pretty(&payload)?);
    }
    Ok(0)
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_is_accepted() {
        assert_eq!(run(&[OsString::from("--help")]).unwrap(), 0);
    }

    #[test]
    fn windowsapps_path_entries_are_removed() {
        let filtered = filter_windowsapps_path(
            std::env::join_paths([
                PathBuf::from(r"C:\Program Files\PowerShell\7"),
                PathBuf::from(r"C:\Program Files\WindowsApps\Microsoft.PowerShell_8wekyb3d8bbwe"),
                PathBuf::from(r"C:\Windows\System32"),
            ])
            .unwrap(),
        )
        .unwrap();
        let text = filtered.to_string_lossy().to_ascii_lowercase();
        assert!(text.contains(r"c:\program files\powershell\7"));
        assert!(text.contains(r"c:\windows\system32"));
        assert!(!text.contains("windowsapps"));
    }

    #[test]
    fn unknown_option_is_rejected() {
        let error = run(&[OsString::from("spawn"), OsString::from("--proxy")]).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("invalid native executor options")
        );
    }

    #[test]
    fn tui_args_open_a_profile_session_not_headless_exec() {
        let args = tui_args("xai", Path::new(r"D:\wt\xai"), "do the work", &[]).unwrap();
        assert_eq!(args[0], "--profile");
        assert_eq!(args[1], "xai");
        assert!(args.contains(&"-C".to_string()));
        assert!(args.contains(&r"D:\wt\xai".to_string()));
        assert!(!args.iter().any(|arg| arg == "exec" || arg == "--json"));
        assert!(!args.iter().any(|arg| arg == "--remote"));
        assert_eq!(args.last().unwrap(), "do the work");
    }

    #[test]
    fn remote_tui_cannot_take_worktree_flag() {
        let error = tui_args(
            "xai",
            Path::new(r"D:\wt\xai"),
            "do the work",
            &["--remote".into(), "--worktree".into()],
        )
        .unwrap_err();
        assert!(error.to_string().contains("--worktree"));
    }

    #[test]
    fn terminal_tab_is_used_only_inside_the_current_terminal() {
        let client = Path::new(r"C:\term\wt.exe");
        assert!(prefers_terminal_tab(
            Some(std::ffi::OsStr::new("session")),
            Some(client)
        ));
        assert!(!prefers_terminal_tab(None, Some(client)));
        assert!(!prefers_terminal_tab(
            Some(std::ffi::OsStr::new("session")),
            None
        ));
    }

    #[test]
    fn terminal_tab_args_open_last_window_without_focus_flags() {
        let args = terminal_tab_args(
            "Codex executor (xai)",
            Path::new(r"D:\wt\xai"),
            Path::new(r"C:\harness\codex-harness.exe"),
            Path::new(r"C:\codex.exe"),
            &["--profile".into(), "xai".into(), "do;the work".into()],
        )
        .unwrap();
        assert_eq!(args[0], "-w");
        assert_eq!(args[1], "0");
        assert_eq!(args[2], "new-tab");
        assert!(args.contains(&"--suppressApplicationTitle".to_string()));
        let wrapper = args
            .iter()
            .position(|arg| arg == r"C:\harness\codex-harness.exe")
            .expect("wrapper executable");
        assert_eq!(args[wrapper + 1], "run");
        assert_eq!(args[wrapper + 2], r"C:\codex.exe");
        assert_eq!(args.last().unwrap(), r"do\;the work");
        assert!(args.contains(&r"do\;the work".to_string()));
        assert!(
            !args.iter().any(|arg| {
                arg == "--focus" || arg == "-f" || arg == "--maximized" || arg == "-M"
            })
        );
    }

    #[test]
    fn steer_writes_visible_turn_without_polling() {
        let out = std::env::temp_dir().join(format!("steer-{}.json", std::process::id()));
        let code = run(&[
            OsString::from("steer"),
            OsString::from("--thread"),
            OsString::from("exec-xai"),
            OsString::from("--worktree"),
            OsString::from(r"D:\wt\xai"),
            OsString::from("--text"),
            OsString::from("use the fixture"),
            OsString::from("--out"),
            OsString::from(out.as_os_str()),
        ])
        .unwrap();
        assert_eq!(code, 0);
        let payload: serde_json::Value = serde_json::from_slice(&fs::read(&out).unwrap()).unwrap();
        assert_eq!(payload["method"], "turn/start");
        assert_eq!(payload["hiddenModelCall"], false);
        assert_eq!(payload["statusPoll"], false);
        let _ = fs::remove_file(out);
    }

    #[test]
    fn spawn_summary_reports_the_dispatched_session() {
        let bound = ProfileBinding {
            profile: "ds".into(),
            model: Some("deepseek-flash".into()),
            model_provider: Some("deepseek".into()),
            reasoning_effort: Some("max".into()),
        };
        let summary = spawn_summary(
            "ds",
            &bound,
            "Codex executor (ds)",
            Path::new(r"D:\wt\ds"),
            "windows-terminal-tab",
        );
        assert!(summary.contains("profile=ds"));
        assert!(summary.contains("model=deepseek-flash"));
        assert!(summary.contains("provider=deepseek"));
        assert!(summary.contains("effort=max"));
        assert!(summary.contains("host=windows-terminal-tab"));
        assert!(summary.contains("title=\"Codex executor (ds)\""));
        assert!(summary.contains(r"D:\wt\ds\executor-spawn.json"));
    }

    #[test]
    fn executor_env_drops_inherited_session_identity() {
        let mut spec = CommandSpec::new(Path::new("codex.exe"));
        apply_executor_env(&mut spec, Path::new(r"C:\codex-home"));
        for name in INHERITED_SESSION_ENV {
            assert_eq!(spec.env.get(std::ffi::OsStr::new(name)), Some(&None));
        }
        assert_eq!(
            spec.env.get(std::ffi::OsStr::new("CODEX_HOME")),
            Some(&Some(PathBuf::from(r"C:\codex-home").into_os_string()))
        );
    }

    #[test]
    fn workspace_trust_is_written_once_in_codex_format() {
        let root = std::env::temp_dir().join(format!("executor-trust-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let config = root.join("config.toml");
        fs::write(&config, "model = \"x\"\n").unwrap();
        let workspace = PathBuf::from(r"D:\WT\DS");
        ensure_workspace_trust(&root, &workspace).unwrap();
        ensure_workspace_trust(&root, &workspace).unwrap();
        let text = fs::read_to_string(&config).unwrap();
        assert_eq!(text.matches("[projects.'d:\\wt\\ds']").count(), 1);
        assert!(text.contains("trust_level = \"trusted\""));
        assert!(text.contains("model = \"x\""));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn goal_prefix_is_added_only_without_an_explicit_command() {
        assert_eq!(
            goal_prompt("finish the outcome"),
            "/goal finish the outcome"
        );
        assert_eq!(goal_prompt("/goal finish"), "/goal finish");
        assert_eq!(goal_prompt("/compact"), "/compact");
    }

    #[test]
    fn executor_run_requires_an_absolute_launcher() {
        let error = run_exec(&[OsString::from(r"codex.exe")]).unwrap_err();
        assert!(error.to_string().contains("absolute"));
        let error = run_exec(&[]).unwrap_err();
        assert!(error.to_string().contains("launcher path"));
    }
}
