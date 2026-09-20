//! Dispatch a configured executor through native `codex --profile`, and
//! replace one exact session's CLI process under refreshed instructions
//! (instruction-refresh succession, OFAP 4.1).
#![cfg(windows)]

use harness_core::orchestration_config::{
    self, ProfileBinding, executor_profile, load, profile_args,
};
use harness_core::process::{Job, Limits, StopReason};
use harness_core::process_service::ServiceProcess;
use harness_core::task_control::ControlConnection;
use harness_core::task_succession::{
    self, Boundary, NativeFacts, Reload, ReloadExpectation, Request as SuccessionRequest,
    SessionFacts, SuccessorPlan,
};
use harness_core::task_view;
use serde_json::json;
use std::{
    ffi::OsString,
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

use harness_core::process::{CommandSpec, suppress_loader_dialogs};
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};

const USAGE: &str = "codex-harness executor spawn --source CHECKOUT --codex-home DIRECTORY --workspace DIRECTORY [--profile ID] [--mode exec|tui] [--terminal-profile NAME] --exec PROMPT\ncodex-harness executor steer --thread ID --worktree DIRECTORY --text TEXT [--out FILE]\ncodex-harness executor run LAUNCHER [ARG...]\ncodex-harness executor succeed --request PATH\nSpawn opens a tab in the current Windows terminal when WT_SESSION is set, otherwise a visible console, and returns so the lead can keep working. The default exec mode streams the assignment visibly and exits on completion, so the tab closes itself; continue or correct the exact session later with codex exec resume SESSION_ID. The tui mode keeps an interactive conversation. Assignments live on the beads board; executors set lead_review when done. Steer delivers visible turn/start with no status polling. Succeed replaces one exact session's CLI process through the verified non-interactive `codex exec resume` path at a safe boundary: it writes a durable handover record, stops the predecessor, resumes the exact session under refreshed instructions and reports 'succession not established' when the reload cannot be verified.";
const STARTUP: Duration = Duration::from_secs(20);
const SUCCESSION_LIMIT: u64 = 4 * 1024 * 1024;
const INSTRUCTION_READ_LIMIT: u64 = 1024 * 1024;
const BOUNDARY_POLL: Duration = Duration::from_millis(500);
const STOP_GRACE: Duration = Duration::from_secs(30);
const SUCCESSION_EXIT_CODE: u32 = 130;
/// Runtime identity of the dispatching session must not leak into the
/// executor: an inherited session/thread id makes the child attach to the
/// lead's conversation instead of the assignment.
const INHERITED_SESSION_ENV: [&str; 5] = [
    "CODEX_SESSION_ID",
    "CODEX_THREAD_ID",
    "CODEX_CI",
    // The lead's tooling may force monochrome TUI output; executors render
    // in their own terminal host and must not inherit that decision.
    "NO_COLOR",
    "TERM",
];

pub fn run(args: &[OsString]) -> io::Result<i32> {
    if let Some(log) = std::env::var_os("HARNESS_EXECUTOR_ARGV_LOG") {
        let dump = args
            .iter()
            .map(|arg| format!("{arg:?}"))
            .collect::<Vec<_>>()
            .join("\n");
        let _ = fs::write(&log, dump);
    }
    if args.first().is_some_and(|arg| arg == "--help") {
        println!("{USAGE}");
        return Ok(0);
    }
    match args.first().and_then(|arg| arg.to_str()) {
        Some("spawn") => spawn(&args[1..]),
        Some("steer") => steer(&args[1..]),
        Some("run") => run_exec(&args[1..]),
        Some("succeed") => succeed(&args[1..]),
        _ => Err(invalid("invalid native executor options")),
    }
}

