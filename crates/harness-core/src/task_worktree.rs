//! Executor checkouts: the harness-owned worktree pool plus the `Mapping`
//! reader retained for per-thread worktree records written by older builds.
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
use sha2::{Digest, Sha256};
use std::{
    ffi::OsString,
    fs,
    io::{self, BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
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
}

/// Outcome of returning a lane worktree after an accepted merge. Lanes are
/// lane-owned: a successfully reset lane stays in place for the next task in
/// the same lane, keeping its ignored build caches; unresettable state and
/// lane retirement stay the lead's decision.
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

pub fn load(path: &Path) -> io::Result<Mapping> {
    serde_json::from_slice(&fs::read(path)?).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("worktree mapping: {error}"),
        )
    })
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
                        // The last owner stays recorded so the interrupted
                        // session can resume this exact slot; only the lead's
                        // release or a clean return to the pool clears it.
                        owner: existing.owner.clone(),
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

/// Rebind one exact slot to its interrupted session. Resume is the one
/// allocation path that must not resynchronize: partial work stays in the
/// tree, so a missing record or directory, a foreign source, a missing base,
/// a live owner or another owner's claim is refused instead of being repaired
/// by a reset. A clean slot whose owner was cleared back to the pool is
/// adopted for the explicitly named owner; a slot left awaiting review keeps
/// its interrupted owner recorded, so only that owner resumes it.
pub fn adopt_slot(
    codex_home: &Path,
    pool: &Pool,
    index: u32,
    owner: &str,
    is_live: &dyn Fn(&str) -> bool,
) -> io::Result<SlotRecord> {
    if owner.trim().is_empty() {
        return Err(pool_error(
            "a slot resume requires the executor session identity",
        ));
    }
    reconcile_slots(codex_home, pool, is_live)?;
    let slot = pool.slot(index)?;
    let record = load_slot_record(codex_home, &pool.source, index)?.ok_or_else(|| {
        pool_error(&format!(
            "slot {index} has no recorded binding to resume; dispatch executor spawn first"
        ))
    })?;
    if !same_path(&record.source, &pool.source) {
        return Err(pool_error(&format!(
            "slot record {index} belongs to another source checkout {}; the lead resolves it",
            record.source.display()
        )));
    }
    if !slot.path.is_dir() {
        return Err(pool_error(&format!(
            "slot {index} directory {} is missing; run `git worktree prune` in {} and retry",
            slot.path.display(),
            pool.source.display()
        )));
    }
    match record.owner.as_deref() {
        Some(existing) if existing == owner => (),
        Some(other) => {
            return Err(pool_error(&format!(
                "slot {index} is bound to session {other} instead of {owner}; resume that owner's session or release the slot instead of sharing one checkout"
            )));
        }
        None => (),
    }
    if record.owner.as_deref().is_some_and(is_live) {
        return Err(pool_error(&format!(
            "session {owner} is already live in slot {index}; stop it before resuming"
        )));
    }
    let base = record
        .base
        .clone()
        .filter(|base| !base.trim().is_empty())
        .ok_or_else(|| {
            pool_error(&format!(
                "slot {index} has no synchronized base to resume; dispatch executor spawn first"
            ))
        })?;
    let bound = SlotRecord {
        state: SlotState::Occupied,
        owner: Some(owner.to_owned()),
        base: Some(base),
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
/// pool without an owner, while a slot holding work becomes awaiting review
/// with its reason and its interrupted owner still recorded for resume.
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
                // Preserve the interrupted session's identity for resume.
                owner: record.owner.clone(),
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
/// `origin` when present, otherwise the only configured remote. Resume reads
/// the same pair for its slot binding without resynchronizing the tree.
pub fn upstream(path: &Path) -> io::Result<(String, Option<String>)> {
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

// ---------------------------------------------------------------------------
// Experiment checkouts: frozen task copies and owned candidate worktrees.
//
// These additive operations serve the improvement experiment consumer. A
// frozen task copy is an independent minimal repository materialized from an
// exact committed tree: linked worktrees share objects and references and
// therefore cannot hide a sibling solution. A candidate checkout is a
// dedicated branch in its own worktree. The reuse verdict is read-only: it
// never adopts, resets or deletes state it did not create.
// ---------------------------------------------------------------------------

const FROZEN_COMMIT_MESSAGE: &str = "frozen task snapshot";
const FROZEN_IDENTITY_NAME: &str = "frozen-task";
const FROZEN_IDENTITY_EMAIL: &str = "frozen-task@invalid";

/// An independent minimal repository holding exactly the committed tree of
/// one task revision. It has no remote, no parent history and no reference to
/// the source checkout, so a sibling solution kept in the source repository is
/// not discoverable from the copy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FrozenCopy {
    pub source: PathBuf,
    /// Resolved commit in the source repository.
    pub source_revision: String,
    pub path: PathBuf,
    /// Root commit created in the copy; identical across copies of one
    /// revision that share identity and dates.
    pub revision: String,
    /// Tree object recorded by the root commit.
    pub tree: String,
    /// Content digest over the snapshot's tree entries (`mode`, `object`,
    /// `path`), independent of repository or commit identity.
    pub tree_sha256: String,
}

/// A dedicated candidate branch in its own owned worktree, bound to an
/// explicit accepted base revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CandidateCheckout {
    pub source: PathBuf,
    pub path: PathBuf,
    pub branch: String,
    pub base: String,
    pub revision: String,
}

/// Why an existing worktree cannot be reused. Every refusal leaves the
/// worktree exactly as found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ReuseBlock {
    /// The source checkout itself or the running session's checkout.
    CurrentCheckout,
    Missing,
    /// Not a registered worktree of the source repository.
    Foreign,
    /// An active experiment attempt or a Git operation owns the tree.
    Busy,
    /// Local, untracked or unmerged work would be lost by reuse.
    Unpreserved,
}

