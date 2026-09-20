//! Executor checkouts: the harness-owned worktree pool plus the legacy native
//! mapping helpers kept for the installed CLI surface.
//!
//! Executor isolation is a fixed pool of ordinary Git worktrees created as
//! sibling directories of the source checkout and named `<repo-name>-wt1` ..
//! `<repo-name>-wtN` for the configured `max_concurrent_executors`. The
//! dispatch command is the sole allocator: it selects a free slot, creates it
//! only when its position does not exist yet, synchronizes it with upstream
//! before the first model request (fetch, reset to the resolved base, remove
//! untracked files while keeping ignored build caches) and records the slot
//! mapping in kit-local task state. Slot claims use exclusive file creation, so
//! a lost race surfaces as an occupied-slot refusal instead of two sessions in
//! one tree; reconciliation frees only slots whose recorded session is not
//! live, and a slot holding unreviewed work stays awaiting review until the
//! lead merges it or records an explicit discard.
use crate::build_identity::hash_bytes;
use serde::{Deserialize, Serialize};
use std::{
    ffi::OsString,
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mapping {
    pub schema: u32,
    pub path: PathBuf,
    pub source: PathBuf,
    pub head: String,
    pub owner_thread: Option<String>,
    pub archived: bool,
    pub unavailable: bool,
    pub remote_tui_omits_worktree_flag: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Retirement {
    Deleted,
    Preserved { limitation: String },
}

/// Outcome of returning a lane worktree after an accepted merge. Lanes are
/// lane-owned: a successfully reset lane stays in place for the next task in
/// the same lane, keeping its ignored build caches. Lane retirement and
/// unresettable state remain `retire`'s decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaneDisposition {
    Reused { base: String },
    Preserved { limitation: String },
}

pub fn limitation(detail: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        format!("managed worktrees unavailable: {detail}"),
    )
}

pub fn worktrees_enabled(codex_home: &Path) -> io::Result<bool> {
    let path = codex_home.join("config.toml");
    match fs::read_to_string(&path) {
        Ok(text) => Ok(text.contains("worktrees") && text.contains("true")),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

pub fn enable_worktrees(args: &mut Vec<OsString>) {
    if !args.iter().any(|arg| arg == "--enable") {
        args.splice(0..0, ["--enable".into(), "worktrees".into()]);
    }
}

pub fn strip_worktree_flag(args: &[OsString]) -> Vec<OsString> {
    args.iter()
        .filter(|arg| *arg != "--worktree")
        .cloned()
        .collect()
}

pub fn remote_tui_args(mapping: &Mapping, remote: &[OsString]) -> io::Result<Vec<OsString>> {
    if remote.iter().any(|arg| arg == "--worktree") && remote.iter().any(|arg| arg == "--remote") {
        return Err(limitation(
            "native CLI rejects --worktree with --remote; attach the view to the managed cwd",
        ));
    }
    let mut args = strip_worktree_flag(remote);
    let cwd = mapping
        .path
        .to_str()
        .ok_or_else(|| limitation("worktree path must be unicode"))?;
    if !args.iter().any(|arg| arg == "-C" || arg == "--cd") {
        args.splice(0..0, ["-C".into(), cwd.into()]);
    }
    Ok(args)
}

pub fn record(path: &Path, mapping: &Mapping) -> io::Result<()> {
    fs::create_dir_all(path.parent().unwrap_or(path))?;
    fs::write(path, serde_json::to_vec_pretty(mapping)?)?;
    Ok(())
}

pub fn load(path: &Path) -> io::Result<Mapping> {
    serde_json::from_slice(&fs::read(path)?).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("worktree mapping: {error}"),
        )
    })
}

pub fn refuse_shared_checkout(shared: &Path, mapping: &Mapping) -> io::Result<()> {
    let shared = fs::canonicalize(shared).unwrap_or_else(|_| shared.to_path_buf());
    let tree = fs::canonicalize(&mapping.path).unwrap_or_else(|_| mapping.path.clone());
    if shared == tree {
        return Err(limitation(
            "executor would write to the shared checkout; refusing substitution",
        ));
    }
    Ok(())
}

pub fn native_delete_eligible(mapping: &Mapping, current: &Path) -> io::Result<Result<(), String>> {
    let current = fs::canonicalize(current).unwrap_or_else(|_| current.to_path_buf());
    let tree = fs::canonicalize(&mapping.path).unwrap_or_else(|_| mapping.path.clone());
    if current == tree {
        return Ok(Err(
            "native confirmed deletion refuses the current checkout".into(),
        ));
    }
    if mapping.archived || mapping.unavailable {
        return Ok(Err(
            "agents-overview archive or unavailability is not worktree retirement".into(),
        ));
    }
    if !mapping.path.is_dir() {
        return Ok(Err("managed worktree path is missing".into()));
    }
    let inside = git(&mapping.path, &["rev-parse", "--is-inside-work-tree"])?;
    if inside.trim() != "true" {
        return Ok(Err("checkout is not a Git worktree".into()));
    }
    let porcelain = git(
        &mapping.path,
        &["status", "--porcelain=v1", "--untracked-files=all"],
    )?;
    if !porcelain.trim().is_empty() {
        return Ok(Err(
            "native confirmed deletion refuses local or untracked changes".into(),
        ));
    }
    let ignored = git(
        &mapping.path,
        &["ls-files", "--others", "--ignored", "--exclude-standard"],
    )?;
    if !ignored.trim().is_empty() {
        return Ok(Err("native confirmed deletion refuses ignored files".into()));
    }
    Ok(Ok(()))
}

pub fn retire(mapping: &Mapping, current: &Path) -> io::Result<Retirement> {
    match native_delete_eligible(mapping, current)? {
        Ok(()) => Ok(Retirement::Preserved {
            limitation: "native confirmed deletion is TUI-only on CLI 0.155.1; checkout preserved"
                .into(),
        }),
        Err(limitation) => Ok(Retirement::Preserved { limitation }),
    }
}

/// Reset a lane worktree to the committed base the lead merged, so the next
/// lane task starts from the new base with the lane's ignored build caches
/// intact (`git reset --hard <base>` plus `git clean -fd`). Unresettable state
/// is preserved with its reason for lane retirement instead of deleting or
/// reusing it; the lane inventory guard (`audit`) is unchanged because a reset
/// lane is not a new worktree.
pub fn reset_for_reuse(
    mapping: &Mapping,
    current: &Path,
    base: &str,
) -> io::Result<LaneDisposition> {
    let current = fs::canonicalize(current).unwrap_or_else(|_| current.to_path_buf());
    let tree = fs::canonicalize(&mapping.path).unwrap_or_else(|_| mapping.path.clone());
    if current == tree {
        return Ok(LaneDisposition::Preserved {
            limitation: "lane reset refuses the current checkout".into(),
        });
    }
    if mapping.archived || mapping.unavailable {
        return Ok(LaneDisposition::Preserved {
            limitation: "agents-overview archive or unavailability is not lane reuse".into(),
        });
    }
    if !mapping.path.is_dir() {
        return Ok(LaneDisposition::Preserved {
            limitation: "managed worktree path is missing".into(),
        });
    }
    let inside = git(&mapping.path, &["rev-parse", "--is-inside-work-tree"])?;
    if inside.trim() != "true" {
        return Ok(LaneDisposition::Preserved {
            limitation: "checkout is not a Git worktree".into(),
        });
    }
    let Ok(commit) = git(
        &mapping.path,
        &["rev-parse", "--verify", &format!("{base}^{{commit}}")],
    ) else {
        return Ok(LaneDisposition::Preserved {
            limitation: "committed base is not available in the lane; preserving the lane".into(),
        });
    };
    let commit = commit.trim().to_owned();
    if git(&mapping.path, &["reset", "--hard", &commit]).is_err() {
        return Ok(LaneDisposition::Preserved {
            limitation: "lane reset failed; preserving the lane for retirement".into(),
        });
    }
    if git(&mapping.path, &["clean", "-fd"]).is_err() {
        return Ok(LaneDisposition::Preserved {
            limitation: "lane cleanup failed; preserving the lane for retirement".into(),
        });
    }
    let head = git(&mapping.path, &["rev-parse", "HEAD"])?;
    if head.trim() != commit {
        return Ok(LaneDisposition::Preserved {
            limitation: "lane did not reach the merged base; preserving the lane for retirement"
                .into(),
        });
    }
    let porcelain = git(
        &mapping.path,
        &["status", "--porcelain=v1", "--untracked-files=all"],
    )?;
    if !porcelain.trim().is_empty() {
        return Ok(LaneDisposition::Preserved {
            limitation: "lane is not clean after reset; preserving the lane for retirement".into(),
        });
    }
    Ok(LaneDisposition::Reused { base: commit })
}

pub fn exec_isolation_args(codex_home: &Path, workspace: &Path) -> io::Result<Vec<String>> {
    if !is_shared_git_checkout(workspace)? {
        return Ok(Vec::new());
    }
    if !worktrees_enabled(codex_home)? {
        return Err(limitation(
            "experimental feature worktrees is disabled; refusing the shared checkout",
        ));
    }
    Ok(vec![
        "--enable".into(),
        "worktrees".into(),
        "--worktree".into(),
    ])
}

pub fn is_shared_git_checkout(path: &Path) -> io::Result<bool> {
    Ok(path.join(".git").is_dir())
}

