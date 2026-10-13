//! Managed task checkouts: the harness-owned worktree pool shared by every
//! isolated task in one repository, plus the `Mapping` reader retained for
//! per-thread worktree records written by older builds.
//!
//! One repository has one pool, whether a caller enters through the main
//! checkout or any linked worktree: the pool identity is the Git common
//! directory, and the sibling layout `<repo-name>-wt1` .. `<repo-name>-wtN`
//! belongs to the primary worktree. Capacity comes from the independent
//! orchestration `worktree_pool_size`; positions are created lazily and never
//! beyond the configured range. A slot can be bound to a durable task identity
//! with a non-reusable generation handle, and that binding survives process
//! exit, suspension and review until an explicit release resets the slot to a
//! verified committed base while keeping ignored build caches. Preparing a
//! free slot resolves an exact committed base: an explicit local base is
//! resolved without fetching, while the executor default still refreshes its
//! upstream first. Slot claims use exclusive file creation, so a lost race
//! surfaces as an occupied-slot refusal instead of two sessions in one tree. A
//! startup reservation keeps that claim exclusive until a host lease exists.
//! Reconciliation frees only a resolved-dead reservation, or a
//! reservation-less session slot whose recorded session is not live; a slot
//! holding unreviewed work, and any slot holding a task binding, stays held
//! until the lead merges it, records an explicit discard or releases the task.
use crate::build_identity::hash_bytes;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
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

/// Largest supported pool; matches the orchestration `worktree_pool_size`
/// bound, so a configured value is always a valid pool size.
pub const MAX_POOL_SLOTS: u32 = 32;
const SLOT_STATE_SCHEMA: u32 = 2;
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
    /// Something exists at the position that is not a worktree of this
    /// repository; it is reported for lead review, never adopted or replaced.
    Foreign,
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
    /// Layout root that owns the sibling slots: the primary worktree.
    pub source: PathBuf,
    /// Checkout the caller entered through; equal to `source` for the main
    /// checkout, so legacy per-path records of this caller stay visible.
    pub caller: PathBuf,
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

/// One repository's pool identity: the Git common directory shared by every
/// linked worktree, the primary worktree root that owns the sibling slot
/// layout, and the layout name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryIdentity {
    pub common_dir: PathBuf,
    pub primary_root: PathBuf,
    pub name: String,
}

/// Resolve the pool identity of a checkout or any linked worktree of it.
/// Entering through the main checkout or a linked worktree yields the same
/// identity, so both see one pool inventory and one slot-state directory.
pub fn repository_identity(source: &Path) -> io::Result<RepositoryIdentity> {
    let checkout = resolve_source(source)?;
    if !is_git_checkout(&checkout)? {
        return Err(pool_error(&format!(
            "{} is not a Git checkout; a repository pool needs a Git worktree",
            checkout.display()
        )));
    }
    let common = git(&checkout, &["rev-parse", "--git-common-dir"])?;
    let common = native_path(common.trim());
    let common = if common.is_absolute() {
        common
    } else {
        checkout.join(common)
    };
    let common_dir = resolve_source(&common).map_err(|_| {
        pool_error(&format!(
            "the Git common directory of {} cannot be resolved; the repository identity is unsupported",
            checkout.display()
        ))
    })?;
    let primary_root = primary_worktree(&checkout).unwrap_or_else(|| checkout.clone());
    let name = primary_root
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_owned)
        .ok_or_else(|| pool_error("repository name must be unicode"))?;
    Ok(RepositoryIdentity {
        common_dir,
        primary_root,
        name,
    })
}

/// First registered worktree of the repository: the primary checkout whose
/// parent owns the sibling slot layout.
fn primary_worktree(checkout: &Path) -> Option<PathBuf> {
    let listed = git(checkout, &["worktree", "list", "--porcelain"]).ok()?;
    let first = listed
        .lines()
        .find_map(|line| line.strip_prefix("worktree "))?;
    resolve_source(&native_path(first)).ok()
}

/// Layout root of one repository: the primary worktree when its Git identity
/// resolves, otherwise the supplied checkout itself so non-repository callers
/// keep a meaningful error instead of a cross-repository guess.
pub fn pool_root(source: &Path) -> io::Result<PathBuf> {
    if let Ok(identity) = repository_identity(source) {
        return Ok(identity.primary_root);
    }
    resolve_source(source)
}

/// Shared pool-size validation; the bound matches the orchestration schema.
fn validate_pool_size(size: u32) -> io::Result<()> {
    if size == 0 || size > MAX_POOL_SLOTS {
        return Err(pool_error(&format!(
            "pool size {size} is out of range 1..={MAX_POOL_SLOTS}; it derives from the configured worktree_pool_size"
        )));
    }
    Ok(())
}

/// Resolve the pool of a repository from any of its checkouts. A pool
/// position occupied by anything that is not a registered worktree of this
/// repository - an unrelated directory or another repository's tree - is
/// refused instead of adopted or replaced. Positions beyond the configured
/// size are never created, registered or reported as pool slots.
pub fn pool(source: &Path, size: u32) -> io::Result<Pool> {
    validate_pool_size(size)?;
    let caller = resolve_source(source)?;
    let identity = repository_identity(&caller)?;
    let source = identity.primary_root;
    let registered = registered_trees(&source)?;
    let mut slots = Vec::new();
    for index in 1..=size {
        let path = slot_path(&source, index)?;
        let presence = match slot_presence(&path, &registered) {
            SlotPresence::Foreign => {
                return Err(pool_error(&format!(
                    "slot {index} {} exists but is not a registered worktree of {}; refusing to adopt or replace a foreign path",
                    path.display(),
                    source.display()
                )));
            }
            presence => presence,
        };
        slots.push(PoolSlot {
            index,
            path,
            presence,
        });
    }
    Ok(Pool {
        source,
        caller,
        size,
        slots,
    })
}

/// Registry position of one pool path.
enum SlotPosition {
    Registered,
    RegisteredMissing,
    /// Something exists at the position, but it is not a worktree of this
    /// repository; it is never adopted, replaced or deleted.
    Foreign,
    Absent,
}

fn classify_slot(path: &Path, registered: &[RegisteredTree]) -> SlotPosition {
    match registered.iter().find(|tree| same_path(&tree.path, path)) {
        Some(tree) if tree.prunable || !path.is_dir() => SlotPosition::RegisteredMissing,
        Some(_) => SlotPosition::Registered,
        None if path.exists() => SlotPosition::Foreign,
        None => SlotPosition::Absent,
    }
}