/// Read-only reuse verdict for an existing allocation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WorktreeReuse {
    Eligible { revision: String },
    Blocked { kind: ReuseBlock, reason: String },
}

struct TreeEntry {
    mode: String,
    object: String,
    path: Vec<u8>,
}

fn git_bytes(cwd: &Path, args: &[&str]) -> io::Result<Vec<u8>> {
    let out = Command::new(git_program())
        .args(args)
        .current_dir(cwd)
        .output()?;
    if out.status.success() {
        Ok(out.stdout)
    } else {
        Err(io::Error::other(format!(
            "git {}: {}",
            args.first().unwrap_or(&"git"),
            String::from_utf8_lossy(&out.stderr).trim()
        )))
    }
}

fn frozen_error(detail: impl std::fmt::Display) -> io::Error {
    pool_error(&format!("frozen task copy: {detail}"))
}

/// Refuse paths a Git tree cannot represent on an ordinary filesystem: no
/// absolute prefix, no drive or backslash component, no `.`/`..` traversal.
fn frozen_relative(path: &[u8]) -> io::Result<String> {
    let text = std::str::from_utf8(path)
        .map_err(|_| frozen_error("a tree path is not Unicode; refusing an inexact copy"))?;
    if text.is_empty()
        || text.starts_with('/')
        || text.contains('\\')
        || text.contains(':')
        || text.contains('\0')
        || !text
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
    {
        return Err(frozen_error(format!(
            "unsupported tree path '{text}'; refusing an inexact copy"
        )));
    }
    Ok(text.to_owned())
}

/// The committed tree of `revision`, refusing entries the copy cannot
/// represent exactly (submodules, symbolic links).
fn frozen_tree_entries(repo: &Path, revision: &str) -> io::Result<Vec<TreeEntry>> {
    let raw = git_bytes(repo, &["ls-tree", "-r", "-z", revision])?;
    let mut entries = Vec::new();
    for record in raw.split(|byte| *byte == 0) {
        if record.is_empty() {
            continue;
        }
        let tab = record
            .iter()
            .position(|byte| *byte == b'\t')
            .ok_or_else(|| frozen_error("unreadable tree entry"))?;
        let meta = std::str::from_utf8(&record[..tab])
            .map_err(|_| frozen_error("unreadable tree entry"))?;
        let mut fields = meta.split(' ');
        let mode = fields.next().unwrap_or_default();
        let kind = fields.next().unwrap_or_default();
        let object = fields.next().unwrap_or_default();
        if mode == "160000" {
            return Err(frozen_error(
                "the frozen revision contains a submodule; materialize it explicitly instead of an incomplete copy",
            ));
        }
        if mode == "120000" {
            return Err(frozen_error(
                "the frozen revision contains a symbolic link; this platform cannot represent it exactly",
            ));
        }
        if kind != "blob" || !matches!(mode, "100644" | "100755") {
            return Err(frozen_error(format!(
                "unsupported tree entry ({mode} {kind}) in the frozen revision"
            )));
        }
        let path = record[tab + 1..].to_vec();
        frozen_relative(&path)?;
        entries.push(TreeEntry {
            mode: mode.to_owned(),
            object: object.to_owned(),
            path,
        });
    }
    Ok(entries)
}