pub fn is_git_checkout(path: &Path) -> io::Result<bool> {
    Ok(path.join(".git").exists())
}

/// Largest supported pool; matches the orchestration `max_concurrent_executors`
/// bound, so a configured value is always a valid pool size.
pub const MAX_POOL_SLOTS: u32 = 32;
const SLOT_STATE_SCHEMA: u32 = 1;
const CLAIM_ATTEMPTS: u32 = 4;

/// Where a pool position stands in the source checkout's worktree registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotPresence {
    /// The position does not exist yet; dispatch may create it.
    Absent,
    /// A worktree of this source checkout is registered at the position.
    Registered,
    /// Registered to this source checkout, but the directory is gone.
    RegisteredMissing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoolSlot {
    pub index: u32,
    pub path: PathBuf,
    pub presence: SlotPresence,
}

/// Deterministic sibling slot layout of one source checkout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pool {
    pub source: PathBuf,
    pub size: u32,
    pub slots: Vec<PoolSlot>,
}

impl Pool {
    pub fn slot(&self, index: u32) -> io::Result<&PoolSlot> {
        self.slots
            .iter()
            .find(|slot| slot.index == index)
            .ok_or_else(|| {
                pool_error(&format!(
                    "slot {index} is outside the configured pool of {} slots",
                    self.size
                ))
            })
    }
}

pub fn pool_error(detail: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        format!("executor worktree pool: {detail}"),
    )
}

/// Deterministic path of a pool slot: a sibling directory of the checkout.
pub fn slot_path(source: &Path, index: u32) -> io::Result<PathBuf> {
    if index == 0 || index > MAX_POOL_SLOTS {
        return Err(pool_error(&format!(
            "slot index {index} is out of range 1..={MAX_POOL_SLOTS}"
        )));
    }
    let parent = source
        .parent()
        .ok_or_else(|| pool_error("source checkout has no parent directory"))?;
    let name = source
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| pool_error("repository name must be unicode"))?;
    Ok(parent.join(format!("{name}-wt{index}")))
}

/// Resolve the pool of a source checkout. A pool position occupied by anything
/// that is not a registered worktree of this checkout - an unrelated directory
/// or another repository's tree - is refused instead of adopted or replaced.
pub fn pool(source: &Path, size: u32) -> io::Result<Pool> {
    if size == 0 || size > MAX_POOL_SLOTS {
        return Err(pool_error(&format!(
            "pool size {size} is out of range 1..={MAX_POOL_SLOTS}; it derives from max_concurrent_executors"
        )));
    }
    let source = resolve_source(source)?;
    let registered = registered_trees(&source)?;
    let mut slots = Vec::new();
    for index in 1..=size {
        let path = slot_path(&source, index)?;
        let presence = match registered.iter().find(|tree| same_path(&tree.path, &path)) {
            Some(tree) if tree.prunable || !path.is_dir() => SlotPresence::RegisteredMissing,
            Some(_) => SlotPresence::Registered,
            None if path.exists() => {
                return Err(pool_error(&format!(
                    "slot {index} {} exists but is not a registered worktree of {}; refusing to adopt or replace a foreign path",
                    path.display(),
                    source.display()
                )));
            }
            None => SlotPresence::Absent,
        };
        slots.push(PoolSlot {
            index,
            path,
            presence,
        });
    }
    Ok(Pool {
        source,
        size,
        slots,
    })
}

/// Create the slot at its pool position when it does not exist yet. Creation
/// never exceeds the pool: the position is fixed by the configured size.
pub fn create_slot(pool: &Pool, index: u32) -> io::Result<PathBuf> {
    let slot = pool.slot(index)?;
    match slot.presence {
        SlotPresence::Registered => Ok(slot.path.clone()),
        SlotPresence::RegisteredMissing => Err(pool_error(&format!(
            "slot {index} {} is registered but missing; run `git worktree prune` in {} and retry",
            slot.path.display(),
            pool.source.display()
        ))),
        SlotPresence::Absent => {
            let path = slot
                .path
                .to_str()
                .ok_or_else(|| pool_error("slot path must be unicode"))?;
            let out = Command::new(git_program())
                .args(["worktree", "add", "--detach", path, "HEAD"])
                .current_dir(&pool.source)
                .output()?;
            if !out.status.success() {
                return Err(pool_error(&format!(
                    "slot {index} could not be created at {}: {}",
                    slot.path.display(),
                    String::from_utf8_lossy(&out.stderr).trim()
                )));
            }
            Ok(slot.path.clone())
        }
    }
}

/// Pool slot index of a path that is a sibling position of this checkout.
pub fn slot_index(source: &Path, path: &Path) -> io::Result<Option<u32>> {
    let source = resolve_source(source)?;
    let (Some(parent), Some(name)) = (source.parent(), source.file_name()) else {
        return Ok(None);
    };
    let Some(name) = name.to_str() else {
        return Ok(None);
    };
    let Some(candidate) = path.parent() else {
        return Ok(None);
    };
    if !same_path(parent, candidate) {
        return Ok(None);
    }
    let Some(file) = path.file_name().and_then(|file| file.to_str()) else {
        return Ok(None);
    };
    let Some(digits) = file.strip_prefix(&format!("{name}-wt")) else {
        return Ok(None);
    };
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return Ok(None);
    }
    Ok(digits.parse::<u32>().ok().filter(|index| *index > 0))
}

/// Slot state machine. A slot is occupied only while its bound executor
/// session is live; a session-less slot holding unreviewed work stays awaiting
/// review until the lead merges it or records an explicit discard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SlotState {
    Free,
    Synchronizing,
    Occupied,
    AwaitingReview,
    Released,
}

/// Lead-recorded release disposition. A recorded disposition is what
/// authorizes destroying unreviewed work in a slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SlotDisposition {
    Merged,
    Discarded,
}

/// Kit-local task-state record of one pool slot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SlotRecord {
    pub schema: u32,
    pub source: PathBuf,
    pub index: u32,
    pub path: PathBuf,
    pub state: SlotState,
    /// Executor session identity that holds the slot while its session is live.
    pub owner: Option<String>,
    /// Committed base the slot was last synchronized to.
    pub base: Option<String>,
    /// Recorded release disposition (merged or explicitly discarded).
    pub disposition: Option<SlotDisposition>,
    /// Why the slot is preserved, or why the last dispatch aborted.
    pub reason: Option<String>,
}

/// Private slot state root; one directory per source checkout.
pub fn pool_state_dir(codex_home: &Path, source: &Path) -> io::Result<PathBuf> {
    let source = resolve_source(source)?;
    let name: String = source
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("checkout")
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let digest = hash_bytes(source.to_string_lossy().as_bytes());
    Ok(codex_home
        .join("harness/executor-pool")
        .join(format!("{name}-{}", &digest[..12])))
}

pub fn slot_record_path(codex_home: &Path, source: &Path, index: u32) -> io::Result<PathBuf> {
    Ok(pool_state_dir(codex_home, source)?.join(format!("slot-{index}.json")))
}

pub fn load_slot_record(
    codex_home: &Path,
    source: &Path,
    index: u32,
) -> io::Result<Option<SlotRecord>> {
    let path = slot_record_path(codex_home, source, index)?;
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    serde_json::from_slice(&bytes).map(Some).map_err(|error| {
        pool_error(&format!(
            "slot record {} is unreadable ({error}); the lead resolves or removes it",
            path.display()
        ))
    })
}

/// Claim a pool slot for one executor session. Exclusive file creation is the
/// compare-and-set step, so a concurrent claim loses the race instead of
/// sharing the tree; a claim whose session is gone is reclaimed only when the
/// slot is clean at its recorded base.
pub fn claim_slot(
    codex_home: &Path,
    pool: &Pool,
    index: u32,
    owner: &str,
    is_live: &dyn Fn(&str) -> bool,
) -> io::Result<SlotClaim> {
    if owner.trim().is_empty() {
        return Err(pool_error(
            "a slot claim requires the executor session identity",
        ));
    }
    let slot = pool.slot(index)?;
    let record_path = slot_record_path(codex_home, &pool.source, index)?;
    for _ in 0..CLAIM_ATTEMPTS {
        let previous = load_slot_record(codex_home, &pool.source, index)?;
        let claim = SlotRecord {
            schema: SLOT_STATE_SCHEMA,
            source: pool.source.clone(),
            index,
            path: slot.path.clone(),
            state: SlotState::Synchronizing,
            owner: Some(owner.to_owned()),
            base: previous.as_ref().and_then(|record| record.base.clone()),
            disposition: None,
            reason: None,
        };
        if create_slot_record(&record_path, &claim)? {
            return Ok(SlotClaim::Claimed(claim));
        }
        let Some(existing) = load_slot_record(codex_home, &pool.source, index)? else {
            continue;
        };
        if !same_path(&existing.source, &pool.source) {
            return Err(pool_error(&format!(
                "slot record {} belongs to another source checkout {}; the lead resolves it",
                record_path.display(),
                existing.source.display()
            )));
        }
        if existing.owner.as_deref() == Some(owner) {
            return Ok(SlotClaim::Claimed(existing));
        }
        if let Some(live) = existing.owner.as_deref().filter(|owner| is_live(owner)) {
            return Ok(SlotClaim::Unavailable(SlotRefusal {
                index,
                path: slot.path.clone(),
                reason: format!("slot {index} is occupied by live session {live}"),
            }));
        }
        match slot_content(pool, index, existing.base.as_deref())? {
            SlotContent::Reusable => match fs::remove_file(&record_path) {
                Ok(()) => continue,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            },
            SlotContent::Unreviewed(reason) | SlotContent::Missing(reason) => {
                write_slot_record(
                    &record_path,
                    &SlotRecord {
                        state: SlotState::AwaitingReview,
                        owner: None,
                        disposition: None,
                        reason: Some(reason.clone()),
                        ..existing
                    },
                )?;
                return Ok(SlotClaim::Unavailable(SlotRefusal {
                    index,
                    path: slot.path.clone(),
                    reason,
                }));
            }
        }
    }
    Err(pool_error(&format!(
        "slot {index} claim lost a race with another harness process; retry"
    )))
}