/// Presence of one pool position, including the foreign collision that `pool`
/// refuses.
fn slot_presence(path: &Path, registered: &[RegisteredTree]) -> SlotPresence {
    match classify_slot(path, registered) {
        SlotPosition::Registered => SlotPresence::Registered,
        SlotPosition::RegisteredMissing => SlotPresence::RegisteredMissing,
        SlotPosition::Foreign => SlotPresence::Foreign,
        SlotPosition::Absent => SlotPresence::Absent,
    }
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
        SlotPresence::Foreign => Err(pool_error(&format!(
            "slot {index} {} exists but is not a worktree of {}; it is never replaced, move it aside or reconfigure the pool",
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

/// Creating process of one dispatch, recorded before a host lease exists.
///
/// Pid reuse is distinguished by the process creation time. A missing
/// reservation means the record predates this boundary and still follows
/// session liveness alone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupReservation {
    pub pid: u32,
    pub created: u64,
    pub program: PathBuf,
}

/// Whether a startup reservation still excludes other dispatches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReservationLiveness {
    /// The creating process is the recorded identity and is running.
    Live,
    /// The recorded process has exited or its pid was reused.
    Dead,
    /// Identity could not be resolved. The claim stays exclusive.
    Uncertain,
}

/// Kit-local task-state record of one pool slot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SlotRecord {
    // Legacy readers require `schema` and ignore unknown fields. Renaming the
    // required discriminator makes them refuse these records instead of
    // treating durable tasks as expired executor sessions after rollback.
    #[serde(rename = "ownershipSchema", alias = "schema")]
    pub schema: u32,
    pub source: PathBuf,
    pub index: u32,
    pub path: PathBuf,
    pub state: SlotState,
    /// Executor session identity that holds the slot while its session is live.
    pub owner: Option<String>,
    /// Durable task identity that owns the slot across process lifetimes.
    /// `None` on executor session records written before task bindings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    /// Non-reusable generation handle of the task binding, so a stale handle
    /// cannot resume a slot whose binding was released or reassigned.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handle: Option<String>,
    /// Committed base the slot was last synchronized to.
    pub base: Option<String>,
    /// Recorded release disposition (merged or explicitly discarded).
    pub disposition: Option<SlotDisposition>,
    /// Why the slot is preserved, or why the last dispatch aborted.
    pub reason: Option<String>,
    /// Process that owns the claim until a host lease is published. Absent on
    /// records written before startup reservations, which keep session liveness.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub startup: Option<StartupReservation>,
}

/// Private slot state root shared by every checkout of one repository. The
/// name and key come from the primary worktree, which also owns the sibling
/// slots, so the main checkout and its linked worktrees resolve one directory.
pub fn pool_state_dir(codex_home: &Path, source: &Path) -> io::Result<PathBuf> {
    let source = resolve_source(source)?;
    match repository_identity(&source) {
        Ok(identity) => Ok(state_dir(
            codex_home,
            &identity.name,
            &identity.primary_root,
        )),
        // A path that is not (yet) a usable Git checkout keeps the historical
        // per-path directory so status commands stay total.
        Err(_) => Ok(state_dir(codex_home, file_name_of(&source), &source)),
    }
}

/// Directory a build without pool identity used for one checkout path. Kept as
/// the legacy fallback so existing slot records stay owned and readable.
fn legacy_pool_state_dir(codex_home: &Path, source: &Path) -> io::Result<PathBuf> {
    let source = resolve_source(source)?;
    Ok(state_dir(codex_home, file_name_of(&source), &source))
}

fn state_dir(codex_home: &Path, name: &str, key: &Path) -> PathBuf {
    let name: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let digest = hash_bytes(key.to_string_lossy().as_bytes());
    codex_home
        .join("harness/executor-pool")
        .join(format!("{name}-{}", &digest[..12]))
}

fn file_name_of(path: &Path) -> &str {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("checkout")
}

/// Record locations of one repository: the canonical directory of its pool
/// identity, plus every legacy per-path directory a checkout of it may have
/// written old records into. Entering through the main checkout or any linked
/// worktree resolves the same locations, so one repository never hides a
/// record - or a conflict - behind the caller's path.
struct SlotStateLayout {
    canonical: PathBuf,
    legacy: Vec<PathBuf>,
}

impl SlotStateLayout {
    fn candidates(&self, index: u32) -> Vec<PathBuf> {
        let file = format!("slot-{index}.json");
        let mut candidates = vec![self.canonical.join(&file)];
        for legacy in &self.legacy {
            candidates.push(legacy.join(&file));
        }
        candidates
    }

    /// Path that keeps an existing record in place: the canonical location
    /// first, then the legacy directories in repository order. Reads,
    /// exclusive claim creation and updates use this resolution, so a legacy
    /// record is migrated by its next mutation instead of being forked.
    fn resolved(&self, index: u32) -> PathBuf {
        let file = format!("slot-{index}.json");
        self.candidates(index)
            .into_iter()
            .find(|candidate| candidate.is_file())
            .unwrap_or_else(|| self.canonical.join(file))
    }
}

fn slot_state_layout(codex_home: &Path, caller: &Path) -> io::Result<SlotStateLayout> {
    let canonical = pool_state_dir(codex_home, caller)?;
    let mut legacy = Vec::new();
    let source = resolve_source(caller)?;
    // Every registered worktree of the repository may own legacy records
    // written under its own path by a build without pool identity. Listing is
    // best-effort: a broken primary tree must not make status unusable.
    if let Ok(identity) = repository_identity(&source)
        && let Ok(trees) = registered_trees(&identity.primary_root)
    {
        for tree in trees {
            // A prunable tree no longer exists: its path cannot resolve to a
            // legacy directory, and its missing position already refuses new
            // allocation with the prune hint.
            if tree.prunable || !tree.path.is_dir() {
                continue;
            }
            push_legacy_dir(&mut legacy, codex_home, &tree.path, &canonical)?;
        }
    }
    push_legacy_dir(&mut legacy, codex_home, &source, &canonical)?;
    Ok(SlotStateLayout { canonical, legacy })
}

fn push_legacy_dir(
    legacy: &mut Vec<PathBuf>,
    codex_home: &Path,
    path: &Path,
    canonical: &Path,
) -> io::Result<()> {
    let dir = legacy_pool_state_dir(codex_home, path)?;
    if dir != canonical && !legacy.contains(&dir) {
        legacy.push(dir);
    }
    Ok(())
}

/// Canonical record path of a repository. Existing records may still live in
/// a legacy directory; `SlotStateLayout::resolved` finds them.
pub fn slot_record_path(codex_home: &Path, source: &Path, index: u32) -> io::Result<PathBuf> {
    Ok(pool_state_dir(codex_home, source)?.join(format!("slot-{index}.json")))
}

/// Conflicting records for one slot - the canonical location against a legacy
/// directory, or two legacy pools of one repository - cannot both own the
/// slot. The files are preserved; ownership-changing operations and new
/// allocation are refused until the lead reconciles.
fn layout_record_conflict(layout: &SlotStateLayout, index: u32) -> io::Result<Option<String>> {
    let mut existing: Vec<(PathBuf, SlotRecord)> = Vec::new();
    for path in layout.candidates(index) {
        match read_slot_record(&path, index) {
            Ok(record) => existing.push((path, record)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    let Some((first_path, first)) = existing.first() else {
        return Ok(None);
    };
    let Some((other_path, _)) = existing.iter().find(|(_, record)| record != first) else {
        return Ok(None);
    };
    Ok(Some(format!(
        "slot {index} has conflicting records at {} and {}; the lead reconciles the pools before new allocation",
        first_path.display(),
        other_path.display()
    )))
}

/// Fail-closed guard: ownership-changing operations refuse while a slot has
/// records in more than one location that disagree.
fn refuse_record_conflict(layout: &SlotStateLayout, index: u32) -> io::Result<()> {
    match layout_record_conflict(layout, index)? {
        Some(conflict) => Err(pool_error(&conflict)),
        None => Ok(()),
    }
}

fn read_slot_record(path: &Path, index: u32) -> io::Result<SlotRecord> {
    let bytes = fs::read(path)?;
    let mut record: SlotRecord = serde_json::from_slice(&bytes).map_err(|error| {
        pool_error(&format!(
            "slot record {} is unreadable ({error}); the lead resolves or removes it",
            path.display()
        ))
    })?;
    validate_slot_record(path, index, &record)?;
    // Read legacy records without rewriting or relocating them. The next
    // already-authorized state transition publishes the current format.
    record.schema = SLOT_STATE_SCHEMA;
    Ok(record)
}

/// Fail closed on record shapes this build does not own. `schema` is the
/// ownership format version, and the task binding fields are written together
/// and only while a task holds the slot; any other shape would let an
/// operation misread durable ownership, so it is refused without mutation.
fn validate_slot_record(path: &Path, index: u32, record: &SlotRecord) -> io::Result<()> {
    if !matches!(record.schema, 1 | SLOT_STATE_SCHEMA) {
        return Err(pool_error(&format!(
            "slot record {} declares ownership schema {}; this build supports schema {SLOT_STATE_SCHEMA} and refuses to operate on the unsupported durable binding - the lead resolves or removes the record",
            path.display(),
            record.schema
        )));
    }
    if record.index != index {
        return Err(pool_error(&format!(
            "slot record {} declares position {} instead of {index}; a mislocated durable binding is refused - the lead resolves or removes the record",
            path.display(),
            record.index
        )));
    }
    match (record.task.as_deref(), record.handle.as_deref()) {
        (Some(task), Some(handle)) => {
            if task.trim().is_empty() || handle.trim().is_empty() {
                return Err(pool_error(&format!(
                    "slot record {} has an incomplete task binding (task {task:?}, handle {handle:?}); the lead resolves or removes the record",
                    path.display()
                )));
            }
            if matches!(record.state, SlotState::Free | SlotState::Released) {
                return Err(pool_error(&format!(
                    "slot record {} is {} but still records task '{task}'; a durable binding never appears in a free or released position - the lead resolves or removes the record",
                    path.display(),
                    state_name(record.state)
                )));
            }
        }
        (Some(task), None) => {
            return Err(pool_error(&format!(
                "slot record {} records task '{task}' without its generation handle; an unverifiable durable binding is refused - the lead restores the handle or removes the record",
                path.display()
            )));
        }
        (None, Some(handle)) => {
            return Err(pool_error(&format!(
                "slot record {} records generation handle '{handle}' without a task; an unowned binding is refused - the lead resolves or removes the record",
                path.display()
            )));
        }
        (None, None) => {}
    }
    Ok(())
}

fn load_record_at(layout: &SlotStateLayout, index: u32) -> io::Result<Option<SlotRecord>> {
    match read_slot_record(&layout.resolved(index), index) {
        Ok(record) => Ok(Some(record)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

pub fn load_slot_record(
    codex_home: &Path,
    source: &Path,
    index: u32,
) -> io::Result<Option<SlotRecord>> {
    load_record_at(&slot_state_layout(codex_home, source)?, index)
}

/// Bound for the per-slot record lock. It covers the claim mutation only,
/// not fetch, host attach, or the model conversation.
const SLOT_CLAIM_LOCK_BOUND: std::time::Duration = std::time::Duration::from_secs(15);

fn slot_lock_path(codex_home: &Path, source: &Path, index: u32) -> io::Result<PathBuf> {
    Ok(pool_state_dir(codex_home, source)?.join(format!("slot-{index}.lock")))
}

/// Serialize one slot's claim mutation. The lock file stays in place: replacing
/// it would let two callers lock different objects. Process exit releases it.
fn lock_slot(
    codex_home: &Path,
    source: &Path,
    index: u32,
) -> io::Result<crate::process::ExclusiveFileLock> {
    let path = slot_lock_path(codex_home, source, index)?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let deadline = crate::process::Deadline::after(SLOT_CLAIM_LOCK_BOUND)?;
    match crate::process::ExclusiveFileLock::acquire(
        &path,
        deadline,
        &crate::process::Cancellation::default(),
    ) {
        Ok(lock) => Ok(lock),
        Err(error) if error.kind() == io::ErrorKind::TimedOut => Err(pool_error(&format!(
            "slot {index} claim record stayed locked by another dispatch for {SLOT_CLAIM_LOCK_BOUND:?}; refusing to replace the pending claim"
        ))),
        Err(error) => Err(error),
    }
}

/// Identity of this process as a startup reservation. Inspection failure is
/// returned rather than claiming a slot that later recovery cannot attribute.
#[cfg(windows)]
pub fn current_startup_reservation() -> io::Result<StartupReservation> {
    let program = std::env::current_exe().map_err(|error| {
        pool_error(&format!(
            "startup reservation needs the current executable: {error}"
        ))
    })?;
    let user = crate::process_service::current_user().map_err(|error| {
        pool_error(&format!(
            "startup reservation needs the current user identity: {error}"
        ))
    })?;
    let identity =
        crate::process_service::ServiceProcess::observe(std::process::id(), &program, 0, &user)
            .map_err(|error| {
                pool_error(&format!(
                    "startup reservation could not observe this process: {error}"
                ))
            })?
            .identity();
    Ok(StartupReservation {
        pid: identity.pid,
        created: identity.creation_time,
        program,
    })
}

#[cfg(not(windows))]
pub fn current_startup_reservation() -> io::Result<StartupReservation> {
    Err(pool_error(
        "startup reservation requires Windows process identity",
    ))
}

fn startup_matches(reservation: &StartupReservation, current: &StartupReservation) -> bool {
    reservation.pid == current.pid && reservation.created == current.created
}

/// True when this process created the reservation. An identity that cannot be
/// observed is not treated as a match, so an uncertain caller cannot release
/// someone else's claim.
pub fn startup_matches_current(reservation: &StartupReservation) -> bool {
    current_startup_reservation()
        .ok()
        .is_some_and(|current| startup_matches(reservation, &current))
}

fn startup_liveness(reservation: &StartupReservation) -> ReservationLiveness {
    #[cfg(windows)]
    {
        if !reservation.program.is_file() {
            return ReservationLiveness::Dead;
        }
        let Ok(user) = crate::process_service::current_user() else {
            return ReservationLiveness::Uncertain;
        };
        match crate::process_service::ServiceProcess::inspect(
            crate::process::ProcessIdentity {
                pid: reservation.pid,
                creation_time: reservation.created,
            },
            &reservation.program,
            &user,
        ) {
            Ok(Some(_)) => ReservationLiveness::Live,
            Ok(None) => ReservationLiveness::Dead,
            Err(_) => ReservationLiveness::Uncertain,
        }
    }
    #[cfg(not(windows))]
    {
        let _ = reservation;
        ReservationLiveness::Uncertain
    }
}

fn startup_exclusive(reservation: &StartupReservation) -> bool {
    matches!(
        startup_liveness(reservation),
        ReservationLiveness::Live | ReservationLiveness::Uncertain
    )
}

fn pending_startup_reason(
    index: u32,
    owner: Option<&str>,
    liveness: ReservationLiveness,
) -> String {
    let owner = owner.unwrap_or("no session");
    match liveness {
        ReservationLiveness::Uncertain => format!(
            "slot {index} startup of session {owner} has uncertain process identity; the claim is preserved until that identity is resolved"
        ),
        ReservationLiveness::Live | ReservationLiveness::Dead => format!(
            "slot {index} is reserved by in-flight startup of session {owner} until its host lease exists"
        ),
    }
}

fn refusal(slot: &PoolSlot, reason: String) -> SlotClaim {
    SlotClaim::Unavailable(SlotRefusal {
        index: slot.index,
        path: slot.path.clone(),
        reason,
    })
}

/// The task-claim view of the shared refusal constructor.
fn task_refusal(slot: &PoolSlot, reason: String) -> TaskSlotClaim {
    match refusal(slot, reason) {
        SlotClaim::Unavailable(refusal) => TaskSlotClaim::Unavailable(refusal),
        SlotClaim::Claimed(_) => unreachable!("a refusal never produces a claim"),
    }
}

/// Claim a pool slot for one executor session. Exclusive file creation is the
/// compare-and-set step, so a concurrent claim loses the race instead of
/// sharing the tree. A claim whose session is gone is reclaimed only when the
/// slot is clean at its recorded base and no live or uncertain startup
/// reservation still owns it.
pub fn claim_slot(
    codex_home: &Path,
    pool: &Pool,
    index: u32,
    owner: &str,
    is_live: &dyn Fn(&str) -> bool,
) -> io::Result<SlotClaim> {
    claim_slot_for(codex_home, pool, index, owner, is_live, None)
}

/// Claim one slot, optionally recording the creating process as its startup
/// reservation. `startup` of `None` preserves the session-liveness claim used
/// by records and tests that predate the reservation.
pub fn claim_slot_for(
    codex_home: &Path,
    pool: &Pool,
    index: u32,
    owner: &str,
    is_live: &dyn Fn(&str) -> bool,
    startup: Option<&StartupReservation>,
) -> io::Result<SlotClaim> {
    if owner.trim().is_empty() {
        return Err(pool_error(
            "a slot claim requires the executor session identity",
        ));
    }
    let slot = pool.slot(index)?;
    let layout = slot_state_layout(codex_home, &pool.caller)?;
    for _ in 0..CLAIM_ATTEMPTS {
        let _lock = lock_slot(codex_home, &pool.source, index)?;
        refuse_record_conflict(&layout, index)?;
        let record_path = layout.resolved(index);
        let previous = load_record_at(&layout, index)?;
        let claim = SlotRecord {
            schema: SLOT_STATE_SCHEMA,
            source: pool.source.clone(),
            index,
            path: slot.path.clone(),
            state: SlotState::Synchronizing,
            owner: Some(owner.to_owned()),
            task: None,
            handle: None,
            base: previous.as_ref().and_then(|record| record.base.clone()),
            disposition: None,
            reason: None,
            startup: startup.cloned(),
        };
        if create_slot_record(&record_path, &claim)? {
            return Ok(SlotClaim::Claimed(claim));
        }
        let Some(existing) = load_record_at(&layout, index)? else {
            continue;
        };
        if !same_repository(&existing.source, &pool.source) {
            return Err(pool_error(&format!(
                "slot record {} belongs to another source checkout {}; the lead resolves it",
                record_path.display(),
                existing.source.display()
            )));
        }
        if let Some(task) = existing.task.as_deref() {
            return Ok(refusal(
                slot,
                format!(
                    "slot {index} is held by task '{task}' at base {}; release that task before another session or task uses the slot",
                    existing.base.as_deref().unwrap_or("none")
                ),
            ));
        }
        if let Some(reserved) = existing.startup.as_ref() {
            let liveness = startup_liveness(reserved);
            if matches!(
                liveness,
                ReservationLiveness::Live | ReservationLiveness::Uncertain
            ) {
                if existing.owner.as_deref() == Some(owner) && startup_matches_current(reserved) {
                    return Ok(SlotClaim::Claimed(existing));
                }
                return Ok(refusal(
                    slot,
                    pending_startup_reason(index, existing.owner.as_deref(), liveness),
                ));
            }
        } else if existing.owner.as_deref() == Some(owner) {
            // A reservation-less record is the same generation retry used by
            // in-process claim recovery. A dead startup reservation must not
            // take this shortcut: that owner string may now belong to a new process.
            return Ok(SlotClaim::Claimed(existing));
        }
        if let Some(live) = existing.owner.as_deref().filter(|owner| is_live(owner)) {
            return Ok(refusal(
                slot,
                format!("slot {index} is occupied by live session {live}"),
            ));
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
                        owner: existing.owner.clone(),
                        disposition: None,
                        reason: Some(reason.clone()),
                        startup: None,
                        ..existing
                    },
                )?;
                return Ok(refusal(slot, reason));
            }
        }
    }
    Err(pool_error(&format!(
        "slot {index} claim lost a race with another harness process; retry"
    )))
}

/// Bind the claimed slot to its session at the synchronized base. A startup
/// reservation stays recorded until the host lease is published; another
/// generation cannot bind over it.
pub fn bind_slot(
    codex_home: &Path,
    pool: &Pool,
    index: u32,
    owner: &str,
    base: &str,
) -> io::Result<SlotRecord> {
    let _lock = lock_slot(codex_home, &pool.source, index)?;
    let layout = slot_state_layout(codex_home, &pool.caller)?;
    let record = load_record_at(&layout, index)?
        .ok_or_else(|| pool_error(&format!("slot {index} has no claim to bind")))?;
    if record.owner.as_deref() != Some(owner) {
        return Err(pool_error(&format!(
            "slot {index} is claimed by {}; refusing to bind another session",
            record.owner.as_deref().unwrap_or("no session")
        )));
    }
    if let Some(startup) = record.startup.as_ref()
        && startup_exclusive(startup)
        && !startup_matches_current(startup)
    {
        return Err(pool_error(&pending_startup_reason(
            index,
            record.owner.as_deref(),
            startup_liveness(startup),
        )));
    }
    let bound = SlotRecord {
        state: SlotState::Occupied,
        base: Some(base.to_owned()),
        disposition: None,
        reason: None,
        ..record
    };
    write_slot_record(&layout.resolved(index), &bound)?;
    Ok(bound)
}

/// Rebind one exact slot to its interrupted session. Resume is the one
/// allocation path that must not resynchronize: partial work stays in the
/// tree, so a missing record or directory, a foreign source, a missing base,
/// a live owner or another owner's claim is refused instead of being repaired
/// by a reset. A clean slot whose owner was cleared back to the pool is
/// adopted for the explicitly named owner; a slot left awaiting review keeps
/// its interrupted owner recorded, so only that owner resumes it. A live or
/// uncertain startup reservation of another process is not resumed over.
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
    let layout = slot_state_layout(codex_home, &pool.caller)?;
    let record = load_record_at(&layout, index)?.ok_or_else(|| {
        pool_error(&format!(
            "slot {index} has no recorded binding to resume; dispatch executor spawn first"
        ))
    })?;
    refuse_record_conflict(&layout, index)?;
    if !same_repository(&record.source, &pool.source) {
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
    if let Some(startup) = record.startup.as_ref()
        && startup_exclusive(startup)
        && !startup_matches_current(startup)
    {
        return Err(pool_error(&format!(
            "slot {index} is reserved by in-flight startup of session {}; resume waits until that dispatch hands off or its process identity is resolved",
            record.owner.as_deref().unwrap_or("no session")
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
    let _lock = lock_slot(codex_home, &pool.source, index)?;
    // Resume has no host lease yet. Record this process when it can be
    // observed so another dispatch cannot reclaim the adopted slot in the
    // interval before handoff. Observation failure leaves the previous record.
    let startup = current_startup_reservation().ok();
    let bound = SlotRecord {
        state: SlotState::Occupied,
        owner: Some(owner.to_owned()),
        base: Some(base),
        disposition: None,
        reason: None,
        startup,
        ..record
    };
    write_slot_record(&layout.resolved(index), &bound)?;
    Ok(bound)
}

/// What a release call expects to find when it finally owns the slot lock.
/// The binding is re-validated under the lock, so a slot reassigned between a
/// caller's preflight and its lock is never reset by the stale caller.
enum ReleaseExpectation<'a> {
    /// Release by position for a legacy executor-session record: a session
    /// release must never clear a durable task binding.
    Session,
    /// Release one exact task binding; the generation handle is required when
    /// the record carries one.
    Task {
        task: &'a str,
        handle: Option<&'a str>,
    },
}

/// The durable identity one release observed before it took the slot lock.
///
/// A release is authorized for the binding its caller looked at, not for
/// whatever occupies the position later: the record read under the lock must
/// still carry this task binding, generation handle, owner and startup
/// reservation. A handle-less caller freezes the generation it actually
/// observed, so a reacquired generation of the same task is a different
/// binding that the superseded caller must not reset.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ReleaseIdentity {
    task: Option<String>,
    handle: Option<String>,
    owner: Option<String>,
    startup: Option<StartupReservation>,
}

impl ReleaseIdentity {
    fn from_record(record: &SlotRecord) -> Self {
        Self {
            task: record.task.clone(),
            handle: record.handle.clone(),
            owner: record.owner.clone(),
            startup: record.startup.clone(),
        }
    }

    /// Refuse before any effect when the locked record no longer carries the
    /// exact binding this release observed.
    fn validate(&self, index: u32, record: &SlotRecord) -> io::Result<()> {
        if self.task != record.task || self.handle != record.handle {
            return Err(pool_error(&format!(
                "slot {index} now carries {} instead of the {} this release observed; the binding was replaced after the call started and nothing was reset",
                binding_label(record.task.as_deref(), record.handle.as_deref()),
                binding_label(self.task.as_deref(), self.handle.as_deref()),
            )));
        }
        if self.owner != record.owner {
            return Err(pool_error(&format!(
                "slot {index} is now owned by session {} instead of {}; the owner changed after this release observed the binding and nothing was reset",
                record.owner.as_deref().unwrap_or("no session"),
                self.owner.as_deref().unwrap_or("no session"),
            )));
        }
        if self.startup != record.startup {
            return Err(pool_error(&format!(
                "slot {index} startup reservation changed after this release observed the binding; nothing was reset"
            )));
        }
        Ok(())
    }
}

/// Plain-language form of one observed binding, used only in release refusals.
fn binding_label(task: Option<&str>, handle: Option<&str>) -> String {
    match (task, handle) {
        (Some(task), Some(handle)) => format!("task '{task}' generation {handle}"),
        (Some(task), None) => format!("task '{task}' without a generation handle"),
        (None, Some(handle)) => format!("generation {handle} without a task"),
        (None, None) => "no task binding".to_owned(),
    }
}

/// Refuse a release whose expected binding no longer matches the record read
/// under the slot lock, before any effect touches the checkout.
fn validate_release_expectation(
    index: u32,
    record: &SlotRecord,
    expected: &ReleaseExpectation<'_>,
) -> io::Result<()> {
    match expected {
        ReleaseExpectation::Session => {
            if let Some(task) = record.task.as_deref() {
                return Err(pool_error(&format!(
                    "slot {index} is bound to task '{task}'; a session release must not clear a durable task binding - release the task instead"
                )));
            }
        }
        ReleaseExpectation::Task { task, handle } => {
            if record.task.as_deref() != Some(*task) {
                return Err(pool_error(&format!(
                    "slot {index} is {} instead of the released task '{task}'; the release was superseded and nothing was reset",
                    match record.task.as_deref() {
                        Some(held) => format!("bound to task '{held}'"),
                        None => "no longer bound to that task".to_owned(),
                    }
                )));
            }
            match (handle, record.handle.as_deref()) {
                (Some(expected), Some(recorded)) if *expected == recorded => {}
                (Some(expected), recorded) => {
                    return Err(pool_error(&format!(
                        "task '{task}' binding in slot {index} has generation {} instead of {expected}; the handle is stale after release or reassignment",
                        recorded.unwrap_or("none")
                    )));
                }
                // A handle-less release keeps the optional-handle API: the
                // task text is checked here, while the generation handle the
                // caller actually observed is frozen in the release identity
                // and re-validated under the lock before any effect.
                (None, _) => {}
            }
        }
    }
    Ok(())
}

/// The durable release-in-progress record. It keeps the task binding, owner
/// and base while the reset and its verification are pending, so process loss
/// or a failed reset can never advertise the slot as free.
fn release_pending_record(
    record: &SlotRecord,
    disposition: SlotDisposition,
    base: &str,
) -> SlotRecord {
    SlotRecord {
        state: SlotState::AwaitingReview,
        disposition: Some(disposition),
        reason: Some(format!(
            "release in progress: reset to base {base} is not confirmed yet; the binding stays held"
        )),
        startup: None,
        ..record.clone()
    }
}

pub fn release_slot(
    codex_home: &Path,
    pool: &Pool,
    index: u32,
    disposition: SlotDisposition,
    reason: &str,
    base: &str,
    is_live: &dyn Fn(&str) -> bool,
) -> io::Result<LaneDisposition> {
    let layout = slot_state_layout(codex_home, &pool.caller)?;
    release_slot_with(
        codex_home,
        &pool.source,
        &layout,
        index,
        disposition,
        reason,
        ReleaseExpectation::Session,
        None,
        base,
        is_live,
    )
}

// Every release input is validated under the slot lock; the explicit list is
// kept over a params struct because the call sites read as one release call.
// `observed` is the binding the caller looked up, when it did one.
#[allow(clippy::too_many_arguments)]
fn release_slot_with(
    codex_home: &Path,
    layout_root: &Path,
    layout: &SlotStateLayout,
    index: u32,
    disposition: SlotDisposition,
    reason: &str,
    expected: ReleaseExpectation<'_>,
    observed: Option<ReleaseIdentity>,
    base: &str,
    is_live: &dyn Fn(&str) -> bool,
) -> io::Result<LaneDisposition> {
    let record = load_record_at(layout, index)?.ok_or_else(|| {
        pool_error(&format!(
            "slot {index} has no recorded assignment to release"
        ))
    })?;
    refuse_record_conflict(layout, index)?;
    validate_release_expectation(index, &record, &expected)?;
    // Freeze the identity this release is authorized for: the binding the
    // caller looked up when it supplied one, otherwise this preflight read.
    let identity = observed.unwrap_or_else(|| ReleaseIdentity::from_record(&record));
    identity.validate(index, &record)?;
    if let Some(startup) = record.startup.as_ref()
        && startup_exclusive(startup)
        && !startup_matches_current(startup)
    {
        return Err(pool_error(&format!(
            "slot {index} is reserved by in-flight startup of session {}; release waits until that dispatch hands off or its process identity is resolved",
            record.owner.as_deref().unwrap_or("no session")
        )));
    }
    if let Some(owner) = record.owner.as_deref().filter(|owner| is_live(owner)) {
        return Err(pool_error(&format!(
            "slot {index} is still owned by live session {owner}; stop it before release"
        )));
    }
    let checkout = slot_path(layout_root, index)?;
    let commit = git(
        &checkout,
        &["rev-parse", "--verify", &format!("{base}^{{commit}}")],
    )
    .map(|text| text.trim().to_owned())
    .map_err(|_| {
        pool_error(&format!(
            "release base '{base}' is not a commit in slot {index}; the slot is preserved unchanged"
        ))
    })?;
    let _lock = lock_slot(codex_home, layout_root, index)?;
    // Re-validate the complete observed identity under the lock: a slot
    // released and reassigned after this caller's preflight must never be
    // reset by the superseded caller.
    let record = load_record_at(layout, index)?.ok_or_else(|| {
        pool_error(&format!(
            "slot {index} has no recorded assignment to release"
        ))
    })?;
    refuse_record_conflict(layout, index)?;
    validate_release_expectation(index, &record, &expected)?;
    identity.validate(index, &record)?;
    if let Some(startup) = record.startup.as_ref()
        && startup_exclusive(startup)
        && !startup_matches_current(startup)
    {
        return Err(pool_error(&format!(
            "slot {index} is reserved by in-flight startup of session {}; release waits until that dispatch hands off or its process identity is resolved",
            record.owner.as_deref().unwrap_or("no session")
        )));
    }
    if let Some(owner) = record
        .owner
        .as_deref()
        .filter(|owner| Some(*owner) != record.task.as_deref())
        .filter(|owner| is_live(owner))
    {
        return Err(pool_error(&format!(
            "slot {index} is still owned by live session {owner}; stop it before release"
        )));
    }
    let record_path = layout.resolved(index);
    // Phase 1 keeps the binding, owner and base durable while reset and its
    // verification are pending, so process loss or a failed reset can never
    // advertise the slot as free.
    let pending = release_pending_record(&record, disposition, base);
    write_slot_record(&record_path, &pending)?;
    let mapping = Mapping {
        schema: 1,
        path: checkout.clone(),
        source: layout_root.to_path_buf(),
        head: commit.clone(),
        owner_thread: None,
        archived: false,
        unavailable: false,
    };
    match reset_for_reuse(&mapping, layout_root, &commit) {
        Ok(LaneDisposition::Reused { base }) => {
            let released = SlotRecord {
                state: SlotState::Released,
                owner: None,
                task: None,
                handle: None,
                base: Some(commit.clone()),
                disposition: Some(disposition),
                reason: Some(reason.to_owned()),
                startup: None,
                ..pending
            };
            write_slot_record(&record_path, &released)?;
            Ok(LaneDisposition::Reused { base })
        }
        Ok(LaneDisposition::Preserved { limitation }) => {
            let preserved = SlotRecord {
                state: SlotState::AwaitingReview,
                reason: Some(limitation.clone()),
                startup: None,
                ..pending
            };
            write_slot_record(&record_path, &preserved)?;
            Ok(LaneDisposition::Preserved { limitation })
        }
        Err(error) => {
            // Keep the original error and its recovery inputs; the pending
            // record already holds the slot even if this bookkeeping write
            // fails, and the caller gets the original cause either way.
            let failed = SlotRecord {
                state: SlotState::AwaitingReview,
                reason: Some(format!(
                    "release failed before the reset could complete: {error}; the binding, base {base} and the checkout are preserved - resolve the cause and release again"
                )),
                startup: None,
                ..pending
            };
            let _ = write_slot_record(&record_path, &failed);
            Err(error)
        }
    }
}

/// Reconcile recorded occupancy with executor session liveness. A live or
/// uncertain startup reservation is left untouched even when no host lease
/// exists yet. Only a resolved-dead reservation, or a reservation-less record,
/// follows session liveness: a clean slot returns to the pool, and a slot
/// holding work becomes awaiting review with its interrupted owner recorded.
pub fn reconcile_slots(
    codex_home: &Path,
    pool: &Pool,
    is_live: &dyn Fn(&str) -> bool,
) -> io::Result<Vec<SlotRecord>> {
    let layout = slot_state_layout(codex_home, &pool.caller)?;
    let mut records = Vec::new();
    for slot in &pool.slots {
        let Some(record) = load_record_at(&layout, slot.index)? else {
            records.push(free_slot_record(pool, slot));
            continue;
        };
        // Ambiguous record locations stop reconciliation and allocation.
        refuse_record_conflict(&layout, slot.index)?;
        // A task binding persists without process liveness: only an explicit
        // release frees it.
        if record.task.is_some() {
            records.push(record);
            continue;
        }
        if record.startup.as_ref().is_some_and(startup_exclusive) {
            records.push(record);
            continue;
        }
        let live = record.owner.as_deref().is_some_and(is_live);
        if live || !matches!(record.state, SlotState::Occupied | SlotState::Synchronizing) {
            records.push(record);
            continue;
        }
        let _lock = lock_slot(codex_home, &pool.source, slot.index)?;
        let Some(record) = load_record_at(&layout, slot.index)? else {
            records.push(free_slot_record(pool, slot));
            continue;
        };
        if record.task.is_some() {
            records.push(record);
            continue;
        }
        if record.startup.as_ref().is_some_and(startup_exclusive) {
            records.push(record);
            continue;
        }
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
                startup: None,
                ..record.clone()
            },
            SlotContent::Unreviewed(reason) | SlotContent::Missing(reason) => SlotRecord {
                state: SlotState::AwaitingReview,
                owner: record.owner.clone(),
                reason: Some(reason),
                startup: None,
                ..record.clone()
            },
        };
        write_slot_record(&layout.resolved(slot.index), &next)?;
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

/// Fail-closed synchronization of one slot: resolve the requested base, reset,
/// remove untracked files keeping ignored build caches, and verify a clean
/// HEAD. An explicit base is resolved locally and never fetched; without one
/// the configured upstream is refreshed first, and a fetch or resolution
/// failure aborts before the slot changes. Unreviewed work aborts the reset.
pub fn synchronize_slot(
    codex_home: &Path,
    pool: &Pool,
    index: u32,
    base: Option<&str>,
) -> io::Result<SynchronizedSlot> {
    refuse_record_conflict(&slot_state_layout(codex_home, &pool.caller)?, index)?;
    let checkout = slot_worktree(pool, index)?;
    let (remote, branch) = match base {
        // An explicit local base needs no network access; the upstream pair is
        // still reported when it can be inspected locally.
        Some(_) => upstream(&checkout).unwrap_or_default(),
        None => upstream(&checkout)?,
    };
    if base.is_none() {
        git(&checkout, &["fetch", &remote]).map_err(|error| {
            pool_error(&format!(
                "upstream fetch of '{remote}' failed for slot {index}; the slot keeps its previous state: {error}"
            ))
        })?;
    }
    let commit = match base {
        Some(base) => resolve_commit(&checkout, base).map_err(|_| {
            pool_error(&format!(
                "base '{base}' is not a commit in slot {index}; an explicit base is resolved locally and never fetched, so the slot keeps its previous state"
            ))
        })?,
        None => {
            let branch = branch.clone().ok_or_else(|| {
                pool_error(&format!(
                    "the upstream default branch for remote '{remote}' cannot be resolved; pass an explicit base"
                ))
            })?;
            resolve_commit(&checkout, &format!("refs/remotes/{remote}/{branch}")).map_err(|_| {
                pool_error(&format!(
                    "the upstream default branch '{remote}/{branch}' cannot be resolved in slot {index}; pass an explicit base"
                ))
            })?
        }
    };
    let previous_head = reset_slot_to(codex_home, pool, index, &commit)?;
    Ok(SynchronizedSlot {
        base: commit,
        remote,
        branch,
        previous_head,
    })
}

/// Resolve the executor's upstream-default base: refresh the configured
/// upstream and name the exact commit of its default branch. A fetch or
/// resolution failure is reported before any slot changes, so an executor
/// adapter can pass the returned commit as an explicit local base to the task
/// acquisition without a second network access.
pub fn refresh_upstream_base(checkout: &Path) -> io::Result<(String, String, String)> {
    let checkout = resolve_source(checkout)?;
    let (remote, branch) = upstream(&checkout)?;
    git(&checkout, &["fetch", &remote]).map_err(|error| {
        pool_error(&format!(
            "upstream fetch of '{remote}' failed for {}; no base is selected from stale state: {error}",
            checkout.display()
        ))
    })?;
    let branch = branch.ok_or_else(|| {
        pool_error(&format!(
            "the upstream default branch for remote '{remote}' cannot be resolved; name an explicit base"
        ))
    })?;
    let commit = resolve_commit(&checkout, &format!("refs/remotes/{remote}/{branch}")).map_err(
        |_| {
            pool_error(&format!(
                "the upstream default branch '{remote}/{branch}' cannot be resolved; name an explicit base"
            ))
        },
    )?;
    Ok((commit, remote, branch))
}

/// The checkout of one pool position, verified to be a Git worktree.
fn slot_worktree(pool: &Pool, index: u32) -> io::Result<PathBuf> {
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
    Ok(slot.path.clone())
}

/// Reset one slot to an exact committed base, keeping ignored build caches.
/// A reset destroys data only when the slot is clean or its release
/// disposition is recorded as merged or explicitly discarded; unreviewed work
/// aborts before any change. The previous HEAD is returned for the receipt.
fn reset_slot_to(
    codex_home: &Path,
    pool: &Pool,
    index: u32,
    commit: &str,
) -> io::Result<Option<String>> {
    let checkout = slot_worktree(pool, index)?;
    let record = load_slot_record(codex_home, &pool.caller, index)?;
    if !record
        .as_ref()
        .is_some_and(|record| record.disposition.is_some())
    {
        let head = git(&checkout, &["rev-parse", "HEAD"])
            .map(|text| text.trim().to_owned())
            .unwrap_or_default();
        let recorded = record.as_ref().and_then(|record| record.base.clone());
        let status = slot_status(&checkout)?;
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
    let previous_head = git(&checkout, &["rev-parse", "HEAD"])
        .ok()
        .map(|text| text.trim().to_owned());
    git(&checkout, &["reset", "--hard", commit]).map_err(|error| {
        pool_error(&format!(
            "slot {index} could not reset to {commit}: {error}"
        ))
    })?;
    git(&checkout, &["clean", "-fd"]).map_err(|error| {
        pool_error(&format!(
            "slot {index} could not remove untracked files: {error}"
        ))
    })?;
    let head = git(&checkout, &["rev-parse", "HEAD"])
        .map(|text| text.trim().to_owned())
        .unwrap_or_default();
    let status = slot_status(&checkout)?;
    if head != commit || !status.trim().is_empty() {
        return Err(pool_error(&format!(
            "slot {index} did not reach a clean synchronized state at {commit} (HEAD {head})"
        )));
    }
    Ok(previous_head)
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
    acquire_slot_with(codex_home, source, size, owner, base, is_live, None)
}

/// Dispatch acquisition that records this process as the startup reservation.
/// The reservation stays exclusive until `release_startup_reservation` runs
/// at host handoff. Competing dispatches cannot reclaim the slot merely
/// because no host lease exists yet.
pub fn acquire_slot_for_startup(
    codex_home: &Path,
    source: &Path,
    size: u32,
    owner: &str,
    base: Option<&str>,
    startup: &StartupReservation,
    is_live: &dyn Fn(&str) -> bool,
) -> io::Result<AcquiredSlot> {
    acquire_slot_with(
        codex_home,
        source,
        size,
        owner,
        base,
        is_live,
        Some(startup),
    )
}

fn acquire_slot_with(
    codex_home: &Path,
    source: &Path,
    size: u32,
    owner: &str,
    base: Option<&str>,
    is_live: &dyn Fn(&str) -> bool,
    startup: Option<&StartupReservation>,
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
        match claim_slot_for(codex_home, &pool, slot.index, owner, is_live, startup)? {
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

/// Drop the startup reservation after the host lease is the authority.
/// A different session's reservation is left in place.
pub fn release_startup_reservation(
    codex_home: &Path,
    source: &Path,
    index: u32,
    owner: &str,
) -> io::Result<()> {
    let layout = slot_state_layout(codex_home, source)?;
    let Some(record) = load_record_at(&layout, index)? else {
        return Ok(());
    };
    if record.owner.as_deref() != Some(owner) || record.startup.is_none() {
        return Ok(());
    }
    let _lock = lock_slot(codex_home, source, index)?;
    let Some(record) = load_record_at(&layout, index)? else {
        return Ok(());
    };
    if record.owner.as_deref() != Some(owner) || record.startup.is_none() {
        return Ok(());
    }
    write_slot_record(
        &layout.resolved(index),
        &SlotRecord {
            startup: None,
            ..record
        },
    )
}

/// Refuse to publish a receipt when another generation now owns the slot.
pub fn claim_still_held(
    codex_home: &Path,
    source: &Path,
    index: u32,
    owner: &str,
) -> io::Result<()> {
    let record = load_slot_record(codex_home, source, index)?.ok_or_else(|| {
        pool_error(&format!(
            "slot {index} has no claim; refusing to replace its assignment or receipt"
        ))
    })?;
    if record.owner.as_deref() != Some(owner) {
        return Err(pool_error(&format!(
            "slot {index} is claimed by {}; refusing to replace its assignment or receipt",
            record.owner.as_deref().unwrap_or("no session")
        )));
    }
    if let Some(startup) = record.startup.as_ref()
        && startup_exclusive(startup)
        && !startup_matches_current(startup)
    {
        return Err(pool_error(&pending_startup_reason(
            index,
            record.owner.as_deref(),
            startup_liveness(startup),
        )));
    }
    Ok(())
}

/// Record this process as the startup owner of a slot it already holds.
/// Resume uses this after adoption so the interval before its host lease is
/// not reclaimable by another dispatch.
pub fn hold_startup_reservation(
    codex_home: &Path,
    source: &Path,
    index: u32,
    owner: &str,
) -> io::Result<()> {
    let startup = current_startup_reservation()?;
    let layout = slot_state_layout(codex_home, source)?;
    let _lock = lock_slot(codex_home, source, index)?;
    let record = load_record_at(&layout, index)?
        .ok_or_else(|| pool_error(&format!("slot {index} has no claim to reserve for startup")))?;
    if record.owner.as_deref() != Some(owner) {
        return Err(pool_error(&format!(
            "slot {index} is claimed by {}; refusing to reserve it for {owner}",
            record.owner.as_deref().unwrap_or("no session")
        )));
    }
    if let Some(existing) = record.startup.as_ref()
        && startup_exclusive(existing)
        && !startup_matches(existing, &startup)
    {
        return Err(pool_error(&pending_startup_reason(
            index,
            record.owner.as_deref(),
            startup_liveness(existing),
        )));
    }
    write_slot_record(
        &layout.resolved(index),
        &SlotRecord {
            startup: Some(startup),
            ..record
        },
    )
}

// ---------------------------------------------------------------------------
// Generic task lifecycle: durable task bindings in the shared pool.
//
// Ordinary tasks, experiments and reviews acquire a slot through these
// operations without starting a process. The binding records the task
// identity, a non-reusable generation handle and the exact prepared base; it
// persists until an explicit release, independent of any session liveness.
// Executor dispatch keeps its own startup reservation flow and can bind task
// identity through the same records.
// ---------------------------------------------------------------------------

/// Observable ownership state of one pool position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    /// No binding: the position may serve the next acquisition.
    Free,
    /// A task owns the slot; its work survives without a live process.
    Held,
    /// Ownership is retained with a concrete reason - unreviewed work or an
    /// incomplete release keeps the slot out of the free pool.
    AwaitingReview,
}

/// Durable task binding of one pool slot, as recorded by the common owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskSlot {
    pub index: u32,
    pub path: PathBuf,
    /// Registry position of the path; `Foreign` is reported, never adopted.
    pub presence: SlotPresence,
    pub state: TaskState,
    /// Durable task identity; `None` on a free slot or a legacy session record.
    pub task: Option<String>,
    /// Generation handle of the binding; a stale handle cannot continue it.
    pub handle: Option<String>,
    /// Owner identity recorded in the slot record: the task identity for a
    /// task binding, the executor session for legacy records.
    pub owner: Option<String>,
    /// Exact committed base the slot was prepared at.
    pub base: Option<String>,
    /// Recorded release disposition, when a release attempt was recorded.
    pub disposition: Option<SlotDisposition>,
    /// Why the slot is preserved, or why the last operation aborted.
    pub reason: Option<String>,
}

impl TaskSlot {
    fn from_record(
        record: &SlotRecord,
        index: u32,
        path: PathBuf,
        presence: SlotPresence,
    ) -> TaskSlot {
        TaskSlot {
            index,
            path,
            presence,
            state: task_state(record),
            task: record.task.clone(),
            handle: record.handle.clone(),
            owner: record.owner.clone(),
            base: record.base.clone(),
            disposition: record.disposition,
            reason: record.reason.clone(),
        }
    }

    fn free(index: u32, path: PathBuf, presence: SlotPresence) -> TaskSlot {
        TaskSlot {
            index,
            path,
            presence,
            state: TaskState::Free,
            task: None,
            handle: None,
            owner: None,
            base: None,
            disposition: None,
            reason: None,
        }
    }
}

fn task_state(record: &SlotRecord) -> TaskState {
    match record.state {
        SlotState::Free | SlotState::Released => TaskState::Free,
        SlotState::Synchronizing | SlotState::Occupied => TaskState::Held,
        SlotState::AwaitingReview => TaskState::AwaitingReview,
    }
}

fn state_name(state: SlotState) -> &'static str {
    match state {
        SlotState::Free => "free",
        SlotState::Synchronizing => "synchronizing",
        SlotState::Occupied => "occupied",
        SlotState::AwaitingReview => "awaiting review",
        SlotState::Released => "released",
    }
}

/// Exact committed base for a new task: an explicit request is resolved
/// locally and never fetched, and without a request the checkout's current
/// HEAD is the exact local base. Returns the commit and whether the base was
/// explicitly requested.
pub fn resolve_task_base(checkout: &Path, requested: Option<&str>) -> io::Result<(String, bool)> {
    match requested {
        Some(base) => resolve_commit(checkout, base)
            .map(|commit| (commit, true))
            .map_err(|_| {
                pool_error(&format!(
                    "base '{base}' is not a commit available locally in {}; an explicit base never fetches from a remote",
                    checkout.display()
                ))
            }),
        None => resolve_commit(checkout, "HEAD")
            .map(|commit| (commit, false))
            .map_err(|_| {
                pool_error(&format!(
                    "{} has no committed HEAD to use as the exact local base; name an explicit base",
                    checkout.display()
                ))
            }),
    }
}

/// Acquire or continue one durable task binding in the repository's pool.
///
/// Repeating acquisition for a held task returns its binding without reset or
/// a second slot; a different explicitly requested base is refused as a
/// conflict. Otherwise the first free position is claimed, prepared at the
/// exact committed base and bound to the task. Preparation never fetches: an
/// explicit base is resolved locally, and without one the primary checkout's
/// HEAD is the exact local base.
pub fn acquire_task(
    codex_home: &Path,
    source: &Path,
    size: u32,
    task: &str,
    base: Option<&str>,
    is_live: &dyn Fn(&str) -> bool,
) -> io::Result<TaskSlot> {
    acquire_task_with(codex_home, source, size, task, base, is_live, None)
}

/// Task acquisition that also records this process as the slot's startup
/// reservation until the consumer publishes its host lease; the reservation
/// is released with `release_startup_reservation` like any other.
pub fn acquire_task_for_startup(
    codex_home: &Path,
    source: &Path,
    size: u32,
    task: &str,
    base: Option<&str>,
    startup: &StartupReservation,
    is_live: &dyn Fn(&str) -> bool,
) -> io::Result<TaskSlot> {
    acquire_task_with(codex_home, source, size, task, base, is_live, Some(startup))
}

fn acquire_task_with(
    codex_home: &Path,
    source: &Path,
    size: u32,
    task: &str,
    base: Option<&str>,
    is_live: &dyn Fn(&str) -> bool,
    startup: Option<&StartupReservation>,
) -> io::Result<TaskSlot> {
    if task.trim().is_empty() {
        return Err(pool_error(
            "a task acquisition requires a non-empty task identity",
        ));
    }
    let pool = pool(source, size)?;
    let (exact, explicit) = resolve_task_base(&pool.source, base)?;
    let requested = explicit.then_some(exact.as_str());
    reconcile_slots(codex_home, &pool, is_live)?;
    if let Some(binding) = find_task_binding(codex_home, &pool, task, requested)? {
        return Ok(binding);
    }
    // Capacity reduction is refused before new acquisition: positions beyond
    // the configured size still hold owned work, so the pool is full at its
    // reduced limit until those bindings are released or explicitly retired.
    // A binding beyond the size continues through the lookup above instead.
    let beyond = held_tasks_beyond(codex_home, &pool.source, size)?;
    if !beyond.is_empty() {
        let held = beyond
            .iter()
            .map(|slot| {
                format!(
                    "slot {} holds task {}",
                    slot.index,
                    slot.task.as_deref().unwrap_or("(recorded session owner)")
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        return Err(pool_error(&format!(
            "worktree_pool_size {size} is below the recorded positions still holding tasks ({held}); release or explicitly retire those bindings before acquiring another task"
        )));
    }
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
        match claim_task_slot(codex_home, &pool, slot.index, task, is_live, startup)? {
            TaskSlotClaim::Claimed(record) => {
                let Some(handle) = record.handle.clone() else {
                    let _ = abort_task_claim(
                        codex_home,
                        &pool,
                        slot.index,
                        task,
                        "",
                        "the claim has no generation handle".into(),
                    );
                    return Err(pool_error(&format!(
                        "slot {} claim of task '{task}' has no generation handle; the lead resolves the record",
                        slot.index
                    )));
                };
                if record.state == SlotState::Occupied {
                    if record.base.as_deref() == Some(exact.as_str()) {
                        // A binding completed between the lookup and the claim.
                        return Ok(TaskSlot::from_record(
                            &record,
                            slot.index,
                            slot.path.clone(),
                            slot.presence,
                        ));
                    }
                    // A concurrent acquisition bound this task at another base;
                    // never reset a binding that may already hold work.
                    return Err(pool_error(&format!(
                        "task '{task}' is bound concurrently in slot {} at base {}; retry to continue the recorded binding",
                        slot.index,
                        record.base.as_deref().unwrap_or("none")
                    )));
                }
                if let Err(error) = reset_slot_to(codex_home, &pool, slot.index, &exact) {
                    // Bookkeeping is best-effort: the preparation failure is
                    // the reported cause, and the slot is not advertised as
                    // free while it still holds unverified state.
                    let _ = abort_task_claim(
                        codex_home,
                        &pool,
                        slot.index,
                        task,
                        &handle,
                        error.to_string(),
                    );
                    return Err(error);
                }
                let bound = bind_task_slot(codex_home, &pool, slot.index, task, &handle, &exact)?;
                return Ok(TaskSlot::from_record(
                    &bound,
                    slot.index,
                    slot.path.clone(),
                    slot.presence,
                ));
            }
            // The same task is already acquiring or holding a position; it
            // must never be walked on to a second slot, so the whole pool
            // acquisition stops with the recorded reason.
            TaskSlotClaim::InFlightSameTask(reason) => return Err(pool_error(&reason)),
            TaskSlotClaim::Unavailable(refusal) => refusals.push(refusal.reason),
        }
    }
    Err(pool_error(&format!(
        "no free slot in the pool of {size} (worktree_pool_size): {}",
        refusals.join("; ")
    )))
}

pub fn continue_task(
    codex_home: &Path,
    source: &Path,
    task: &str,
    handle: Option<&str>,
) -> io::Result<TaskSlot> {
    let root = pool_root(source)?;
    let layout = slot_state_layout(codex_home, &root)?;
    let registered = registered_trees(&root)?;
    let Some((index, record)) = find_record_for_task(&layout, &root, task)? else {
        return Err(pool_error(&format!(
            "no recorded binding for task '{task}' in {}; acquire it first",
            root.display()
        )));
    };
    if let Some(handle) = handle
        && record.handle.as_deref() != Some(handle)
    {
        return Err(pool_error(&format!(
            "task '{task}' binding in slot {index} has generation {} instead of {handle}; the handle is stale after release or reassignment",
            record.handle.as_deref().unwrap_or("none")
        )));
    }
    if record.state == SlotState::Released {
        return Err(pool_error(&format!(
            "task '{task}' was released from slot {index}; acquire it again instead of continuing"
        )));
    }
    if let Some(reserved) = record.startup.as_ref()
        && startup_exclusive(reserved)
        && !startup_matches_current(reserved)
    {
        return Err(pool_error(&pending_startup_reason(
            index,
            record.owner.as_deref(),
            startup_liveness(reserved),
        )));
    }
    // The generic path cannot verify a session binding, so it fails closed on
    // one: a former owner must not continue a task that was reassigned.
    if let Some(owner) = record
        .owner
        .as_deref()
        .filter(|owner| Some(*owner) != record.task.as_deref())
    {
        return Err(pool_error(&format!(
            "task '{task}' in slot {index} is bound to session {owner}; continue it as that session or hand it to another session explicitly"
        )));
    }
    let path = slot_path(&root, index)?;
    if !path.is_dir() {
        return Err(pool_error(&format!(
            "slot {index} directory {} is missing; recover the worktree before continuing task '{task}'",
            path.display()
        )));
    }
    let presence = slot_presence(&path, &registered);
    Ok(TaskSlot::from_record(&record, index, path, presence))
}

/// Validate the session binding of a held task before it is returned or
/// rebound: a live foreign session must be resolved first, while a task-owned
/// binding or a former owner whose session is not live may be rebound.
fn validate_session_binding(
    index: u32,
    task: &str,
    record: &SlotRecord,
    session: &str,
    is_live: &dyn Fn(&str) -> bool,
) -> io::Result<()> {
    if let Some(owner) = record
        .owner
        .as_deref()
        .filter(|owner| Some(*owner) != record.task.as_deref())
        && owner != session
        && is_live(owner)
    {
        return Err(pool_error(&format!(
            "task '{task}' in slot {index} is held by live session {owner}; stop it before continuing as {session}"
        )));
    }
    Ok(())
}

/// Continue one held task as an explicit executor session. The recorded
/// session binding is validated before the checkout is returned: a live
/// foreign session refuses, while a former owner whose session resolved dead
/// is rebound to the asking session with its task, generation handle, base and
/// work preserved. Nothing is reset here.
pub fn continue_task_for_session(
    codex_home: &Path,
    source: &Path,
    task: &str,
    handle: Option<&str>,
    session: &str,
    is_live: &dyn Fn(&str) -> bool,
) -> io::Result<TaskSlot> {
    if session.trim().is_empty() {
        return Err(pool_error(
            "continuing a task for a session requires the executor session identity",
        ));
    }
    let root = pool_root(source)?;
    let layout = slot_state_layout(codex_home, &root)?;
    let registered = registered_trees(&root)?;
    let Some((index, record)) = find_record_for_task(&layout, &root, task)? else {
        return Err(pool_error(&format!(
            "no recorded binding for task '{task}' in {}; acquire it first",
            root.display()
        )));
    };
    if let Some(handle) = handle
        && record.handle.as_deref() != Some(handle)
    {
        return Err(pool_error(&format!(
            "task '{task}' binding in slot {index} has generation {} instead of {handle}; the handle is stale after release or reassignment",
            record.handle.as_deref().unwrap_or("none")
        )));
    }
    if record.state == SlotState::Released {
        return Err(pool_error(&format!(
            "task '{task}' was released from slot {index}; acquire it again instead of continuing"
        )));
    }
    validate_session_binding(index, task, &record, session, is_live)?;
    let path = slot_path(&root, index)?;
    if !path.is_dir() {
        return Err(pool_error(&format!(
            "slot {index} directory {} is missing; recover the worktree before continuing task '{task}'",
            path.display()
        )));
    }
    let _lock = lock_slot(codex_home, &root, index)?;
    let Some(record) = load_record_at(&layout, index)? else {
        return Err(pool_error(&format!(
            "task '{task}' binding in slot {index} disappeared while continuing; retry"
        )));
    };
    refuse_record_conflict(&layout, index)?;
    if let Some(handle) = handle
        && record.handle.as_deref() != Some(handle)
    {
        return Err(pool_error(&format!(
            "task '{task}' binding in slot {index} has generation {} instead of {handle}; the handle is stale after release or reassignment",
            record.handle.as_deref().unwrap_or("none")
        )));
    }
    if record.task.as_deref() != Some(task) || record.state == SlotState::Released {
        return Err(pool_error(&format!(
            "task '{task}' is no longer bound to slot {index}; the binding was reassigned"
        )));
    }
    if let Some(reserved) = record.startup.as_ref()
        && startup_exclusive(reserved)
        && !startup_matches_current(reserved)
    {
        return Err(pool_error(&pending_startup_reason(
            index,
            record.owner.as_deref(),
            startup_liveness(reserved),
        )));
    }
    validate_session_binding(index, task, &record, session, is_live)?;
    let rebound = if record.owner.as_deref() == Some(session) {
        record
    } else {
        let rebound = SlotRecord {
            owner: Some(session.to_owned()),
            ..record
        };
        write_slot_record(&layout.resolved(index), &rebound)?;
        rebound
    };
    let presence = slot_presence(&path, &registered);
    Ok(TaskSlot::from_record(&rebound, index, path, presence))
}

/// Hand a held task binding to an executor session before launch: the session
/// becomes the recorded owner and this process holds the startup reservation
/// through the host handoff, while the task identity, generation handle and
/// exact base stay unchanged. The task's checkout is never released or reset
/// by a handoff. A stale handle, a released binding, a foreign exclusive
/// startup reservation, or a different live session currently owning the
/// binding refuses the transfer.
pub fn handoff_task_to_session(
    codex_home: &Path,
    source: &Path,
    task: &str,
    handle: &str,
    session: &str,
    is_live: &dyn Fn(&str) -> bool,
) -> io::Result<TaskSlot> {
    if session.trim().is_empty() {
        return Err(pool_error(
            "a task handoff requires the executor session identity",
        ));
    }
    let root = pool_root(source)?;
    let layout = slot_state_layout(codex_home, &root)?;
    let startup = current_startup_reservation()?;
    let registered = registered_trees(&root)?;
    for index in recorded_indexes(&layout)? {
        let Some(record) = load_record_at(&layout, index)? else {
            continue;
        };
        if record.task.as_deref() != Some(task) || !same_repository(&record.source, &root) {
            continue;
        }
        refuse_record_conflict(&layout, index)?;
        if record.handle.as_deref() != Some(handle) {
            return Err(pool_error(&format!(
                "task '{task}' binding in slot {index} has generation {} instead of {handle}; the handle is stale",
                record.handle.as_deref().unwrap_or("none")
            )));
        }
        if record.state == SlotState::Released {
            return Err(pool_error(&format!(
                "task '{task}' was released from slot {index}; acquire it again before handoff"
            )));
        }
        validate_session_binding(index, task, &record, session, is_live)?;
        let _lock = lock_slot(codex_home, &root, index)?;
        let Some(record) = load_record_at(&layout, index)? else {
            continue;
        };
        refuse_record_conflict(&layout, index)?;
        if record.task.as_deref() != Some(task)
            || record.handle.as_deref() != Some(handle)
            || record.state == SlotState::Released
        {
            return Err(pool_error(&format!(
                "slot {index} is no longer bound to task '{task}' generation {handle}; the binding was reassigned"
            )));
        }
        // Re-validate the session binding under the lock, immediately before
        // the effect: a live owner that appeared after the preflight must
        // resolve before the binding moves, and a dead former owner is
        // explicitly replaced without reset.
        validate_session_binding(index, task, &record, session, is_live)?;
        if let Some(reserved) = record.startup.as_ref()
            && startup_exclusive(reserved)
            && !startup_matches_current(reserved)
        {
            return Err(pool_error(&pending_startup_reason(
                index,
                record.owner.as_deref(),
                startup_liveness(reserved),
            )));
        }
        let handed = SlotRecord {
            owner: Some(session.to_owned()),
            startup: Some(startup),
            ..record
        };
        write_slot_record(&layout.resolved(index), &handed)?;
        let path = slot_path(&root, index)?;
        let presence = slot_presence(&path, &registered);
        return Ok(TaskSlot::from_record(&handed, index, path, presence));
    }
    Err(pool_error(&format!(
        "no recorded binding for task '{task}' in {}; acquire it before handoff",
        root.display()
    )))
}

// Public release API kept arity-compatible with its existing callers.
#[allow(clippy::too_many_arguments)]
pub fn release_task(
    codex_home: &Path,
    source: &Path,
    task: &str,
    disposition: SlotDisposition,
    reason: &str,
    base: &str,
    handle: Option<&str>,
    is_live: &dyn Fn(&str) -> bool,
) -> io::Result<LaneDisposition> {
    let root = pool_root(source)?;
    let layout = slot_state_layout(codex_home, &root)?;
    let Some((index, binding)) = find_record_for_task(&layout, &root, task)? else {
        return Err(pool_error(&format!(
            "no recorded binding for task '{task}' in {}; nothing was released",
            root.display()
        )));
    };
    release_slot_with(
        codex_home,
        &root,
        &layout,
        index,
        disposition,
        reason,
        ReleaseExpectation::Task { task, handle },
        // Freeze the binding this call looked up, so the same task reclaimed
        // as a new generation before the slot lock is still a different
        // binding and this superseded release is refused.
        Some(ReleaseIdentity::from_record(&binding)),
        base,
        is_live,
    )
}

/// Pool inventory of one repository, shared by every checkout of it. The
/// configured positions are always listed; recorded positions beyond a
/// reduced size stay visible so status and recovery remain usable.
pub fn task_inventory(codex_home: &Path, source: &Path, size: u32) -> io::Result<Vec<TaskSlot>> {
    validate_pool_size(size)?;
    let root = pool_root(source)?;
    let layout = slot_state_layout(codex_home, &root)?;
    let registered = registered_trees(&root)?;
    let mut indexes: Vec<u32> = (1..=size).collect();
    for index in recorded_indexes(&layout)? {
        if !indexes.contains(&index) {
            indexes.push(index);
        }
    }
    indexes.sort_unstable();
    let mut slots = Vec::new();
    for index in indexes {
        let path = slot_path(&root, index)?;
        let presence = slot_presence(&path, &registered);
        let Some(record) = load_record_at(&layout, index)? else {
            slots.push(TaskSlot::free(index, path, presence));
            continue;
        };
        if !same_repository(&record.source, &root) {
            return Err(pool_error(&format!(
                "slot record {index} belongs to another source checkout {}; the lead resolves it",
                record.source.display()
            )));
        }
        let mut slot = TaskSlot::from_record(&record, index, path, presence);
        if let Some(conflict) = layout_record_conflict(&layout, index)? {
            slot.reason = Some(conflict);
        }
        slots.push(slot);
    }
    Ok(slots)
}

/// Recorded bindings beyond a proposed pool size. A capacity reduction is
/// rejected while this returns entries; the files are never touched here, so
/// status, release and authorized retirement remain usable.
pub fn held_tasks_beyond(codex_home: &Path, source: &Path, size: u32) -> io::Result<Vec<TaskSlot>> {
    Ok(task_inventory(codex_home, source, size)?
        .into_iter()
        .filter(|slot| slot.index > size && slot.state != TaskState::Free)
        .collect())
}

/// Slot positions with a record, across the canonical and legacy record
/// directories, so a reduced capacity still shows held work.
fn recorded_indexes(layout: &SlotStateLayout) -> io::Result<Vec<u32>> {
    let mut indexes = BTreeSet::new();
    for dir in std::iter::once(&layout.canonical).chain(layout.legacy.iter()) {
        let Ok(entries) = fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            let Some(digits) = name
                .strip_prefix("slot-")
                .and_then(|rest| rest.strip_suffix(".json"))
            else {
                continue;
            };
            if let Ok(index) = digits.parse::<u32>()
                && index > 0
                && index <= MAX_POOL_SLOTS
            {
                indexes.insert(index);
            }
        }
    }
    Ok(indexes.into_iter().collect())
}

/// Locate the current record of one durable task binding in a repository's
/// pool, refusing conflicting record locations instead of choosing a winner.
fn find_record_for_task(
    layout: &SlotStateLayout,
    root: &Path,
    task: &str,
) -> io::Result<Option<(u32, SlotRecord)>> {
    for index in recorded_indexes(layout)? {
        let Some(record) = load_record_at(layout, index)? else {
            continue;
        };
        if record.task.as_deref() == Some(task) && same_repository(&record.source, root) {
            refuse_record_conflict(layout, index)?;
            return Ok(Some((index, record)));
        }
    }
    Ok(None)
}

/// Find the current binding of one task, refusing a conflicting base request
/// and an in-flight startup reservation of another process. A binding beyond
/// a reduced capacity still continues instead of allocating a new slot.
fn find_task_binding(
    codex_home: &Path,
    pool: &Pool,
    task: &str,
    requested: Option<&str>,
) -> io::Result<Option<TaskSlot>> {
    let layout = slot_state_layout(codex_home, &pool.caller)?;
    let mut indexes: Vec<u32> = pool.slots.iter().map(|slot| slot.index).collect();
    for index in recorded_indexes(&layout)? {
        if !indexes.contains(&index) {
            indexes.push(index);
        }
    }
    indexes.sort_unstable();
    let mut registered: Option<Vec<RegisteredTree>> = None;
    for index in indexes {
        let Some(record) = load_record_at(&layout, index)? else {
            continue;
        };
        if record.task.as_deref() != Some(task)
            || record.state == SlotState::Released
            || !same_repository(&record.source, &pool.source)
        {
            continue;
        }
        refuse_record_conflict(&layout, index)?;
        if let Some(requested) = requested
            && record.base.as_deref() != Some(requested)
        {
            return Err(pool_error(&format!(
                "task '{task}' is held in slot {index} at base {}; the requested base {requested} conflicts with the binding - release it or continue with the recorded base",
                record.base.as_deref().unwrap_or("none")
            )));
        }
        if let Some(reserved) = record.startup.as_ref()
            && startup_exclusive(reserved)
            && !startup_matches_current(reserved)
        {
            return Err(pool_error(&pending_startup_reason(
                index,
                record.owner.as_deref(),
                startup_liveness(reserved),
            )));
        }
        match record.state {
            // A completed binding continues as the same task's work.
            SlotState::Occupied => {}
            // An in-flight acquisition is never returned as a binding: its
            // process may still be resetting this tree, so the caller is
            // refused here and never walks on to allocate the same task a
            // second position. A confirmed-dead reservation resumes through
            // the claim path; this process's own reservation resumes itself.
            SlotState::Synchronizing => {
                let abandoned = record
                    .startup
                    .as_ref()
                    .is_some_and(|reserved| !startup_exclusive(reserved));
                if abandoned {
                    continue;
                }
                if !record.startup.as_ref().is_some_and(startup_matches_current) {
                    let reason = match record.startup.as_ref() {
                        Some(reserved) => pending_startup_reason(
                            index,
                            record.owner.as_deref(),
                            startup_liveness(reserved),
                        ),
                        None => format!(
                            "slot {index} is already acquiring task '{task}' in another process; retry after that acquisition finishes"
                        ),
                    };
                    return Err(pool_error(&reason));
                }
            }
            // Preserved or unclear state without a completed binding: never
            // continue it implicitly and never allocate a second position for
            // the same task.
            SlotState::Free | SlotState::AwaitingReview => {
                return Err(pool_error(&format!(
                    "slot {index} records task '{task}' in state {} without a completed binding; continue it or release it explicitly",
                    state_name(record.state)
                )));
            }
            SlotState::Released => unreachable!("released records are skipped above"),
        }
        let path = slot_path(&pool.source, index)?;
        let presence = if let Some(slot) = pool.slots.iter().find(|slot| slot.index == index) {
            slot.presence
        } else {
            if registered.is_none() {
                registered = Some(registered_trees(&pool.source)?);
            }
            let trees = registered.as_deref().unwrap_or_default();
            slot_presence(&path, trees)
        };
        return Ok(Some(TaskSlot::from_record(&record, index, path, presence)));
    }
    Ok(None)
}

/// Result of one task-position claim attempt. `InFlightSameTask` is kept
/// distinct from an ordinary refusal because the caller must stop walking the
/// pool: the same task already holds or is acquiring a position, and trying
/// later positions would allocate that one owner a second slot.
enum TaskSlotClaim {
    Claimed(SlotRecord),
    Unavailable(SlotRefusal),
    InFlightSameTask(String),
}

/// Claim one pool position for a durable task binding. A task binding is never
/// reclaimed through session liveness: the same task continues it, and only a
/// confirmed-abandoned startup that never admitted work can be recovered by
/// another task. Legacy session records keep their existing liveness rules.
fn claim_task_slot(
    codex_home: &Path,
    pool: &Pool,
    index: u32,
    task: &str,
    is_live: &dyn Fn(&str) -> bool,
    startup: Option<&StartupReservation>,
) -> io::Result<TaskSlotClaim> {
    let slot = pool.slot(index)?;
    let layout = slot_state_layout(codex_home, &pool.caller)?;
    for _ in 0..CLAIM_ATTEMPTS {
        let _lock = lock_slot(codex_home, &pool.source, index)?;
        refuse_record_conflict(&layout, index)?;
        let record_path = layout.resolved(index);
        let previous = load_record_at(&layout, index)?;
        let claim = SlotRecord {
            schema: SLOT_STATE_SCHEMA,
            source: pool.source.clone(),
            index,
            path: slot.path.clone(),
            state: SlotState::Synchronizing,
            owner: Some(task.to_owned()),
            task: Some(task.to_owned()),
            handle: Some(new_task_handle(task)),
            base: previous.as_ref().and_then(|record| record.base.clone()),
            disposition: None,
            reason: None,
            startup: startup.cloned(),
        };
        if create_slot_record(&record_path, &claim)? {
            return Ok(TaskSlotClaim::Claimed(claim));
        }
        let Some(existing) = load_record_at(&layout, index)? else {
            continue;
        };
        if !same_repository(&existing.source, &pool.source) {
            return Err(pool_error(&format!(
                "slot record {} belongs to another source checkout {}; the lead resolves it",
                record_path.display(),
                existing.source.display()
            )));
        }
        if let Some(held) = existing.task.as_deref() {
            if held == task {
                if let Some(reserved) = existing.startup.as_ref()
                    && startup_exclusive(reserved)
                    && !startup_matches_current(reserved)
                {
                    return Ok(TaskSlotClaim::InFlightSameTask(pending_startup_reason(
                        index,
                        existing.owner.as_deref(),
                        startup_liveness(reserved),
                    )));
                }
                match existing.state {
                    // A completed binding for this task: the caller returns it
                    // or reports the recorded base instead of resetting it.
                    SlotState::Occupied => return Ok(TaskSlotClaim::Claimed(existing)),
                    // A confirmed-abandoned acquisition that never bound the
                    // slot resumes through this claim; a competing in-flight
                    // acquisition waits instead of resetting the same tree.
                    SlotState::Synchronizing
                        if existing
                            .startup
                            .as_ref()
                            .is_some_and(|reserved| !startup_exclusive(reserved)) =>
                    {
                        return Ok(TaskSlotClaim::Claimed(existing));
                    }
                    SlotState::Synchronizing => {
                        return Ok(TaskSlotClaim::InFlightSameTask(format!(
                            "slot {index} is already acquiring task '{task}' in another process; retry after that acquisition finishes"
                        )));
                    }
                    // Not a live binding: fall through to re-claiming below.
                    SlotState::Released => {}
                    // Preserved or unclear state without a completed binding:
                    // never silently reclaimed for a fresh acquisition, and
                    // never a reason to place the same task in a second slot.
                    SlotState::Free | SlotState::AwaitingReview => {
                        return Ok(TaskSlotClaim::InFlightSameTask(format!(
                            "slot {index} records task '{task}' in state {} without a completed binding; continue it or release it explicitly",
                            state_name(existing.state)
                        )));
                    }
                }
            }
            // A confirmed-abandoned startup that never admitted work can be
            // recovered; a bound or dirty task slot is never stolen.
            let abandoned = existing
                .startup
                .as_ref()
                .is_some_and(|reserved| startup_liveness(reserved) == ReservationLiveness::Dead);
            if abandoned && existing.state == SlotState::Synchronizing {
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
                                reason: Some(reason.clone()),
                                startup: None,
                                ..existing
                            },
                        )?;
                        return Ok(task_refusal(slot, reason));
                    }
                }
            }
            return Ok(task_refusal(
                slot,
                format!(
                    "slot {index} is held by task '{held}' at base {}; release that task before another acquisition",
                    existing.base.as_deref().unwrap_or("none")
                ),
            ));
        }
        if let Some(reserved) = existing.startup.as_ref() {
            let liveness = startup_liveness(reserved);
            if matches!(
                liveness,
                ReservationLiveness::Live | ReservationLiveness::Uncertain
            ) {
                return Ok(task_refusal(
                    slot,
                    pending_startup_reason(index, existing.owner.as_deref(), liveness),
                ));
            }
        }
        if let Some(live) = existing.owner.as_deref().filter(|owner| is_live(owner)) {
            return Ok(task_refusal(
                slot,
                format!("slot {index} is occupied by live session {live}"),
            ));
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
                        owner: existing.owner.clone(),
                        disposition: None,
                        reason: Some(reason.clone()),
                        startup: None,
                        ..existing
                    },
                )?;
                return Ok(task_refusal(slot, reason));
            }
        }
    }
    Err(pool_error(&format!(
        "slot {index} claim lost a race with another harness process; retry"
    )))
}