/// Content digest over the snapshot's tree entries; independent of repository
/// identity, commit metadata and working-tree state.
fn frozen_tree_digest(entries: &[(String, String, String)]) -> String {
    let mut hasher = Sha256::new();
    for (mode, object, path) in entries {
        hasher.update(mode.as_bytes());
        hasher.update(b" ");
        hasher.update(object.as_bytes());
        hasher.update(b"\0");
        hasher.update(path.as_bytes());
        hasher.update(b"\0");
    }
    format!("{:x}", hasher.finalize())
}

fn frozen_entries_digest(entries: &[TreeEntry]) -> String {
    let plain: Vec<(String, String, String)> = entries
        .iter()
        .map(|entry| {
            (
                entry.mode.clone(),
                entry.object.clone(),
                String::from_utf8_lossy(&entry.path).into_owned(),
            )
        })
        .collect();
    frozen_tree_digest(&plain)
}

/// Materialize every blob of the committed tree into `target` exactly as the
/// object database stores it, without working-tree filters.
fn frozen_materialize(
    repo: &Path,
    entries: &[TreeEntry],
    target: &Path,
) -> io::Result<Vec<(String, String)>> {
    let mut child = Command::new(git_program())
        .args(["cat-file", "--batch"])
        .current_dir(repo)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| frozen_error("git cat-file stdin was not captured"))?;
    let mut stdout = BufReader::new(
        child
            .stdout
            .take()
            .ok_or_else(|| frozen_error("git cat-file stdout was not captured"))?,
    );
    let result = (|| -> io::Result<Vec<(String, String)>> {
        let mut written = Vec::new();
        for entry in entries {
            writeln!(stdin, "{}", entry.object)?;
            let mut header = String::new();
            if stdout.read_line(&mut header)? == 0 {
                return Err(frozen_error("git cat-file ended before the tree was read"));
            }
            let mut fields = header.trim_end().split(' ');
            let object = fields.next().unwrap_or_default();
            let kind = fields.next().unwrap_or_default();
            let size: u64 = fields
                .next()
                .and_then(|size| size.parse().ok())
                .ok_or_else(|| frozen_error("git cat-file returned an unreadable entry"))?;
            if object != entry.object || kind != "blob" {
                return Err(frozen_error(
                    "git cat-file did not return the expected blob",
                ));
            }
            let relative = frozen_relative(&entry.path)?;
            let file = target.join(&relative);
            if let Some(parent) = file.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut file = fs::File::create_new(&file)?;
            let mut remaining = size;
            let mut buffer = [0_u8; 64 * 1024];
            while remaining > 0 {
                let want = remaining.min(buffer.len() as u64) as usize;
                let read = stdout.read(&mut buffer[..want])?;
                if read == 0 {
                    return Err(frozen_error("git cat-file ended inside a blob"));
                }
                file.write_all(&buffer[..read])?;
                remaining -= read as u64;
            }
            let mut newline = [0_u8; 1];
            stdout.read_exact(&mut newline)?;
            if newline[0] != b'\n' {
                return Err(frozen_error("git cat-file framing is inconsistent"));
            }
            file.sync_all()?;
            written.push((relative, entry.object.clone()));
        }
        Ok(written)
    })();
    drop(stdin);
    if result.is_err() {
        let _ = child.kill();
    }
    let status = child.wait()?;
    if result.is_ok() && !status.success() {
        let mut stderr = String::new();
        if let Some(mut pipe) = child.stderr.take() {
            let _ = pipe.read_to_string(&mut stderr);
        }
        return Err(frozen_error(format!(
            "git cat-file failed: {}",
            stderr.trim()
        )));
    }
    result
}