/// Bind the claimed slot to its session at the synchronized base.
pub fn bind_slot(
    codex_home: &Path,
    pool: &Pool,
    index: u32,
    owner: &str,
    base: &str,
) -> io::Result<SlotRecord> {
    let record = load_slot_record(codex_home, &pool.source, index)?
        .ok_or_else(|| pool_error(&format!("slot {index} has no claim to bind")))?;
    if record.owner.as_deref() != Some(owner) {
        return Err(pool_error(&format!(
            "slot {index} is claimed by {}; refusing to bind another session",
            record.owner.as_deref().unwrap_or("no session")
        )));
    }
    let bound = SlotRecord {
        state: SlotState::Occupied,
        base: Some(base.to_owned()),
        disposition: None,
        reason: None,
        ..record
    };
    write_slot_record(&slot_record_path(codex_home, &pool.source, index)?, &bound)?;
    Ok(bound)
}

/// Explicit slot release: record the lead's merged or discarded disposition,
/// then reset or preserve the tree under the existing reset-for-reuse rules.
/// A live owner session is never reset beneath.
pub fn release_slot(
    codex_home: &Path,
    pool: &Pool,
    index: u32,
    disposition: SlotDisposition,
    reason: &str,
    base: &str,
    is_live: &dyn Fn(&str) -> bool,
) -> io::Result<LaneDisposition> {
    let record = load_slot_record(codex_home, &pool.source, index)?.ok_or_else(|| {
        pool_error(&format!(
            "slot {index} has no recorded assignment to release"
        ))
    })?;
    if let Some(owner) = record.owner.as_deref().filter(|owner| is_live(owner)) {
        return Err(pool_error(&format!(
            "slot {index} is still owned by live session {owner}; stop it before release"
        )));
    }
    let slot = pool.slot(index)?;
    let commit = git(
        &slot.path,
        &["rev-parse", "--verify", &format!("{base}^{{commit}}")],
    )
    .map(|text| text.trim().to_owned())
    .map_err(|_| {
        pool_error(&format!(
            "release base '{base}' is not a commit in slot {index}; the slot is preserved unchanged"
        ))
    })?;
    let record_path = slot_record_path(codex_home, &pool.source, index)?;
    // The disposition is recorded before anything is destroyed: from here the
    // reset of this slot is authorized.
    let released = SlotRecord {
        state: SlotState::Released,
        owner: None,
        base: Some(commit.clone()),
        disposition: Some(disposition),
        reason: Some(reason.to_owned()),
        ..record
    };
    write_slot_record(&record_path, &released)?;
    let mapping = Mapping {
        schema: 1,
        path: slot.path.clone(),
        source: pool.source.clone(),
        head: commit.clone(),
        owner_thread: None,
        archived: false,
        unavailable: false,
        remote_tui_omits_worktree_flag: true,
    };
    match reset_for_reuse(&mapping, &pool.source, &commit)? {
        LaneDisposition::Reused { base } => Ok(LaneDisposition::Reused { base }),
        LaneDisposition::Preserved { limitation } => {
            write_slot_record(
                &record_path,
                &SlotRecord {
                    state: SlotState::AwaitingReview,
                    reason: Some(limitation.clone()),
                    ..released
                },
            )?;
            Ok(LaneDisposition::Preserved { limitation })
        }
    }
}

/// Reconcile recorded occupancy with executor session liveness. Only slots
/// whose recorded session is gone change state: a clean slot returns to the
/// pool, a slot holding work becomes awaiting review with its reason.
pub fn reconcile_slots(
    codex_home: &Path,
    pool: &Pool,
    is_live: &dyn Fn(&str) -> bool,
) -> io::Result<Vec<SlotRecord>> {
    let mut records = Vec::new();
    for slot in &pool.slots {
        let Some(record) = load_slot_record(codex_home, &pool.source, slot.index)? else {
            records.push(free_slot_record(pool, slot));
            continue;
        };
        let live = record.owner.as_deref().is_some_and(is_live);
        if live || !matches!(record.state, SlotState::Occupied | SlotState::Synchronizing) {
            records.push(record);
            continue;
        }
        let next = match slot_content(pool, slot.index, record.base.as_deref())? {
            SlotContent::Reusable => SlotRecord {
                state: SlotState::Free,
                owner: None,
                disposition: None,
                reason: None,
                ..record.clone()
            },
            SlotContent::Unreviewed(reason) | SlotContent::Missing(reason) => SlotRecord {
                state: SlotState::AwaitingReview,
                owner: None,
                reason: Some(reason),
                ..record.clone()
            },
        };
        write_slot_record(
            &slot_record_path(codex_home, &pool.source, slot.index)?,
            &next,
        )?;
        records.push(next);
    }
    Ok(records)
}