/// Bind a claimed position to its task at the synchronized base. The record
/// keeps its generation handle and any startup reservation; a replaced claim
/// is refused instead of bound over.
fn bind_task_slot(
    codex_home: &Path,
    pool: &Pool,
    index: u32,
    task: &str,
    handle: &str,
    base: &str,
) -> io::Result<SlotRecord> {
    let layout = slot_state_layout(codex_home, &pool.caller)?;
    let _lock = lock_slot(codex_home, &pool.source, index)?;
    let record = load_record_at(&layout, index)?
        .ok_or_else(|| pool_error(&format!("slot {index} has no claim to bind")))?;
    if record.task.as_deref() != Some(task) || record.handle.as_deref() != Some(handle) {
        return Err(pool_error(&format!(
            "slot {index} is no longer bound to task '{task}' generation {handle}; the claim was replaced"
        )));
    }
    if let Some(startup) = record.startup.as_ref()
        && startup_exclusive(startup)
        && !startup_matches_current(startup)
    {
        return Err(pool_error(&pending_startup_reason(
            index,
            record.owner.as_deref(),
            startup_liveness(startup),
        )));
    }
    let bound = SlotRecord {
        state: SlotState::Occupied,
        base: Some(base.to_owned()),
        disposition: None,
        reason: None,
        ..record
    };
    write_slot_record(&layout.resolved(index), &bound)?;
    Ok(bound)
}