fn spawn(args: &[OsString]) -> io::Result<i32> {
    let mut source = None;
    let mut codex_home = None;
    let mut workspace = None;
    let mut profile = None;
    let mut terminal_profile = None;
    let mut mode = None;
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
            "--terminal-profile" => {
                terminal_profile = Some(
                    value
                        .to_str()
                        .ok_or_else(|| invalid("invalid native executor options"))?
                        .to_owned(),
                )
            }
            "--mode" => {
                mode = Some(
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
    let prompt = prompt.ok_or_else(|| invalid("--exec is required"))?;
    if !source.is_absolute() || !codex_home.is_absolute() || !workspace.is_absolute() {
        return Err(invalid("executor spawn paths must be absolute"));
    }
    let config = load(&source)?;
    let profile = executor_profile(&config, profile.as_deref())?.to_owned();
    let mode = SpawnMode::parse(mode.as_deref())?;
    let audit = harness_core::task_worktree::audit(&source)?;
    if audit.total >= config.worktree_limit {
        eprintln!(
            "worktree warning: {} registered worktrees reach the limit {}; retire finished lanes or reset them for reuse (git worktree list)",
            audit.total, config.worktree_limit
        );
    }
    dispatch(
        &codex_home,
        &workspace,
        &profile,
        &prompt,
        mode,
        terminal_profile.as_deref(),
    )
}

/// `exec` streams the assignment in a visible tab and exits on completion, so
/// the tab closes itself and corrections reopen the exact session via
/// `codex resume`. `tui` keeps an interactive conversation for cases that
/// need a human-attended executor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SpawnMode {
    Exec,
    Tui,
}

impl SpawnMode {
    fn parse(value: Option<&str>) -> io::Result<Self> {
        match value {
            None | Some("exec") => Ok(Self::Exec),
            Some("tui") => Ok(Self::Tui),
            Some(other) => Err(invalid(&format!(
                "unknown executor mode {other}; use exec or tui"
            ))),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Exec => "exec",
            Self::Tui => "tui",
        }
    }
}

fn dispatch(
    codex_home: &Path,
    workspace: &Path,
    profile: &str,
    prompt: &str,
    mode: SpawnMode,
    terminal_profile: Option<&str>,
) -> io::Result<i32> {
    let bound = orchestration_config::binding(codex_home, profile)?;
    fs::create_dir_all(workspace)?;
    ensure_workspace_trust(codex_home, workspace)?;
    let isolation = harness_core::task_worktree::exec_isolation_args(codex_home, workspace)?;
    let launcher = codex_home.join("harness/bin/codex.exe");
    if !launcher.is_file() {
        return Err(invalid("installed Codex launcher is missing"));
    }
    let args = child_args(profile, workspace, prompt, &isolation, mode)?;
    let title = format!("Codex executor ({profile})");
    let session = std::env::var_os("WT_SESSION");
    let client = windows_terminal_client();
    if prefers_terminal_tab(session.as_deref(), client.as_deref()) {
        dispatch_terminal_tab(
            client.as_ref().expect("terminal client"),
            &launcher,
            workspace,
            profile,
            mode,
            terminal_profile,
            &title,
            &args,
            &bound,
            codex_home,
        )
    } else {
        dispatch_owned_console(
            &launcher, workspace, profile, mode, &args, &bound, codex_home,
        )
    }
}

/// Tab host: forward argv to the launcher and exit successfully regardless of
/// the child outcome, so Windows Terminal closes the tab on any exit instead
/// of leaving a dead tab that someone must remember to close.
fn run_exec(args: &[OsString]) -> io::Result<i32> {
    if args.len() == 2 && args[0] == "--file" {
        return run_receipt(&args[1]);
    }
    let Some((launcher, rest)) = args.split_first() else {
        return Err(invalid("executor run requires the launcher path"));
    };
    let launcher = normalize_launcher(launcher)?;
    run_child(&launcher, rest)
}

fn run_receipt(path: &std::ffi::OsStr) -> io::Result<i32> {
    let bytes =
        fs::read(path).map_err(|error| invalid(&format!("executor run receipt: {error}")))?;
    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|error| invalid(&format!("executor run receipt JSON: {error}")))?;
    let launcher = value
        .get("launcher")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| invalid("executor run receipt launcher is missing"))?
        .to_owned();
    let rest = value
        .get("args")
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(OsString::from)
                .collect::<Vec<_>>()
        })
        .ok_or_else(|| invalid("executor run receipt args are missing"))?;
    run_child(&launcher, &rest)
}

