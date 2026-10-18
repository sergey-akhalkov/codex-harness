//! Generic ordinary-task lifecycle over the shared worktree pool: acquire,
//! continue, list and release one durable task binding through the common
//! `harness_core::task_worktree` owner, without starting a conversation.
//!
//! `--source` names the kit/configuration checkout whose
//! `global/orchestration.toml` supplies `worktree_pool_size`; it defaults to
//! the installed kit source recorded under `--codex-home`. `--repository`
//! names the independent target repository and defaults to `--source`, so an
//! existing caller keeps its previous target. A consuming repository needs no
//! kit configuration of its own: it only needs to be a Git checkout.
#![cfg(windows)]

use harness_core::orchestration_config::{self, worktree_pool_size};
use harness_core::task_worktree::{
    self, LaneDisposition, RepositoryIdentity, SlotDisposition, SlotPresence, TaskSlot, TaskState,
};
use std::{
    ffi::OsString,
    io,
    path::{Path, PathBuf},
};

/// Documented default capacity used only when no orchestration configuration
/// is available at all; an existing kit configuration keeps its own
/// `worktree_pool_size` (or its legacy executor-derived value).
const DEFAULT_POOL_CAPACITY: u32 = 4;

const USAGE: &str = concat!(
    "codex-harness worktree acquire --task ID [--repository REPO] [--source CHECKOUT] [--codex-home DIRECTORY] [--base REV]\n",
    "codex-harness worktree continue --task ID [--handle HANDLE] [--repository REPO] [--source CHECKOUT] [--codex-home DIRECTORY]\n",
    "codex-harness worktree list [--repository REPO] [--source CHECKOUT] [--codex-home DIRECTORY]\n",
    "codex-harness worktree release --task ID --disposition merged|discarded --reason TEXT [--handle HANDLE] [--base REV] [--repository REPO] [--source CHECKOUT] [--codex-home DIRECTORY]\n",
    "  One repository's shared harness worktree pool for ordinary tasks. --source names the kit/configuration checkout (default: the installed kit source recorded under --codex-home) and supplies worktree_pool_size; --repository names the independent target repository (default: --source), which needs only to be a Git checkout. Acquire resolves the exact local base without fetching and prints the slot receipt with its path, base, task identity and generation handle; repeating an acquisition for a held task returns the same binding, and a different requested base refuses as a conflict. Continue returns the held binding without synchronization. Release requires the current binding, records merged|discarded with its reason and resets the slot to the committed base for reuse, preserving ignored caches; a slot that cannot be safely reset stays preserved (exit 2). List reports every pool position with its task, handle, owner, base and preservation reason.\n",
);

pub fn run(args: &[OsString]) -> io::Result<i32> {
    if args.first().is_some_and(|arg| arg == "--help") {
        println!("{USAGE}");
        return Ok(0);
    }
    match args.first().and_then(|arg| arg.to_str()) {
        Some("acquire") => acquire(&args[1..]),
        Some("continue") => continue_task(&args[1..]),
        Some("list") => list(&args[1..]),
        Some("release") => release(&args[1..]),
        _ => Err(invalid(
            "invalid native worktree options; use acquire, continue, list or release",
        )),
    }
}

#[derive(Default)]
struct Options {
    source: Option<PathBuf>,
    repository: Option<PathBuf>,
    codex_home: Option<PathBuf>,
    task: Option<String>,
    base: Option<String>,
    handle: Option<String>,
    disposition: Option<String>,
    reason: Option<String>,
}

impl Options {
    fn parse(args: &[OsString]) -> io::Result<Self> {
        let mut options = Options::default();
        let mut iter = args.iter();
        while let Some(arg) = iter.next() {
            let key = arg
                .to_str()
                .ok_or_else(|| invalid("invalid native worktree options"))?;
            let value = iter
                .next()
                .ok_or_else(|| invalid("invalid native worktree options"))?;
            match key {
                "--source" => options.source = Some(PathBuf::from(value)),
                "--repository" => options.repository = Some(PathBuf::from(value)),
                "--codex-home" => options.codex_home = Some(PathBuf::from(value)),
                "--task" => options.task = Some(text(value)?),
                "--base" => options.base = Some(text(value)?),
                "--handle" => options.handle = Some(text(value)?),
                "--disposition" => options.disposition = Some(text(value)?),
                "--reason" => options.reason = Some(text(value)?),
                _ => return Err(invalid("invalid native worktree options")),
            }
        }
        Ok(options)
    }

    fn refuse_unused(&self, command: &str, keys: &[&str]) -> io::Result<()> {
        let used = [
            ("--task", self.task.is_some()),
            ("--base", self.base.is_some()),
            ("--handle", self.handle.is_some()),
            ("--disposition", self.disposition.is_some()),
            ("--reason", self.reason.is_some()),
        ];
        for (key, present) in used {
            if present && !keys.contains(&key) {
                return Err(invalid(&format!(
                    "worktree {command} does not read {key}; see `codex-harness worktree --help`"
                )));
            }
        }
        Ok(())
    }
}