/// Hand a slot whose task acquisition aborted back to the pool, or preserve it
/// when it still holds unverified work. Only this task's own in-flight claim -
/// the recorded task, generation handle and synchronizing state - is touched,
/// so an acquisition that lost a race cannot clear a completed binding.
fn abort_task_claim(
    codex_home: &Path,
    pool: &Pool,
    index: u32,
    task: &str,
    handle: &str,
    cause: String,
) -> io::Result<()> {
    let layout = slot_state_layout(codex_home, &pool.caller)?;
    let Some(record) = load_record_at(&layout, index)? else {
        return Ok(());
    };
    if record.task.as_deref() != Some(task)
        || record.handle.as_deref() != Some(handle)
        || record.state != SlotState::Synchronizing
    {
        return Ok(());
    }
    if let Some(reserved) = record.startup.as_ref()
        && startup_exclusive(reserved)
        && !startup_matches_current(reserved)
    {
        return Ok(());
    }
    let _lock = lock_slot(codex_home, &pool.source, index)?;
    let Some(record) = load_record_at(&layout, index)? else {
        return Ok(());
    };
    if record.task.as_deref() != Some(task)
        || record.handle.as_deref() != Some(handle)
        || record.state != SlotState::Synchronizing
    {
        return Ok(());
    }
    if let Some(reserved) = record.startup.as_ref()
        && startup_exclusive(reserved)
        && !startup_matches_current(reserved)
    {
        return Ok(());
    }
    let state = match slot_content(pool, index, record.base.as_deref())? {
        SlotContent::Reusable => SlotState::Free,
        SlotContent::Unreviewed(_) | SlotContent::Missing(_) => SlotState::AwaitingReview,
    };
    let retained = state == SlotState::AwaitingReview;
    write_slot_record(
        &layout.resolved(index),
        &SlotRecord {
            state,
            owner: if retained { record.owner.clone() } else { None },
            task: if retained { record.task.clone() } else { None },
            handle: if retained {
                record.handle.clone()
            } else {
                None
            },
            disposition: None,
            reason: Some(format!("task acquisition aborted: {cause}")),
            startup: None,
            ..record
        },
    )
}

