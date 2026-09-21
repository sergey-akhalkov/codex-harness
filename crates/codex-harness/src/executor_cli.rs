//! Dispatch a configured executor through native `codex --profile` into a
//! harness-owned pool slot, release that slot for reuse, and replace one exact
//! session's CLI process under refreshed instructions (instruction-refresh
//! succession, OFAP 4.1).
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
use harness_core::task_worktree::{self, AcquiredSlot, LaneDisposition, SlotDisposition};
use serde::{Deserialize, Serialize};
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

const USAGE: &str = "codex-harness executor spawn --source CHECKOUT --codex-home DIRECTORY [--workspace DIRECTORY] [--profile ID] [--mode exec|tui] [--base REV] [--owner ID] [--terminal-profile NAME] [--terminal-window NAME] --exec PROMPT\ncodex-harness executor release --source CHECKOUT --codex-home DIRECTORY --slot N --disposition merged|discarded --reason TEXT [--base REV]\ncodex-harness executor pool --source CHECKOUT --codex-home DIRECTORY\ncodex-harness executor steer --state DIRECTORY --thread ID --text TEXT [--worktree DIRECTORY] [--out FILE]\ncodex-harness executor run LAUNCHER [ARG...]\ncodex-harness executor succeed --request PATH\nSpawn selects, synchronizes and binds one slot of the harness-owned worktree pool of --source (sibling directories named <repository>-wt1..N, sized to max_concurrent_executors) before the first model request, then opens a tab in the lead's own Windows Terminal window when WT_SESSION is set: the terminal cannot address that window by id, so dispatch briefly holds it foreground, resolves the tab there through the most-recently-used rule, and restores the user's foreground window and selected tab afterwards. When that window is unavailable (another virtual desktop or a blocked activation) the tab goes to the stable per-checkout window codex-harness-<repository>, which the terminal creates on first use instead of using the user's focused window; --terminal-window targets an explicitly named window. Without WT_SESSION spawn opens a visible console. --workspace is optional and no longer the isolation mechanism: it must be the source checkout or one of its pool slots, and ad-hoc worktree paths are refused. --base overrides the synchronized base (the upstream default branch by default); --owner labels the session binding (default exec-<profile>-<pid>) and reusing it keeps the same slot across an interruption. Release records the lead's merged or discarded disposition with its reason, resets the slot with ignored build caches kept, and preserves it with its limitation when it cannot be safely reset. Pool reports the recorded slot mapping (index, path, state, owner, base), the tree and lease state, and the foreign or legacy worktrees that only the lead retires; worktree_limit is superseded by the pool size. The default exec mode streams the assignment visibly and exits on completion, so the tab closes itself; continue or correct the exact session later with codex exec resume SESSION_ID. The tui mode keeps an interactive conversation. Assignments live on the beads board; executors set lead_review when done. Steer delivers a visible turn/start through the named session task-control endpoint with no status polling; without an endpoint it refuses instead of pretending to deliver, and the remedy names codex exec resume. Succeed replaces one exact session's CLI process through the verified non-interactive `codex exec resume` path at a safe boundary: it writes a durable handover record, stops the predecessor, resumes the exact session under refreshed instructions and reports 'succession not established' when the reload cannot be verified.";
const STARTUP: Duration = Duration::from_secs(20);
/// Windows Terminal activates the receiving window asynchronously around the
/// launcher exit; this bounds how long dispatch keeps undoing that activation.
const TERMINAL_TAB_SETTLE: Duration = Duration::from_millis(1500);
/// The lead window must stay foreground until the terminal resolves the tab's
/// destination; its titled selection is the observable completion signal.
const TERMINAL_TAB_TITLE_TIMEOUT: Duration = Duration::from_millis(2500);
/// Bounded wait for the lead window to actually reach the foreground before a
/// targeted dispatch falls back to the named per-checkout window.
const ACTIVATION_WAIT: Duration = Duration::from_millis(400);
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
        Some("release") => release(&args[1..]),
        Some("pool") => pool_status(&args[1..]),
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
    let mut terminal_window = None;
    let mut mode = None;
    let mut prompt = None;
    let mut base = None;
    let mut owner = None;
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
            "--base" => base = Some(option_text(value)?),
            "--owner" => owner = Some(option_text(value)?),
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
            "--terminal-window" => {
                terminal_window = Some(
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
    let prompt = prompt.ok_or_else(|| invalid("--exec is required"))?;
    if !source.is_absolute()
        || !codex_home.is_absolute()
        || workspace.as_ref().is_some_and(|path| !path.is_absolute())
    {
        return Err(invalid("executor spawn paths must be absolute"));
    }
    let config = load(&source)?;
    let profile = executor_profile(&config, profile.as_deref())?.to_owned();
    let mode = SpawnMode::parse(mode.as_deref())?;
    let pool_size = config.max_concurrent_executors;
    let named_slot = named_slot(&source, pool_size, workspace.as_deref())?;
    let owner = owner
        .filter(|owner| !owner.trim().is_empty())
        .unwrap_or_else(|| format!("exec-{profile}-{}", std::process::id()));
    dispatch(&Dispatch {
        codex_home: &codex_home,
        source: &source,
        pool_size,
        named_slot,
        owner: &owner,
        base: base.as_deref(),
        profile: &profile,
        prompt: &prompt,
        mode,
        terminal_profile: terminal_profile.as_deref(),
        terminal_window: terminal_window.as_deref(),
    })
}

/// `--workspace` is no longer the isolation mechanism: it may name the source
/// checkout, whose pool then selects the slot, or one of that checkout's pool
/// slots. Ad-hoc task-named worktree paths are refused with a migration hint
/// instead of recreating unbounded lanes; the pool always picks the free slot.
fn named_slot(source: &Path, pool_size: u32, workspace: Option<&Path>) -> io::Result<Option<u32>> {
    let Some(workspace) = workspace else {
        return Ok(None);
    };
    let checkout = source
        .canonicalize()
        .unwrap_or_else(|_| source.to_path_buf());
    let named = workspace
        .canonicalize()
        .unwrap_or_else(|_| workspace.to_path_buf());
    if checkout == named {
        return Ok(None);
    }
    match task_worktree::slot_index(source, workspace)? {
        Some(index) if index <= pool_size => Ok(Some(index)),
        Some(index) => Err(invalid(&format!(
            "executor isolation comes from the harness pool: {} is slot {index}, outside the configured pool of {pool_size} (max_concurrent_executors); dispatch with --source {} instead of the extra lane",
            workspace.display(),
            source.display()
        ))),
        None => Err(invalid(&format!(
            "executor isolation comes from the harness pool: --workspace must be the source checkout {} or one of its slots (<repository>-wt1..{pool_size}); drop the ad-hoc worktree path {} and let the dispatch select and synchronize the slot",
            source.display(),
            workspace.display()
        ))),
    }
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

/// One pooled dispatch. The slot is selected, synchronized and bound before any
/// launcher process starts, so the session never runs from a stale base.
struct Dispatch<'a> {
    codex_home: &'a Path,
    source: &'a Path,
    pool_size: u32,
    named_slot: Option<u32>,
    owner: &'a str,
    base: Option<&'a str>,
    profile: &'a str,
    prompt: &'a str,
    mode: SpawnMode,
    terminal_profile: Option<&'a str>,
    terminal_window: Option<&'a str>,
}

/// Recorded session binding of one pool slot: the mapping the lead reloads
/// after an interruption through the kit-local task state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SlotBinding {
    index: u32,
    path: PathBuf,
    source: PathBuf,
    owner: String,
    base: String,
    remote: String,
    branch: Option<String>,
}