/// Resolved inputs of one worktree command: the kit home that owns the slot
/// records, the configuration source, the target repository identity and the
/// effective pool capacity with its account.
struct Context {
    codex_home: PathBuf,
    source: Option<PathBuf>,
    repository: PathBuf,
    identity: RepositoryIdentity,
    size: u32,
    account: String,
}

fn context(options: &Options) -> io::Result<Context> {
    for (flag, path) in [
        ("--source", options.source.as_deref()),
        ("--repository", options.repository.as_deref()),
        ("--codex-home", options.codex_home.as_deref()),
    ] {
        if let Some(path) = path.filter(|path| !path.is_absolute()) {
            return Err(invalid(&format!(
                "worktree {flag} must be an absolute path: {}",
                path.display()
            )));
        }
    }
    if let Some(source) = options.source.as_deref()
        && !source.is_dir()
    {
        return Err(invalid(&format!(
            "source checkout {} is missing; --source names the checkout whose global/orchestration.toml supplies the pool capacity",
            source.display()
        )));
    }
    let codex_home = match options.codex_home.clone() {
        Some(home) => home,
        None => harness_core::native_launcher::codex_home().map_err(|error| {
            invalid(&format!(
                "--codex-home is not given and the kit home cannot be resolved: {error}; pass --codex-home DIRECTORY"
            ))
        })?,
    };
    let source = match options.source.clone() {
        Some(source) => Some(source),
        None => crate::feedback_cli::installed_kit_source_in(&codex_home),
    };
    let repository = match options.repository.clone() {
        Some(repository) => repository,
        None => source.clone().ok_or_else(|| {
            invalid(
                "worktree commands need a target repository: pass --repository TARGET, or name --source CHECKOUT (no installed configuration source was found to default to)",
            )
        })?,
    };
    let identity = task_worktree::repository_identity(&repository)?;
    let (size, account) = capacity(source.as_deref())?;
    Ok(Context {
        codex_home,
        source,
        repository,
        identity,
        size,
        account,
    })
}

/// Pool capacity from the configuration source: an explicit
/// `worktree_pool_size`, else the legacy executor-derived value, else the
/// documented default when no configuration exists at all.
fn capacity(source: Option<&Path>) -> io::Result<(u32, String)> {
    let Some(source) = source else {
        return Ok((
            DEFAULT_POOL_CAPACITY,
            "defaults(no installed configuration source was found)".to_owned(),
        ));
    };
    let config = source.join("global/orchestration.toml");
    if !config.is_file() {
        return Ok((
            DEFAULT_POOL_CAPACITY,
            format!("defaults({} is absent)", config.display()),
        ));
    }
    let loaded = orchestration_config::load(source)?;
    Ok((
        worktree_pool_size(&loaded),
        format!("configured({})", config.display()),
    ))
}

fn acquire(args: &[OsString]) -> io::Result<i32> {
    let options = Options::parse(args)?;
    options.refuse_unused("acquire", &["--task", "--base"])?;
    let context = context(&options)?;
    let task = required_task(&options.task)?;
    let live = |owner: &str| {
        crate::executor_cli::owner_live(&context.codex_home, &context.repository, owner)
    };
    let (exact, _) =
        task_worktree::resolve_task_base(&context.repository, options.base.as_deref())?;
    let mut slot = task_worktree::acquire_task(
        &context.codex_home,
        &context.repository,
        context.size,
        &task,
        Some(&exact),
        &live,
    )?;
    // The claim reports the position as observed when it was selected, which
    // predates this acquisition creating and preparing a fresh position.
    // Receipts describe the acquired state, so re-read the position now that
    // the durable binding exists.
    if let Some(current) =
        task_worktree::task_inventory(&context.codex_home, &context.repository, context.size)?
            .into_iter()
            .find(|current| current.index == slot.index)
    {
        slot.presence = current.presence;
    }
    print_pool(&context);
    println!("{}", slot_line("acquire", &slot));
    Ok(0)
}

fn continue_task(args: &[OsString]) -> io::Result<i32> {
    let options = Options::parse(args)?;
    options.refuse_unused("continue", &["--task", "--handle"])?;
    let context = context(&options)?;
    let task = required_task(&options.task)?;
    let slot = task_worktree::continue_task(
        &context.codex_home,
        &context.repository,
        &task,
        options.handle.as_deref(),
    )?;
    print_pool(&context);
    println!("{}", slot_line("continue", &slot));
    Ok(0)
}