/// `git worktree list --porcelain` reports forward-slash paths on Windows; the
/// guard compares them with native paths, so normalize the separator and drop
/// the verbatim prefix `Path::canonicalize` adds.
fn native_path(path: &str) -> PathBuf {
    let text = path.strip_prefix(r"\\?\").unwrap_or(path);
    if cfg!(windows) {
        PathBuf::from(text.replace('/', "\\"))
    } else {
        PathBuf::from(text)
    }
}

/// Fail-closed upstream synchronization of one slot: fetch the configured
/// remote, resolve the default base or the explicit override, reset, remove
/// untracked files keeping ignored build caches, and verify a clean HEAD. A
/// failed fetch, an unresolvable base or unreviewed work aborts before the
/// slot changes.
pub fn synchronize_slot(
    codex_home: &Path,
    pool: &Pool,
    index: u32,
    base: Option<&str>,
) -> io::Result<SynchronizedSlot> {
    let slot = pool.slot(index)?;
    if !slot.path.is_dir() {
        return Err(pool_error(&format!(
            "slot {index} directory {} is missing",
            slot.path.display()
        )));
    }
    if git(&slot.path, &["rev-parse", "--is-inside-work-tree"])?.trim() != "true" {
        return Err(pool_error(&format!(
            "slot {index} {} is not a Git worktree",
            slot.path.display()
        )));
    }
    let (remote, branch) = upstream(&slot.path)?;
    git(&slot.path, &["fetch", &remote]).map_err(|error| {
        pool_error(&format!(
            "upstream fetch of '{remote}' failed for slot {index}; the slot keeps its previous state: {error}"
        ))
    })?;
    let commit = match base {
        Some(base) => resolve_commit(&slot.path, base).map_err(|_| {
            pool_error(&format!(
                "base '{base}' is not a commit in slot {index} after the upstream fetch; the slot keeps its previous state"
            ))
        })?,
        None => {
            let branch = branch.clone().ok_or_else(|| {
                pool_error(&format!(
                    "the upstream default branch for remote '{remote}' cannot be resolved; pass an explicit base"
                ))
            })?;
            resolve_commit(&slot.path, &format!("refs/remotes/{remote}/{branch}")).map_err(|_| {
                pool_error(&format!(
                    "the upstream default branch '{remote}/{branch}' cannot be resolved in slot {index}; pass an explicit base"
                ))
            })?
        }
    };
    // A reset destroys data only when the slot is clean or its release
    // disposition is recorded as merged or explicitly discarded.
    let record = load_slot_record(codex_home, &pool.source, index)?;
    if !record
        .as_ref()
        .is_some_and(|record| record.disposition.is_some())
    {
        let head = git(&slot.path, &["rev-parse", "HEAD"])
            .map(|text| text.trim().to_owned())
            .unwrap_or_default();
        let recorded = record.as_ref().and_then(|record| record.base.clone());
        let status = slot_status(&slot.path)?;
        if !status.trim().is_empty() {
            return Err(pool_error(&format!(
                "slot {index} holds unreviewed changes; merge or explicitly discard them before reuse: {}",
                status.trim()
            )));
        }
        if let Some(recorded) = recorded.filter(|recorded| *recorded != head) {
            return Err(pool_error(&format!(
                "slot {index} holds commits beyond its synchronized base {recorded}; review, merge or explicitly discard them before reuse"
            )));
        }
    }
    let previous_head = git(&slot.path, &["rev-parse", "HEAD"])
        .ok()
        .map(|text| text.trim().to_owned());
    git(&slot.path, &["reset", "--hard", &commit]).map_err(|error| {
        pool_error(&format!(
            "slot {index} could not reset to {commit}: {error}"
        ))
    })?;
    git(&slot.path, &["clean", "-fd"]).map_err(|error| {
        pool_error(&format!(
            "slot {index} could not remove untracked files: {error}"
        ))
    })?;
    let head = git(&slot.path, &["rev-parse", "HEAD"])
        .map(|text| text.trim().to_owned())
        .unwrap_or_default();
    let status = slot_status(&slot.path)?;
    if head != commit || !status.trim().is_empty() {
        return Err(pool_error(&format!(
            "slot {index} did not reach a clean synchronized state at {commit} (HEAD {head})"
        )));
    }
    Ok(SynchronizedSlot {
        base: commit,
        remote,
        branch,
        previous_head,
    })
}

/// Select, synchronize and bind a pool slot for one executor session. Every
/// unavailable position is reported with its cause; when the pool is full,
/// missing or unreviewed, dispatch aborts instead of allocating another tree.
pub fn acquire_slot(
    codex_home: &Path,
    source: &Path,
    size: u32,
    owner: &str,
    base: Option<&str>,
    is_live: &dyn Fn(&str) -> bool,
) -> io::Result<AcquiredSlot> {
    let pool = pool(source, size)?;
    reconcile_slots(codex_home, &pool, is_live)?;
    let mut refusals = Vec::new();
    for slot in &pool.slots {
        if slot.presence == SlotPresence::RegisteredMissing {
            return Err(pool_error(&format!(
                "slot {} {} is registered but missing; run `git worktree prune` in {} and retry",
                slot.index,
                slot.path.display(),
                pool.source.display()
            )));
        }
        create_slot(&pool, slot.index)?;
        match claim_slot(codex_home, &pool, slot.index, owner, is_live)? {
            SlotClaim::Claimed(_) => {
                let synchronized = match synchronize_slot(codex_home, &pool, slot.index, base) {
                    Ok(synchronized) => synchronized,
                    Err(error) => {
                        // Best-effort bookkeeping: the dispatch failure is the reported cause.
                        let _ =
                            abort_claim(codex_home, &pool, slot.index, owner, error.to_string());
                        return Err(error);
                    }
                };
                bind_slot(codex_home, &pool, slot.index, owner, &synchronized.base)?;
                return Ok(AcquiredSlot {
                    index: slot.index,
                    path: slot.path.clone(),
                    base: synchronized.base,
                    remote: synchronized.remote,
                    branch: synchronized.branch,
                    previous_head: synchronized.previous_head,
                });
            }
            SlotClaim::Unavailable(refusal) => refusals.push(refusal.reason),
        }
    }
    Err(pool_error(&format!(
        "no free slot in the pool of {size} (max_concurrent_executors): {}",
        refusals.join("; ")
    )))
}

/// Worktree inventory classification: pool slots of this checkout, foreign or
/// legacy trees for lead review, and - when the pool size is known - pool
/// positions beyond it, which harness dispatch never creates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeAudit {
    pub total: u32,
    /// Every registered path of the checkout, including the source itself.
    pub paths: Vec<PathBuf>,
    pub slots: Vec<PoolSlotAudit>,
    pub foreign: Vec<PathBuf>,
    pub beyond_pool: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoolSlotAudit {
    pub index: u32,
    pub path: PathBuf,
}

/// Pool inventory without a configured size: every pool-shaped position of
/// this checkout is reported as a slot.
pub fn audit(source: &Path) -> io::Result<WorktreeAudit> {
    classify_worktrees(source, None)
}

/// Pool inventory with the configured size: positions beyond the pool are
/// reported separately instead of being absorbed into the pool.
pub fn audit_pool(source: &Path, size: u32) -> io::Result<WorktreeAudit> {
    if size == 0 || size > MAX_POOL_SLOTS {
        return Err(pool_error(&format!(
            "pool size {size} is out of range 1..={MAX_POOL_SLOTS}; it derives from max_concurrent_executors"
        )));
    }
    classify_worktrees(source, Some(size))
}

fn classify_worktrees(source: &Path, size: Option<u32>) -> io::Result<WorktreeAudit> {
    // A source that is not a Git checkout has no registered trees; the audit
    // must not fail with an unrelated `git worktree` error.
    if !is_git_checkout(source)? {
        return Ok(WorktreeAudit {
            total: 0,
            paths: Vec::new(),
            slots: Vec::new(),
            foreign: Vec::new(),
            beyond_pool: Vec::new(),
        });
    }
    let output = git(source, &["worktree", "list", "--porcelain"])?;
    let mut paths = Vec::new();
    for line in output.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            paths.push(native_path(path));
        }
    }
    let mut slots = Vec::new();
    let mut foreign = Vec::new();
    let mut beyond_pool = Vec::new();
    for path in &paths {
        if same_path(path, source) {
            continue;
        }
        match slot_index(source, path)? {
            Some(index) if size.is_none_or(|size| index <= size) => {
                slots.push(PoolSlotAudit {
                    index,
                    path: path.clone(),
                });
            }
            Some(_) => beyond_pool.push(path.clone()),
            None => foreign.push(path.clone()),
        }
    }
    Ok(WorktreeAudit {
        total: paths.len() as u32,
        paths,
        slots,
        foreign,
        beyond_pool,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotRefusal {
    pub index: u32,
    pub path: PathBuf,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlotClaim {
    Claimed(SlotRecord),
    Unavailable(SlotRefusal),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SynchronizedSlot {
    pub base: String,
    pub remote: String,
    pub branch: Option<String>,
    pub previous_head: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcquiredSlot {
    pub index: u32,
    pub path: PathBuf,
    pub base: String,
    pub remote: String,
    pub branch: Option<String>,
    pub previous_head: Option<String>,
}

enum SlotContent {
    Reusable,
    Unreviewed(String),
    Missing(String),
}

fn slot_content(pool: &Pool, index: u32, base: Option<&str>) -> io::Result<SlotContent> {
    let slot = pool.slot(index)?;
    if !slot.path.is_dir() {
        return Ok(SlotContent::Missing(format!(
            "slot {index} directory {} is missing; run `git worktree prune` in {} and retry",
            slot.path.display(),
            pool.source.display()
        )));
    }
    let Ok(status) = slot_status(&slot.path) else {
        return Ok(SlotContent::Unreviewed(format!(
            "slot {index} is not a usable Git worktree; the lead resolves or retires it"
        )));
    };
    if !status.trim().is_empty() {
        return Ok(SlotContent::Unreviewed(format!(
            "slot {index} holds local or untracked changes; merge or explicitly discard them before reuse"
        )));
    }
    if let Some(base) = base {
        let head = git(&slot.path, &["rev-parse", "HEAD"])
            .map(|text| text.trim().to_owned())
            .unwrap_or_default();
        if head != base {
            return Ok(SlotContent::Unreviewed(format!(
                "slot {index} holds commits beyond its synchronized base {base}; review, merge or explicitly discard them before reuse"
            )));
        }
    }
    Ok(SlotContent::Reusable)
}

/// Hand a slot whose dispatch aborted back to the pool, or preserve it when it
/// still holds unreviewed work.
fn abort_claim(
    codex_home: &Path,
    pool: &Pool,
    index: u32,
    owner: &str,
    cause: String,
) -> io::Result<()> {
    let Some(record) = load_slot_record(codex_home, &pool.source, index)? else {
        return Ok(());
    };
    if record.owner.as_deref() != Some(owner) {
        return Ok(());
    }
    let state = match slot_content(pool, index, record.base.as_deref())? {
        SlotContent::Reusable => SlotState::Free,
        SlotContent::Unreviewed(_) | SlotContent::Missing(_) => SlotState::AwaitingReview,
    };
    write_slot_record(
        &slot_record_path(codex_home, &pool.source, index)?,
        &SlotRecord {
            state,
            owner: None,
            disposition: None,
            reason: Some(format!("dispatch aborted: {cause}")),
            ..record
        },
    )
}

fn free_slot_record(pool: &Pool, slot: &PoolSlot) -> SlotRecord {
    SlotRecord {
        schema: SLOT_STATE_SCHEMA,
        source: pool.source.clone(),
        index: slot.index,
        path: slot.path.clone(),
        state: SlotState::Free,
        owner: None,
        base: None,
        disposition: None,
        reason: None,
    }
}

fn write_slot_record(path: &Path, record: &SlotRecord) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let bytes = serde_json::to_vec_pretty(record)?;
    let tmp = path.with_extension("json.tmp");
    {
        let mut file = fs::File::create(&tmp)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
    }
    fs::rename(&tmp, path)?;
    Ok(())
}

/// Exclusive creation is the compare-and-set step of a slot claim.
fn create_slot_record(path: &Path, record: &SlotRecord) -> io::Result<bool> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let bytes = serde_json::to_vec_pretty(record)?;
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(mut file) => {
            file.write_all(&bytes)?;
            file.sync_all()?;
            Ok(true)
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(false),
        Err(error) => Err(error),
    }
}

fn slot_status(path: &Path) -> io::Result<String> {
    git(path, &["status", "--porcelain=v1", "--untracked-files=all"])
}

/// Remote and default branch of the checkout, resolved instead of hardcoded:
/// `origin` when present, otherwise the only configured remote.
fn upstream(path: &Path) -> io::Result<(String, Option<String>)> {
    let remote_list = git(path, &["remote"])?;
    let remotes: Vec<&str> = remote_list
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let remote = if remotes.contains(&"origin") {
        "origin".to_owned()
    } else if let [only] = remotes.as_slice() {
        (*only).to_owned()
    } else if remotes.is_empty() {
        return Err(pool_error(
            "the source checkout has no configured Git remote; refusing to dispatch from an unsynchronized slot",
        ));
    } else {
        return Err(pool_error(
            "the source checkout has several Git remotes and none named 'origin'; name the upstream explicitly",
        ));
    };
    let branch = default_branch(path, &remote);
    Ok((remote, branch))
}

fn default_branch(path: &Path, remote: &str) -> Option<String> {
    if let Ok(text) = git(
        path,
        &[
            "symbolic-ref",
            "--short",
            &format!("refs/remotes/{remote}/HEAD"),
        ],
    ) {
        let text = text.trim();
        if let Some(branch) = text.strip_prefix(&format!("{remote}/"))
            && !branch.is_empty()
        {
            return Some(branch.to_owned());
        }
    }
    if let Ok(text) = git(path, &["remote", "show", remote]) {
        for line in text.lines() {
            if let Some(branch) = line.trim().strip_prefix("HEAD branch:") {
                let branch = branch.trim();
                if !branch.is_empty() && branch != "(unknown)" {
                    return Some(branch.to_owned());
                }
            }
        }
    }
    None
}

fn resolve_commit(path: &Path, reference: &str) -> io::Result<String> {
    let text = git(
        path,
        &["rev-parse", "--verify", &format!("{reference}^{{commit}}")],
    )?;
    let commit = text.trim().to_owned();
    if commit.is_empty() {
        return Err(io::Error::other("empty revision"));
    }
    Ok(commit)
}

struct RegisteredTree {
    path: PathBuf,
    prunable: bool,
}

fn registered_trees(source: &Path) -> io::Result<Vec<RegisteredTree>> {
    if !is_git_checkout(source)? {
        return Err(pool_error(&format!(
            "source checkout {} is not a Git checkout; the executor worktree pool requires a Git repository",
            source.display()
        )));
    }
    let output = git(source, &["worktree", "list", "--porcelain"])?;
    let mut trees: Vec<RegisteredTree> = Vec::new();
    for line in output.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            trees.push(RegisteredTree {
                path: native_path(path),
                prunable: false,
            });
        } else if line.starts_with("prunable")
            && let Some(tree) = trees.last_mut()
        {
            tree.prunable = true;
        }
    }
    Ok(trees)
}

/// Canonical source path without the verbatim prefix `Path::canonicalize`
/// adds; Git and the checkout's own paths compare in this form.
fn resolve_source(source: &Path) -> io::Result<PathBuf> {
    let resolved = fs::canonicalize(source).map_err(|error| {
        pool_error(&format!(
            "source checkout {} is not usable: {error}",
            source.display()
        ))
    })?;
    Ok(native_path(&resolved.to_string_lossy()))
}

fn same_path(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(a), Ok(b)) if a == b => true,
        _ => {
            cfg!(windows)
                && a.to_string_lossy()
                    .eq_ignore_ascii_case(&b.to_string_lossy())
        }
    }
}