fn dispatch(request: &Dispatch) -> io::Result<i32> {
    let bound = orchestration_config::binding(request.codex_home, request.profile)?;
    let slot = acquire_pool_slot(request)?;
    let binding = SlotBinding {
        index: slot.index,
        path: slot.path.clone(),
        source: request.source.to_path_buf(),
        owner: request.owner.to_owned(),
        base: slot.base.clone(),
        remote: slot.remote.clone(),
        branch: slot.branch.clone(),
    };
    println!("{}", slot_summary(&binding, request.named_slot));
    report_inventory(request)?;
    ensure_workspace_trust(request.codex_home, &binding.path)?;
    let receipt = receipt_path(request.codex_home, request.source, binding.index)?;
    let launcher = request.codex_home.join("harness/bin/codex.exe");
    if !launcher.is_file() {
        return Err(invalid(&format!(
            "installed Codex launcher is missing: {} does not exist; slot {} stays bound to {} and is reused by the next dispatch",
            launcher.display(),
            binding.index,
            binding.owner
        )));
    }
    let args = child_args(request.profile, &binding.path, request.prompt, request.mode)?;
    let title = format!("Codex executor ({})", request.profile);
    let session = std::env::var_os("WT_SESSION");
    let client = windows_terminal_client();
    if prefers_terminal_tab(session.as_deref(), client.as_deref()) {
        dispatch_terminal_tab(
            client.as_ref().expect("terminal client"),
            &launcher,
            request,
            &binding,
            &receipt,
            &title,
            &args,
            &bound,
        )
    } else {
        dispatch_owned_console(&launcher, request, &binding, &receipt, &bound, &args)
    }
}

/// The observable outcome of slot allocation: the lead reads the mapping here
/// and reloads the same fields from `executor pool` or the kit-local record.
fn slot_summary(binding: &SlotBinding, named: Option<u32>) -> String {
    let mut line = format!(
        "executor slot: index={} path={} base={} owner={} remote={}{} source={}",
        binding.index,
        binding.path.display(),
        binding.base,
        binding.owner,
        binding.remote,
        binding
            .branch
            .as_deref()
            .map(|branch| format!("/{branch}"))
            .unwrap_or_default(),
        binding.source.display()
    );
    if let Some(named) = named.filter(|named| *named != binding.index) {
        line.push_str(&format!(
            " (--workspace named slot {named}; the pool bound the free slot {})",
            binding.index
        ));
    }
    line
}

/// Report the inventory the pool invariant covers: dispatch allocates only
/// inside the configured pool and never absorbs or deletes foreign or legacy
/// trees, so the lead reviews them.
fn report_inventory(request: &Dispatch) -> io::Result<()> {
    let audit = task_worktree::audit_pool(request.source, request.pool_size)?;
    if !audit.foreign.is_empty() {
        println!(
            "pool inventory (lead review): foreign worktrees: {}",
            display_paths(&audit.foreign)
        );
    }
    if !audit.beyond_pool.is_empty() {
        println!(
            "pool inventory (lead review): worktrees beyond the configured pool of {}: {}",
            request.pool_size,
            display_paths(&audit.beyond_pool)
        );
    }
    Ok(())
}

fn display_paths(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join("; ")
}

/// The pool core owns selection, claims, synchronization and slot records; the
/// CLI supplies the session identity and the process-level liveness of its host.
fn acquire_pool_slot(request: &Dispatch) -> io::Result<AcquiredSlot> {
    let live = |owner: &str| owner_live(request.codex_home, request.source, owner);
    refuse_a_live_owner(request.codex_home, request.source, request.owner)?;
    task_worktree::acquire_slot(
        request.codex_home,
        request.source,
        request.pool_size,
        request.owner,
        request.base,
        &live,
    )
}

/// A dispatch never shares its slot with a live session that already claims
/// that identity: an interrupted session ends before its slot can be reclaimed,
/// while a running one must be stopped or given another identity.
fn refuse_a_live_owner(codex_home: &Path, source: &Path, owner: &str) -> io::Result<()> {
    match live_lease(codex_home, source, owner) {
        Some(lease) => Err(invalid(&format!(
            "session {owner} is already live in slot {} ({}); stop it or dispatch with another --owner instead of sharing one checkout",
            lease.index,
            lease.path.display()
        ))),
        None => Ok(()),
    }
}

/// Process identity of the session host that holds a slot. The lease is what
/// reconciles slot occupancy with executor session liveness: a claim whose host
/// process is gone is no longer an occupied slot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionLease {
    schema: u32,
    owner: String,
    index: u32,
    path: PathBuf,
    pid: u32,
    /// Windows FILETIME of the host process creation; a reused pid differs.
    created: u64,
    program: PathBuf,
}

fn lease_path(codex_home: &Path, source: &Path, index: u32) -> io::Result<PathBuf> {
    Ok(task_worktree::pool_state_dir(codex_home, source)?.join(format!("lease-{index}.json")))
}

/// The dispatch receipt is kit-local session state: writing it into the slot
/// would make the harness's own bookkeeping look like unreviewed executor work
/// and would travel with the executor's next commit.
fn receipt_path(codex_home: &Path, source: &Path, index: u32) -> io::Result<PathBuf> {
    Ok(task_worktree::pool_state_dir(codex_home, source)?.join(format!("spawn-{index}.json")))
}