fn list(args: &[OsString]) -> io::Result<i32> {
    let options = Options::parse(args)?;
    options.refuse_unused("list", &[])?;
    let context = context(&options)?;
    print_pool(&context);
    let slots =
        task_worktree::task_inventory(&context.codex_home, &context.repository, context.size)?;
    for slot in &slots {
        println!("{}", slot_line("list", slot));
    }
    let audit = task_worktree::audit_pool(&context.repository, context.size)?;
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

fn release(args: &[OsString]) -> io::Result<i32> {
    let options = Options::parse(args)?;
    options.refuse_unused(
        "release",
        &["--task", "--base", "--handle", "--disposition", "--reason"],
    )?;
    let context = context(&options)?;
    let task = required_task(&options.task)?;
    let disposition = match options.disposition.as_deref() {
        Some("merged") => SlotDisposition::Merged,
        Some("discarded") => SlotDisposition::Discarded,
        Some(other) => {
            return Err(invalid(&format!(
                "unknown release disposition {other}; use merged or discarded"
            )));
        }
        None => {
            return Err(invalid(
                "worktree release requires --disposition merged|discarded",
            ));
        }
    };
    let reason = options
        .reason
        .as_deref()
        .map(str::trim)
        .filter(|reason| !reason.is_empty())
        .ok_or_else(|| {
            invalid(
                "worktree release requires --reason TEXT naming the acceptance or discard decision",
            )
        })?;
    let (base, _) = task_worktree::resolve_task_base(&context.repository, options.base.as_deref())?;
    // Freeze the observed generation of the exact binding this release
    // looked at, so a superseded release cannot reset whatever occupies the
    // position later.
    let inventory =
        task_worktree::task_inventory(&context.codex_home, &context.repository, context.size)?;
    let observed = inventory
        .iter()
        .find(|slot| slot.state != TaskState::Free && slot.task.as_deref() == Some(task.as_str()));
    let handle = options
        .handle
        .clone()
        .or_else(|| observed.and_then(|slot| slot.handle.clone()));
    let live = |owner: &str| {
        crate::executor_cli::owner_live(&context.codex_home, &context.repository, owner)
    };
    print_pool(&context);
    match task_worktree::release_task(
        &context.codex_home,
        &context.repository,
        &task,
        disposition,
        reason,
        &base,
        handle.as_deref(),
        &live,
    )? {
        LaneDisposition::Reused { base } => {
            let slot = observed
                .map(|slot| format!(" slot={} path={}", slot.index, slot.path.display()))
                .unwrap_or_default();
            println!(
                "worktree release: task={task} handle={} disposition={} released;{slot} reset to {base} and free for the next task",
                handle.as_deref().unwrap_or("-"),
                disposition_name(disposition)
            );
            Ok(0)
        }
        LaneDisposition::Preserved { limitation } => {
            let slot = observed.map(|slot| slot.index);
            println!(
                "worktree release: task={task} disposition={} in slot {} was recorded but the slot is preserved: {limitation}",
                disposition_name(disposition),
                slot.map(|index| index.to_string())
                    .unwrap_or_else(|| "-".to_owned())
            );
            Ok(2)
        }
    }
}

fn required_task(task: &Option<String>) -> io::Result<String> {
    match task
        .as_deref()
        .map(str::trim)
        .filter(|task| !task.is_empty())
    {
        Some(task) => Ok(task.to_owned()),
        None => Err(invalid(
            "worktree commands require --task ID, the durable task identity",
        )),
    }
}

fn print_pool(context: &Context) {
    let source = context
        .source
        .as_deref()
        .map(|source| source.display().to_string())
        .unwrap_or_else(|| "-".to_owned());
    println!(
        "worktree pool: repository={} identity={} source={source} capacity={} account={}",
        context.repository.display(),
        context.identity.primary_root.display(),
        context.size,
        context.account
    );
}

fn slot_line(action: &str, slot: &TaskSlot) -> String {
    format!(
        "worktree {action}: slot={} path={} presence={} state={} base={} task={} handle={} owner={} disposition={} reason={}",
        slot.index,
        slot.path.display(),
        presence_name(slot.presence),
        state_name(slot.state),
        slot.base.as_deref().unwrap_or("-"),
        slot.task.as_deref().unwrap_or("-"),
        slot.handle.as_deref().unwrap_or("-"),
        slot.owner.as_deref().unwrap_or("-"),
        slot.disposition.map(disposition_name).unwrap_or("-"),
        slot.reason.as_deref().unwrap_or("-"),
    )
}

fn presence_name(presence: SlotPresence) -> &'static str {
    match presence {
        SlotPresence::Absent => "absent",
        SlotPresence::Registered => "registered",
        SlotPresence::RegisteredMissing => "registered-missing",
        SlotPresence::Foreign => "foreign",
    }
}

fn state_name(state: TaskState) -> &'static str {
    match state {
        TaskState::Free => "free",
        TaskState::Held => "held",
        TaskState::AwaitingReview => "awaiting-review",
    }
}

fn disposition_name(disposition: SlotDisposition) -> &'static str {
    match disposition {
        SlotDisposition::Merged => "merged",
        SlotDisposition::Discarded => "discarded",
    }
}

fn text(value: &std::ffi::OsStr) -> io::Result<String> {
    value
        .to_str()
        .map(str::to_owned)
        .ok_or_else(|| invalid("invalid native worktree options"))
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}