/// The Git program. Tests resolve an absolute path once because another test
/// may temporarily replace the process `PATH`; a worktree test must not depend
/// on the live value.
#[cfg(test)]
fn git_program() -> OsString {
    tests::git_program().into_os_string()
}

#[cfg(not(test))]
fn git_program() -> OsString {
    OsString::from("git")
}

fn git(cwd: &Path, args: &[&str]) -> io::Result<String> {
    let out = Command::new(git_program())
        .args(args)
        .current_dir(cwd)
        .output()?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(io::Error::other(format!(
            "git {}: {}",
            args.first().unwrap_or(&"git"),
            String::from_utf8_lossy(&out.stderr)
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audit_counts_the_authoritative_worktree_inventory() {
        let root = tempfile::tempdir().unwrap();
        let repo = repo(root.path());
        let lane = root.path().join("lane");
        git_ok(
            &repo,
            &["worktree", "add", "--detach", lane.to_str().unwrap()],
        );
        let audit = audit(&repo).unwrap();
        assert_eq!(audit.total, 2);
        assert!(audit.paths.contains(&repo));
        // The guard reports native paths (`git` prints forward slashes on
        // Windows); it must not report the verbatim `canonicalize` form.
        assert!(audit.paths.contains(&lane), "{:?}", audit.paths);
        // A task-named lane is foreign: reported for lead review, never pooled.
        assert!(audit.slots.is_empty());
        assert_eq!(audit.foreign, std::slice::from_ref(&lane));
        assert!(audit.beyond_pool.is_empty());
        assert_eq!(audit_pool(&repo, 2).unwrap(), audit);
    }

    #[test]
    fn audit_classifies_pool_slots_legacy_trees_and_beyond_pool() {
        let root = tempfile::tempdir().unwrap();
        let up = upstream(root.path());
        let slot = create_slot(&pool(&up.source, 2).unwrap(), 1).unwrap();
        let legacy = up.source.parent().unwrap().join("task-legacy-lane");
        git_ok(
            &up.source,
            &[
                "worktree",
                "add",
                "--detach",
                legacy.to_str().unwrap(),
                "HEAD",
            ],
        );
        let classified = audit_pool(&up.source, 2).unwrap();
        assert_eq!(classified.total, 3);
        assert_eq!(
            classified.slots,
            [PoolSlotAudit {
                index: 1,
                path: slot.clone()
            }]
        );
        assert_eq!(classified.foreign, std::slice::from_ref(&legacy));
        assert!(classified.beyond_pool.is_empty());
        // Legacy trees are reported, never adopted into the pool or deleted.
        assert!(legacy.is_dir());
        assert!(
            pool(&up.source, 2)
                .unwrap()
                .slots
                .iter()
                .all(|slot| slot.path != legacy)
        );
        let beyond = up.source.parent().unwrap().join("proj-wt3");
        git_ok(
            &up.source,
            &[
                "worktree",
                "add",
                "--detach",
                beyond.to_str().unwrap(),
                "HEAD",
            ],
        );
        let classified = audit_pool(&up.source, 2).unwrap();
        assert_eq!(classified.beyond_pool, std::slice::from_ref(&beyond));
        assert_eq!(classified.slots.len(), 1);
        assert!(classified.foreign.contains(&legacy));
        assert!(
            beyond.is_dir(),
            "a tree beyond the pool is reported, not removed"
        );
        // Without a configured size every pool-shaped position is a slot.
        let unconfigured = audit(&up.source).unwrap();
        assert_eq!(
            unconfigured
                .slots
                .iter()
                .map(|slot| slot.index)
                .collect::<Vec<_>>(),
            [1, 3]
        );
        assert!(unconfigured.beyond_pool.is_empty());
    }

    fn repo(root: &Path) -> PathBuf {
        let repo = root.join("repo");
        fs::create_dir_all(&repo).unwrap();
        git_ok(&repo, &["init", "-q"]);
        git_ok(&repo, &["config", "user.email", "worktree@example.test"]);
        git_ok(&repo, &["config", "user.name", "Worktree"]);
        fs::write(repo.join("README.md"), "shared\n").unwrap();
        git_ok(&repo, &["add", "README.md"]);
        git_ok(&repo, &["commit", "-qm", "seed"]);
        repo
    }

    fn git_ok(cwd: &Path, args: &[&str]) {
        let out = Command::new(git_program())
            .args(args)
            .current_dir(cwd)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// Git resolved once to an absolute path, so later calls do not depend on
    /// the live `PATH`: tests that publish a process `PATH` change run in this
    /// same binary. When the live value is a synthetic replacement, the
    /// standard install locations keep fixture repositories runnable.
    pub(super) fn git_program() -> PathBuf {
        static GIT: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
        GIT.get_or_init(|| {
            discover_git().unwrap_or_else(|| {
                panic!("git is not discoverable on PATH or in a standard location")
            })
        })
        .clone()
    }

    fn discover_git() -> Option<PathBuf> {
        let from_path = std::env::var_os("PATH").and_then(|path| {
            std::env::split_paths(&path)
                .map(|dir| git_file(&dir))
                .find(|candidate| candidate.is_file())
        });
        from_path.or_else(|| {
            standard_git_directories()
                .into_iter()
                .map(|dir| git_file(&dir))
                .find(|candidate| candidate.is_file())
        })
    }

    fn standard_git_directories() -> Vec<PathBuf> {
        let mut directories = Vec::new();
        for (variable, relative) in [
            ("ProgramFiles", r"Git\cmd"),
            ("ProgramFiles(x86)", r"Git\cmd"),
            ("LOCALAPPDATA", r"Programs\Git\cmd"),
            ("ProgramData", r"chocolatey\bin"),
            ("USERPROFILE", r"scoop\shims"),
        ] {
            if let Some(root) = std::env::var_os(variable) {
                directories.push(PathBuf::from(root).join(relative));
            }
        }
        directories.push(PathBuf::from(r"C:\Program Files\Git\cmd"));
        for directory in ["/usr/bin", "/usr/local/bin", "/opt/homebrew/bin"] {
            directories.push(PathBuf::from(directory));
        }
        directories
    }

    fn git_file(directory: &Path) -> PathBuf {
        if cfg!(windows) {
            directory.join("git.exe")
        } else {
            directory.join("git")
        }
    }

    struct Upstream {
        bare: PathBuf,
        source: PathBuf,
        author: PathBuf,
    }

    /// A checkout of a local `file://` upstream: bare remote, the source clone
    /// dispatch runs against, and an author clone that advances the upstream.
    fn upstream(root: &Path) -> Upstream {
        let bare = root.join("remote.git");
        git_ok(
            root,
            &[
                "init",
                "--bare",
                "-q",
                "--initial-branch=main",
                bare.to_str().unwrap(),
            ],
        );
        let seed = root.join("seed");
        git_ok(
            root,
            &[
                "init",
                "-q",
                "--initial-branch=main",
                seed.to_str().unwrap(),
            ],
        );
        configure(&seed);
        fs::write(seed.join(".gitignore"), "cache/\n").unwrap();
        fs::write(seed.join("README.md"), "seed\n").unwrap();
        git_ok(&seed, &["add", "."]);
        git_ok(&seed, &["commit", "-qm", "seed"]);
        git_ok(&seed, &["remote", "add", "origin", &file_url(&bare)]);
        git_ok(&seed, &["push", "-q", "origin", "main"]);
        let source = root.join("proj");
        git_ok(
            root,
            &["clone", "-q", &file_url(&bare), source.to_str().unwrap()],
        );
        configure(&source);
        let author = root.join("author");
        git_ok(
            root,
            &["clone", "-q", &file_url(&bare), author.to_str().unwrap()],
        );
        configure(&author);
        Upstream {
            bare,
            source,
            author,
        }
    }

    fn file_url(path: &Path) -> String {
        format!("file:///{}", path.to_str().unwrap().replace('\\', "/"))
    }

    fn configure(cwd: &Path) {
        git_ok(cwd, &["config", "user.email", "worktree@example.test"]);
        git_ok(cwd, &["config", "user.name", "Worktree"]);
    }

    fn commit(cwd: &Path, file: &str, text: &str) {
        fs::write(cwd.join(file), text).unwrap();
        git_ok(cwd, &["add", file]);
        git_ok(cwd, &["commit", "-qm", &format!("update {file}")]);
    }

    /// A new upstream revision the next dispatch must fetch.
    fn advance(up: &Upstream, file: &str) {
        commit(&up.author, file, "upstream moved on\n");
        git_ok(&up.author, &["push", "-q", "origin", "main"]);
    }

    fn registered_count(source: &Path) -> usize {
        git(source, &["worktree", "list", "--porcelain"])
            .unwrap()
            .lines()
            .filter(|line| line.starts_with("worktree "))
            .count()
    }

    #[test]
    fn remote_tui_rejects_worktree_flag_and_attaches_cwd() {
        let mapping = Mapping {
            schema: 1,
            path: PathBuf::from(r"D:\wt\exec"),
            source: PathBuf::from(r"D:\repo"),
            head: "abc".into(),
            owner_thread: Some("thread-1".into()),
            archived: false,
            unavailable: false,
            remote_tui_omits_worktree_flag: true,
        };
        let error = remote_tui_args(
            &mapping,
            &[
                "--remote".into(),
                "ws://127.0.0.1:1".into(),
                "--worktree".into(),
            ],
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("rejects --worktree with --remote")
        );
        let args = remote_tui_args(
            &mapping,
            &[
                "--remote".into(),
                "ws://127.0.0.1:1".into(),
                "resume".into(),
            ],
        )
        .unwrap();
        assert_eq!(args[0], "-C");
        assert_eq!(args[1], r"D:\wt\exec");
        assert!(!args.iter().any(|arg| arg == "--worktree"));
    }

    #[test]
    fn exec_isolation_uses_native_worktree_flag_not_ordinary_git() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        fs::create_dir_all(&home).unwrap();
        let workspace = root.path().join("scratch");
        fs::create_dir_all(&workspace).unwrap();
        assert!(exec_isolation_args(&home, &workspace).unwrap().is_empty());
        let source = repo(root.path());
        let error = exec_isolation_args(&home, &source).unwrap_err();
        assert!(error.to_string().contains("refusing the shared checkout"));
        fs::write(home.join("config.toml"), "features.worktrees = true\n").unwrap();
        assert_eq!(
            exec_isolation_args(&home, &source).unwrap(),
            ["--enable", "worktrees", "--worktree"]
        );
    }

    #[test]
    fn pool_slot_names_are_deterministic_siblings_within_the_configured_cap() {
        let root = tempfile::tempdir().unwrap();
        let up = upstream(root.path());
        let layout = pool(&up.source, 2).unwrap();
        assert_eq!(layout.size, 2);
        assert_eq!(
            layout
                .slots
                .iter()
                .map(|slot| slot.path.file_name().unwrap().to_str().unwrap())
                .collect::<Vec<_>>(),
            ["proj-wt1", "proj-wt2"]
        );
        assert_eq!(layout.slots[0].path.parent(), up.source.parent());
        assert!(
            layout
                .slots
                .iter()
                .all(|slot| slot.presence == SlotPresence::Absent)
        );
        let error = pool(&up.source, 0).unwrap_err().to_string();
        assert!(error.contains("out of range 1..=32"), "{error}");
        let error = pool(&up.source, 33).unwrap_err().to_string();
        assert!(error.contains("out of range 1..=32"), "{error}");
        assert_eq!(slot_path(&up.source, 1).unwrap(), layout.slots[0].path);
        assert!(slot_path(&up.source, 0).is_err());
        assert!(layout.slot(3).is_err());
        let created = create_slot(&layout, 2).unwrap();
        assert!(created.is_dir());
        assert_eq!(rev(&created), rev(&up.source));
        // Slot creation never reaches beyond the configured pool.
        assert!(!up.source.parent().unwrap().join("proj-wt3").exists());
        let layout = pool(&up.source, 2).unwrap();
        assert_eq!(layout.slots[1].presence, SlotPresence::Registered);
        assert_eq!(layout.slots[0].presence, SlotPresence::Absent);
    }

    #[test]
    fn pool_refuses_a_foreign_path_at_a_slot_position() {
        let root = tempfile::tempdir().unwrap();
        let up = upstream(root.path());
        let foreign = root.path().join("proj-wt1");
        fs::create_dir_all(foreign.join("nested")).unwrap();
        fs::write(foreign.join("keep.txt"), "unrelated work\n").unwrap();
        let error = pool(&up.source, 2).unwrap_err().to_string();
        assert!(error.contains("refusing to adopt or replace"), "{error}");
        assert!(error.contains("proj-wt1"), "{error}");
        // The foreign path is neither adopted nor deleted.
        assert!(foreign.join("keep.txt").is_file());
        assert!(!foreign.join(".git").exists());
        // Another repository's worktree at a slot position is refused too.
        let other = repo(root.path());
        git_ok(
            &other,
            &[
                "worktree",
                "add",
                "--detach",
                root.path().join("proj-wt2").to_str().unwrap(),
                "HEAD",
            ],
        );
        fs::remove_dir_all(&foreign).unwrap();
        let error = pool(&up.source, 2).unwrap_err().to_string();
        assert!(error.contains("proj-wt2"), "{error}");
        let inside = git(
            &root.path().join("proj-wt2"),
            &["rev-parse", "--is-inside-work-tree"],
        )
        .unwrap();
        assert_eq!(inside.trim(), "true");
    }

    #[test]
    fn pool_reports_registered_slots_and_the_prune_hint_for_missing_trees() {
        let root = tempfile::tempdir().unwrap();
        let up = upstream(root.path());
        let created = create_slot(&pool(&up.source, 1).unwrap(), 1).unwrap();
        let layout = pool(&up.source, 1).unwrap();
        assert_eq!(layout.slots[0].presence, SlotPresence::Registered);
        assert_eq!(create_slot(&layout, 1).unwrap(), created);
        fs::remove_dir_all(&created).unwrap();
        let layout = pool(&up.source, 1).unwrap();
        assert_eq!(layout.slots[0].presence, SlotPresence::RegisteredMissing);
        let error = create_slot(&layout, 1).unwrap_err().to_string();
        assert!(error.contains("git worktree prune"), "{error}");
        let error = acquire_slot(
            &root.path().join("home"),
            &up.source,
            1,
            "exec-1",
            None,
            &|_: &str| false,
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("git worktree prune"), "{error}");
    }

    #[test]
    fn pool_requires_a_git_checkout() {
        let root = tempfile::tempdir().unwrap();
        let plain = root.path().join("plain");
        fs::create_dir_all(&plain).unwrap();
        let error = pool(&plain, 2).unwrap_err().to_string();
        assert!(error.contains("is not a Git checkout"), "{error}");
    }

    #[test]
    fn dirty_lane_retirement_is_preserved_and_mapping_round_trips() {
        let root = tempfile::tempdir().unwrap();
        let source = repo(root.path());
        let lane = root.path().join("lane");
        git_ok(
            &source,
            &[
                "worktree",
                "add",
                "--detach",
                lane.to_str().unwrap(),
                "HEAD",
            ],
        );
        let mapping = Mapping {
            schema: 1,
            path: lane.clone(),
            source: source.clone(),
            head: rev(&source),
            owner_thread: Some("exec-1".into()),
            archived: false,
            unavailable: false,
            remote_tui_omits_worktree_flag: true,
        };
        refuse_shared_checkout(&source, &mapping).unwrap();
        assert!(refuse_shared_checkout(&lane, &mapping).is_err());
        fs::write(lane.join("scratch.txt"), "dirty\n").unwrap();
        match retire(&mapping, &source).unwrap() {
            Retirement::Preserved { limitation } => {
                assert!(limitation.contains("untracked") || limitation.contains("TUI-only"));
            }
            Retirement::Deleted => panic!("dirty tree must not be deleted"),
        }
        assert!(lane.exists());
        record(&source.join("executor-worktree.json"), &mapping).unwrap();
        assert_eq!(
            load(&source.join("executor-worktree.json")).unwrap().head,
            mapping.head
        );
    }

    #[test]
    fn claim_refuses_a_double_claim_and_reconciliation_frees_only_session_less_slots() {
        let root = tempfile::tempdir().unwrap();
        let up = upstream(root.path());
        let home = root.path().join("home");
        let layout = pool(&up.source, 1).unwrap();
        create_slot(&layout, 1).unwrap();
        let live = |session: &str| session == "exec-1";
        let SlotClaim::Claimed(record) = claim_slot(&home, &layout, 1, "exec-1", &live).unwrap()
        else {
            panic!("the first claim must win the slot");
        };
        assert_eq!(record.state, SlotState::Synchronizing);
        assert_eq!(record.owner.as_deref(), Some("exec-1"));
        assert!(
            slot_record_path(&home, &up.source, 1)
                .unwrap()
                .starts_with(&home),
            "slot state is kit-local, never recorded in the checkout"
        );
        // A second session cannot take the slot while the first is live.
        match claim_slot(&home, &layout, 1, "exec-2", &live).unwrap() {
            SlotClaim::Unavailable(refusal) => {
                assert!(
                    refusal.reason.contains("occupied by live session exec-1"),
                    "{}",
                    refusal.reason
                );
            }
            SlotClaim::Claimed(_) => panic!("a live claim must not be double-claimed"),
        }
        // The same session re-claims its slot after an interruption.
        assert!(matches!(
            claim_slot(&home, &layout, 1, "exec-1", &live).unwrap(),
            SlotClaim::Claimed(_)
        ));
        // Liveness reconciliation changes only session-less slots.
        let kept = reconcile_slots(&home, &layout, &live).unwrap();
        assert_eq!(kept[0].state, SlotState::Synchronizing);
        assert_eq!(kept[0].owner.as_deref(), Some("exec-1"));
        let dead = |_: &str| false;
        let freed = reconcile_slots(&home, &layout, &dead).unwrap();
        assert_eq!(freed[0].state, SlotState::Free);
        assert_eq!(freed[0].owner, None);
        // The freed slot serves the next session; the live one still would not.
        assert!(matches!(
            claim_slot(&home, &layout, 1, "exec-2", &dead).unwrap(),
            SlotClaim::Claimed(_)
        ));
        assert!(matches!(
            claim_slot(&home, &layout, 1, "exec-3", &live).unwrap(),
            SlotClaim::Claimed(_)
        ));
        let error = claim_slot(&home, &layout, 1, "", &dead).unwrap_err();
        assert!(error.to_string().contains("session identity"), "{error}");
    }

    #[test]
    fn acquire_slot_reuses_the_same_slot_and_refuses_a_full_pool() {
        let root = tempfile::tempdir().unwrap();
        let up = upstream(root.path());
        let home = root.path().join("home");
        let live = |session: &str| session == "exec-1";
        let first = acquire_slot(&home, &up.source, 1, "exec-1", None, &live).unwrap();
        assert_eq!(first.index, 1);
        assert_eq!(first.path.file_name().unwrap(), "proj-wt1");
        assert_eq!(first.remote, "origin");
        assert_eq!(first.branch.as_deref(), Some("main"));
        assert_eq!(first.base, rev(&up.source));
        assert_eq!(rev(&first.path), rev(&up.source));
        assert_eq!(registered_count(&up.source), 2);
        // A full pool refuses dispatch with the occupied slot and adds no tree.
        let error = acquire_slot(&home, &up.source, 1, "exec-2", None, &live)
            .unwrap_err()
            .to_string();
        assert!(error.contains("no free slot in the pool of 1"), "{error}");
        assert!(error.contains("occupied by live session exec-1"), "{error}");
        assert_eq!(registered_count(&up.source), 2);
        // After the session ends the same slot serves the next assignment.
        let dead = |_: &str| false;
        let second = acquire_slot(&home, &up.source, 1, "exec-2", None, &dead).unwrap();
        assert_eq!(second.path, first.path);
        assert_eq!(registered_count(&up.source), 2);
        let record = load_slot_record(&home, &up.source, 1).unwrap().unwrap();
        assert_eq!(record.state, SlotState::Occupied);
        assert_eq!(record.owner.as_deref(), Some("exec-2"));
        assert_eq!(record.base.as_deref(), Some(second.base.as_str()));
    }

    #[test]
    fn fetch_failure_aborts_dispatch_and_leaves_the_slot_untouched() {
        let root = tempfile::tempdir().unwrap();
        let up = upstream(root.path());
        let home = root.path().join("home");
        let acquired = acquire_slot(&home, &up.source, 1, "exec-1", None, &|session: &str| {
            session == "exec-1"
        })
        .unwrap();
        let pristine = rev(&acquired.path);
        git_ok(
            &up.source,
            &[
                "remote",
                "set-url",
                "origin",
                &file_url(&root.path().join("missing.git")),
            ],
        );
        let error = acquire_slot(&home, &up.source, 1, "exec-2", None, &|_: &str| false)
            .unwrap_err()
            .to_string();
        assert!(error.contains("upstream fetch"), "{error}");
        assert_eq!(
            rev(&acquired.path),
            pristine,
            "a failed fetch never degrades to a stale or partial base"
        );
        let record = load_slot_record(&home, &up.source, 1).unwrap().unwrap();
        assert_eq!(record.state, SlotState::Free);
        assert_eq!(record.owner, None);
        // The upstream returns; the next dispatch synchronizes again.
        git_ok(
            &up.source,
            &["remote", "set-url", "origin", &file_url(&up.bare)],
        );
        advance(&up, "upstream.txt");
        let next = acquire_slot(&home, &up.source, 1, "exec-2", None, &|_: &str| false).unwrap();
        assert_eq!(next.base, rev(&up.author));
        assert_eq!(rev(&acquired.path), rev(&up.author));
    }

    #[test]
    fn explicit_base_override_is_honored_and_recorded() {
        let root = tempfile::tempdir().unwrap();
        let up = upstream(root.path());
        let home = root.path().join("home");
        let earlier = rev(&up.source);
        advance(&up, "upstream.txt");
        let acquired = acquire_slot(
            &home,
            &up.source,
            1,
            "exec-1",
            Some(&earlier),
            &|_: &str| false,
        )
        .unwrap();
        assert_eq!(acquired.base, earlier);
        assert_eq!(rev(&acquired.path), earlier);
        assert!(!acquired.path.join("upstream.txt").exists());
        let record = load_slot_record(&home, &up.source, 1).unwrap().unwrap();
        assert_eq!(record.base.as_deref(), Some(earlier.as_str()));
        // An override that resolves to nothing aborts without touching the slot.
        let error = synchronize_slot(
            &home,
            &pool(&up.source, 1).unwrap(),
            1,
            Some("no-such-revision"),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("is not a commit"), "{error}");
        assert_eq!(rev(&acquired.path), earlier);
    }

    #[test]
    fn synchronization_resets_to_the_fetched_upstream_and_keeps_ignored_caches() {
        let root = tempfile::tempdir().unwrap();
        let up = upstream(root.path());
        let home = root.path().join("home");
        let acquired =
            acquire_slot(&home, &up.source, 1, "exec-1", None, &|_: &str| false).unwrap();
        let slot = acquired.path.clone();
        fs::create_dir_all(slot.join("cache")).unwrap();
        fs::write(slot.join("cache/warm.bin"), "warm\n").unwrap();
        fs::write(slot.join("scratch.txt"), "leftover\n").unwrap();
        // The executor commits; the lead merges and accepts the slice.
        commit(&slot, "slice.txt", "executor work\n");
        let merged = rev(&slot);
        git_ok(&up.source, &["merge", "-q", "--no-edit", &merged]);
        let base = rev(&up.source);
        let disposition = release_slot(
            &home,
            &pool(&up.source, 1).unwrap(),
            1,
            SlotDisposition::Merged,
            "slice accepted",
            &base,
            &|_: &str| false,
        )
        .unwrap();
        assert_eq!(disposition, LaneDisposition::Reused { base: base.clone() });
        assert_eq!(rev(&slot), base);
        assert!(!slot.join("scratch.txt").exists());
        assert!(slot.join("cache/warm.bin").is_file());
        // The upstream advances: the next dispatch fetches it mechanically.
        advance(&up, "upstream.txt");
        let tip = rev(&up.author);
        let next = acquire_slot(&home, &up.source, 1, "exec-2", None, &|_: &str| false).unwrap();
        assert_eq!(next.base, tip);
        assert_eq!(rev(&slot), tip);
        assert!(slot.join("upstream.txt").is_file());
        assert!(
            slot.join("cache/warm.bin").is_file(),
            "ignored build caches stay warm through synchronization"
        );
        assert!(!slot.join("scratch.txt").exists());
        assert_eq!(
            load_slot_record(&home, &up.source, 1)
                .unwrap()
                .unwrap()
                .base
                .as_deref(),
            Some(tip.as_str())
        );
    }

    #[test]
    fn overview_archive_is_not_retirement() {
        let mapping = Mapping {
            schema: 1,
            path: PathBuf::from("."),
            source: PathBuf::from("."),
            head: "HEAD".into(),
            owner_thread: None,
            archived: true,
            unavailable: false,
            remote_tui_omits_worktree_flag: true,
        };
        let retired = retire(&mapping, Path::new("..")).unwrap();
        match retired {
            Retirement::Preserved { limitation } => {
                assert!(limitation.contains("archive"));
            }
            Retirement::Deleted => panic!("archive must not delete"),
        }
    }

    #[test]
    fn audit_of_a_non_git_source_has_no_lanes() {
        let root = tempfile::tempdir().unwrap();
        let exported = root.path().join("packaged-kit");
        fs::create_dir_all(&exported).unwrap();
        let audit = audit(&exported).unwrap();
        assert_eq!(audit.total, 0);
        assert!(audit.paths.is_empty());
    }

    #[test]
    fn dirty_slot_without_a_live_session_is_preserved_as_awaiting_review() {
        let root = tempfile::tempdir().unwrap();
        let up = upstream(root.path());
        let home = root.path().join("home");
        let acquired = acquire_slot(&home, &up.source, 1, "exec-1", None, &|session: &str| {
            session == "exec-1"
        })
        .unwrap();
        let slot = acquired.path.clone();
        let base = acquired.base.clone();
        fs::write(slot.join("scratch.txt"), "unreviewed\n").unwrap();
        let dead = |_: &str| false;
        let reconciled = reconcile_slots(&home, &pool(&up.source, 1).unwrap(), &dead).unwrap();
        assert_eq!(reconciled[0].state, SlotState::AwaitingReview);
        assert!(
            reconciled[0]
                .reason
                .as_deref()
                .unwrap()
                .contains("local or untracked changes"),
            "{:?}",
            reconciled[0].reason
        );
        // Dispatch never selects or resets it and reports the concrete cause.
        let error = acquire_slot(&home, &up.source, 1, "exec-2", None, &dead)
            .unwrap_err()
            .to_string();
        assert!(error.contains("no free slot in the pool of 1"), "{error}");
        assert!(error.contains("local or untracked changes"), "{error}");
        assert!(
            slot.join("scratch.txt").is_file(),
            "unreviewed work is never reset"
        );
        assert_eq!(rev(&slot), base);
        // Committed but unmerged work is preserved the same way.
        fs::remove_file(slot.join("scratch.txt")).unwrap();
        commit(&slot, "slice.txt", "committed but unmerged\n");
        let committed = rev(&slot);
        let error = acquire_slot(&home, &up.source, 1, "exec-2", None, &dead)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("commits beyond its synchronized base"),
            "{error}"
        );
        assert_eq!(rev(&slot), committed);
        assert!(slot.join("slice.txt").is_file());
        // An explicit discard is the lead's recorded way back into the pool.
        let disposition = release_slot(
            &home,
            &pool(&up.source, 1).unwrap(),
            1,
            SlotDisposition::Discarded,
            "slice rejected",
            &base,
            &dead,
        )
        .unwrap();
        assert_eq!(disposition, LaneDisposition::Reused { base: base.clone() });
        assert!(!slot.join("slice.txt").exists());
        let again = acquire_slot(&home, &up.source, 1, "exec-2", None, &dead).unwrap();
        assert_eq!(again.path, slot);
        assert_eq!(registered_count(&up.source), 2);
    }

    #[test]
    fn synchronization_never_resets_a_slot_holding_unreviewed_work() {
        let root = tempfile::tempdir().unwrap();
        let up = upstream(root.path());
        let home = root.path().join("home");
        let acquired = acquire_slot(&home, &up.source, 1, "exec-1", None, &|session: &str| {
            session == "exec-1"
        })
        .unwrap();
        fs::write(acquired.path.join("scratch.txt"), "unreviewed\n").unwrap();
        let error = synchronize_slot(&home, &pool(&up.source, 1).unwrap(), 1, None)
            .unwrap_err()
            .to_string();
        assert!(error.contains("unreviewed changes"), "{error}");
        assert!(acquired.path.join("scratch.txt").is_file());
        assert_eq!(rev(&acquired.path), acquired.base);
        // A recorded disposition is what authorizes destroying that work.
        let dead = |_: &str| false;
        let disposition = release_slot(
            &home,
            &pool(&up.source, 1).unwrap(),
            1,
            SlotDisposition::Discarded,
            "work explicitly rejected",
            &acquired.base,
            &dead,
        )
        .unwrap();
        assert_eq!(
            disposition,
            LaneDisposition::Reused {
                base: acquired.base.clone()
            }
        );
        assert!(!acquired.path.join("scratch.txt").exists());
    }

    #[test]
    fn lane_reset_reuses_the_checkout_and_keeps_ignored_build_caches() {
        let root = tempfile::tempdir().unwrap();
        let repo = repo(root.path());
        fs::write(repo.join(".gitignore"), "target/\n").unwrap();
        git_ok(&repo, &["add", ".gitignore"]);
        git_ok(&repo, &["commit", "-qm", "ignore build output"]);
        let lane = root.path().join("lane");
        git_ok(
            &repo,
            &[
                "worktree",
                "add",
                "--detach",
                lane.to_str().unwrap(),
                "HEAD",
            ],
        );
        fs::create_dir_all(lane.join("target")).unwrap();
        fs::write(lane.join("target/cache.bin"), "warm\n").unwrap();
        fs::write(lane.join("scratch.txt"), "leftover\n").unwrap();
        fs::write(lane.join("README.md"), "executor edit\n").unwrap();
        // The lead's accepted merge moves the shared checkout to the new base.
        fs::write(repo.join("merged.txt"), "accepted\n").unwrap();
        git_ok(&repo, &["add", "merged.txt"]);
        git_ok(&repo, &["commit", "-qm", "accepted merge"]);
        let base = rev(&repo);

        let mapping = Mapping {
            schema: 1,
            path: lane.clone(),
            source: repo.clone(),
            head: base.clone(),
            owner_thread: Some("exec-1".into()),
            archived: false,
            unavailable: false,
            remote_tui_omits_worktree_flag: true,
        };
        let disposition = reset_for_reuse(&mapping, &repo, &base).unwrap();
        assert_eq!(disposition, LaneDisposition::Reused { base: base.clone() });
        assert_eq!(rev(&lane), base);
        assert_eq!(
            fs::read(lane.join("target/cache.bin")).unwrap(),
            b"warm\n",
            "ignored build caches stay for the next lane task"
        );
        assert!(!lane.join("scratch.txt").exists());
        let readme = fs::read_to_string(lane.join("README.md")).unwrap();
        assert!(readme.contains("shared"), "{readme}");
        assert!(!readme.contains("executor edit"), "{readme}");
        let status = git(
            &lane,
            &["status", "--porcelain=v1", "--untracked-files=all"],
        )
        .unwrap();
        assert!(status.trim().is_empty(), "{status}");
        assert_eq!(
            audit(&repo).unwrap().total,
            2,
            "a reused lane adds no worktree"
        );
    }

    #[test]
    fn lane_reset_preserves_unresettable_state_for_retirement() {
        let root = tempfile::tempdir().unwrap();
        let repo = repo(root.path());
        let lane = root.path().join("lane");
        git_ok(
            &repo,
            &[
                "worktree",
                "add",
                "--detach",
                lane.to_str().unwrap(),
                "HEAD",
            ],
        );
        let mapping = Mapping {
            schema: 1,
            path: lane.clone(),
            source: repo.clone(),
            head: "HEAD".into(),
            owner_thread: Some("exec-1".into()),
            archived: false,
            unavailable: false,
            remote_tui_omits_worktree_flag: true,
        };
        fs::write(lane.join("scratch.txt"), "dirty\n").unwrap();
        match reset_for_reuse(&mapping, &repo, "not-a-commit").unwrap() {
            LaneDisposition::Preserved { limitation } => {
                assert!(
                    limitation.contains("committed base"),
                    "unavailable base must be explicit: {limitation}"
                );
            }
            LaneDisposition::Reused { .. } => panic!("unknown base must not reset the lane"),
        }
        assert!(lane.join("scratch.txt").exists());
        match reset_for_reuse(&mapping, &lane, "HEAD").unwrap() {
            LaneDisposition::Preserved { limitation } => {
                assert!(limitation.contains("current checkout"));
            }
            LaneDisposition::Reused { .. } => panic!("the current checkout must not reset"),
        }
        let archived = Mapping {
            archived: true,
            ..mapping.clone()
        };
        match reset_for_reuse(&archived, &repo, "HEAD").unwrap() {
            LaneDisposition::Preserved { limitation } => {
                assert!(limitation.contains("archive"));
            }
            LaneDisposition::Reused { .. } => panic!("archive is not lane reuse"),
        }
        let missing_path = Mapping {
            path: root.path().join("gone"),
            ..mapping
        };
        match reset_for_reuse(&missing_path, &repo, "HEAD").unwrap() {
            LaneDisposition::Preserved { limitation } => {
                assert!(limitation.contains("missing"));
            }
            LaneDisposition::Reused { .. } => panic!("a missing lane cannot be reused"),
        }
    }

    fn rev(cwd: &Path) -> String {
        git(cwd, &["rev-parse", "HEAD"]).unwrap().trim().to_owned()
    }
}