/// Record this process as the live host of a bound slot. A slot that was
/// rebound to another session is refused instead of sharing the tree.
fn record_lease(codex_home: &Path, binding: &SlotBinding) -> io::Result<()> {
    let record = task_worktree::load_slot_record(codex_home, &binding.source, binding.index)?
        .ok_or_else(|| {
            invalid(&format!(
                "slot {} has no recorded session binding",
                binding.index
            ))
        })?;
    if record.owner.as_deref() != Some(binding.owner.as_str()) {
        return Err(invalid(&format!(
            "slot {} is bound to session {} instead of {}; dispatch again instead of sharing one checkout",
            binding.index,
            record.owner.as_deref().unwrap_or("no session"),
            binding.owner
        )));
    }
    let program = std::env::current_exe()
        .map_err(|error| invalid(&format!("executor session host path: {error}")))?;
    let user = harness_core::process_service::current_user()?;
    let identity = ServiceProcess::observe(std::process::id(), &program, 0, &user)?.identity();
    let lease = SessionLease {
        schema: 1,
        owner: binding.owner.clone(),
        index: binding.index,
        path: binding.path.clone(),
        pid: identity.pid,
        created: identity.creation_time,
        program,
    };
    let path = lease_path(codex_home, &binding.source, binding.index)?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(&path, serde_json::to_vec_pretty(&lease)?)
}

/// Drop the lease this process recorded and leave another session's lease alone.
fn remove_lease(codex_home: &Path, binding: &SlotBinding) -> io::Result<()> {
    let path = lease_path(codex_home, &binding.source, binding.index)?;
    match read_lease(&path) {
        Some(lease) if lease.owner == binding.owner && lease.pid == std::process::id() => {
            match fs::remove_file(&path) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(error),
            }
        }
        _ => Ok(()),
    }
}

fn read_lease(path: &Path) -> Option<SessionLease> {
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}

/// A recorded host is live while its exact process identity is running; an
/// exited host, a missing image or a reused pid is stale. Unverifiable
/// identity preserves the slot instead of reclaiming it.
fn lease_live(lease: &SessionLease) -> bool {
    if !lease.program.is_file() {
        return false;
    }
    let Ok(user) = harness_core::process_service::current_user() else {
        return true;
    };
    match ServiceProcess::inspect(
        harness_core::process::ProcessIdentity {
            pid: lease.pid,
            creation_time: lease.created,
        },
        &lease.program,
        &user,
    ) {
        Ok(Some(_)) => true,
        Ok(None) => false,
        Err(_) => true,
    }
}

/// The live lease of one owner, if a host process holds it: the pool core asks
/// liveness per owner, so every lease in the kit-local state directory counts.
fn live_lease(codex_home: &Path, source: &Path, owner: &str) -> Option<SessionLease> {
    let dir = task_worktree::pool_state_dir(codex_home, source).ok()?;
    let entries = fs::read_dir(&dir).ok()?;
    entries
        .flatten()
        .filter_map(|entry| read_lease(&entry.path()))
        .find(|lease| lease.owner == owner && lease_live(lease))
}

/// Is the recorded owner's host process still running?
fn owner_live(codex_home: &Path, source: &Path, owner: &str) -> bool {
    live_lease(codex_home, source, owner).is_some()
}

/// Explicit slot release for the lead: record the merged or discarded
/// disposition with its reason, then reset the slot for reuse under the
/// existing reset-for-reuse rules or preserve it with its limitation.
fn release(args: &[OsString]) -> io::Result<i32> {
    let mut source = None;
    let mut codex_home = None;
    let mut slot = None;
    let mut disposition = None;
    let mut reason = None;
    let mut base = None;
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
            "--slot" => slot = Some(option_text(value)?),
            "--disposition" => disposition = Some(option_text(value)?),
            "--reason" => reason = Some(option_text(value)?),
            "--base" => base = Some(option_text(value)?),
            _ => return Err(invalid("invalid native executor options")),
        }
    }
    let source = required(source, "--source")?;
    let codex_home = required(codex_home, "--codex-home")?;
    let index: u32 = slot
        .ok_or_else(|| invalid("--slot is required"))?
        .parse()
        .map_err(|_| invalid("--slot must be a positive pool slot index"))?;
    let disposition = match disposition.as_deref() {
        Some("merged") => SlotDisposition::Merged,
        Some("discarded") => SlotDisposition::Discarded,
        Some(other) => {
            return Err(invalid(&format!(
                "unknown release disposition {other}; use merged or discarded"
            )));
        }
        None => return Err(invalid("--disposition merged|discarded is required")),
    };
    let reason = reason
        .filter(|reason| !reason.trim().is_empty())
        .ok_or_else(|| invalid("--reason TEXT is required"))?;
    let config = load(&source)?;
    let layout = task_worktree::pool(&source, config.max_concurrent_executors)?;
    layout.slot(index)?;
    let base = match base {
        Some(base) => base,
        None => committed_head(&source)?,
    };
    let live = |owner: &str| owner_live(&codex_home, &source, owner);
    match task_worktree::release_slot(
        &codex_home,
        &layout,
        index,
        disposition,
        &reason,
        &base,
        &live,
    )? {
        LaneDisposition::Reused { base } => {
            println!(
                "executor slot {index} released as {}: reset to {base} and free for the next dispatch",
                disposition_name(disposition)
            );
            Ok(0)
        }
        LaneDisposition::Preserved { limitation } => {
            println!(
                "executor slot {index} release recorded as {} but the slot is preserved: {limitation}",
                disposition_name(disposition)
            );
            Ok(2)
        }
    }
}

fn disposition_name(disposition: SlotDisposition) -> &'static str {
    match disposition {
        SlotDisposition::Merged => "merged",
        SlotDisposition::Discarded => "discarded",
    }
}