fn git_ok(cwd: &Path, args: &[&str]) -> io::Result<()> {
    git(cwd, args).map(|_| ())
}

/// Create the copy's root commit from the materialized tree through plumbing
/// only, so no attribute, filter or template can alter the frozen content.
fn frozen_commit(
    target: &Path,
    tree: &str,
    entries: &[TreeEntry],
    files: &[(String, String)],
    date: &str,
) -> io::Result<String> {
    git_ok(target, &["init", "-q", "-b", "main"])?;
    for (key, value) in [
        ("user.name", FROZEN_IDENTITY_NAME),
        ("user.email", FROZEN_IDENTITY_EMAIL),
        ("core.autocrlf", "false"),
        ("core.safecrlf", "false"),
        ("commit.gpgsign", "false"),
    ] {
        git_ok(target, &["config", key, value])?;
    }
    let mut hash_object = Command::new(git_program())
        .args(["hash-object", "-w", "--stdin-paths", "--no-filters"])
        .current_dir(target)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    {
        let mut stdin = hash_object
            .stdin
            .take()
            .ok_or_else(|| frozen_error("git hash-object stdin was not captured"))?;
        for (relative, _) in files {
            writeln!(stdin, "{relative}")?;
        }
    }
    let output = hash_object.wait_with_output()?;
    if !output.status.success() {
        return Err(frozen_error(format!(
            "git hash-object failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let hashed: Vec<&str> = std::str::from_utf8(&output.stdout)
        .map_err(|_| frozen_error("git hash-object returned non-Unicode output"))?
        .lines()
        .collect();
    if hashed.len() != files.len() {
        return Err(frozen_error("git hash-object returned an unexpected count"));
    }
    for ((relative, expected), actual) in files.iter().zip(hashed) {
        if actual.trim() != expected {
            return Err(frozen_error(format!(
                "materialized content of '{relative}' does not match the committed blob"
            )));
        }
    }
    let mut index_info = Vec::new();
    for entry in entries {
        index_info.extend_from_slice(entry.mode.as_bytes());
        index_info.push(b' ');
        index_info.extend_from_slice(entry.object.as_bytes());
        index_info.push(b'\t');
        index_info.extend_from_slice(&entry.path);
        index_info.push(b'\n');
    }
    let mut update = Command::new(git_program())
        .args(["update-index", "--index-info"])
        .current_dir(target)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    {
        let mut stdin = update
            .stdin
            .take()
            .ok_or_else(|| frozen_error("git update-index stdin was not captured"))?;
        stdin.write_all(&index_info)?;
    }
    let output = update.wait_with_output()?;
    if !output.status.success() {
        return Err(frozen_error(format!(
            "git update-index failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let written_tree = git(target, &["write-tree"])?.trim().to_owned();
    if written_tree != tree {
        return Err(frozen_error(
            "the copy's tree does not match the frozen revision's tree",
        ));
    }
    let revision = {
        let out = Command::new(git_program())
            .args(["commit-tree", tree, "-m", FROZEN_COMMIT_MESSAGE])
            .current_dir(target)
            .env("GIT_AUTHOR_NAME", FROZEN_IDENTITY_NAME)
            .env("GIT_AUTHOR_EMAIL", FROZEN_IDENTITY_EMAIL)
            .env("GIT_COMMITTER_NAME", FROZEN_IDENTITY_NAME)
            .env("GIT_COMMITTER_EMAIL", FROZEN_IDENTITY_EMAIL)
            .env("GIT_AUTHOR_DATE", date)
            .env("GIT_COMMITTER_DATE", date)
            .output()?;
        if !out.status.success() {
            return Err(frozen_error(format!(
                "git commit-tree failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    };
    git_ok(target, &["update-ref", "refs/heads/main", &revision])?;
    git_ok(target, &["symbolic-ref", "HEAD", "refs/heads/main"])?;
    git_ok(target, &["read-tree", "--reset", "HEAD"])?;
    Ok(revision)
}

/// Materialize an independent minimal repository containing exactly the
/// committed tree of `revision` in `source`. The target must not exist: the
/// copy never merges into, adopts or overwrites existing state.
pub fn frozen_copy(source: &Path, revision: &str, target: &Path) -> io::Result<FrozenCopy> {
    let source = resolve_source(source)?;
    if !is_git_checkout(&source)? {
        return Err(frozen_error(
            "a frozen task copy requires a Git source checkout",
        ));
    }
    let absolute = std::path::absolute(target)?;
    if absolute.exists() {
        return Err(frozen_error(format!(
            "{} already exists; refusing to merge or overwrite it",
            absolute.display()
        )));
    }
    let commit = resolve_commit(&source, revision)?;
    let entries = frozen_tree_entries(&source, &commit)?;
    let tree = git(&source, &["rev-parse", &format!("{commit}^{{tree}}")])?
        .trim()
        .to_owned();
    let tree_sha256 = frozen_entries_digest(&entries);
    let date = git(&source, &["show", "-s", "--format=%cI", &commit])?
        .trim()
        .to_owned();
    if let Some(parent) = absolute.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::create_dir(&absolute)?;
    let populated = (|| -> io::Result<String> {
        let files = frozen_materialize(&source, &entries, &absolute)?;
        frozen_commit(&absolute, &tree, &entries, &files, &date)
    })();
    let revision = match populated {
        Ok(revision) => revision,
        Err(error) => {
            // The target was exclusively created by this call; a partial copy
            // is removed instead of being left as a half-materialized task.
            let _ = fs::remove_dir_all(&absolute);
            return Err(error);
        }
    };
    let single = git(&absolute, &["rev-list", "--all", "--count"])?
        .trim()
        .to_owned();
    let remotes = git(&absolute, &["remote"])?;
    if single != "1" || !remotes.trim().is_empty() {
        let _ = fs::remove_dir_all(&absolute);
        return Err(frozen_error(
            "the materialized copy is not an independent single-commit repository",
        ));
    }
    Ok(FrozenCopy {
        source,
        source_revision: commit,
        path: absolute,
        revision,
        tree,
        tree_sha256,
    })
}

/// Re-verify a frozen copy from its own repository content: the recorded root
/// commit, tree and content digest still match, the repository has no remote
/// and no shared parent history. Working-tree edits made by a task executor do
/// not change the verified snapshot.
pub fn verify_frozen(copy: &FrozenCopy) -> io::Result<()> {
    if !copy.path.is_dir() {
        return Err(frozen_error(format!("{} is missing", copy.path.display())));
    }
    let parents = git(
        &copy.path,
        &["rev-list", "--parents", "-n", "1", &copy.revision],
    )?;
    if parents.split_whitespace().count() != 1 {
        return Err(frozen_error(
            "the frozen revision has parent history; it is not an independent copy",
        ));
    }
    let tree = git(
        &copy.path,
        &["rev-parse", &format!("{}^{{tree}}", copy.revision)],
    )?;
    if tree.trim() != copy.tree {
        return Err(frozen_error("the frozen revision's tree changed"));
    }
    if !git(&copy.path, &["remote"])?.trim().is_empty() {
        return Err(frozen_error(
            "the frozen copy has a remote; sibling history may be reachable",
        ));
    }
    let refs = git(&copy.path, &["for-each-ref", "--format=%(refname)"])?;
    for reference in refs.lines().map(str::trim).filter(|line| !line.is_empty()) {
        if !reference.starts_with("refs/heads/") && !reference.starts_with("refs/tags/") {
            return Err(frozen_error(format!(
                "the frozen copy holds foreign references ({reference})"
            )));
        }
    }
    let entries = frozen_tree_entries(&copy.path, &copy.revision)?;
    if frozen_entries_digest(&entries) != copy.tree_sha256 {
        return Err(frozen_error("the frozen content digest changed"));
    }
    Ok(())
}

/// Pre-attempt gate for a frozen copy: the snapshot identity still verifies and
/// the copy is pristine — no prior solution edits or untracked artifacts, no
/// extra or moved references, no unreachable sibling objects and no alternate
/// or shared object database. The check is read-only: contaminated state is
/// reported with its cause and preserved, never cleaned or reset. Use
/// [`verify_frozen`] for post-attempt snapshot checks after an executor has
/// legitimately worked in the copy.
pub fn verify_frozen_pristine(copy: &FrozenCopy) -> io::Result<()> {
    verify_frozen(copy)?;
    let git_dir = copy.path.join(".git");
    if !git_dir.is_dir() {
        return Err(frozen_error(
            "the copy's Git directory is not an independent repository",
        ));
    }
    if git_dir.join("commondir").exists() {
        return Err(frozen_error(
            "the copy shares a common Git directory; sibling history may be reachable",
        ));
    }
    let alternates = git(
        &copy.path,
        &["rev-parse", "--git-path", "objects/info/alternates"],
    )?;
    let alternates = PathBuf::from(alternates.trim());
    let alternates = if alternates.is_absolute() {
        alternates
    } else {
        copy.path.join(alternates)
    };
    if alternates.exists() {
        return Err(frozen_error(
            "the copy shares an alternate object store; sibling history may be reachable",
        ));
    }
    let refs = git(&copy.path, &["for-each-ref", "--format=%(refname)"])?;
    let refs: Vec<&str> = refs
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    if refs != ["refs/heads/main"] {
        return Err(frozen_error(
            "the copy holds extra or missing references; a pre-attempt copy has exactly its frozen branch",
        ));
    }
    let branch = git(&copy.path, &["rev-parse", "refs/heads/main"])?
        .trim()
        .to_owned();
    if branch != copy.revision {
        return Err(frozen_error(
            "the frozen branch moved off the frozen revision; prior work is preserved but the copy is not pristine",
        ));
    }
    let head = git(&copy.path, &["rev-parse", "HEAD"])?.trim().to_owned();
    if head != copy.revision {
        return Err(frozen_error(
            "HEAD is not the frozen revision; a prior checkout or solution is present",
        ));
    }
    let status = git(
        &copy.path,
        &["status", "--porcelain=v1", "--untracked-files=all"],
    )?;
    if !status.trim().is_empty() {
        return Err(frozen_error(
            "the working tree holds edits or artifacts from an earlier solution; they are preserved",
        ));
    }
    let fsck = Command::new(git_program())
        .args(["fsck", "--unreachable", "--no-reflogs", "--no-progress"])
        .current_dir(&copy.path)
        .output()?;
    let stdout = String::from_utf8_lossy(&fsck.stdout);
    let stderr = String::from_utf8_lossy(&fsck.stderr);
    for line in stdout.lines().chain(stderr.lines()) {
        let line = line.trim();
        if line.starts_with("unreachable ") || line.starts_with("dangling ") {
            return Err(frozen_error(format!(
                "the copy holds Git objects outside the frozen revision ({line}); reconstruct it instead of reusing contaminated state"
            )));
        }
    }
    if !fsck.status.success() {
        return Err(frozen_error(format!(
            "the copy does not pass Git integrity verification: {}",
            stderr.trim()
        )));
    }
    Ok(())
}

/// Read-only reuse verdict for an existing worktree allocation. `current` is
/// the checkout the running session executes from; `active` reports an active
/// experiment attempt on the candidate. Refusals never touch the tree.
pub fn worktree_reuse(
    source: &Path,
    path: &Path,
    current: &Path,
    expected_revision: &str,
    active: bool,
) -> io::Result<WorktreeReuse> {
    let source = resolve_source(source)?;
    let absolute = std::path::absolute(path)?;
    let blocked = |kind: ReuseBlock, reason: String| Ok(WorktreeReuse::Blocked { kind, reason });
    if same_path(&absolute, &source) || same_path(&absolute, current) {
        return blocked(
            ReuseBlock::CurrentCheckout,
            format!(
                "{} is the current checkout; reuse is refused",
                absolute.display()
            ),
        );
    }
    if !absolute.is_dir() {
        return blocked(
            ReuseBlock::Missing,
            format!("{} does not exist", absolute.display()),
        );
    }
    if !is_git_checkout(&absolute)? {
        return blocked(
            ReuseBlock::Foreign,
            format!("{} is not a Git checkout", absolute.display()),
        );
    }
    let registered = registered_trees(&source)?;
    if !registered
        .iter()
        .any(|tree| same_path(&tree.path, &absolute))
    {
        return blocked(
            ReuseBlock::Foreign,
            format!(
                "{} is not a registered worktree of {}",
                absolute.display(),
                source.display()
            ),
        );
    }
    if active {
        return blocked(
            ReuseBlock::Busy,
            "an experiment attempt is active in this worktree; it keeps its allocation".to_owned(),
        );
    }
    let lock = git(&absolute, &["rev-parse", "--git-path", "index.lock"])?;
    let lock = PathBuf::from(lock.trim());
    let lock = if lock.is_absolute() {
        lock
    } else {
        absolute.join(lock)
    };
    if lock.exists() {
        return blocked(
            ReuseBlock::Busy,
            "a Git operation is in progress (index.lock exists); reuse is refused".to_owned(),
        );
    }
    if !slot_status(&absolute)?.trim().is_empty() {
        return blocked(
            ReuseBlock::Unpreserved,
            "the worktree holds local or untracked changes; they are preserved".to_owned(),
        );
    }
    let head = git(&absolute, &["rev-parse", "HEAD"])?.trim().to_owned();
    let expected = resolve_commit(&absolute, expected_revision)?;
    if head != expected {
        return blocked(
            ReuseBlock::Unpreserved,
            format!(
                "the worktree holds commits beyond the recorded revision {expected_revision}; they are preserved"
            ),
        );
    }
    Ok(WorktreeReuse::Eligible { revision: head })
}

/// Allocate the dedicated candidate branch in a new owned worktree from an
/// explicit accepted base. An existing path or branch is another task's state
/// and is never adopted, reset or reused here.
pub fn allocate_candidate_checkout(
    source: &Path,
    path: &Path,
    branch: &str,
    base: &str,
) -> io::Result<CandidateCheckout> {
    let source = resolve_source(source)?;
    if !is_git_checkout(&source)? {
        return Err(pool_error(
            "candidate worktrees require a Git source checkout",
        ));
    }
    let branch = branch.trim();
    if branch.is_empty()
        || branch.len() > 200
        || branch.starts_with('-')
        || branch.contains(char::is_whitespace)
    {
        return Err(pool_error(
            "a candidate branch name is required and must be a plain Git branch name",
        ));
    }
    let absolute = std::path::absolute(path)?;
    if absolute.exists() {
        return Err(pool_error(&format!(
            "{} already exists; allocation refuses to adopt or reset an existing path",
            absolute.display()
        )));
    }
    if git(
        &source,
        &["rev-parse", "--verify", &format!("refs/heads/{branch}")],
    )
    .is_ok()
    {
        return Err(pool_error(&format!(
            "branch {branch} already exists; refusing to reuse another task's branch"
        )));
    }
    let commit = resolve_commit(&source, base)?;
    if let Some(parent) = absolute.parent() {
        fs::create_dir_all(parent)?;
    }
    let path_text = absolute
        .to_str()
        .ok_or_else(|| pool_error("candidate worktree path must be Unicode"))?;
    let out = Command::new(git_program())
        .args(["worktree", "add", "-b", branch, path_text, &commit])
        .current_dir(&source)
        .output()?;
    if !out.status.success() {
        return Err(pool_error(&format!(
            "candidate worktree could not be created: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    let revision = git(&absolute, &["rev-parse", "HEAD"])?.trim().to_owned();
    let head_branch = git(&absolute, &["rev-parse", "--abbrev-ref", "HEAD"])?
        .trim()
        .to_owned();
    if revision != commit || head_branch != branch {
        return Err(pool_error(
            "the new candidate worktree did not land on its branch and base",
        ));
    }
    Ok(CandidateCheckout {
        source,
        path: absolute,
        branch: branch.to_owned(),
        base: commit.clone(),
        revision: commit,
    })
}

/// Verify a candidate checkout binding: the worktree is registered to its
/// source repository, is on its dedicated branch and still records the exact
/// committed revision. Uncommitted work in the tree is not a binding failure.
pub fn verify_candidate_checkout(checkout: &CandidateCheckout) -> io::Result<()> {
    if !checkout.path.is_dir() {
        return Err(pool_error(&format!(
            "candidate worktree {} is missing",
            checkout.path.display()
        )));
    }
    let head = git(&checkout.path, &["rev-parse", "HEAD"])?
        .trim()
        .to_owned();
    if head != checkout.revision {
        return Err(pool_error(&format!(
            "candidate worktree is at {head} instead of the bound revision {}",
            checkout.revision
        )));
    }
    let branch = git(
        &checkout.path,
        &["rev-parse", &format!("refs/heads/{}", checkout.branch)],
    )?
    .trim()
    .to_owned();
    if branch != checkout.revision {
        return Err(pool_error(
            "the candidate branch does not point at the bound revision",
        ));
    }
    let registered = registered_trees(&checkout.source)?;
    if !registered
        .iter()
        .any(|tree| same_path(&tree.path, &checkout.path))
    {
        return Err(pool_error(
            "the candidate worktree is not registered to its source repository",
        ));
    }
    Ok(())
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
    fn legacy_mapping_records_load_without_the_retired_remote_tui_flag() {
        let root = tempfile::tempdir().unwrap();
        let lane = root.path().join("lane");
        let path = root.path().join("executor-worktree.json");
        fs::write(
            &path,
            format!(
                "{{\"schema\":1,\"path\":{},\"source\":{},\"head\":\"abc\",\"ownerThread\":\"exec-1\",\"archived\":false,\"unavailable\":false,\"remoteTuiOmitsWorktreeFlag\":true}}",
                serde_json::to_string(&lane).unwrap(),
                serde_json::to_string(root.path()).unwrap(),
            ),
        )
        .unwrap();
        let mapping = load(&path).unwrap();
        assert_eq!(mapping.path, lane);
        assert_eq!(mapping.owner_thread.as_deref(), Some("exec-1"));
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
    fn adopt_slot_resumes_without_reset_and_refuses_foreign_or_live_owners() {
        let root = tempfile::tempdir().unwrap();
        let up = upstream(root.path());
        let home = root.path().join("home");
        let layout = pool(&up.source, 1).unwrap();
        let dead = |_: &str| false;
        let acquired = acquire_slot(&home, &up.source, 1, "exec-1", None, &dead).unwrap();
        // The interrupted session leaves partial work in its slot.
        let partial = acquired.path.join("partial.txt");
        fs::write(&partial, "partial work\n").unwrap();
        let head = rev(&acquired.path);
        // Reconciliation keeps the interrupted owner recorded on the dirty
        // slot; adoption rebinds it without fetch, reset or clean.
        let adopted = adopt_slot(&home, &layout, 1, "exec-1", &dead).unwrap();
        assert_eq!(adopted.state, SlotState::Occupied);
        assert_eq!(adopted.owner.as_deref(), Some("exec-1"));
        assert_eq!(adopted.base.as_deref(), Some(acquired.base.as_str()));
        assert!(partial.is_file(), "resume must not clean partial work");
        assert_eq!(rev(&acquired.path), head);
        // The same owner resumes again after another interruption.
        assert!(adopt_slot(&home, &layout, 1, "exec-1", &dead).is_ok());
        assert!(partial.is_file());
        // Another owner's claim is refused with both identities named.
        let error = adopt_slot(&home, &layout, 1, "exec-2", &dead)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("bound to session exec-1 instead of exec-2"),
            "{error}"
        );
        assert!(partial.is_file());
        // A live owner never resumes over its own running host.
        let live = |session: &str| session == "exec-1";
        let error = adopt_slot(&home, &layout, 1, "exec-1", &live)
            .unwrap_err()
            .to_string();
        assert!(error.contains("already live"), "{error}");
        // A slot without a record has nothing to resume.
        let wider = pool(&up.source, 2).unwrap();
        let error = adopt_slot(&home, &wider, 2, "exec-1", &dead)
            .unwrap_err()
            .to_string();
        assert!(error.contains("no recorded binding to resume"), "{error}");
        let error = adopt_slot(&home, &layout, 1, "", &dead).unwrap_err();
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