/// Generation handle of one task binding: a fresh value per claim, so a handle
/// from a released or reassigned slot can never resume it.
fn new_task_handle(task: &str) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let sequence = COUNTER.fetch_add(1, Ordering::Relaxed);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or_default();
    hash_bytes(format!("{task}:{}:{now}:{sequence}", std::process::id()).as_bytes())
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
    validate_pool_size(size)?;
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
    // One inventory per repository: entering through a linked worktree
    // reports the pool of the primary checkout, not a second nested layout.
    let source = pool_root(source)?;
    let output = git(&source, &["worktree", "list", "--porcelain"])?;
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
        if same_path(path, &source) {
            continue;
        }
        match slot_index(&source, path)? {
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
/// still holds unreviewed work. Only this process's own reservation is
/// released; a live or uncertain reservation of another generation is left
/// untouched and the original caller still reports its own failure.
fn abort_claim(
    codex_home: &Path,
    pool: &Pool,
    index: u32,
    owner: &str,
    cause: String,
) -> io::Result<()> {
    let layout = slot_state_layout(codex_home, &pool.caller)?;
    let Some(record) = load_record_at(&layout, index)? else {
        return Ok(());
    };
    if record.owner.as_deref() != Some(owner) {
        return Ok(());
    }
    if let Some(startup) = record.startup.as_ref()
        && startup_exclusive(startup)
        && !startup_matches_current(startup)
    {
        return Ok(());
    }
    let _lock = lock_slot(codex_home, &pool.source, index)?;
    let Some(record) = load_record_at(&layout, index)? else {
        return Ok(());
    };
    if record.owner.as_deref() != Some(owner) {
        return Ok(());
    }
    if let Some(startup) = record.startup.as_ref()
        && startup_exclusive(startup)
        && !startup_matches_current(startup)
    {
        return Ok(());
    }
    let state = match slot_content(pool, index, record.base.as_deref())? {
        SlotContent::Reusable => SlotState::Free,
        SlotContent::Unreviewed(_) | SlotContent::Missing(_) => SlotState::AwaitingReview,
    };
    write_slot_record(
        &layout.resolved(index),
        &SlotRecord {
            state,
            owner: None,
            disposition: None,
            reason: Some(format!("dispatch aborted: {cause}")),
            startup: None,
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
        task: None,
        handle: None,
        base: None,
        disposition: None,
        reason: None,
        startup: None,
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

/// True when two recorded checkout paths belong to one repository pool: the
/// same path, or two linked worktrees of one Git repository. Legacy records
/// written from a linked checkout keep their old `source` path and are still
/// this repository's records; they are normalized on their next mutation.
fn same_repository(left: &Path, right: &Path) -> bool {
    if same_path(left, right) {
        return true;
    }
    match (repository_identity(left), repository_identity(right)) {
        (Ok(left), Ok(right)) => left.common_dir == right.common_dir,
        _ => false,
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
    #[cfg(windows)]
    fn startup_reservation_excludes_other_dispatches_until_the_creator_is_dead() {
        let root = tempfile::tempdir().unwrap();
        let up = upstream(root.path());
        let home = root.path().join("home");
        let layout = pool(&up.source, 1).unwrap();
        create_slot(&layout, 1).unwrap();
        let dead = |_: &str| false;
        let startup = current_startup_reservation().expect("this process is observable");
        let claimed = claim_slot_for(&home, &layout, 1, "exec-1", &dead, Some(&startup)).unwrap();
        let SlotClaim::Claimed(record) = claimed else {
            panic!("the creating process must claim the slot");
        };
        assert_eq!(
            record.startup.as_ref().map(|startup| startup.pid),
            Some(startup.pid)
        );
        match claim_slot_for(&home, &layout, 1, "exec-2", &dead, None).unwrap() {
            SlotClaim::Unavailable(refusal) => {
                assert!(
                    refusal
                        .reason
                        .contains("in-flight startup of session exec-1"),
                    "{}",
                    refusal.reason
                );
            }
            SlotClaim::Claimed(_) => panic!("a live startup reservation must not be reclaimed"),
        }
        assert!(matches!(
            claim_slot_for(&home, &layout, 1, "exec-1", &dead, Some(&startup)).unwrap(),
            SlotClaim::Claimed(_)
        ));
        let kept = reconcile_slots(&home, &layout, &dead).unwrap();
        assert_eq!(kept[0].state, SlotState::Synchronizing);
        assert_eq!(kept[0].owner.as_deref(), Some("exec-1"));

        let path = slot_record_path(&home, &up.source, 1).unwrap();
        let mut stored = load_slot_record(&home, &up.source, 1).unwrap().unwrap();
        stored.startup = Some(StartupReservation {
            pid: u32::MAX,
            created: 1,
            program: std::env::current_exe().unwrap(),
        });
        fs::write(&path, serde_json::to_vec_pretty(&stored).unwrap()).unwrap();
        let SlotClaim::Claimed(reclaimed) = claim_slot(&home, &layout, 1, "exec-2", &dead).unwrap()
        else {
            panic!("a confirmed-dead creator's clean reservation must be reclaimable");
        };
        assert_eq!(reclaimed.owner.as_deref(), Some("exec-2"));
        assert!(reclaimed.startup.is_none());
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

    #[test]
    fn linked_checkouts_share_one_pool_and_one_state_directory() {
        let root = tempfile::tempdir().unwrap();
        let up = upstream(root.path());
        let home = root.path().join("home");
        let linked = root.path().join("lead");
        git_ok(
            &up.source,
            &[
                "worktree",
                "add",
                "--detach",
                linked.to_str().unwrap(),
                "HEAD",
            ],
        );
        // The linked checkout resolves the same identity and slot layout as
        // the primary checkout, never a nested pool under its own path.
        let layout = pool(&linked, 2).unwrap();
        assert_eq!(layout.source, resolve_source(&up.source).unwrap());
        assert_eq!(layout.slots[0].path, slot_path(&up.source, 1).unwrap());
        assert_eq!(
            pool_state_dir(&home, &up.source).unwrap(),
            pool_state_dir(&home, &linked).unwrap()
        );
        assert_ne!(
            pool_state_dir(&home, &linked).unwrap(),
            legacy_pool_state_dir(&home, &linked).unwrap(),
            "new state never uses the per-path legacy directory"
        );
        let dead = |_: &str| false;
        let base = rev(&up.source);
        let acquired = acquire_task(&home, &linked, 2, "task-1", Some(&base), &dead).unwrap();
        assert_eq!(acquired.index, 1);
        assert_eq!(acquired.path, slot_path(&up.source, 1).unwrap());
        assert_eq!(acquired.path.parent(), up.source.parent());
        assert!(
            slot_record_path(&home, &up.source, 1).unwrap().is_file(),
            "the binding lives in the canonical state directory"
        );
        // The primary checkout sees and continues the same binding.
        let continued =
            continue_task(&home, &up.source, "task-1", acquired.handle.as_deref()).unwrap();
        assert_eq!(continued.index, 1);
        assert_eq!(continued.path, acquired.path);
        assert_eq!(continued.state, TaskState::Held);
        // A second task from the primary checkout takes the next position of
        // the same pool; both positions then refuse a third acquisition.
        let second = acquire_task(&home, &up.source, 2, "task-2", Some(&base), &dead).unwrap();
        assert_eq!(second.index, 2);
        assert_eq!(second.path, slot_path(&up.source, 2).unwrap());
        // The primary checkout, the linked checkout and both slots are
        // registered worktrees of the one repository.
        assert_eq!(registered_count(&up.source), 4);
        let error = acquire_task(&home, &linked, 2, "task-3", Some(&base), &dead).unwrap_err();
        assert!(
            error.to_string().contains("no free slot in the pool of 2"),
            "{error}"
        );
        assert!(
            !root.path().join("lead-wt1").exists(),
            "a caller's linked checkout path must never become a slot root"
        );
        let inventory = task_inventory(&home, &up.source, 2).unwrap();
        assert_eq!(inventory.len(), 2);
        assert_eq!(inventory[0].task.as_deref(), Some("task-1"));
        assert_eq!(inventory[1].task.as_deref(), Some("task-2"));
    }

    #[test]
    fn legacy_records_written_through_a_linked_checkout_import_into_the_shared_pool() {
        let root = tempfile::tempdir().unwrap();
        let up = upstream(root.path());
        let home = root.path().join("home");
        let linked = root.path().join("lead");
        git_ok(
            &up.source,
            &[
                "worktree",
                "add",
                "--detach",
                linked.to_str().unwrap(),
                "HEAD",
            ],
        );
        let dead = |_: &str| false;
        let base = rev(&up.source);
        let acquired = acquire_task(&home, &linked, 2, "task-1", Some(&base), &dead).unwrap();
        let canonical = slot_record_path(&home, &up.source, 1).unwrap();
        assert!(canonical.is_file());
        // Simulate the record a build without pool identity wrote through the
        // linked checkout: keyed by that path and naming it as the source.
        let legacy_dir = legacy_pool_state_dir(&home, &linked).unwrap();
        fs::create_dir_all(&legacy_dir).unwrap();
        let mut record: SlotRecord =
            serde_json::from_slice(&fs::read(&canonical).unwrap()).unwrap();
        record.source = linked.clone();
        fs::write(
            legacy_dir.join("slot-1.json"),
            serde_json::to_vec_pretty(&record).unwrap(),
        )
        .unwrap();
        fs::remove_file(&canonical).unwrap();
        // The primary checkout imports the record under the shared identity.
        let continued =
            continue_task(&home, &up.source, "task-1", acquired.handle.as_deref()).unwrap();
        assert_eq!(continued.index, 1);
        assert_eq!(continued.path, acquired.path);
        assert_eq!(continued.state, TaskState::Held);
        // The linked checkout resolves the same binding, not a second pool.
        let linked_view =
            continue_task(&home, &linked, "task-1", acquired.handle.as_deref()).unwrap();
        assert_eq!(linked_view.path, acquired.path);
        // Mutation migrates in place: the legacy file is updated where it
        // lives and no canonical fork appears.
        let handed = handoff_task_to_session(
            &home,
            &up.source,
            "task-1",
            acquired.handle.as_deref().unwrap(),
            "exec-9",
            &dead,
        )
        .unwrap();
        assert_eq!(handed.owner.as_deref(), Some("exec-9"));
        assert!(
            !canonical.is_file(),
            "an imported legacy record must not be forked into the canonical location"
        );
        assert!(legacy_dir.join("slot-1.json").is_file());
        let stored = load_slot_record(&home, &up.source, 1).unwrap().unwrap();
        assert_eq!(stored.owner.as_deref(), Some("exec-9"));
        assert_eq!(stored.handle, acquired.handle);
        assert_eq!(stored.base.as_deref(), Some(base.as_str()));
    }

    #[test]
    fn conflicting_records_refuse_allocation_and_preserve_both_files() {
        let root = tempfile::tempdir().unwrap();
        let up = upstream(root.path());
        let home = root.path().join("home");
        let linked = root.path().join("lead");
        git_ok(
            &up.source,
            &[
                "worktree",
                "add",
                "--detach",
                linked.to_str().unwrap(),
                "HEAD",
            ],
        );
        let dead = |_: &str| false;
        let base = rev(&up.source);
        create_slot(&pool(&up.source, 1).unwrap(), 1).unwrap();
        let slot_one = slot_path(&up.source, 1).unwrap();
        // Two legacy pools of one repository disagree about the same position.
        let primary = SlotRecord {
            schema: SLOT_STATE_SCHEMA,
            source: up.source.clone(),
            index: 1,
            path: slot_one.clone(),
            state: SlotState::Occupied,
            owner: Some("task-1".into()),
            task: Some("task-1".into()),
            handle: Some("gen-primary".into()),
            base: Some(base.clone()),
            disposition: None,
            reason: None,
            startup: None,
        };
        let legacy = SlotRecord {
            source: linked.clone(),
            handle: Some("gen-linked".into()),
            ..primary.clone()
        };
        let canonical = slot_record_path(&home, &up.source, 1).unwrap();
        fs::create_dir_all(canonical.parent().unwrap()).unwrap();
        let primary_bytes = serde_json::to_vec_pretty(&primary).unwrap();
        fs::write(&canonical, &primary_bytes).unwrap();
        let legacy_dir = legacy_pool_state_dir(&home, &linked).unwrap();
        fs::create_dir_all(&legacy_dir).unwrap();
        let legacy_path = legacy_dir.join("slot-1.json");
        let legacy_bytes = serde_json::to_vec_pretty(&legacy).unwrap();
        fs::write(&legacy_path, &legacy_bytes).unwrap();
        // Status stays usable and names the conflict, never a free position.
        let inventory = task_inventory(&home, &up.source, 1).unwrap();
        assert_eq!(inventory.len(), 1);
        assert!(
            inventory[0]
                .reason
                .as_deref()
                .is_some_and(|reason| reason.contains("conflicting records")),
            "{:?}",
            inventory[0]
        );
        // Every ownership-changing or continuing operation fails closed.
        for error in [
            acquire_task(&home, &up.source, 1, "task-1", Some(&base), &dead)
                .unwrap_err()
                .to_string(),
            acquire_task(&home, &up.source, 1, "task-2", Some(&base), &dead)
                .unwrap_err()
                .to_string(),
            continue_task(&home, &up.source, "task-1", None)
                .unwrap_err()
                .to_string(),
            handoff_task_to_session(&home, &up.source, "task-1", "gen-primary", "exec-1", &dead)
                .unwrap_err()
                .to_string(),
            release_task(
                &home,
                &up.source,
                "task-1",
                SlotDisposition::Discarded,
                "attempted reconcile",
                &base,
                None,
                &dead,
            )
            .unwrap_err()
            .to_string(),
        ] {
            assert!(error.contains("conflicting records"), "{error}");
        }
        // Nothing is moved, rewritten or reset while the conflict stands.
        assert_eq!(fs::read(&canonical).unwrap(), primary_bytes);
        assert_eq!(fs::read(&legacy_path).unwrap(), legacy_bytes);
        assert_eq!(rev(&slot_one), base);
    }

    #[test]
    fn distinct_repositories_and_foreign_slot_paths_are_refused() {
        let root = tempfile::tempdir().unwrap();
        let first_root = root.path().join("first");
        let second_root = root.path().join("second");
        fs::create_dir_all(&first_root).unwrap();
        fs::create_dir_all(&second_root).unwrap();
        let first = upstream(&first_root);
        let second = upstream(&second_root);
        let home = root.path().join("home");
        let dead = |_: &str| false;
        // Different repositories keep different identities and state.
        let identity_a = repository_identity(&first.source).unwrap();
        let identity_b = repository_identity(&second.source).unwrap();
        assert_ne!(identity_a.common_dir, identity_b.common_dir);
        assert_ne!(
            pool_state_dir(&home, &first.source).unwrap(),
            pool_state_dir(&home, &second.source).unwrap()
        );
        // A worktree of the second repository at the first repository's slot
        // position is foreign: refused, never adopted or replaced.
        let collision = slot_path(&first.source, 1).unwrap();
        git_ok(
            &second.source,
            &[
                "worktree",
                "add",
                "--detach",
                collision.to_str().unwrap(),
                "HEAD",
            ],
        );
        let error = pool(&first.source, 1).unwrap_err().to_string();
        assert!(error.contains("refusing to adopt or replace"), "{error}");
        assert!(error.contains("proj-wt1"), "{error}");
        let error = acquire_task(
            &home,
            &first.source,
            1,
            "task-1",
            Some(&rev(&first.source)),
            &dead,
        )
        .unwrap_err();
        assert!(
            error.to_string().contains("refusing to adopt or replace"),
            "{error}"
        );
        // The foreign tree keeps its own registration and is never touched.
        assert!(collision.join("README.md").is_file());
        assert_eq!(registered_count(&second.source), 2);
        // The other repository's own pool is unaffected and usable.
        let own = acquire_task(
            &home,
            &second.source,
            2,
            "task-2",
            Some(&rev(&second.source)),
            &dead,
        )
        .unwrap();
        assert_eq!(own.path, slot_path(&second.source, 1).unwrap());
        assert!(load_slot_record(&home, &first.source, 1).unwrap().is_none());
        assert!(
            load_slot_record(&home, &second.source, 1)
                .unwrap()
                .is_some()
        );
        // A path that is not a Git checkout is refused, not guessed.
        let stray = root.path().join("stray");
        fs::create_dir_all(&stray).unwrap();
        let error = pool(&stray, 1).unwrap_err();
        assert!(
            error.to_string().contains("is not a Git checkout"),
            "{error}"
        );
    }

    #[test]
    fn twenty_sequential_tasks_reuse_three_slots_and_keep_ignored_caches() {
        let root = tempfile::tempdir().unwrap();
        let up = upstream(root.path());
        let home = root.path().join("home");
        let dead = |_: &str| false;
        let base = rev(&up.source);
        let expected: Vec<PathBuf> = (1..=3)
            .map(|index| slot_path(&up.source, index).unwrap())
            .collect();
        for i in 1..=20u32 {
            if i == 4 {
                // All three positions are held: the fourth task is refused
                // without creating another worktree.
                let error =
                    acquire_task(&home, &up.source, 3, "task-4", Some(&base), &dead).unwrap_err();
                assert!(
                    error.to_string().contains("no free slot in the pool of 3"),
                    "{error}"
                );
                assert!(!up.source.parent().unwrap().join("proj-wt4").exists());
            }
            if i > 3 {
                let done = format!("task-{}", i - 3);
                let disposition = release_task(
                    &home,
                    &up.source,
                    &done,
                    SlotDisposition::Merged,
                    &format!("{done} merged"),
                    &base,
                    None,
                    &dead,
                )
                .unwrap();
                assert!(
                    matches!(disposition, LaneDisposition::Reused { .. }),
                    "{done} must release cleanly"
                );
            }
            let task = format!("task-{i}");
            let acquired = acquire_task(&home, &up.source, 3, &task, Some(&base), &dead).unwrap();
            let slot = expected[((i - 1) % 3) as usize].clone();
            assert_eq!(
                acquired.path, slot,
                "{task} must reuse the expected position"
            );
            assert_eq!(acquired.state, TaskState::Held);
            let cache = slot.join("cache");
            fs::create_dir_all(&cache).unwrap();
            fs::write(cache.join(format!("marker-{i}")), "cached build\n").unwrap();
            assert_eq!(
                registered_count(&up.source),
                1 + std::cmp::min(i, 3) as usize,
                "twenty tasks never grow the pool past its three slots"
            );
        }
        for i in 18..=20u32 {
            release_task(
                &home,
                &up.source,
                &format!("task-{i}"),
                SlotDisposition::Merged,
                "final release",
                &base,
                None,
                &dead,
            )
            .unwrap();
        }
        // Every marker written through the earlier cycles survived every
        // reset: ignored build caches are reused, never rebuilt.
        let mut markers = 0;
        for (index, slot) in expected.iter().enumerate() {
            for entry in fs::read_dir(slot.join("cache")).unwrap() {
                let name = entry.unwrap().file_name().to_string_lossy().into_owned();
                let number: u32 = name
                    .strip_prefix("marker-")
                    .and_then(|number| number.parse().ok())
                    .unwrap();
                assert_eq!(
                    ((number - 1) % 3) as usize,
                    index,
                    "{name} must live in the position that served its task"
                );
                markers += 1;
            }
        }
        assert_eq!(markers, 20, "every task's cache marker survives reuse");
        let inventory = task_inventory(&home, &up.source, 3).unwrap();
        assert_eq!(inventory.len(), 3);
        assert!(inventory.iter().all(|slot| slot.state == TaskState::Free));
        assert!(
            inventory
                .iter()
                .all(|slot| slot.presence == SlotPresence::Registered)
        );
    }

    #[test]
    fn a_full_held_pool_refuses_further_acquisition_without_overflow() {
        let root = tempfile::tempdir().unwrap();
        let up = upstream(root.path());
        let home = root.path().join("home");
        let dead = |_: &str| false;
        let base = rev(&up.source);
        let first = acquire_task(&home, &up.source, 2, "task-1", Some(&base), &dead).unwrap();
        let second = acquire_task(&home, &up.source, 2, "task-2", Some(&base), &dead).unwrap();
        fs::write(first.path.join("work-1.txt"), "held work\n").unwrap();
        fs::write(second.path.join("work-2.txt"), "held work\n").unwrap();
        let error = acquire_task(&home, &up.source, 2, "task-3", Some(&base), &dead).unwrap_err();
        assert!(
            error.to_string().contains("no free slot in the pool of 2"),
            "{error}"
        );
        assert!(error.to_string().contains("held by task"), "{error}");
        // Nothing overflows and no held slot is released or cleaned.
        assert!(!up.source.parent().unwrap().join("proj-wt3").exists());
        assert_eq!(registered_count(&up.source), 3);
        assert!(first.path.join("work-1.txt").is_file());
        assert!(second.path.join("work-2.txt").is_file());
        let inventory = task_inventory(&home, &up.source, 2).unwrap();
        assert_eq!(inventory.len(), 2);
        assert!(inventory.iter().all(|slot| slot.state == TaskState::Held));
        // The same tasks continue idempotently instead of being refused.
        let again = acquire_task(&home, &up.source, 2, "task-1", Some(&base), &dead).unwrap();
        assert_eq!(again.handle, first.handle);
        assert_eq!(again.path, first.path);
    }

    #[test]
    fn an_exact_offline_base_never_fetches_while_upstream_refresh_fails() {
        let root = tempfile::tempdir().unwrap();
        let up = upstream(root.path());
        let home = root.path().join("home");
        let dead = |_: &str| false;
        let base = rev(&up.source);
        // The upstream is unreachable: every fetch must fail.
        git_ok(
            &up.source,
            &[
                "remote",
                "set-url",
                "origin",
                &file_url(&root.path().join("gone.git")),
            ],
        );
        let error = refresh_upstream_base(&up.source).unwrap_err().to_string();
        assert!(error.contains("fetch"), "{error}");
        // The explicit local base is prepared without network access.
        let acquired = acquire_task(&home, &up.source, 1, "task-1", Some(&base), &dead).unwrap();
        assert_eq!(acquired.base.as_deref(), Some(base.as_str()));
        assert_eq!(rev(&acquired.path), base);
        // The executor upstream default fails visibly instead of using stale
        // state, and the slot keeps the exact prepared base.
        let layout = pool(&up.source, 1).unwrap();
        let error = synchronize_slot(&home, &layout, 1, None)
            .unwrap_err()
            .to_string();
        assert!(error.contains("fetch"), "{error}");
        assert_eq!(rev(&acquired.path), base, "a failed fetch changes nothing");
        // An explicit-base synchronization stays local too.
        let synchronized = synchronize_slot(&home, &layout, 1, Some(&base)).unwrap();
        assert_eq!(synchronized.base, base);
        assert_eq!(synchronized.previous_head.as_deref(), Some(base.as_str()));
    }

    #[test]
    fn a_conflicting_base_request_is_refused_and_preserves_the_binding() {
        let root = tempfile::tempdir().unwrap();
        let up = upstream(root.path());
        let home = root.path().join("home");
        let dead = |_: &str| false;
        let base = rev(&up.source);
        let acquired = acquire_task(&home, &up.source, 1, "task-1", Some(&base), &dead).unwrap();
        let handle = acquired.handle.clone().unwrap();
        let partial = acquired.path.join("partial.txt");
        fs::write(&partial, "partial work\n").unwrap();
        let head = rev(&acquired.path);
        // A new commit becomes the source HEAD, but the binding stays at base.
        commit(&up.source, "later.txt", "later\n");
        let later = rev(&up.source);
        let error = acquire_task(&home, &up.source, 1, "task-1", Some(&later), &dead)
            .unwrap_err()
            .to_string();
        assert!(error.contains("conflicts with the binding"), "{error}");
        // Nothing moved: the binding, its work and its generation survive.
        assert!(partial.is_file());
        assert_eq!(rev(&acquired.path), head);
        let stored = load_slot_record(&home, &up.source, 1).unwrap().unwrap();
        assert_eq!(stored.base.as_deref(), Some(base.as_str()));
        assert_eq!(stored.handle.as_deref(), Some(handle.as_str()));
        // Repeating the same acquisition continues the binding idempotently.
        let continued = acquire_task(&home, &up.source, 1, "task-1", Some(&base), &dead).unwrap();
        assert_eq!(continued.handle.as_deref(), Some(handle.as_str()));
        assert_eq!(continued.path, acquired.path);
        assert!(partial.is_file());
        // Without a requested base the recorded binding is returned unchanged.
        let silent = acquire_task(&home, &up.source, 1, "task-1", None, &dead).unwrap();
        assert_eq!(silent.base.as_deref(), Some(base.as_str()));
        assert_eq!(silent.handle.as_deref(), Some(handle.as_str()));
    }

    #[test]
    fn stale_generation_handles_cannot_continue_release_or_hand_off() {
        let root = tempfile::tempdir().unwrap();
        let up = upstream(root.path());
        let home = root.path().join("home");
        let dead = |_: &str| false;
        let base = rev(&up.source);
        let acquired = acquire_task(&home, &up.source, 1, "task-1", Some(&base), &dead).unwrap();
        let handle = acquired.handle.clone().unwrap();
        for error in [
            continue_task(&home, &up.source, "task-1", Some("stale-handle"))
                .unwrap_err()
                .to_string(),
            release_task(
                &home,
                &up.source,
                "task-1",
                SlotDisposition::Discarded,
                "discarded",
                &base,
                Some("stale-handle"),
                &dead,
            )
            .unwrap_err()
            .to_string(),
            handoff_task_to_session(&home, &up.source, "task-1", "stale-handle", "exec-9", &dead)
                .unwrap_err()
                .to_string(),
        ] {
            assert!(error.contains("stale"), "{error}");
        }
        // The recorded binding is untouched and continues with its handle.
        let continued = continue_task(&home, &up.source, "task-1", Some(&handle)).unwrap();
        assert_eq!(continued.handle.as_deref(), Some(handle.as_str()));
        assert_eq!(continued.index, 1);
        // A repair read without a handle still finds the binding.
        assert_eq!(
            continue_task(&home, &up.source, "task-1", None)
                .unwrap()
                .index,
            1
        );
        // After the explicit release the old handle is gone with the binding.
        let disposition = release_task(
            &home,
            &up.source,
            "task-1",
            SlotDisposition::Discarded,
            "discarded",
            &base,
            Some(&handle),
            &dead,
        )
        .unwrap();
        assert!(matches!(disposition, LaneDisposition::Reused { .. }));
        let error = continue_task(&home, &up.source, "task-1", Some(&handle))
            .unwrap_err()
            .to_string();
        assert!(error.contains("no recorded binding"), "{error}");
        let error = handoff_task_to_session(&home, &up.source, "task-1", &handle, "exec-9", &dead)
            .unwrap_err()
            .to_string();
        assert!(error.contains("no recorded binding"), "{error}");
    }

    #[test]
    fn a_failed_release_keeps_the_task_bound_until_recovery() {
        let root = tempfile::tempdir().unwrap();
        let up = upstream(root.path());
        let home = root.path().join("home");
        let dead = |_: &str| false;
        let base = rev(&up.source);
        let acquired = acquire_task(&home, &up.source, 1, "task-1", Some(&base), &dead).unwrap();
        let handle = acquired.handle.clone().unwrap();
        let pristine = fs::read_to_string(acquired.path.join("README.md")).unwrap();
        // A held index lock makes the release reset fail, so the slot must be
        // preserved instead of advertised as free.
        let lock_hint = git(&acquired.path, &["rev-parse", "--git-path", "index.lock"]).unwrap();
        let lock_hint = PathBuf::from(lock_hint.trim());
        let lock_path = if lock_hint.is_absolute() {
            lock_hint
        } else {
            acquired.path.join(lock_hint)
        };
        fs::write(&lock_path, "held\n").unwrap();
        let disposition = release_task(
            &home,
            &up.source,
            "task-1",
            SlotDisposition::Merged,
            "merged",
            &base,
            Some(&handle),
            &dead,
        )
        .unwrap();
        let LaneDisposition::Preserved { limitation } = disposition else {
            panic!("a failed reset must preserve the slot");
        };
        assert!(!limitation.is_empty());
        // The record keeps the task, its generation and a visible reason.
        let stored = load_slot_record(&home, &up.source, 1).unwrap().unwrap();
        assert_eq!(stored.state, SlotState::AwaitingReview);
        assert_eq!(stored.task.as_deref(), Some("task-1"));
        assert_eq!(stored.handle.as_deref(), Some(handle.as_str()));
        assert!(
            stored
                .reason
                .as_deref()
                .is_some_and(|reason| !reason.is_empty())
        );
        // The slot is not advertised as free and refuses another task.
        let inventory = task_inventory(&home, &up.source, 1).unwrap();
        assert_eq!(inventory[0].state, TaskState::AwaitingReview);
        let error = acquire_task(&home, &up.source, 1, "task-2", Some(&base), &dead)
            .unwrap_err()
            .to_string();
        assert!(error.contains("no free slot in the pool of 1"), "{error}");
        // The interrupted task still continues and keeps the obstruction.
        let continued = continue_task(&home, &up.source, "task-1", Some(&handle)).unwrap();
        assert_eq!(continued.state, TaskState::AwaitingReview);
        assert!(lock_path.is_file());
        // Recovery clears the obstruction; the explicit release then succeeds.
        fs::remove_file(&lock_path).unwrap();
        let disposition = release_task(
            &home,
            &up.source,
            "task-1",
            SlotDisposition::Merged,
            "merged after review",
            &base,
            Some(&handle),
            &dead,
        )
        .unwrap();
        assert!(matches!(disposition, LaneDisposition::Reused { .. }));
        assert_eq!(
            task_inventory(&home, &up.source, 1).unwrap()[0].state,
            TaskState::Free
        );
        assert_eq!(
            fs::read_to_string(acquired.path.join("README.md")).unwrap(),
            pristine
        );
        // A process-loss boundary at the release reset itself: after its
        // preflight, the slot stops being a Git worktree before the reset can
        // run. The durable record must stay held with the original error, and
        // the checkout must survive for recovery.
        let second = acquire_task(&home, &up.source, 1, "task-2", Some(&base), &dead).unwrap();
        let second_handle = second.handle.clone().unwrap();
        fs::write(second.path.join("partial-2.txt"), "unreviewed\n").unwrap();
        let record_path = slot_record_path(&home, &up.source, 1).unwrap();
        let mut stored = load_slot_record(&home, &up.source, 1).unwrap().unwrap();
        stored.owner = Some("exec-gone".into());
        fs::write(&record_path, serde_json::to_vec_pretty(&stored).unwrap()).unwrap();
        let git_pointer = second.path.join(".git");
        let stashed_pointer = second.path.join(".git.stashed");
        let calls = std::cell::Cell::new(0u32);
        let fault_after_preflight = |_: &str| {
            let call = calls.get() + 1;
            calls.set(call);
            if call == 2 {
                fs::rename(&git_pointer, &stashed_pointer).unwrap();
            }
            false
        };
        let failed = release_task(
            &home,
            &up.source,
            "task-2",
            SlotDisposition::Merged,
            "merged",
            &base,
            Some(&second_handle),
            &fault_after_preflight,
        )
        .unwrap_err();
        assert!(failed.to_string().contains("rev-parse"), "{failed}");
        let stored = load_slot_record(&home, &up.source, 1).unwrap().unwrap();
        assert_eq!(stored.task.as_deref(), Some("task-2"));
        assert_eq!(stored.handle.as_deref(), Some(second_handle.as_str()));
        assert!(
            stored
                .reason
                .as_deref()
                .is_some_and(|reason| reason.contains("release failed")),
            "{:?}",
            stored.reason
        );
        assert!(second.path.join("partial-2.txt").is_file());
        assert_eq!(
            task_inventory(&home, &up.source, 1).unwrap()[0].state,
            TaskState::AwaitingReview
        );
        // A dead former owner's interrupted binding is rebound explicitly and
        // still continues without reset.
        let continued = continue_task_for_session(
            &home,
            &up.source,
            "task-2",
            Some(&second_handle),
            "exec-next",
            &dead,
        )
        .unwrap();
        assert_eq!(continued.state, TaskState::AwaitingReview);
        assert_eq!(continued.owner.as_deref(), Some("exec-next"));
        // Restoring the checkout lets the recovery release complete.
        fs::rename(&stashed_pointer, &git_pointer).unwrap();
        let disposition = release_task(
            &home,
            &up.source,
            "task-2",
            SlotDisposition::Merged,
            "merged after recovery",
            &base,
            Some(&second_handle),
            &dead,
        )
        .unwrap();
        assert!(matches!(disposition, LaneDisposition::Reused { .. }));
        assert_eq!(
            task_inventory(&home, &up.source, 1).unwrap()[0].state,
            TaskState::Free
        );
    }

    #[test]
    fn unknown_task_record_schema_is_refused_without_mutation() {
        let root = tempfile::tempdir().unwrap();
        let up = upstream(root.path());
        let home = root.path().join("home");
        let base = rev(&up.source);
        let dead = |_: &str| false;
        let held = acquire_task(&home, &up.source, 1, "schema-task", Some(&base), &dead).unwrap();
        let path = slot_record_path(&home, &up.source, 1).unwrap();
        let mut record: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        record["ownershipSchema"] = serde_json::json!(999);
        let unsupported = serde_json::to_vec(&record).unwrap();
        fs::write(&path, &unsupported).unwrap();

        let result = continue_task(&home, &up.source, "schema-task", held.handle.as_deref());
        assert!(
            result.is_err(),
            "unsupported ownership schema was accepted: {result:?}"
        );
        assert_eq!(fs::read(&path).unwrap(), unsupported);
    }

    #[test]
    fn stale_release_cannot_reset_a_task_acquired_after_its_preflight() {
        let root = tempfile::tempdir().unwrap();
        let up = upstream(root.path());
        let home = root.path().join("home");
        let base = rev(&up.source);
        let dead = |_: &str| false;
        let original = acquire_task(&home, &up.source, 1, "original", Some(&base), &dead).unwrap();
        let marker = original.path.join("unreviewed-successor.txt");
        let swapped = std::cell::Cell::new(false);
        // Deterministically schedule a release and a new acquisition between
        // the original caller's binding preflight and its slot-lock acquisition.
        let concurrent_transition = |_: &str| {
            if !swapped.replace(true) {
                release_task(
                    &home,
                    &up.source,
                    "original",
                    SlotDisposition::Discarded,
                    "reviewed original task",
                    &base,
                    original.handle.as_deref(),
                    &dead,
                )
                .unwrap();
                acquire_task(&home, &up.source, 1, "successor", Some(&base), &dead).unwrap();
                fs::write(&marker, "unfinished successor work\n").unwrap();
            }
            false
        };

        let stale_result = release_task(
            &home,
            &up.source,
            "original",
            SlotDisposition::Merged,
            "stale caller",
            &base,
            original.handle.as_deref(),
            &concurrent_transition,
        );
        assert!(
            swapped.get(),
            "the controlled ownership transition was not exercised"
        );
        assert!(
            marker.is_file(),
            "the stale release deleted the successor's unreviewed file"
        );
        assert!(
            stale_result.is_err(),
            "stale release succeeded: {stale_result:?}"
        );
        let current = continue_task(&home, &up.source, "successor", None).unwrap();
        assert_eq!(current.task.as_deref(), Some("successor"));
    }

    #[test]
    fn handleless_release_cannot_reset_a_reacquired_generation_of_the_same_task() {
        let root = tempfile::tempdir().unwrap();
        let up = upstream(root.path());
        let home = root.path().join("home");
        let base = rev(&up.source);
        let dead = |_: &str| false;
        let original = acquire_task(&home, &up.source, 1, "same-task", Some(&base), &dead).unwrap();
        let marker = original.path.join("unreviewed-new-generation.txt");
        let swapped = std::cell::Cell::new(false);
        let concurrent_transition = |_: &str| {
            if !swapped.replace(true) {
                release_task(
                    &home,
                    &up.source,
                    "same-task",
                    SlotDisposition::Discarded,
                    "reviewed original generation",
                    &base,
                    original.handle.as_deref(),
                    &dead,
                )
                .unwrap();
                let successor =
                    acquire_task(&home, &up.source, 1, "same-task", Some(&base), &dead).unwrap();
                assert_ne!(successor.handle, original.handle);
                fs::write(&marker, "unfinished new-generation work\n").unwrap();
            }
            false
        };
        let stale_result = release_task(
            &home,
            &up.source,
            "same-task",
            SlotDisposition::Merged,
            "stale caller without an explicit handle",
            &base,
            None,
            &concurrent_transition,
        );
        assert!(swapped.get(), "the generation transition was not exercised");
        assert!(
            marker.is_file(),
            "stale release deleted the new generation's work"
        );
        assert!(
            stale_result.is_err(),
            "stale release succeeded: {stale_result:?}"
        );
    }

    #[test]
    fn stale_session_release_cannot_reset_a_successor_session() {
        let root = tempfile::tempdir().unwrap();
        let up = upstream(root.path());
        let home = root.path().join("home");
        let base = rev(&up.source);
        let dead = |_: &str| false;
        let original = acquire_slot(&home, &up.source, 1, "original", Some(&base), &dead).unwrap();
        let layout = pool(&up.source, 1).unwrap();
        let marker = original.path.join("unreviewed-successor-session.txt");
        let swapped = std::cell::Cell::new(false);
        let concurrent_transition = |_: &str| {
            if !swapped.replace(true) {
                release_slot(
                    &home,
                    &layout,
                    1,
                    SlotDisposition::Discarded,
                    "reviewed original session",
                    &base,
                    &dead,
                )
                .unwrap();
                acquire_slot(&home, &up.source, 1, "successor", Some(&base), &dead).unwrap();
                fs::write(&marker, "unfinished successor session work\n").unwrap();
            }
            false
        };
        let stale_result = release_slot(
            &home,
            &layout,
            1,
            SlotDisposition::Merged,
            "stale session release",
            &base,
            &concurrent_transition,
        );
        assert!(swapped.get(), "the session transition was not exercised");
        assert!(
            marker.is_file(),
            "stale release deleted the successor session's work"
        );
        assert!(
            stale_result.is_err(),
            "stale release succeeded: {stale_result:?}"
        );
    }

    #[test]
    fn a_shrunk_pool_keeps_held_slots_visible_and_never_releases_work() {
        let root = tempfile::tempdir().unwrap();
        let up = upstream(root.path());
        let home = root.path().join("home");
        let dead = |_: &str| false;
        let base = rev(&up.source);
        let mut handles = Vec::new();
        for i in 1..=3u32 {
            let acquired = acquire_task(
                &home,
                &up.source,
                3,
                &format!("task-{i}"),
                Some(&base),
                &dead,
            )
            .unwrap();
            assert_eq!(acquired.index, i);
            handles.push(acquired.handle.clone().unwrap());
            fs::write(acquired.path.join(format!("work-{i}.txt")), "unreviewed\n").unwrap();
        }
        // A reduced size does not release or hide the third held slot.
        let beyond = held_tasks_beyond(&home, &up.source, 2).unwrap();
        assert_eq!(beyond.len(), 1);
        assert_eq!(beyond[0].index, 3);
        assert_eq!(beyond[0].task.as_deref(), Some("task-3"));
        assert_eq!(beyond[0].state, TaskState::Held);
        let inventory = task_inventory(&home, &up.source, 2).unwrap();
        assert_eq!(inventory.len(), 3);
        assert_eq!(inventory[2].index, 3);
        assert_eq!(inventory[2].state, TaskState::Held);
        // A new task cannot take either of the two remaining positions: the
        // capacity reduction is refused before allocation while the owned
        // position beyond the reduced size exists.
        let error = acquire_task(&home, &up.source, 2, "task-4", Some(&base), &dead)
            .unwrap_err()
            .to_string();
        assert!(error.contains("worktree_pool_size 2"), "{error}");
        assert!(error.contains("slot 3"), "{error}");
        // The recorded binding beyond the reduced size still continues.
        let continued = acquire_task(&home, &up.source, 2, "task-3", Some(&base), &dead).unwrap();
        assert_eq!(continued.index, 3);
        assert_eq!(continued.handle.as_deref(), Some(handles[2].as_str()));
        assert!(continued.path.join("work-3.txt").is_file());
        assert_eq!(registered_count(&up.source), 4);
        // Capacity returns only through explicit releases, never automatically.
        // Releasing the in-range tasks leaves the beyond-range binding owned,
        // so the reduced capacity still refuses new acquisition with both low
        // positions free.
        for i in 1..=2u32 {
            release_task(
                &home,
                &up.source,
                &format!("task-{i}"),
                SlotDisposition::Discarded,
                "reviewed",
                &base,
                Some(&handles[(i - 1) as usize]),
                &dead,
            )
            .unwrap();
        }
        let error = acquire_task(&home, &up.source, 2, "task-4", Some(&base), &dead)
            .unwrap_err()
            .to_string();
        assert!(error.contains("worktree_pool_size 2"), "{error}");
        assert!(error.contains("slot 3"), "{error}");
        assert!(error.contains("task-3"), "{error}");
        // Releasing the beyond-range binding restores acquisition inside the
        // reduced capacity without a replacement worktree.
        release_task(
            &home,
            &up.source,
            "task-3",
            SlotDisposition::Discarded,
            "reviewed",
            &base,
            Some(&handles[2]),
            &dead,
        )
        .unwrap();
        assert!(held_tasks_beyond(&home, &up.source, 2).unwrap().is_empty());
        let fourth = acquire_task(&home, &up.source, 2, "task-4", Some(&base), &dead).unwrap();
        assert_eq!(fourth.index, 1);
        assert_eq!(registered_count(&up.source), 4);
        release_task(
            &home,
            &up.source,
            "task-4",
            SlotDisposition::Discarded,
            "reviewed",
            &base,
            Some(fourth.handle.as_deref().unwrap()),
            &dead,
        )
        .unwrap();
        assert!(
            task_inventory(&home, &up.source, 3)
                .unwrap()
                .iter()
                .all(|slot| slot.state == TaskState::Free)
        );
    }

    #[test]
    fn an_inflight_same_task_claim_never_allocates_a_second_slot() {
        let root = tempfile::tempdir().unwrap();
        let up = upstream(root.path());
        let home = root.path().join("home");
        let dead = |_: &str| false;
        let base = rev(&up.source);
        let pool = pool(&up.source, 2).unwrap();
        let slot = create_slot(&pool, 1).unwrap();
        let path = slot_record_path(&home, &up.source, 1).unwrap();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let in_flight = SlotRecord {
            schema: SLOT_STATE_SCHEMA,
            source: up.source.clone(),
            index: 1,
            path: slot,
            state: SlotState::Synchronizing,
            owner: Some("task-1".into()),
            task: Some("task-1".into()),
            handle: Some("gen-inflight".into()),
            base: Some(base.clone()),
            disposition: None,
            reason: None,
            startup: None,
        };
        fs::write(&path, serde_json::to_vec_pretty(&in_flight).unwrap()).unwrap();
        // The same task is already being acquired elsewhere: the caller gets
        // the in-flight refusal and never walks on to a second slot.
        let error = acquire_task(&home, &up.source, 2, "task-1", Some(&base), &dead)
            .unwrap_err()
            .to_string();
        assert!(error.contains("already acquiring task 'task-1'"), "{error}");
        assert_eq!(registered_count(&up.source), 2);
        let inventory = task_inventory(&home, &up.source, 2).unwrap();
        assert_eq!(inventory[0].state, TaskState::Held);
        assert_eq!(inventory[1].state, TaskState::Free);
        // A different task is unaffected by the same-task refusal.
        let other = acquire_task(&home, &up.source, 2, "task-2", Some(&base), &dead).unwrap();
        assert_eq!(other.index, 2);
        assert_eq!(registered_count(&up.source), 3);
        // The recorded in-flight binding still continues with its handle.
        let continued = continue_task(&home, &up.source, "task-1", Some("gen-inflight")).unwrap();
        assert_eq!(continued.state, TaskState::Held);
        assert_eq!(continued.index, 1);
    }

    #[test]
    fn a_confirmed_abandoned_task_reservation_is_recovered_without_overflow() {
        let root = tempfile::tempdir().unwrap();
        let up = upstream(root.path());
        let home = root.path().join("home");
        let dead = |_: &str| false;
        let base = rev(&up.source);
        let pool = pool(&up.source, 1).unwrap();
        let slot = create_slot(&pool, 1).unwrap();
        let path = slot_record_path(&home, &up.source, 1).unwrap();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let abandoned = SlotRecord {
            schema: SLOT_STATE_SCHEMA,
            source: up.source.clone(),
            index: 1,
            path: slot,
            state: SlotState::Synchronizing,
            owner: Some("dead-task".into()),
            task: Some("dead-task".into()),
            handle: Some("gen-dead".into()),
            base: Some(base.clone()),
            disposition: None,
            reason: None,
            startup: Some(StartupReservation {
                pid: u32::MAX,
                created: 1,
                program: std::env::current_exe().unwrap(),
            }),
        };
        fs::write(&path, serde_json::to_vec_pretty(&abandoned).unwrap()).unwrap();
        // The reservation never admitted work (clean tree at the exact base),
        // so a new acquisition recovers the position instead of overflowing.
        let acquired = acquire_task(&home, &up.source, 1, "live-task", Some(&base), &dead).unwrap();
        assert_eq!(acquired.index, 1);
        assert_eq!(acquired.task.as_deref(), Some("live-task"));
        assert_eq!(registered_count(&up.source), 2);
        // The abandoned binding is gone with its generation.
        let error = continue_task(&home, &up.source, "dead-task", Some("gen-dead"))
            .unwrap_err()
            .to_string();
        assert!(error.contains("no recorded binding"), "{error}");
    }

    #[test]
    fn concurrent_acquisitions_of_one_task_never_allocate_a_second_slot() {
        let root = tempfile::tempdir().unwrap();
        let up = upstream(root.path());
        let home = root.path().join("home");
        let base = rev(&up.source);
        let pool = pool(&up.source, 2).unwrap();
        create_slot(&pool, 1).unwrap();
        let barrier = std::sync::Barrier::new(4);
        let results: Vec<std::io::Result<TaskSlot>> = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..4)
                .map(|_| {
                    scope.spawn(|| {
                        let dead = |_: &str| false;
                        barrier.wait();
                        acquire_task(&home, &up.source, 2, "race-task", Some(&base), &dead)
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|handle| handle.join().unwrap())
                .collect()
        });
        let mut indexes = Vec::new();
        for result in results {
            match result {
                Ok(slot) => indexes.push(slot.index),
                Err(error) => {
                    let message = error.to_string();
                    // A loser observes the same-task in-flight claim, or the
                    // winner's record between its atomic publication and an
                    // unlocked read. Any other outcome means the acquisition
                    // walked on and could have allocated the task twice.
                    assert!(
                        message.contains("already acquiring task 'race-task'")
                            || message.contains("is unreadable"),
                        "unexpected concurrent refusal: {message}"
                    );
                }
            }
        }
        assert!(
            !indexes.is_empty(),
            "one concurrent caller must acquire the task"
        );
        assert!(
            indexes.iter().all(|index| *index == 1),
            "one task acquired more than one slot: {indexes:?}"
        );
        assert_eq!(registered_count(&up.source), 2);
        let inventory = task_inventory(&home, &up.source, 2).unwrap();
        assert_eq!(inventory[0].state, TaskState::Held);
        assert_eq!(inventory[0].task.as_deref(), Some("race-task"));
        assert_eq!(inventory[1].state, TaskState::Free);
        let continued = continue_task(
            &home,
            &up.source,
            "race-task",
            inventory[0].handle.as_deref(),
        )
        .unwrap();
        assert_eq!(continued.index, 1);
        assert_eq!(continued.path, slot_path(&up.source, 1).unwrap());
    }

    #[test]
    fn concurrent_different_tasks_compete_for_the_last_slot_without_overflow() {
        let root = tempfile::tempdir().unwrap();
        let up = upstream(root.path());
        let home = root.path().join("home");
        let base = rev(&up.source);
        let pool = pool(&up.source, 1).unwrap();
        create_slot(&pool, 1).unwrap();
        let barrier = std::sync::Barrier::new(2);
        let barrier = &barrier;
        let home_ref = &home;
        let source = &up.source;
        let base_ref = base.as_str();
        let results: Vec<std::io::Result<TaskSlot>> = std::thread::scope(|scope| {
            let handles: Vec<_> = ["task-a", "task-b"]
                .into_iter()
                .map(|task| {
                    scope.spawn(move || {
                        let dead = |_: &str| false;
                        barrier.wait();
                        acquire_task(home_ref, source, 1, task, Some(base_ref), &dead)
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|handle| handle.join().unwrap())
                .collect()
        });
        let winners: Vec<&std::io::Result<TaskSlot>> =
            results.iter().filter(|result| result.is_ok()).collect();
        assert_eq!(winners.len(), 1, "exactly one task must win the last slot");
        let loser = results.iter().find(|result| result.is_err()).unwrap();
        let message = loser.as_ref().unwrap_err().to_string();
        assert!(
            message.contains("no free slot in the pool of 1") || message.contains("is unreadable"),
            "unexpected last-slot refusal: {message}"
        );
        assert_eq!(registered_count(&up.source), 2, "no overflow worktree");
        let inventory = task_inventory(&home, &up.source, 1).unwrap();
        assert_eq!(inventory.len(), 1);
        assert_eq!(inventory[0].state, TaskState::Held);
        assert_eq!(
            inventory[0].task.as_deref(),
            winners[0].as_ref().unwrap().task.as_deref()
        );
    }

    #[test]
    fn handoff_and_continuation_refuse_a_live_former_owner_and_rebind_a_dead_one() {
        let root = tempfile::tempdir().unwrap();
        let up = upstream(root.path());
        let home = root.path().join("home");
        let dead = |_: &str| false;
        let base = rev(&up.source);
        let acquired = acquire_task(&home, &up.source, 1, "task-1", Some(&base), &dead).unwrap();
        let handle = acquired.handle.clone().unwrap();
        let partial = acquired.path.join("partial.txt");
        fs::write(&partial, "partial work\n").unwrap();
        let head = rev(&acquired.path);
        // Hand the held task to a live executor session.
        let live_a = |session: &str| session == "exec-a";
        let handed =
            handoff_task_to_session(&home, &up.source, "task-1", &handle, "exec-a", &live_a)
                .unwrap();
        assert_eq!(handed.owner.as_deref(), Some("exec-a"));
        assert_eq!(handed.handle.as_deref(), Some(handle.as_str()));
        // A live foreign session cannot be handed over or continued around.
        let error =
            handoff_task_to_session(&home, &up.source, "task-1", &handle, "exec-b", &live_a)
                .unwrap_err()
                .to_string();
        assert!(error.contains("live session exec-a"), "{error}");
        let error = continue_task_for_session(
            &home,
            &up.source,
            "task-1",
            Some(&handle),
            "exec-b",
            &live_a,
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("live session exec-a"), "{error}");
        let error = continue_task(&home, &up.source, "task-1", Some(&handle))
            .unwrap_err()
            .to_string();
        assert!(error.contains("session exec-a"), "{error}");
        // The owner session itself continues its binding unchanged.
        let same = continue_task_for_session(
            &home,
            &up.source,
            "task-1",
            Some(&handle),
            "exec-a",
            &live_a,
        )
        .unwrap();
        assert_eq!(same.owner.as_deref(), Some("exec-a"));
        assert_eq!(rev(&acquired.path), head);
        assert!(partial.is_file());
        // A former owner that is gone no longer blocks an explicit rebind: the
        // work, generation handle and base survive without reset.
        let rebound =
            handoff_task_to_session(&home, &up.source, "task-1", &handle, "exec-b", &dead).unwrap();
        assert_eq!(rebound.owner.as_deref(), Some("exec-b"));
        assert_eq!(rebound.handle.as_deref(), Some(handle.as_str()));
        assert_eq!(rebound.base.as_deref(), Some(base.as_str()));
        assert_eq!(rebound.state, TaskState::Held);
        assert_eq!(rev(&acquired.path), head);
        assert!(partial.is_file());
        // The newly bound session continues; the stale former owner is refused
        // while its successor is live.
        let live_b = |session: &str| session == "exec-b";
        let continued = continue_task_for_session(
            &home,
            &up.source,
            "task-1",
            Some(&handle),
            "exec-b",
            &live_b,
        )
        .unwrap();
        assert_eq!(continued.owner.as_deref(), Some("exec-b"));
        let error = continue_task_for_session(
            &home,
            &up.source,
            "task-1",
            Some(&handle),
            "exec-a",
            &live_b,
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("live session exec-b"), "{error}");
        // A session release must not clear a durable task binding by position.
        let single = pool(&up.source, 1).unwrap();
        let error = release_slot(
            &home,
            &single,
            1,
            SlotDisposition::Discarded,
            "session release",
            &base,
            &dead,
        )
        .unwrap_err()
        .to_string();
        assert!(
            error.contains("session release must not clear a durable task binding"),
            "{error}"
        );
        assert!(partial.is_file());
        // The explicit task release ends the binding and reuses the tree.
        let disposition = release_task(
            &home,
            &up.source,
            "task-1",
            SlotDisposition::Merged,
            "merged",
            &base,
            Some(&handle),
            &dead,
        )
        .unwrap();
        assert!(matches!(disposition, LaneDisposition::Reused { .. }));
    }

    #[test]
    fn a_released_and_reassigned_slot_refuses_the_old_session_continuation() {
        let root = tempfile::tempdir().unwrap();
        let up = upstream(root.path());
        let home = root.path().join("home");
        let dead = |_: &str| false;
        let base = rev(&up.source);
        let acquired = acquire_task(&home, &up.source, 1, "task-1", Some(&base), &dead).unwrap();
        let handle = acquired.handle.clone().unwrap();
        let live = |session: &str| session == "exec-1";
        handoff_task_to_session(&home, &up.source, "task-1", &handle, "exec-1", &live).unwrap();
        release_startup_reservation(&home, &up.source, 1, "exec-1").unwrap();
        release_task(
            &home,
            &up.source,
            "task-1",
            SlotDisposition::Merged,
            "accepted",
            &base,
            Some(&handle),
            &dead,
        )
        .unwrap();
        acquire_task(&home, &up.source, 1, "task-2", Some(&base), &dead).unwrap();
        // The old conversation's binding must not resume into the reassigned
        // slot, by any of the continuation entry points.
        for error in [
            continue_task_for_session(&home, &up.source, "task-1", Some(&handle), "exec-1", &live)
                .unwrap_err()
                .to_string(),
            handoff_task_to_session(&home, &up.source, "task-1", &handle, "exec-1", &live)
                .unwrap_err()
                .to_string(),
            continue_task(&home, &up.source, "task-1", Some(&handle))
                .unwrap_err()
                .to_string(),
        ] {
            assert!(error.contains("no recorded binding"), "{error}");
        }
        // The reassigned task continues normally.
        assert_eq!(
            continue_task(&home, &up.source, "task-2", None)
                .unwrap()
                .task
                .as_deref(),
            Some("task-2")
        );
    }

    /// Legacy readers (historical base `4f2ff08`) ignore added task fields and
    /// even an increased schema number. They must fail to deserialize the
    /// required discriminator, before applying executor-session liveness.
    #[test]
    fn a_legacy_reader_refuses_a_durable_task_binding() {
        let root = tempfile::tempdir().unwrap();
        let up = upstream(root.path());
        let home = root.path().join("home");
        let base = rev(&up.source);
        let dead = |_: &str| false;
        let held = acquire_task(&home, &up.source, 1, "task-1", Some(&base), &dead).unwrap();
        let path = slot_record_path(&home, &up.source, 1).unwrap();
        let bytes = fs::read(&path).unwrap();
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct LegacySlotRecord {
            schema: u32,
            state: SlotState,
            owner: Option<String>,
            base: Option<String>,
        }
        let error = serde_json::from_slice::<LegacySlotRecord>(&bytes)
            .err()
            .expect("an older reader must refuse current ownership records");
        assert!(error.to_string().contains("missing field `schema`"));
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["ownershipSchema"], SLOT_STATE_SCHEMA);
        assert!(value.get("schema").is_none());

        // A number-only bump would not protect the held task: the historical
        // decoder still accepts it and sees no durable ownership contract.
        let mut number_only = value.clone();
        number_only
            .as_object_mut()
            .unwrap()
            .remove("ownershipSchema");
        number_only["schema"] = serde_json::json!(SLOT_STATE_SCHEMA);
        let legacy: LegacySlotRecord = serde_json::from_value(number_only).unwrap();
        assert_eq!(legacy.schema, SLOT_STATE_SCHEMA);
        assert_eq!(
            legacy.owner.as_deref(),
            Some("task-1"),
            "the legacy reader sees only an owner string, never the binding"
        );
        assert_eq!(
            legacy.state,
            SlotState::Occupied,
            "the legacy reader sees a completed session claim, not a task binding"
        );
        assert_eq!(value["task"].as_str(), Some("task-1"));
        assert_eq!(
            legacy.base.as_deref(),
            value["base"].as_str(),
            "the legacy reader sees the base but never the task binding"
        );
        assert_eq!(value["handle"].as_str(), held.handle.as_deref());
        // This build refuses the binding once its generation handle is gone,
        // instead of reading task ownership as an unknown session owner.
        let mut tampered = value;
        tampered.as_object_mut().unwrap().remove("handle");
        fs::write(&path, serde_json::to_vec(&tampered).unwrap()).unwrap();
        let error = continue_task(&home, &up.source, "task-1", held.handle.as_deref())
            .unwrap_err()
            .to_string();
        assert!(error.contains("without its generation handle"), "{error}");
    }

    #[test]
    fn legacy_record_upgrade_preserves_binding_and_files_until_owned_transition() {
        let root = tempfile::tempdir().unwrap();
        let up = upstream(root.path());
        let home = root.path().join("home");
        let base = rev(&up.source);
        let dead = |_: &str| false;
        let held = acquire_task(&home, &up.source, 1, "retained-task", Some(&base), &dead).unwrap();
        fs::write(held.path.join("untracked.txt"), "retained input").unwrap();
        let path = slot_record_path(&home, &up.source, 1).unwrap();
        let mut legacy: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        legacy.as_object_mut().unwrap().remove("ownershipSchema");
        legacy["schema"] = serde_json::json!(1);
        let original = serde_json::to_vec_pretty(&legacy).unwrap();
        fs::write(&path, &original).unwrap();

        let loaded = load_slot_record(&home, &up.source, 1).unwrap().unwrap();
        assert_eq!(loaded.schema, SLOT_STATE_SCHEMA);
        let continued =
            continue_task(&home, &up.source, "retained-task", held.handle.as_deref()).unwrap();
        assert_eq!(continued.path, held.path);
        assert_eq!(continued.base, held.base);
        assert_eq!(continued.handle, held.handle);
        assert_eq!(
            fs::read(&path).unwrap(),
            original,
            "reads do not rewrite state"
        );

        let handed = handoff_task_to_session(
            &home,
            &up.source,
            "retained-task",
            held.handle.as_deref().unwrap(),
            "executor-after-update",
            &dead,
        )
        .unwrap();
        assert_eq!(handed.path, held.path);
        assert_eq!(handed.base, held.base);
        assert_eq!(
            fs::read_to_string(held.path.join("untracked.txt")).unwrap(),
            "retained input"
        );
        let upgraded: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(upgraded["ownershipSchema"], SLOT_STATE_SCHEMA);
        assert!(upgraded.get("schema").is_none());
    }

    #[test]
    fn concurrent_owner_claims_keep_one_winner_and_never_grow_capacity() {
        let root = tempfile::tempdir().unwrap();
        let up = upstream(root.path());
        let home = root.path().join("home");
        let layout = pool(&up.source, 1).unwrap();
        create_slot(&layout, 1).unwrap();
        let only_a = |session: &str| session == "exec-a";
        let SlotClaim::Claimed(winner) =
            claim_slot_for(&home, &layout, 1, "exec-a", &only_a, None).unwrap()
        else {
            panic!("the first owner must win the slot");
        };
        assert_eq!(winner.owner.as_deref(), Some("exec-a"));
        // A concurrent second owner loses the same position.
        match claim_slot_for(&home, &layout, 1, "exec-b", &only_a, None).unwrap() {
            SlotClaim::Unavailable(refusal) => {
                assert!(
                    refusal.reason.contains("occupied by live session exec-a"),
                    "{}",
                    refusal.reason
                );
            }
            SlotClaim::Claimed(_) => panic!("two owners must never share one slot"),
        }
        // The winner's own retry keeps its claim; capacity never grows.
        assert!(matches!(
            claim_slot_for(&home, &layout, 1, "exec-a", &only_a, None).unwrap(),
            SlotClaim::Claimed(_)
        ));
        assert_eq!(registered_count(&up.source), 2);
        // A task acquisition cannot take a session-owned slot away.
        let dead = |_: &str| false;
        let base = rev(&up.source);
        let error = acquire_task(&home, &up.source, 1, "task-1", Some(&base), &only_a)
            .unwrap_err()
            .to_string();
        assert!(error.contains("occupied by live session exec-a"), "{error}");
        // Once that session is gone, its clean claim is reclaimed by the task.
        let acquired = acquire_task(&home, &up.source, 1, "task-1", Some(&base), &dead).unwrap();
        assert_eq!(acquired.index, 1);
        assert_eq!(acquired.task.as_deref(), Some("task-1"));
        assert_eq!(
            registered_count(&up.source),
            2,
            "a second claimant never adds a worktree"
        );
        // And the task binding refuses an executor-session claim in turn.
        match claim_slot(&home, &layout, 1, "exec-b", &dead).unwrap() {
            SlotClaim::Unavailable(refusal) => {
                assert!(
                    refusal.reason.contains("held by task 'task-1'"),
                    "{}",
                    refusal.reason
                );
            }
            SlotClaim::Claimed(_) => panic!("a task-held slot must not be claimed by a session"),
        }
    }

    #[test]
    fn handoff_moves_the_binding_without_reset_and_refuses_stale_or_foreign_owners() {
        let root = tempfile::tempdir().unwrap();
        let up = upstream(root.path());
        let home = root.path().join("home");
        let dead = |_: &str| false;
        let base = rev(&up.source);
        let acquired = acquire_task(&home, &up.source, 1, "task-1", Some(&base), &dead).unwrap();
        let handle = acquired.handle.clone().unwrap();
        let partial = acquired.path.join("partial.txt");
        fs::write(&partial, "partial work\n").unwrap();
        let head = rev(&acquired.path);
        // A stale handle cannot move the binding.
        let error = handoff_task_to_session(&home, &up.source, "task-1", "stale", "exec-9", &dead)
            .unwrap_err()
            .to_string();
        assert!(error.contains("stale"), "{error}");
        assert_eq!(
            load_slot_record(&home, &up.source, 1)
                .unwrap()
                .unwrap()
                .owner
                .as_deref(),
            Some("task-1")
        );
        // A genuinely live foreign startup reservation excludes both the
        // handoff and the release until its process identity is resolved.
        let program = std::env::current_exe().unwrap();
        let mut sleeper = Command::new(&program)
            .args([
                "--exact",
                "--ignored",
                "task_worktree::tests::startup_liveness_hold",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let user = crate::process_service::current_user().unwrap();
        let identity =
            crate::process_service::ServiceProcess::observe(sleeper.id(), &program, 0, &user)
                .unwrap()
                .identity();
        let reservation = StartupReservation {
            pid: identity.pid,
            created: identity.creation_time,
            program,
        };
        for _ in 0..100 {
            if startup_liveness(&reservation) == ReservationLiveness::Live {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert_eq!(startup_liveness(&reservation), ReservationLiveness::Live);
        let record_path = slot_record_path(&home, &up.source, 1).unwrap();
        let mut stored = load_slot_record(&home, &up.source, 1).unwrap().unwrap();
        stored.startup = Some(reservation.clone());
        fs::write(&record_path, serde_json::to_vec_pretty(&stored).unwrap()).unwrap();
        let error = handoff_task_to_session(&home, &up.source, "task-1", &handle, "exec-9", &dead)
            .unwrap_err()
            .to_string();
        assert!(error.contains("in-flight startup"), "{error}");
        let error = release_task(
            &home,
            &up.source,
            "task-1",
            SlotDisposition::Discarded,
            "discarded",
            &base,
            Some(&handle),
            &dead,
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("in-flight startup"), "{error}");
        let _ = sleeper.kill();
        let _ = sleeper.wait();
        for _ in 0..100 {
            if startup_liveness(&reservation) == ReservationLiveness::Dead {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert_eq!(startup_liveness(&reservation), ReservationLiveness::Dead);
        // With the dispatching process resolved dead, the handoff proceeds and
        // never resets, cleans or fetches the checkout.
        let handed =
            handoff_task_to_session(&home, &up.source, "task-1", &handle, "exec-9", &dead).unwrap();
        assert_eq!(handed.owner.as_deref(), Some("exec-9"));
        assert_eq!(handed.handle.as_deref(), Some(handle.as_str()));
        assert_eq!(handed.base.as_deref(), Some(base.as_str()));
        assert_eq!(handed.path, acquired.path);
        assert_eq!(handed.state, TaskState::Held);
        assert!(partial.is_file(), "handoff must not clean partial work");
        assert_eq!(rev(&acquired.path), head);
        release_startup_reservation(&home, &up.source, 1, "exec-9").unwrap();
        assert!(
            load_slot_record(&home, &up.source, 1)
                .unwrap()
                .unwrap()
                .startup
                .is_none()
        );
        // After the explicit release the binding - and its handle - is gone.
        let disposition = release_task(
            &home,
            &up.source,
            "task-1",
            SlotDisposition::Merged,
            "merged",
            &base,
            Some(&handle),
            &dead,
        )
        .unwrap();
        assert!(matches!(disposition, LaneDisposition::Reused { .. }));
        let error = handoff_task_to_session(&home, &up.source, "task-1", &handle, "exec-9", &dead)
            .unwrap_err()
            .to_string();
        assert!(error.contains("no recorded binding"), "{error}");
        let error = continue_task(&home, &up.source, "task-1", Some(&handle))
            .unwrap_err()
            .to_string();
        assert!(error.contains("no recorded binding"), "{error}");
    }

    /// Sleeper process that holds a genuine in-flight startup reservation;
    /// it runs only when spawned by name from the handoff test.
    #[test]
    #[ignore = "helper process for the handoff reservation test"]
    fn startup_liveness_hold() {
        std::thread::sleep(std::time::Duration::from_secs(60));
    }

    fn rev(cwd: &Path) -> String {
        git(cwd, &["rev-parse", "HEAD"]).unwrap().trim().to_owned()
    }
}