/// The merged committed base of the source checkout, used when the lead does
/// not name the base explicitly.
fn committed_head(source: &Path) -> io::Result<String> {
    let out = Command::new("git")
        .args(["rev-parse", "--verify", "HEAD^{commit}"])
        .current_dir(source)
        .output()
        .map_err(|error| invalid(&format!("release base: {error}")))?;
    if !out.status.success() {
        return Err(invalid(&format!(
            "release base: {} is not a Git checkout with a committed HEAD; pass --base REV",
            source.display()
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

/// The lead's read path for the recorded slot mapping: index, path, state,
/// owner, base, disposition and reason, beside the tree and lease state and the
/// foreign or legacy worktrees that only the lead retires.
fn pool_status(args: &[OsString]) -> io::Result<i32> {
    let mut source = None;
    let mut codex_home = None;
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
            _ => return Err(invalid("invalid native executor options")),
        }
    }
    let source = required(source, "--source")?;
    let codex_home = required(codex_home, "--codex-home")?;
    let size = load(&source)?.max_concurrent_executors;
    println!(
        "executor worktree pool: source={} size={size} (max_concurrent_executors)",
        source.display()
    );
    match task_worktree::pool(&source, size) {
        Ok(layout) => {
            for slot in &layout.slots {
                println!("{}", slot_report(&codex_home, &source, slot));
            }
        }
        Err(error) => println!("pool layout unavailable: {error}"),
    }
    let audit = task_worktree::audit_pool(&source, size)?;
    for path in &audit.foreign {
        println!("foreign worktree (lead review): {}", path.display());
    }
    for path in &audit.beyond_pool {
        println!(
            "worktree beyond the configured pool (lead review): {}",
            path.display()
        );
    }
    Ok(0)
}

fn slot_report(codex_home: &Path, source: &Path, slot: &task_worktree::PoolSlot) -> String {
    let presence = match slot.presence {
        task_worktree::SlotPresence::Absent => "absent",
        task_worktree::SlotPresence::Registered => "registered",
        task_worktree::SlotPresence::RegisteredMissing => "registered-missing",
    };
    let (state, owner, base, disposition, reason): (String, String, String, String, String) =
        match task_worktree::load_slot_record(codex_home, source, slot.index) {
            Ok(Some(record)) => (
                slot_state_name(record.state).to_owned(),
                record.owner.unwrap_or_else(|| "-".into()),
                record.base.unwrap_or_else(|| "-".into()),
                record
                    .disposition
                    .map(|disposition| disposition_name(disposition).to_owned())
                    .unwrap_or_else(|| "-".into()),
                record.reason.unwrap_or_else(|| "-".into()),
            ),
            Ok(None) => (
                "free".into(),
                "-".into(),
                "-".into(),
                "-".into(),
                "-".into(),
            ),
            Err(error) => (
                "unreadable".into(),
                "-".into(),
                "-".into(),
                "-".into(),
                error.to_string(),
            ),
        };
    let lease = match lease_path(codex_home, source, slot.index) {
        Ok(path) => read_lease(&path),
        Err(_) => None,
    };
    let lease_state = match &lease {
        Some(lease) if lease_live(lease) => "live",
        Some(_) => "stale",
        None => "none",
    };
    format!(
        "slot {} {} presence={presence} state={state} tree={} lease={lease_state} owner={owner} base={base} disposition={disposition} reason={reason}",
        slot.index,
        slot.path.display(),
        tree_state(&slot.path)
    )
}

fn slot_state_name(state: task_worktree::SlotState) -> &'static str {
    match state {
        task_worktree::SlotState::Free => "free",
        task_worktree::SlotState::Synchronizing => "synchronizing",
        task_worktree::SlotState::Occupied => "occupied",
        task_worktree::SlotState::AwaitingReview => "awaiting-review",
        task_worktree::SlotState::Released => "released",
    }
}

fn tree_state(path: &Path) -> &'static str {
    if !path.is_dir() {
        return "missing";
    }
    match Command::new("git")
        .args(["status", "--porcelain=v1", "--untracked-files=all"])
        .current_dir(path)
        .output()
    {
        Ok(out) if !out.status.success() => "unknown",
        Ok(out) if String::from_utf8_lossy(&out.stdout).trim().is_empty() => "clean",
        Ok(_) => "dirty",
        Err(_) => "unknown",
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
    // The tab host is the process that stays alive for the whole session, so
    // it records the slot's liveness for exactly as long as the session runs.
    let binding = receipt_binding(&value)?;
    let codex_home = match &binding {
        Some(binding) => Some(receipt_codex_home(binding)?),
        None => None,
    };
    if let (Some(binding), Some(codex_home)) = (&binding, &codex_home) {
        record_lease(codex_home, binding)?;
    }
    let outcome = run_child(&launcher, &rest);
    if let (Some(binding), Some(codex_home)) = (&binding, &codex_home) {
        let _ = remove_lease(codex_home, binding);
    }
    outcome
}

/// The recorded pool slot binding of a receipt; receipts written before the
/// pool existed carry none and run without a lease.
fn receipt_binding(value: &serde_json::Value) -> io::Result<Option<SlotBinding>> {
    let slot = &value["slot"];
    if slot.is_null() {
        return Ok(None);
    }
    let binding: SlotBinding = serde_json::from_value(slot.clone())
        .map_err(|error| invalid(&format!("executor run receipt slot binding: {error}")))?;
    Ok(Some(binding))
}

fn receipt_codex_home(binding: &SlotBinding) -> io::Result<PathBuf> {
    let home = std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or_else(|| {
            invalid(&format!(
                "executor run receipt: CODEX_HOME is required to bind slot {} to session {}",
                binding.index, binding.owner
            ))
        })?;
    Ok(home)
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

/// Windows Terminal exposes no supported address for the window of the calling
/// process: `wt -w 0` resolves to the most recently used window on the current
/// desktop (where the user works, not the calling lead), and numeric window ids
/// are internal to the terminal. A stable window name bound to the source
/// checkout is the supported precise target, and the terminal creates that
/// window on first use, so an executor tab can never land in the user's
/// focused window of another project.
fn terminal_window_name(source: &Path, requested: Option<&str>) -> io::Result<String> {
    let fallback = source
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("workspace");
    let mut slug = String::with_capacity(fallback.len() + 16);
    for ch in fallback.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
            slug.push(ch);
        } else {
            slug.push('-');
        }
    }
    let slug = slug.trim_matches('-');
    let derived = if slug.is_empty() { "workspace" } else { slug };
    match requested {
        Some(name) => validate_terminal_window_name(name),
        None => validate_terminal_window_name(&format!("codex-harness-{derived}")),
    }
}

/// The lead's own terminal window, when it can be safely held foreground for
/// targeted dispatch. A window on another virtual desktop is skipped: forcing
/// it foreground would switch the user's desktop.
fn lead_terminal_target() -> Option<usize> {
    let window = task_view::console_terminal_window()?;
    task_view::window_on_current_virtual_desktop(window).then_some(window)
}

fn activate_lead_window(window: usize) -> bool {
    if !task_view::activate_window(window) {
        return false;
    }
    let deadline = Instant::now() + ACTIVATION_WAIT;
    while Instant::now() < deadline {
        if task_view::foreground_window() == Some(window) {
            return true;
        }
        thread::sleep(Duration::from_millis(10));
    }
    task_view::foreground_window() == Some(window)
}

fn validate_terminal_window_name(name: &str) -> io::Result<String> {
    if name.trim().is_empty() || name.len() > 128 {
        return Err(invalid(
            "terminal window name must be 1-128 characters of non-whitespace text",
        ));
    }
    if name.chars().any(|ch| ch == ';' || ch.is_control()) {
        return Err(invalid(
            "terminal window name must not contain ';' or control characters",
        ));
    }
    Ok(name.to_owned())
}

fn terminal_tab_args(
    window: &str,
    title: &str,
    workspace: &Path,
    wrapper: &Path,
    receipt: &Path,
    terminal_profile: Option<&str>,
) -> io::Result<Vec<String>> {
    let mut args = vec![
        "-w".into(),
        window.to_owned(),
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
    request: &Dispatch,
    binding: &SlotBinding,
    receipt: &Path,
    title: &str,
    tui: &[String],
    bound: &ProfileBinding,
) -> io::Result<i32> {
    let workspace = binding.path.as_path();
    let wrapper = std::env::current_exe()
        .map_err(|error| io::Error::other(format!("executor wrapper path: {error}")))?;
    let named_window = terminal_window_name(request.source, request.terminal_window)?;
    // Windows Terminal cannot address the lead's window by name or id, so hold
    // that window foreground briefly: `-w 0` then resolves to it as the most
    // recently used window of the current desktop. When that is impossible
    // (explicit name requested, unknown window, other virtual desktop, or a
    // failed activation), the stable per-checkout window name keeps the tab out
    // of the user's focused window of another project.
    let lead_window = if request.terminal_window.is_some() {
        None
    } else {
        lead_terminal_target()
    };
    let previous = task_view::foreground_window();
    let mut target_lead_window = lead_window;
    if let Some(window) = lead_window.filter(|window| previous != Some(*window))
        && !activate_lead_window(window)
    {
        target_lead_window = None;
    }
    let target = match target_lead_window {
        Some(_) => "0",
        None => named_window.as_str(),
    };
    let args = terminal_tab_args(
        target,
        title,
        workspace,
        &wrapper,
        receipt,
        request.terminal_profile,
    )?;
    save_receipt(
        receipt,
        launcher,
        request.profile,
        request.mode,
        tui,
        bound,
        None,
        "windows-terminal-tab",
        Some(&args),
        Some(binding),
    )?;
    suppress_loader_dialogs();
    let mut cmd = Command::new(wt);
    cmd.args(&args)
        .current_dir(workspace)
        .env("CODEX_HOME", request.codex_home)
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
    let status = match target_lead_window {
        Some(window) => task_view::run_terminal_tab_in_window(
            &mut cmd,
            window,
            title,
            TERMINAL_TAB_TITLE_TIMEOUT,
        )?,
        None => task_view::run_restoring_foreground(
            &mut cmd,
            TERMINAL_TAB_SETTLE,
            previous.unwrap_or(0),
        )?,
    };
    task_view::restore_foreground_to(previous.unwrap_or(0));
    if !status.success() {
        return Err(invalid("windows terminal tab spawn failed"));
    }
    println!(
        "{}",
        spawn_summary(
            request.profile,
            bound,
            title,
            receipt,
            "windows-terminal-tab",
        )
    );
    Ok(0)
}

fn dispatch_owned_console(
    launcher: &Path,
    request: &Dispatch,
    binding: &SlotBinding,
    receipt: &Path,
    bound: &ProfileBinding,
    args: &[String],
) -> io::Result<i32> {
    let workspace = binding.path.as_path();
    let profile = request.profile;
    // This process hosts the session for as long as the view runs, so it is
    // the recorded liveness of the slot.
    record_lease(request.codex_home, binding)?;
    let outcome = (|| -> io::Result<i32> {
        save_receipt(
            receipt,
            launcher,
            profile,
            request.mode,
            args,
            bound,
            None,
            "owned-console",
            None,
            Some(binding),
        )?;
        println!(
            "{}",
            spawn_summary(
                profile,
                bound,
                &format!("Codex executor ({profile})"),
                receipt,
                "owned-console",
            )
        );
        let view = task_view::preserve_foreground(|| {
            let mut spec = CommandSpec::new(launcher);
            spec.args = args.iter().map(OsString::from).collect();
            spec.current_dir = Some(workspace.to_path_buf());
            spec.new_console = Some(format!("Opening Codex executor ({profile})").into());
            apply_executor_env(&mut spec, request.codex_home);
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
            receipt,
            launcher,
            profile,
            request.mode,
            args,
            bound,
            Some(&snapshot),
            "owned-console",
            None,
            Some(binding),
        )?;
        while view.is_running()? {
            thread::sleep(Duration::from_millis(200));
        }
        Ok(view.exit_code()?.unwrap_or(1) as i32)
    })();
    let _ = remove_lease(request.codex_home, binding);
    outcome
}

fn spawn_summary(
    profile: &str,
    bound: &ProfileBinding,
    title: &str,
    receipt: &Path,
    host: &str,
) -> String {
    let model = bound.model.as_deref().unwrap_or("unknown");
    let provider = bound.model_provider.as_deref().unwrap_or("unknown");
    let effort = bound.reasoning_effort.as_deref().unwrap_or("default");
    format!(
        "executor started: profile={profile} model={model} provider={provider} effort={effort} host={host} title=\"{title}\"\nreceipt: {}",
        receipt.display()
    )
}

fn tui_args(profile: &str, workspace: &Path, prompt: &str) -> io::Result<Vec<String>> {
    let mut args = profile_args(profile)?;
    args.extend(["-C".into(), native_path(workspace)?, prompt.to_owned()]);
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
        // The pooled slot is the isolation: no native --worktree flag is
        // passed on this path.
        SpawnMode::Tui => tui_args(profile, workspace, prompt),
    }
}

// The receipt records every dispatch input the watcher, the tab host and the
// resume path need; it is kit-local state beside the slot records.
#[allow(clippy::too_many_arguments)]
fn save_receipt(
    receipt: &Path,
    launcher: &Path,
    profile: &str,
    mode: SpawnMode,
    args: &[String],
    bound: &ProfileBinding,
    window: Option<&task_view::Snapshot>,
    host: &str,
    terminal: Option<&[String]>,
    slot: Option<&SlotBinding>,
) -> io::Result<()> {
    let window = match window {
        Some(snapshot) => serde_json::to_value(snapshot)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?,
        None => json!(null),
    };
    let slot = match slot {
        Some(binding) => serde_json::to_value(binding)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?,
        None => json!(null),
    };
    if let Some(dir) = receipt.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(
        receipt,
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
            "slot": slot,
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

fn option_text(value: &std::ffi::OsStr) -> io::Result<String> {
    value
        .to_str()
        .map(str::to_owned)
        .ok_or_else(|| invalid("invalid native executor options"))
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
    let mut state = None;
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
            "--state" => state = Some(PathBuf::from(value)),
            _ => return Err(invalid("invalid native executor options")),
        }
    }
    let thread = thread.ok_or_else(|| invalid("--thread is required"))?;
    let worktree = worktree.ok_or_else(|| invalid("--worktree is required"))?;
    let text = text.ok_or_else(|| invalid("--text is required"))?;
    let Some(state) = state else {
        eprintln!(
            "codex-harness: executor steer needs --state DIRECTORY with the session's task-control endpoint.json; a plain pooled exec session has no control channel, so wait for it to finish and continue it with `codex exec resume SESSION_ID`"
        );
        return Ok(2);
    };
    let Ok(Some(endpoint)) = read_json::<serde_json::Value>(&state.join("endpoint.json")) else {
        eprintln!(
            "codex-harness: no task-control endpoint at {}; steering was not delivered. Start the session under task control, or wait and continue it with `codex exec resume SESSION_ID`",
            state.display()
        );
        return Ok(2);
    };
    let (Some(port), Some(token)) = (endpoint["port"].as_u64(), endpoint["token"].as_str()) else {
        return Err(invalid("task-control endpoint is malformed"));
    };
    let mut connection = ControlConnection::connect(port as u16, token, Duration::from_secs(5))?;
    native_call(
        &mut connection,
        1,
        "initialize",
        json!({"clientInfo":{"name":"harness-steer","version":"1"},"capabilities":{"experimentalApi":true}}),
    )?;
    connection.send(&json!({"method":"initialized"}), Duration::from_secs(5))?;
    let params = json!({"threadId":thread,"input":[{"type":"text","text":text}]});
    let response = native_call(&mut connection, 2, "turn/start", params.clone())?;
    let payload = json!({
        "schema": 1,
        "delivered": true,
        "method": "turn/start",
        "hiddenModelCall": false,
        "statusPoll": false,
        "worktree": worktree,
        "params": params,
        "response": response,
    });
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
        let args = tui_args("xai", Path::new(r"D:\wt\xai"), "do the work").unwrap();
        assert_eq!(args[0], "--profile");
        assert_eq!(args[1], "xai");
        assert!(args.contains(&"-C".to_string()));
        assert!(args.contains(&r"D:\wt\xai".to_string()));
        assert!(!args.iter().any(|arg| arg == "exec" || arg == "--json"));
        assert!(!args.iter().any(|arg| arg == "--remote"));
        assert_eq!(args.last().unwrap(), "do the work");
    }

    #[test]
    fn pooled_dispatch_keeps_native_worktree_isolation_out_of_the_arguments() {
        let slot = Path::new(r"D:\wt\proj-wt1");
        for args in [
            child_args("xai", slot, "do the work", SpawnMode::Exec).unwrap(),
            child_args("xai", slot, "do the work", SpawnMode::Tui).unwrap(),
        ] {
            assert!(
                !args
                    .iter()
                    .any(|arg| arg == "--worktree" || arg == "--enable" || arg == "worktrees"),
                "{args:?}"
            );
        }
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
    fn terminal_tab_args_target_a_named_window_without_focus_flags() {
        let args = terminal_tab_args(
            "codex-harness-proj",
            "Codex executor (xai)",
            Path::new(r"D:\wt\xai"),
            Path::new(r"C:\harness\codex-harness.exe"),
            Path::new(r"C:\wt\xai\executor-spawn.json"),
            None,
        )
        .unwrap();
        assert_eq!(args[0], "-w");
        assert_eq!(args[1], "codex-harness-proj");
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
    fn terminal_window_name_is_bound_to_the_source_checkout() {
        let derived = terminal_window_name(Path::new(r"D:\home\Proj Studio!"), None).unwrap();
        assert_eq!(derived, "codex-harness-Proj-Studio");
        let requested =
            terminal_window_name(Path::new(r"D:\home\Proj Studio!"), Some("lead-window")).unwrap();
        assert_eq!(requested, "lead-window");
        assert_eq!(
            terminal_window_name(Path::new(r"D:\home\###"), None).unwrap(),
            "codex-harness-workspace"
        );
    }

    #[test]
    fn terminal_window_name_rejects_terminal_metacharacters() {
        let error = terminal_window_name(Path::new(r"D:\home\proj"), Some("left;right"))
            .unwrap_err()
            .to_string();
        assert!(error.contains("';'"), "{error}");
        assert!(terminal_window_name(Path::new(r"D:\home\proj"), Some("  ")).is_err());
        assert!(terminal_window_name(Path::new(r"D:\home\proj"), Some(&"x".repeat(129))).is_err());
    }

    #[test]
    #[ignore = "opens and closes a real Windows Terminal window; requires an interactive desktop and no user window switching during the run"]
    fn terminal_tab_dispatch_targets_its_named_window_and_restores_foreground() {
        let Some(client) = windows_terminal_client() else {
            eprintln!("skipped: Windows Terminal is not installed");
            return;
        };
        let name = format!("codex-harness-probe-{}", std::process::id());
        let before = task_view::terminal_windows().len();
        let previous = task_view::foreground_window();
        let dispatch = |tab_seconds: u64| {
            let mut cmd = Command::new(&client);
            cmd.args([
                "-w",
                &name,
                "new-tab",
                "--title",
                "harness window targeting probe",
                "--suppressApplicationTitle",
                "pwsh",
                "-NoLogo",
                "-NoProfile",
                "-Command",
                "Start-Sleep",
                "-Seconds",
                &tab_seconds.to_string(),
            ]);
            let status = task_view::run_restoring_foreground(
                &mut cmd,
                TERMINAL_TAB_SETTLE,
                previous.unwrap_or(0),
            )
            .expect("terminal dispatch");
            assert!(status.success(), "terminal dispatch failed: {status}");
            task_view::restore_foreground_to(previous.unwrap_or(0));
        };

        dispatch(12);
        let opened = Instant::now();
        while task_view::terminal_windows().len() <= before
            && opened.elapsed() < Duration::from_secs(3)
        {
            thread::sleep(Duration::from_millis(50));
        }
        let first = task_view::terminal_windows().len();
        assert!(first > before, "probe window was not created");
        if let Some(previous) = previous {
            assert_eq!(
                task_view::foreground_window(),
                Some(previous),
                "the user's foreground window must survive the dispatch"
            );
        }

        dispatch(12);
        thread::sleep(Duration::from_millis(800));
        assert_eq!(
            task_view::terminal_windows().len(),
            first,
            "the second tab must reuse the named window, not create or take another one"
        );
        if let Some(previous) = previous {
            assert_eq!(
                task_view::foreground_window(),
                Some(previous),
                "the user's foreground window must survive the second dispatch"
            );
        }

        let closed = Instant::now();
        while task_view::terminal_windows().len() > before
            && closed.elapsed() < Duration::from_secs(20)
        {
            thread::sleep(Duration::from_millis(100));
        }
        assert_eq!(
            task_view::terminal_windows().len(),
            before,
            "the probe window must close itself after its tabs exit"
        );
    }

    #[test]
    #[ignore = "opens a short-lived tab in this session's real Windows Terminal window; requires an interactive desktop and no user window switching during the run"]
    fn terminal_tab_dispatch_targets_the_leads_own_window_and_restores_foreground() {
        let Some(client) = windows_terminal_client() else {
            eprintln!("skipped: Windows Terminal is not installed");
            return;
        };
        if let Some(window) = task_view::console_terminal_window() {
            eprintln!(
                "lead window {window:x} on current desktop: {}",
                task_view::window_on_current_virtual_desktop(window)
            );
        } else {
            eprintln!("lead window: console is not parented to a terminal window");
        }
        if let Some(foreground) = task_view::foreground_window() {
            eprintln!(
                "foreground {foreground:x} on current desktop: {}",
                task_view::window_on_current_virtual_desktop(foreground)
            );
        }
        let Some(lead) = lead_terminal_target() else {
            eprintln!(
                "skipped: this session's console is not attached to a Windows Terminal window on the current desktop"
            );
            return;
        };
        let before = task_view::terminal_windows().len();
        let previous = task_view::foreground_window();
        // A targeted dispatch must be able to activate the lead window while
        // another terminal window holds the foreground; exercise that
        // mechanism in both directions when a second terminal window exists.
        if let Some(other) = task_view::terminal_windows()
            .into_iter()
            .find(|window| Some(*window) != Some(lead))
        {
            assert!(
                task_view::activate_window(other),
                "cross-window activation into another terminal window must work"
            );
            assert!(
                activate_lead_window(lead),
                "cross-window activation back into the lead window must work"
            );
            task_view::restore_foreground_to(previous.unwrap_or(0));
        }
        if previous != Some(lead) {
            assert!(
                activate_lead_window(lead),
                "the lead window must become foreground for targeted dispatch"
            );
        }
        let title = format!("harness lead-window probe {}", std::process::id());
        let mut cmd = Command::new(&client);
        cmd.args([
            "-w",
            "0",
            "new-tab",
            "--title",
            &title,
            "--suppressApplicationTitle",
            "pwsh",
            "-NoLogo",
            "-NoProfile",
            "-Command",
            "Start-Sleep",
            "-Seconds",
            "6",
        ]);
        let status = task_view::run_terminal_tab_in_window(
            &mut cmd,
            lead,
            &title,
            TERMINAL_TAB_TITLE_TIMEOUT,
        )
        .expect("terminal dispatch");
        assert!(status.success(), "terminal dispatch failed: {status}");
        assert!(
            task_view::window_title(lead).contains(&title),
            "the tab must open in the lead's own window, not another window"
        );
        task_view::restore_foreground_to(previous.unwrap_or(0));
        assert_eq!(
            task_view::terminal_windows().len(),
            before,
            "a targeted dispatch must not create a new terminal window"
        );
        if let Some(previous) = previous {
            assert_eq!(
                task_view::foreground_window(),
                Some(previous),
                "the user's foreground window must be restored after the tab opens"
            );
        }
        let closed = Instant::now();
        while task_view::window_title(lead).contains(&title)
            && closed.elapsed() < Duration::from_secs(12)
        {
            thread::sleep(Duration::from_millis(100));
        }
        assert!(
            !task_view::window_title(lead).contains(&title),
            "the probe tab must close itself after its command exits"
        );
    }

    #[test]
    fn steer_refuses_without_an_endpoint_instead_of_pretending() {
        let root = tempfile::tempdir().unwrap();
        let code = run(&[
            OsString::from("steer"),
            OsString::from("--thread"),
            OsString::from("exec-xai"),
            OsString::from("--worktree"),
            OsString::from(root.path().join("wt").as_os_str()),
            OsString::from("--text"),
            OsString::from("use the fixture"),
            OsString::from("--state"),
            OsString::from(root.path().join("missing-state").as_os_str()),
        ])
        .unwrap();
        // A missing endpoint must refuse loudly: the old stub printed a
        // turn/start payload and exited 0 while nothing was delivered.
        assert_eq!(code, 2);
        let with_state_only = run(&[
            OsString::from("steer"),
            OsString::from("--thread"),
            OsString::from("exec-xai"),
            OsString::from("--worktree"),
            OsString::from(root.path().join("wt").as_os_str()),
            OsString::from("--text"),
            OsString::from("use the fixture"),
        ])
        .unwrap();
        assert_eq!(with_state_only, 2);
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
            Path::new(r"C:\home\harness\executor-pool\proj-0123456789ab\spawn-1.json"),
            "windows-terminal-tab",
        );
        assert!(summary.contains("profile=ds"));
        assert!(summary.contains("model=deepseek-flash"));
        assert!(summary.contains("provider=deepseek"));
        assert!(summary.contains("effort=max"));
        assert!(summary.contains("host=windows-terminal-tab"));
        assert!(summary.contains("title=\"Codex executor (ds)\""));
        assert!(summary.contains(r"C:\home\harness\executor-pool\proj-0123456789ab\spawn-1.json"));
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
                r"C:/Users/dev/.codex/harness/bin/codex.exe"
            ))
            .unwrap(),
            r"C:\Users\dev\.codex\harness\bin\codex.exe"
        );
    }

    #[test]
    fn terminal_tab_paths_use_native_separators() {
        let args = terminal_tab_args(
            "codex-harness-xai",
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

    #[test]
    fn workspace_is_restricted_to_the_source_checkout_or_a_pool_slot() {
        let root = std::env::temp_dir().join(format!("executor-workspace-{}", std::process::id()));
        let source = root.join("proj");
        fs::create_dir_all(source.join("global")).unwrap();
        fs::create_dir_all(root.join("proj-wt1")).unwrap();
        fs::create_dir_all(root.join("proj-wt3")).unwrap();
        let lane = root.join("task-legacy-lane");
        fs::create_dir_all(&lane).unwrap();
        assert_eq!(named_slot(&source, 2, None).unwrap(), None);
        assert_eq!(
            named_slot(&source, 2, Some(&source)).unwrap(),
            None,
            "the source checkout means the pool"
        );
        assert_eq!(
            named_slot(&source, 2, Some(&root.join("proj-wt1"))).unwrap(),
            Some(1)
        );
        let error = named_slot(&source, 2, Some(&lane)).unwrap_err().to_string();
        assert!(
            error.contains("executor isolation comes from the harness pool")
                && error.contains("--workspace must be the source checkout")
                && error.contains("task-legacy-lane"),
            "{error}"
        );
        let error = named_slot(&source, 2, Some(&root.join("proj-wt3")))
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("outside the configured pool of 2"),
            "{error}"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn slot_summary_reports_the_mapping_and_the_selected_slot() {
        let binding = SlotBinding {
            index: 1,
            path: PathBuf::from(r"D:\lanes\proj-wt1"),
            source: PathBuf::from(r"D:\lanes\proj"),
            owner: "exec-ds-7".into(),
            base: "abc123".into(),
            remote: "origin".into(),
            branch: Some("main".into()),
        };
        let line = slot_summary(&binding, None);
        for expected in [
            "index=1",
            r"D:\lanes\proj-wt1",
            "base=abc123",
            "owner=exec-ds-7",
            "remote=origin/main",
            r"source=D:\lanes\proj",
        ] {
            assert!(line.contains(expected), "{line}");
        }
        assert!(!line.contains("--workspace named slot"), "{line}");
        let named = slot_summary(&binding, Some(2));
        assert!(
            named.contains("--workspace named slot 2; the pool bound the free slot 1"),
            "{named}"
        );
    }

    #[test]
    fn lease_marks_the_host_live_and_a_rebound_slot_is_refused() {
        let root = std::env::temp_dir().join(format!("executor-lease-{}", std::process::id()));
        let source = root.join("proj");
        let home = root.join("home");
        fs::create_dir_all(&source).unwrap();
        let binding = SlotBinding {
            index: 1,
            path: source.parent().unwrap().join("proj-wt1"),
            source: source.clone(),
            owner: "exec-ds-7".into(),
            base: "abc123".into(),
            remote: "origin".into(),
            branch: Some("main".into()),
        };
        assert!(
            record_lease(&home, &binding).is_err(),
            "a slot without a recorded binding cannot host a session"
        );
        write_test_slot_record(&home, &source, &binding);
        record_lease(&home, &binding).unwrap();
        assert!(
            owner_live(&home, &source, "exec-ds-7"),
            "this test process is the recorded live host"
        );
        assert!(!owner_live(&home, &source, "exec-other"));
        let mut rebind = binding.clone();
        rebind.owner = "exec-other".into();
        let error = record_lease(&home, &rebind).unwrap_err().to_string();
        assert!(error.contains("bound to session exec-ds-7"), "{error}");
        // A live identity is never dispatched again, so two sessions cannot
        // share one checkout; after the host exits the slot is reclaimable.
        let error = refuse_a_live_owner(&home, &source, "exec-ds-7")
            .unwrap_err()
            .to_string();
        assert!(error.contains("is already live in slot 1"), "{error}");
        refuse_a_live_owner(&home, &source, "exec-other").unwrap();
        assert!(remove_lease(&home, &binding).is_ok());
        assert!(!owner_live(&home, &source, "exec-ds-7"));
        refuse_a_live_owner(&home, &source, "exec-ds-7").unwrap();
        // A host that cannot own the slot has no live owner either.
        let path = lease_path(&home, &source, 1).unwrap();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            serde_json::to_vec_pretty(&SessionLease {
                schema: 1,
                owner: "exec-ds-7".into(),
                index: 1,
                path: binding.path.clone(),
                pid: 999_999_999,
                created: 0,
                program: std::env::current_exe().unwrap(),
            })
            .unwrap(),
        )
        .unwrap();
        assert!(!owner_live(&home, &source, "exec-ds-7"));
        let _ = fs::remove_dir_all(root);
    }

    fn write_test_slot_record(home: &Path, source: &Path, binding: &SlotBinding) {
        let path = task_worktree::slot_record_path(home, source, binding.index).unwrap();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            serde_json::to_vec_pretty(&json!({
                "schema": 1,
                "source": binding.source,
                "index": binding.index,
                "path": binding.path,
                "state": "occupied",
                "owner": binding.owner,
                "base": binding.base,
                "disposition": null,
                "reason": null,
            }))
            .unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn pool_report_lists_the_recorded_mapping_and_tree_state() {
        let root = std::env::temp_dir().join(format!("executor-report-{}", std::process::id()));
        let source = root.join("proj");
        let home = root.join("home");
        let slot_path = root.join("proj-wt1");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&slot_path).unwrap();
        let binding = SlotBinding {
            index: 1,
            path: slot_path.clone(),
            source: source.clone(),
            owner: "exec-ds-9".into(),
            base: "abc123".into(),
            remote: "origin".into(),
            branch: Some("main".into()),
        };
        write_test_slot_record(&home, &source, &binding);
        let report = slot_report(
            &home,
            &source,
            &task_worktree::PoolSlot {
                index: 1,
                path: slot_path.clone(),
                presence: task_worktree::SlotPresence::Registered,
            },
        );
        for expected in [
            "presence=registered",
            "state=occupied",
            "owner=exec-ds-9",
            "base=abc123",
            "lease=none",
        ] {
            assert!(report.contains(expected), "{report}");
        }
        assert!(report.contains("proj-wt1"), "{report}");
        // A slot position without a record reads as free, and a missing tree
        // is reported instead of guessed.
        let free = slot_report(
            &home,
            &source,
            &task_worktree::PoolSlot {
                index: 2,
                path: root.join("proj-wt2"),
                presence: task_worktree::SlotPresence::Absent,
            },
        );
        assert!(
            free.contains("state=free") && free.contains("tree=missing"),
            "{free}"
        );
        assert_eq!(tree_state(&source), "unknown");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn session_state_stays_out_of_the_slot_checkout() {
        let root = std::env::temp_dir().join(format!("executor-state-{}", std::process::id()));
        let home = root.join("home");
        let source = root.join("proj");
        fs::create_dir_all(&source).unwrap();
        let receipt = receipt_path(&home, &source, 2).unwrap();
        assert!(receipt.starts_with(&home), "{}", receipt.display());
        assert!(receipt.ends_with("spawn-2.json"), "{}", receipt.display());
        let lease = lease_path(&home, &source, 2).unwrap();
        assert!(lease.starts_with(&home), "{}", lease.display());
        assert!(lease.ends_with("lease-2.json"), "{}", lease.display());
        assert!(
            receipt.to_string_lossy().contains("harness/executor-pool")
                || receipt.to_string_lossy().contains(r"harness\executor-pool"),
            "{}",
            receipt.display()
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn receipt_binding_is_optional_for_older_receipts() {
        assert!(receipt_binding(&json!({"schema": 1})).unwrap().is_none());
        let binding = receipt_binding(&json!({
            "slot": {
                "index": 2,
                "path": r"D:\lanes\proj-wt2",
                "source": r"D:\lanes\proj",
                "owner": "exec-ds-7",
                "base": "abc123",
                "remote": "origin",
                "branch": "main",
            }
        }))
        .unwrap()
        .expect("recorded slot binding");
        assert_eq!(binding.index, 2);
        assert_eq!(binding.owner, "exec-ds-7");
        assert!(receipt_binding(&json!({"slot": {"index": "two"}})).is_err());
    }
}