fn run_child(launcher: &str, rest: &[OsString]) -> io::Result<i32> {
    let launcher = launcher.replace('/', r"\");
    let launcher = launcher.as_str();
    if !Path::new(launcher).is_absolute() {
        return Err(invalid("executor run launcher must be absolute"));
    }
    let mut command = Command::new(launcher);
    command.args(rest);
    // Optional diagnostics: capture the child's stderr without touching its
    // terminal stdout, so launch failures under a tab host stay observable.
    if let Some(log) = std::env::var_os("HARNESS_EXECUTOR_RUN_LOG") {
        let file = fs::File::create(&log)
            .map_err(|error| invalid(&format!("executor run log: {error}")))?;
        command.stderr(file);
    }
    command.status()?;
    Ok(0)
}

/// Forward-slash launcher paths reach `CreateProcess` through a path that
/// splits them; normalize to native separators before dispatch.
fn normalize_launcher(launcher: &std::ffi::OsStr) -> io::Result<String> {
    let text = launcher
        .to_str()
        .ok_or_else(|| invalid("executor run launcher must be unicode"))?;
    Ok(text.replace('/', r"\"))
}

fn prefers_terminal_tab(session: Option<&std::ffi::OsStr>, client: Option<&Path>) -> bool {
    session.is_some() && client.is_some()
}

fn windows_terminal_client() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        candidates.push(PathBuf::from(local).join(r"Microsoft\WindowsApps\wt.exe"));
    }
    if let Some(pf) = std::env::var_os("ProgramFiles")
        && let Ok(entries) = fs::read_dir(PathBuf::from(pf).join("WindowsApps"))
    {
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
    candidates.into_iter().find(|path| path.is_file())
}

fn escape_wt_commandline(arg: &str) -> String {
    arg.replace(';', r"\;")
}

fn terminal_tab_args(
    title: &str,
    workspace: &Path,
    wrapper: &Path,
    receipt: &Path,
    terminal_profile: Option<&str>,
) -> io::Result<Vec<String>> {
    let mut args = vec![
        "-w".into(),
        "0".into(),
        "new-tab".into(),
        "--title".into(),
        title.to_owned(),
        "--suppressApplicationTitle".into(),
    ];
    if let Some(name) = terminal_profile {
        args.extend(["--profile".into(), name.to_owned()]);
    }
    args.extend([
        "-d".into(),
        native_path(workspace)?,
        native_path(wrapper)?,
        "executor".into(),
        "run".into(),
        "--file".into(),
        escape_wt_commandline(&native_path(receipt)?),
    ]);
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

/// Windows Terminal re-tokenizes the tab commandline and mangles option-like
/// tail arguments when paths use forward slashes; native separators keep the
/// command boundary unambiguous.
fn native_path(path: &Path) -> io::Result<String> {
    Ok(unicode(path)?.replace('/', r"\"))
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
    spec.env
        .insert("COLORTERM".into(), Some("truecolor".into()));
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

// One terminal dispatch carries the whole isolated assignment context.
#[allow(clippy::too_many_arguments)]
fn dispatch_terminal_tab(
    wt: &Path,
    launcher: &Path,
    workspace: &Path,
    profile: &str,
    mode: SpawnMode,
    terminal_profile: Option<&str>,
    title: &str,
    tui: &[String],
    bound: &ProfileBinding,
    codex_home: &Path,
) -> io::Result<i32> {
    let wrapper = std::env::current_exe()
        .map_err(|error| io::Error::other(format!("executor wrapper path: {error}")))?;
    let receipt = workspace.join("executor-spawn.json");
    let args = terminal_tab_args(title, workspace, &wrapper, &receipt, terminal_profile)?;
    save_receipt(
        workspace,
        launcher,
        profile,
        mode,
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
        cmd.env("COLORTERM", "truecolor");
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
    mode: SpawnMode,
    args: &[String],
    bound: &ProfileBinding,
    codex_home: &Path,
) -> io::Result<i32> {
    save_receipt(
        workspace,
        launcher,
        profile,
        mode,
        args,
        bound,
        None,
        "owned-console",
        None,
    )?;
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
        launcher,
        profile,
        mode,
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
    args.extend(["-C".into(), native_path(workspace)?, prompt.to_owned()]);
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

fn child_args(
    profile: &str,
    workspace: &Path,
    prompt: &str,
    isolation: &[String],
    mode: SpawnMode,
) -> io::Result<Vec<String>> {
    match mode {
        SpawnMode::Exec => {
            let mut args = profile_args(profile)?;
            args.extend([
                "exec".into(),
                "--skip-git-repo-check".into(),
                "-C".into(),
                native_path(workspace)?,
                prompt.to_owned(),
            ]);
            Ok(args)
        }
        SpawnMode::Tui => tui_args(profile, workspace, prompt, isolation),
    }
}

// The receipt records every dispatch input the watcher and resume path need.
#[allow(clippy::too_many_arguments)]
fn save_receipt(
    workspace: &Path,
    launcher: &Path,
    profile: &str,
    mode: SpawnMode,
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
            "launcher": native_path(launcher)?,
            "profile": profile,
            "mode": mode.as_str(),
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

const SUCCESSION_HELP: &str = "codex-harness executor succeed --request FILE\nReplace one exact session's CLI process through the verified non-interactive `codex exec resume` path at a safe boundary. The request names the session id, profile, workspace, the private session state root (or its recorded pointer), the compact skill revision identity published by skill-evolution, the durable task context and a private evidence directory. The command makes no model calls: it writes the handover record, confirms the predecessor process stopped, spawns the successor, verifies in the session rollout that current instructions and skills were reloaded, and reports 'succession not established' with a non-zero exit when that verification fails.";

fn succeed(args: &[OsString]) -> io::Result<i32> {
    if args.first().is_some_and(|arg| arg == "--help") {
        println!("{SUCCESSION_HELP}");
        return Ok(0);
    }
    if args.len() != 2 || args[0] != "--request" {
        return Err(invalid(
            "usage: codex-harness executor succeed --request FILE",
        ));
    }
    let bytes = read_bounded(Path::new(&args[1]), SUCCESSION_LIMIT)?;
    let request: SuccessionRequest =
        serde_json::from_slice(&bytes).map_err(|_| invalid("succession request is invalid"))?;
    request.validate()?;
    let (code, receipt) = execute_succession(&request);
    write_succession_receipt(&request, &receipt)?;
    if receipt["status"] != "established" {
        eprintln!(
            "codex-harness: {}",
            receipt["message"]
                .as_str()
                .unwrap_or("succession did not complete")
        );
    }
    println!("{}", serde_json::to_string_pretty(&receipt)?);
    Ok(code)
}

fn execute_succession(request: &SuccessionRequest) -> (i32, serde_json::Value) {
    let mut receipt = json!({
        "schema": 1,
        "status": "blocked",
        "message": "",
        "session": request.session,
        "profile": request.profile,
        "revision": request.revision,
        "mechanicsModelCalls": 0,
        "state": serde_json::Value::Null,
        "boundary": serde_json::Value::Null,
        "handover": serde_json::Value::Null,
        "predecessor": serde_json::Value::Null,
        "successor": serde_json::Value::Null,
        "reload": serde_json::Value::Null,
    });
    match attempt_succession(request, &mut receipt) {
        Ok(code) => (code, receipt),
        Err(error) => {
            receipt["status"] = json!("blocked");
            receipt["message"] = json!(error.to_string());
            (2, receipt)
        }
    }
}

fn attempt_succession(
    request: &SuccessionRequest,
    receipt: &mut serde_json::Value,
) -> io::Result<i32> {
    let session = task_succession::exact_session_id(&request.session)?.to_owned();
    let profile = orchestration_config::binding(&request.codex_home, &request.profile)?;
    let launcher = request
        .executable
        .clone()
        .unwrap_or_else(|| request.codex_home.join("harness/bin/codex.exe"));
    if !launcher.is_file() {
        return Err(invalid("installed Codex launcher is missing"));
    }
    let upstream = upstream_executable(&request.codex_home)?;
    let state = match step(
        "resolve the session state root",
        resolve_state_root(request),
    )? {
        Some(state) => state,
        None => {
            return Err(invalid(
                "the session's private state root is unavailable; succession cannot establish a safe boundary or stop the predecessor",
            ));
        }
    };
    receipt["state"] = json!(state);
    let binding = step(
        "read the recorded session binding",
        task_succession::binding_from_leader(&state),
    )?;
    if let Some(binding) = &binding {
        step(
            "verify the recorded session binding",
            task_succession::verify_binding(request, binding, &profile),
        )?;
    }
    let record = step(
        "reconcile the owning task record",
        reconcile_task_record(request, &session),
    )?;
    let instruction_path = request.instruction_path();
    let instruction_text = step(
        "read the current instructions",
        read_text(&instruction_path, INSTRUCTION_READ_LIMIT),
    )?;
    let live_skill = step(
        "read the published skill package",
        skill_evolution::package::load(Path::new(&request.revision.path)).map_err(|error| {
            invalid(&format!(
                "the published skill package is not readable; {}: {error}",
                task_succession::NOT_ESTABLISHED
            ))
        }),
    )?;
    let published_path = Path::new(&request.revision.path)
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(&request.revision.path));
    if live_skill.root != published_path
        || live_skill.name != request.revision.name
        || live_skill.revision != request.revision.revision
    {
        finish_succession(
            receipt,
            "notEstablished",
            format!(
                "{NOT_ESTABLISHED}: the published revision differs from the live skill package"
            ),
        );
        return Ok(1);
    }
    if live_skill.description.trim().is_empty() {
        return Err(invalid(&format!(
            "the published skill description is empty; {NOT_ESTABLISHED}"
        )));
    }
    let deadline = Instant::now()
        .checked_add(Duration::from_secs(request.timeout_seconds))
        .ok_or_else(|| invalid("succession deadline is invalid"))?;
    let (decision, native) = step(
        "wait for a safe boundary",
        wait_for_boundary(request, &state, &upstream, deadline),
    )?;
    let safe = match decision {
        Boundary::Safe(safe) => safe,
        Boundary::Deferred(reason) => {
            receipt["boundary"] =
                json!({"predecessorRunning": serde_json::Value::Null, "native": native});
            finish_succession(
                receipt,
                "deferred",
                format!("succession deferred: {reason}"),
            );
            return Ok(2);
        }
    };
    receipt["boundary"] = json!({
        "predecessorRunning": safe.predecessor_running,
        "native": safe.native,
    });
    let record_path = request.record_path();
    let mut handover = task_succession::handover_record(
        request,
        binding.as_ref(),
        &profile,
        "pending",
        Some(&state),
    );
    if let Some(record) = &record {
        handover["taskRecord"] = json!({
            "id": record.id,
            "authorization": record.authorization,
            "requirements": record.requirements,
            "worktree": record.worktree,
            "stopped": record.stopped,
        });
    }
    step(
        "write the handover record",
        write_json(&record_path, &handover),
    )?;
    let state_record = state.join("succession.json");
    step(
        "write the handover record into the session state",
        write_json(&state_record, &handover),
    )?;
    receipt["handover"] = json!({"record": record_path, "stateRecord": state_record});
    let stop = step(
        "stop the predecessor process",
        stop_predecessor(&state, &upstream, safe.predecessor_running, deadline),
    )?;
    handover["predecessor"]["stop"] = json!(stop.status);
    step(
        "update the handover record",
        write_json(&record_path, &handover),
    )?;
    step(
        "update the session handover record",
        write_json(&state_record, &handover),
    )?;
    receipt["predecessor"] = json!({
        "stop": stop.status,
        "process": stop.process,
        "detail": stop.detail,
    });
    if stop.status == "notConfirmed" {
        finish_succession(
            receipt,
            "notEstablished",
            format!("{NOT_ESTABLISHED}: the predecessor process stop was not confirmed"),
        );
        return Ok(1);
    }
    let plan = step(
        "build the successor invocation",
        task_succession::successor_plan(request, binding.as_ref()),
    )?;
    let cwd = binding
        .as_ref()
        .and_then(|binding| binding.cwd.clone())
        .unwrap_or_else(|| request.workspace.clone());
    let run = step(
        "spawn the successor process",
        spawn_successor(request, &plan, &cwd),
    )?;
    receipt["successor"] = json!({
        "argv": task_succession::argv_text(&plan.program, &plan.args),
        "exitCode": run.exit_code,
        "threadStarted": run.thread_started,
        "stdout": run.stdout,
        "stderr": run.stderr,
    });
    if run.exit_code != 0 {
        finish_succession(
            receipt,
            "notEstablished",
            format!(
                "{NOT_ESTABLISHED}: the successor process exited with code {}",
                run.exit_code
            ),
        );
        return Ok(1);
    }
    if run.thread_started.as_deref() != Some(session.as_str()) {
        finish_succession(
            receipt,
            "notEstablished",
            format!("{NOT_ESTABLISHED}: the successor did not resume the exact session"),
        );
        return Ok(1);
    }
    let marker = task_succession::marker_for(&session);
    let Some(rollout) = step(
        "locate the successor rollout",
        task_succession::find_rollout(&request.codex_home, &session, &marker),
    )?
    else {
        finish_succession(
            receipt,
            "notEstablished",
            format!(
                "{NOT_ESTABLISHED}: no successor continuation turn evidence exists in the session rollout"
            ),
        );
        return Ok(1);
    };
    let rollout_text = step(
        "read the successor rollout",
        task_succession::read_bounded_tail(&rollout),
    )?;
    let verified = task_succession::verify_reload(
        &rollout_text,
        &ReloadExpectation {
            instruction_path: &instruction_path,
            instruction_text: &instruction_text,
            skill_name: &live_skill.name,
            skill_description: &live_skill.description,
            session: &session,
        },
    );
    receipt["reload"] = json!({
        "instructionPath": instruction_path,
        "skill": live_skill.name,
        "skillPath": request.revision.path,
        "revision": request.revision.revision,
        "rollout": rollout,
        "rolloutSha256": task_succession::hash_text(&rollout_text),
    });
    match verified {
        Reload::Verified => {
            finish_succession(
                receipt,
                "established",
                "succession established: the successor reloaded the current instructions and the published skill revision",
            );
            Ok(0)
        }
        Reload::NotVerified(reason) => {
            finish_succession(
                receipt,
                "notEstablished",
                format!("{NOT_ESTABLISHED}: {reason}"),
            );
            Ok(1)
        }
    }
}

fn finish_succession(receipt: &mut serde_json::Value, status: &str, message: impl Into<String>) {
    receipt["status"] = json!(status);
    receipt["message"] = json!(message.into());
}

fn step<T>(name: &str, result: io::Result<T>) -> io::Result<T> {
    result.map_err(|error| io::Error::new(error.kind(), format!("{name}: {error}")))
}

const NOT_ESTABLISHED: &str = task_succession::NOT_ESTABLISHED;

fn resolve_state_root(request: &SuccessionRequest) -> io::Result<Option<PathBuf>> {
    if let Some(state) = &request.state {
        return Ok(state.is_dir().then(|| state.clone()));
    }
    task_succession::read_session_pointer(&request.codex_home, request.session.trim())
}

fn reconcile_task_record(
    request: &SuccessionRequest,
    session: &str,
) -> io::Result<Option<harness_core::task_store::TaskRecord>> {
    let Some(task_id) = request
        .task
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };
    let record = harness_core::task_store::load(&request.codex_home, task_id)?;
    harness_core::task_orchestrate::resume(&record)?;
    if let Some(worktree) = &record.worktree {
        let recorded = worktree.canonicalize().unwrap_or_else(|_| worktree.clone());
        let requested = request
            .workspace
            .canonicalize()
            .unwrap_or_else(|_| request.workspace.clone());
        if recorded != requested {
            return Err(invalid(
                "requested workspace differs from the recorded assignment worktree",
            ));
        }
    }
    if let Some(assignment_id) = request
        .assignment
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let assignment = record
            .assignments
            .iter()
            .find(|assignment| assignment.id == assignment_id)
            .ok_or_else(|| invalid("recorded assignment is missing from the owning task"))?;
        if assignment.owner_thread.as_deref() != Some(session) {
            return Err(invalid(
                "recorded assignment owner differs from the session; refusing conflicting writers",
            ));
        }
        if !assignment.profile.is_empty() && assignment.profile != request.profile {
            return Err(invalid(
                "recorded assignment profile differs from the request; refusing substitution",
            ));
        }
    }
    Ok(Some(record))
}

fn wait_for_boundary(
    request: &SuccessionRequest,
    state: &Path,
    upstream: &Path,
    deadline: Instant,
) -> io::Result<(Boundary, Option<NativeFacts>)> {
    loop {
        let mut facts = task_succession::read_session_facts(state)?;
        facts.predecessor_running = observe_predecessor(state, &facts, upstream)?;
        let native = if facts.predecessor_running == Some(true) {
            observe_native(state, request.session.trim())?
        } else {
            None
        };
        let decision = task_succession::boundary(&facts, native.as_ref());
        if matches!(decision, Boundary::Safe(_)) || Instant::now() >= deadline {
            return Ok((decision, native));
        }
        thread::sleep(BOUNDARY_POLL);
    }
}

fn observe_predecessor(
    state: &Path,
    facts: &SessionFacts,
    upstream: &Path,
) -> io::Result<Option<bool>> {
    if facts.closed {
        return Ok(Some(false));
    }
    let value: serde_json::Value = match read_json(&state.join("view.json"))? {
        Some(value) => value,
        None => return Ok(None),
    };
    let Some(identity) = process_identity(&value["window"]["process"]) else {
        return Ok(None);
    };
    let user = harness_core::process_service::current_user()?;
    match ServiceProcess::inspect(identity, upstream, &user) {
        Ok(Some(process)) => Ok(Some(process.is_running()?)),
        Ok(None) => Ok(Some(false)),
        Err(_) => Ok(None),
    }
}

fn observe_native(state: &Path, session: &str) -> io::Result<Option<NativeFacts>> {
    let Some(endpoint) = read_json::<serde_json::Value>(&state.join("endpoint.json"))? else {
        return Ok(None);
    };
    let (Some(port), Some(token)) = (endpoint["port"].as_u64(), endpoint["token"].as_str()) else {
        return Ok(None);
    };
    let Ok(mut connection) = ControlConnection::connect(port as u16, token, Duration::from_secs(5))
    else {
        return Ok(None);
    };
    if native_call(
        &mut connection,
        1,
        "initialize",
        json!({"clientInfo":{"name":"harness-succession","version":"1"},"capabilities":{"experimentalApi":true}}),
    )
    .is_err()
    {
        return Ok(None);
    }
    if connection
        .send(&json!({"method":"initialized"}), Duration::from_secs(5))
        .is_err()
    {
        return Ok(None);
    }
    let terminals = match native_call(
        &mut connection,
        2,
        "thread/backgroundTerminals/list",
        json!({"threadId":session,"limit":8}),
    ) {
        Ok(value) => value,
        Err(_) => return Ok(None),
    };
    let read = match native_call(
        &mut connection,
        3,
        "thread/read",
        json!({"threadId":session,"includeTurns":true}),
    ) {
        Ok(value) => value,
        Err(_) => return Ok(None),
    };
    let terminals_empty = terminals["data"]
        .as_array()
        .is_some_and(|data| data.is_empty())
        && terminals["nextCursor"].is_null();
    let latest_turn_settled = read["thread"]["turns"]
        .as_array()
        .and_then(|turns| turns.last())
        .is_none_or(|turn| turn["status"] != "inProgress");
    Ok(Some(NativeFacts {
        terminals_empty,
        latest_turn_settled,
    }))
}

fn native_call(
    connection: &mut ControlConnection,
    id: u64,
    method: &str,
    params: serde_json::Value,
) -> io::Result<serde_json::Value> {
    connection.send(
        &json!({"id":id,"method":method,"params":params}),
        Duration::from_secs(5),
    )?;
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        if Instant::now() >= until {
            return Err(io::Error::other("native succession probe deadline"));
        }
        if let Some(value) = connection.receive(Duration::from_millis(200))? {
            if value.get("method").is_some() || value["id"] != json!(id) {
                continue;
            }
            if value.get("error").is_some() {
                return Err(io::Error::other("native succession probe rejected"));
            }
            return Ok(value["result"].clone());
        }
    }
}

struct StopOutcome {
    status: &'static str,
    process: serde_json::Value,
    detail: String,
}

fn stop_predecessor(
    state: &Path,
    upstream: &Path,
    predecessor_running: bool,
    deadline: Instant,
) -> io::Result<StopOutcome> {
    let user = harness_core::process_service::current_user()?;
    let view = read_json::<serde_json::Value>(&state.join("view.json"))?
        .and_then(|value| process_identity(&value["window"]["process"]));
    let runtime = read_json::<serde_json::Value>(&state.join("runtime.json"))?;
    let service = runtime
        .as_ref()
        .and_then(|value| process_identity(&value["process"]));
    let service_executable = runtime
        .as_ref()
        .and_then(|value| value["executable"].as_str().map(PathBuf::from))
        .unwrap_or_else(|| upstream.to_path_buf());
    let process = view.map_or(
        serde_json::Value::Null,
        |identity| json!({"pid": identity.pid, "creationTime": identity.creation_time}),
    );
    if !predecessor_running {
        return Ok(StopOutcome {
            status: "alreadyStopped",
            process,
            detail: "the predecessor had already stopped at a settled boundary".into(),
        });
    }
    // The explicit stop suspends admission and stops the controller; the
    // frontend then exits on its own when its server closes. Killing the
    // exact recorded process stays a bounded fallback, never a first move.
    harness_core::task_runtime::request_stop(state)?;
    let until = deadline.min(Instant::now() + STOP_GRACE);
    let mut attempts = 0u32;
    loop {
        attempts += 1;
        let closed = state.join("closed.json").is_file();
        let kill = attempts >= 3;
        let view_state = match view {
            Some(identity) => inspect_recorded(identity, upstream, &user, kill)?,
            None => Recorded::Gone,
        };
        let service_state = match service {
            Some(identity) => inspect_recorded(identity, &service_executable, &user, false)?,
            None => Recorded::Gone,
        };
        let settled =
            !matches!(view_state, Recorded::Running) && !matches!(service_state, Recorded::Running);
        if (closed && !matches!(view_state, Recorded::Running)) || settled {
            let closure = read_json::<serde_json::Value>(&state.join("closed.json"))?
                .and_then(|value| value["reason"].as_str().map(str::to_owned));
            return Ok(StopOutcome {
                status: "confirmed",
                process,
                detail: format!(
                    "the predecessor process stopped; controller closure: {}",
                    closure.as_deref().unwrap_or("recorded")
                ),
            });
        }
        if Instant::now() >= until {
            return Ok(StopOutcome {
                status: "notConfirmed",
                process,
                detail: format!(
                    "the predecessor did not stop before the deadline (closure recorded: {closed})"
                ),
            });
        }
        thread::sleep(Duration::from_millis(100));
    }
}

enum Recorded {
    Running,
    Gone,
    Unknown,
}

/// Observes one recorded process. Access denied means the process is
/// terminating or the handle was lost in an exit race; it never authorizes a
/// different target and only defers confirmation to the next attempt.
fn inspect_recorded(
    identity: harness_core::process::ProcessIdentity,
    executable: &Path,
    user: &str,
    kill: bool,
) -> io::Result<Recorded> {
    match ServiceProcess::inspect(identity, executable, user) {
        Ok(Some(process)) => {
            if !process.is_running()? {
                return Ok(Recorded::Gone);
            }
            if kill {
                match process.terminate(SUCCESSION_EXIT_CODE) {
                    Ok(_) | Err(_) => (),
                }
                match process.wait_for_exit(harness_core::process::Deadline::after(
                    Duration::from_secs(5),
                )?) {
                    Ok(true) => return Ok(Recorded::Gone),
                    Ok(false) => return Ok(Recorded::Running),
                    Err(_) => return Ok(Recorded::Unknown),
                }
            }
            Ok(Recorded::Running)
        }
        Ok(None) => Ok(Recorded::Gone),
        Err(error) if error.raw_os_error() == Some(5) => Ok(Recorded::Unknown),
        Err(error) => Err(error),
    }
}

struct SuccessorRun {
    exit_code: u32,
    thread_started: Option<String>,
    stdout: PathBuf,
    stderr: PathBuf,
}

fn spawn_successor(
    request: &SuccessionRequest,
    plan: &SuccessorPlan,
    cwd: &Path,
) -> io::Result<SuccessorRun> {
    fs::create_dir_all(&request.evidence)?;
    let stdout_path = request.evidence.join("successor-stdout.jsonl");
    let stderr_path = request.evidence.join("successor-stderr.txt");
    let mut spec = CommandSpec::new(&plan.program);
    spec.args = task_succession::os_argv(plan);
    spec.current_dir = Some(cwd.to_path_buf());
    spec.stdout = Some(fs::File::create(&stdout_path)?);
    spec.stderr = Some(fs::File::create(&stderr_path)?);
    apply_successor_env(&mut spec, &request.codex_home);
    let job = Job::new(Limits {
        memory_bytes: Some(2048 * 1024 * 1024),
        cpu_percent: None,
    })?;
    let process = job.spawn(&spec)?;
    let outcome = job.wait(
        &process,
        harness_core::process::Deadline::after(Duration::from_secs(request.timeout_seconds))?,
        &harness_core::process::Cancellation::default(),
        Duration::from_secs(5),
    )?;
    let exit_code = match outcome.reason {
        StopReason::Exited => outcome.exit_code,
        _ => u32::MAX,
    };
    let stdout = fs::read_to_string(&stdout_path).unwrap_or_default();
    Ok(SuccessorRun {
        exit_code,
        thread_started: parse_thread_started(&stdout),
        stdout: stdout_path,
        stderr: stderr_path,
    })
}

fn apply_successor_env(spec: &mut CommandSpec, codex_home: &Path) {
    spec.env
        .insert("CODEX_HOME".into(), Some(codex_home.as_os_str().to_owned()));
    if let Some(path) = filtered_path() {
        spec.env.insert("PATH".into(), Some(path));
    }
    for name in INHERITED_SESSION_ENV {
        spec.env.insert(name.into(), None);
    }
}

fn parse_thread_started(stdout: &str) -> Option<String> {
    stdout.lines().find_map(|line| {
        let value: serde_json::Value = serde_json::from_str(line).ok()?;
        (value["type"] == "thread.started").then(|| {
            value["thread_id"]
                .as_str()
                .map(str::to_owned)
                .filter(|id| !id.is_empty())
        })?
    })
}

fn upstream_executable(codex_home: &Path) -> io::Result<PathBuf> {
    let registration =
        read_json::<serde_json::Value>(&codex_home.join("harness/native-launch.json"))?
            .ok_or_else(|| invalid("native launch registration is missing"))?;
    let upstream = registration["upstream"]["executable"]
        .as_str()
        .map(PathBuf::from)
        .ok_or_else(|| invalid("native launch registration has no upstream executable"))?;
    Ok(upstream)
}

fn process_identity(value: &serde_json::Value) -> Option<harness_core::process::ProcessIdentity> {
    let pid = value["pid"].as_u64()? as u32;
    let creation_time = value["creation_time"]
        .as_u64()
        .or_else(|| value["creationTime"].as_u64())?;
    (pid != 0 && creation_time != 0)
        .then_some(harness_core::process::ProcessIdentity { pid, creation_time })
}

fn write_succession_receipt(
    request: &SuccessionRequest,
    receipt: &serde_json::Value,
) -> io::Result<()> {
    fs::create_dir_all(&request.evidence)?;
    let bytes = serde_json::to_vec_pretty(receipt)?;
    fs::write(request.evidence.join("succession-receipt.json"), &bytes)?;
    // Every attempt keeps its own record; the stable name above is a pointer.
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_millis())
        .unwrap_or(0);
    let stamp = chrono::DateTime::from_timestamp_millis(millis as i64)
        .map(|value| value.format("%Y%m%dT%H%M%S%.3fZ").to_string())
        .unwrap_or_else(|| millis.to_string());
    let session = receipt["session"].as_str().unwrap_or("session");
    fs::write(
        request
            .evidence
            .join(format!("succession-receipt-{session}-{stamp}.json")),
        &bytes,
    )?;
    if let Some(state) = receipt["state"].as_str() {
        let state = Path::new(state);
        if state.is_dir() {
            let _ = fs::write(state.join("succession-result.json"), &bytes);
        }
    }
    Ok(())
}

fn write_json(path: &Path, value: &serde_json::Value) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec_pretty(value)?;
    let temporary = path.with_extension("json.tmp");
    {
        let mut file = fs::File::create(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
    }
    fs::rename(&temporary, path)
}

fn read_bounded(path: &Path, limit: u64) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|_| invalid("succession input is unreadable"))?
        .take(limit + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(invalid("succession input exceeds its bound"));
    }
    Ok(bytes)
}

fn read_text(path: &Path, limit: u64) -> io::Result<String> {
    let bytes = read_bounded(path, limit)?;
    String::from_utf8(bytes).map_err(|_| invalid("instruction source is not UTF-8"))
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> io::Result<Option<T>> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{}: {error}", path.display()),
            )
        }),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
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
            Path::new(r"C:\wt\xai\executor-spawn.json"),
            None,
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
        assert_eq!(args[wrapper + 1], "executor");
        assert_eq!(args[wrapper + 2], "run");
        assert_eq!(args[wrapper + 3], "--file");
        assert_eq!(args[wrapper + 4], r"C:\wt\xai\executor-spawn.json");
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
        assert!(SpawnMode::parse(None).is_ok());
        assert_eq!(SpawnMode::parse(Some("tui")).unwrap(), SpawnMode::Tui);
        let error = SpawnMode::parse(Some("headless")).unwrap_err();
        assert!(error.to_string().contains("unknown executor mode"));
    }

    #[test]
    fn exec_mode_streams_the_assignment_without_a_fake_goal_prefix() {
        let args = child_args(
            "ds",
            Path::new(r"D:\wt\ds"),
            "Complete the outcome in ASSIGNMENT.md.",
            &[],
            SpawnMode::Exec,
        )
        .unwrap();
        assert_eq!(args[0], "--profile");
        assert_eq!(args[1], "ds");
        assert_eq!(args[2], "exec");
        assert!(args.contains(&"--skip-git-repo-check".to_string()));
        assert!(args.contains(&r"D:\wt\ds".to_string()));
        assert_eq!(
            args.last().unwrap(),
            "Complete the outcome in ASSIGNMENT.md."
        );
        assert!(!args.iter().any(|arg| arg.starts_with("/goal")));
    }

    #[test]
    fn executor_run_requires_an_absolute_launcher() {
        let error = run_exec(&[OsString::from(r"codex.exe")]).unwrap_err();
        assert!(error.to_string().contains("absolute"));
        let error = run_exec(&[]).unwrap_err();
        assert!(error.to_string().contains("launcher path"));
    }

    #[test]
    fn forward_slash_launcher_paths_are_normalized() {
        assert_eq!(
            normalize_launcher(std::ffi::OsStr::new(
                r"C:/Users/noilw/.codex/harness/bin/codex.exe"
            ))
            .unwrap(),
            r"C:\Users\noilw\.codex\harness\bin\codex.exe"
        );
    }

    #[test]
    fn terminal_tab_paths_use_native_separators() {
        let args = terminal_tab_args(
            "t",
            Path::new(r"D:/wt/xai"),
            Path::new(r"D:/harness/codex-harness.exe"),
            Path::new(r"C:/wt/xai/executor-spawn.json"),
            Some("PowerShell"),
        )
        .unwrap();
        let joined = args.join(" ");
        assert!(joined.contains(r"-d D:\wt\xai"));
        assert!(joined.contains(
            r"D:\harness\codex-harness.exe executor run --file C:\wt\xai\executor-spawn.json",
        ));
        assert!(!joined.contains('/'));
        let profile = args
            .iter()
            .position(|arg| arg == "--profile")
            .expect("terminal profile option");
        assert_eq!(args[profile + 1], "PowerShell");
    }

    #[test]
    fn run_receipt_launches_the_recorded_command() {
        let root = std::env::temp_dir().join(format!("executor-receipt-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let receipt = root.join("executor-spawn.json");
        fs::write(
            &receipt,
            serde_json::to_vec_pretty(&json!({
                "schema": 1,
                "launcher": r"C:/Windows/System32/cmd.exe",
                "args": ["/c"],
            }))
            .unwrap(),
        )
        .unwrap();
        let code = run_exec(&[
            OsString::from("--file"),
            OsString::from(receipt.as_os_str()),
        ])
        .unwrap();
        assert_eq!(code, 0);
        let _ = fs::remove_dir_all(root);
    }
}
