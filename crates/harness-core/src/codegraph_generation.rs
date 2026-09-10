//! Owned CodeGraph generation storage. Parent starts and stops the worker;
//! this module only manages identity, committed/active/stage directories,
//! publication and bounded cleanup. Snapshot only after a successful bounded
//! episode while the worker is stopped. Upstream refresh uses multiple
//! transactions, so a live database is never labelled committed.
#![cfg(windows)]

use crate::{
    codegraph_store,
    dependency_discovery::local_path,
    process::{Cancellation, Deadline},
};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub const STORE_DIR_NAME: &str = ".codegraph-harness-store";
pub const ACTIVE_DIR_NAME: &str = ".codegraph-harness-active";
pub const STAGE_DIR_NAME: &str = ".codegraph-harness-stage";
pub const DATABASE_FILE_NAME: &str = "codegraph.db";
pub const OWNERSHIP_FILE_NAME: &str = "OWNER";
pub const OWNERSHIP_KIND: &str = "codegraph-harness-generation";
pub const OWNERSHIP_SCHEMA: u32 = 1;
pub const DEFAULT_ACTIVE_LIMIT_BYTES: u64 = 1 << 30;
pub const DEFAULT_PROJECT_LIMIT_BYTES: u64 = 2 << 30;
pub const DEFAULT_FREE_RESERVE_BYTES: u64 = 5 << 30;
pub const WORKER_LEASE_SECS: u64 = 600;

const METADATA_FILE_NAME: &str = "generation.json";
const METADATA_TEMP_NAME: &str = "generation.json.tmp";
const COMMITTED_DIR_NAME: &str = "committed";
const RETAINED_DIR_NAME: &str = "previous";
const PENDING_COMMIT: &str = "pending-commit";
const PREVIOUS_COMMIT: &str = "previous-commit";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GenerationRole {
    Committed,
    Active,
    Stage,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GenerationStatus {
    Empty,
    Committed,
    Active,
    Staging,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StorageLimits {
    pub active_bytes: u64,
    pub project_bytes: u64,
    pub reserve_bytes: u64,
}

impl Default for StorageLimits {
    fn default() -> Self {
        Self {
            active_bytes: DEFAULT_ACTIVE_LIMIT_BYTES,
            project_bytes: DEFAULT_PROJECT_LIMIT_BYTES,
            reserve_bytes: DEFAULT_FREE_RESERVE_BYTES,
        }
    }
}

impl StorageLimits {
    pub fn for_tests(active_bytes: u64, project_bytes: u64, reserve_bytes: u64) -> Self {
        Self {
            active_bytes,
            project_bytes,
            reserve_bytes,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OwnershipRecord {
    schema: u32,
    kind: String,
    project: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct GenerationRecord {
    schema: u32,
    project: String,
    generation: u64,
    role: GenerationRole,
    status: GenerationStatus,
    files: u64,
    nodes: u64,
    edges: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GenerationStore {
    project: PathBuf,
    limits: StorageLimits,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GenerationLayout {
    pub project: PathBuf,
    pub store: PathBuf,
    pub active: PathBuf,
    pub stage: PathBuf,
    pub committed: PathBuf,
    pub retained: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GenerationHandle {
    pub role: GenerationRole,
    pub status: GenerationStatus,
    pub directory: PathBuf,
    pub database: PathBuf,
    pub data_name: String,
    pub generation: Option<u64>,
    pub files: u64,
    pub nodes: u64,
    pub edges: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StorageUsage {
    pub active_bytes: u64,
    pub project_bytes: u64,
    pub free_bytes: u64,
    pub active_limit_bytes: u64,
    pub project_limit_bytes: u64,
    pub reserve_bytes: u64,
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

fn storage(message: &str) -> io::Error {
    io::Error::other(message)
}

fn vanished(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::NotFound
}

fn storage_kind(kind: &str, error: io::Error) -> io::Error {
    io::Error::new(error.kind(), format!("CodeGraph storage {kind}: {error}"))
}

fn entry_kind(root_kind: &str, path: &Path) -> String {
    match path.file_name().and_then(|name| name.to_str()) {
        Some(name) if root_kind.is_empty() => name.replace('\\', "/"),
        Some(name) => format!("{root_kind}/{name}").replace('\\', "/"),
        None => root_kind.replace('\\', "/"),
    }
}

fn ordinary_existing(path: &Path) -> io::Result<()> {
    crate::build_identity::ordinary(path)
}

fn ordinary_chain(path: &Path) -> io::Result<()> {
    let mut current = Some(path);
    while let Some(path) = current {
        match fs::symlink_metadata(path) {
            Ok(_) => ordinary_existing(path)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        current = path.parent();
    }
    Ok(())
}

fn exact_local(path: &Path) -> io::Result<PathBuf> {
    ordinary_chain(path)?;
    let resolved = local_path(path)?;
    if resolved.as_os_str() != path.as_os_str()
        && !crate::dependency_package::same_path(&resolved, path)
    {
        return Err(invalid(
            "CodeGraph generation path must be the exact local path",
        ));
    }
    Ok(resolved)
}

fn exact_existing_dir(path: &Path) -> io::Result<PathBuf> {
    let path = exact_local(path)?;
    if !path.is_dir() {
        return Err(invalid("CodeGraph generation path is not a directory"));
    }
    ordinary_existing(&path)?;
    Ok(path)
}

fn unix_millis() -> io::Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_millis() as u64)
}

fn encode(record: &impl Serialize) -> io::Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec(record).map_err(io::Error::other)?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    ordinary_chain(path)?;
    let parent = path
        .parent()
        .ok_or_else(|| invalid("owned generation metadata needs a parent"))?;
    exact_existing_dir(parent)?;
    let temp = parent.join(METADATA_TEMP_NAME);
    if temp.exists() {
        ordinary_existing(&temp)?;
        fs::remove_file(&temp)?;
    }
    {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    fs::rename(&temp, path)?;
    OpenOptions::new().write(true).open(path)?.sync_all()?;
    Ok(())
}

const DATABASE_SET: [&str; 4] = [
    DATABASE_FILE_NAME,
    "codegraph.db-wal",
    "codegraph.db-shm",
    "codegraph.db-journal",
];

fn add_size(total: u64, size: u64) -> io::Result<u64> {
    total
        .checked_add(size)
        .ok_or_else(|| storage("CodeGraph generation size overflow"))
}

fn file_len(path: &Path, kind: &str) -> io::Result<Option<u64>> {
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(error) if vanished(&error) => return Ok(None),
        Err(error) => return Err(storage_kind(kind, error)),
    };
    match ordinary_existing(path) {
        Ok(()) => {}
        Err(error) if vanished(&error) => return Ok(None),
        Err(error) => return Err(storage_kind(kind, error)),
    }
    if meta.file_type().is_dir() {
        return Err(storage(&format!("CodeGraph storage {kind} is a directory")));
    }
    match fs::metadata(path) {
        Ok(meta) => Ok(Some(meta.len())),
        Err(error) if vanished(&error) => Ok(None),
        Err(error) => Err(storage_kind(kind, error)),
    }
}

fn directory_bytes(path: &Path, kind: &str) -> io::Result<u64> {
    match fs::symlink_metadata(path) {
        Ok(_) => {}
        Err(error) if vanished(&error) => return Ok(0),
        Err(error) => return Err(storage_kind(kind, error)),
    }
    match ordinary_existing(path) {
        Ok(()) => {}
        Err(error) if vanished(&error) => return Ok(0),
        Err(error) => return Err(storage_kind(kind, error)),
    }
    let mut total = 0u64;
    let mut stack = vec![(path.to_path_buf(), kind.to_owned())];
    while let Some((current, current_kind)) = stack.pop() {
        let meta = match fs::symlink_metadata(&current) {
            Ok(meta) => meta,
            Err(error) if vanished(&error) => continue,
            Err(error) => return Err(storage_kind(&current_kind, error)),
        };
        match ordinary_existing(&current) {
            Ok(()) => {}
            Err(error) if vanished(&error) => continue,
            Err(error) => return Err(storage_kind(&current_kind, error)),
        }
        if meta.is_dir() {
            let entries = match fs::read_dir(&current) {
                Ok(entries) => entries,
                Err(error) if vanished(&error) => continue,
                Err(error) => return Err(storage_kind(&current_kind, error)),
            };
            for entry in entries {
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(error) if vanished(&error) => continue,
                    Err(error) => return Err(storage_kind(&current_kind, error)),
                };
                let child = current.join(entry.file_name());
                let child_kind = entry_kind(&current_kind, &child);
                match ordinary_chain(&child) {
                    Ok(()) => {}
                    Err(error) if vanished(&error) => continue,
                    Err(error) => return Err(storage_kind(&child_kind, error)),
                }
                stack.push((child, child_kind));
            }
        } else {
            if let Some(size) = file_len(&current, &current_kind)? {
                total = add_size(total, size)?;
            }
        }
    }
    Ok(total)
}

fn database_set_bytes(directory: &Path, kind: &str) -> io::Result<u64> {
    let mut total = 0u64;
    for name in DATABASE_SET {
        let path = directory.join(name);
        let file_kind = format!("{kind}/{name}");
        if let Some(size) = file_len(&path, &file_kind)? {
            total = add_size(total, size)?;
        }
    }
    Ok(total)
}

fn write_ownership(path: &Path, project: &Path) -> io::Result<()> {
    atomic_write(
        path,
        &encode(&OwnershipRecord {
            schema: OWNERSHIP_SCHEMA,
            kind: OWNERSHIP_KIND.to_owned(),
            project: project.to_string_lossy().into_owned(),
        })?,
    )
}

fn read_ownership(path: &Path, project: &Path) -> io::Result<OwnershipRecord> {
    ordinary_existing(path)?;
    let record: OwnershipRecord = serde_json::from_slice(&fs::read(path)?)
        .map_err(|_| storage("CodeGraph ownership marker is unreadable"))?;
    if record.schema != OWNERSHIP_SCHEMA
        || record.kind != OWNERSHIP_KIND
        || Path::new(&record.project) != project
    {
        return Err(storage(
            "CodeGraph ownership marker does not match this project",
        ));
    }
    Ok(record)
}

fn write_generation(path: &Path, record: &GenerationRecord) -> io::Result<()> {
    atomic_write(path, &encode(record)?)
}

fn read_generation(path: &Path, project: &Path) -> io::Result<GenerationRecord> {
    ordinary_existing(path)?;
    let record: GenerationRecord = serde_json::from_slice(&fs::read(path)?)
        .map_err(|_| storage("CodeGraph generation metadata is unreadable"))?;
    if record.schema != OWNERSHIP_SCHEMA || Path::new(&record.project) != project {
        return Err(storage(
            "CodeGraph generation metadata does not match this project",
        ));
    }
    Ok(record)
}

fn counts_or_zero(database: &Path) -> io::Result<(u64, u64, u64)> {
    if !database.exists() {
        return Ok((0, 0, 0));
    }
    let value = codegraph_store::counts(database)?;
    Ok((
        value["files"].as_u64().unwrap_or(0),
        value["nodes"].as_u64().unwrap_or(0),
        value["edges"].as_u64().unwrap_or(0),
    ))
}

fn handle_from(
    role: GenerationRole,
    status: GenerationStatus,
    directory: PathBuf,
    data_name: String,
    record: Option<&GenerationRecord>,
) -> GenerationHandle {
    GenerationHandle {
        role,
        status,
        database: directory.join(DATABASE_FILE_NAME),
        directory,
        data_name,
        generation: record.map(|record| record.generation),
        files: record.map(|record| record.files).unwrap_or(0),
        nodes: record.map(|record| record.nodes).unwrap_or(0),
        edges: record.map(|record| record.edges).unwrap_or(0),
    }
}

fn create_owned_dir(path: &Path, project: &Path) -> io::Result<()> {
    ordinary_chain(path)?;
    if path.exists() {
        ordinary_existing(path)?;
        if !path.is_dir() {
            return Err(storage(
                "owned generation path exists and is not a directory",
            ));
        }
        read_ownership(&path.join(OWNERSHIP_FILE_NAME), project)?;
    } else {
        fs::create_dir(path)?;
        write_ownership(&path.join(OWNERSHIP_FILE_NAME), project)?;
    }
    // Private index/source data must not appear in an ordinary git add. The
    // exclusion belongs to this owned cache, never to the project's rules.
    let ignore = path.join(".gitignore");
    ordinary_chain(&ignore)?;
    if ignore.exists() {
        ordinary_existing(&ignore)?;
        if fs::metadata(&ignore)?.len() != 2 || fs::read(&ignore)? != b"*\n" {
            return Err(storage("owned cache ignore changed; preserving it"));
        }
    } else {
        crate::registration_native::StagedFile::create(&ignore, b"*\n")?.commit()?;
    }
    Ok(())
}

fn remove_owned_dir(path: &Path, project: &Path, expected_name: &str) -> io::Result<()> {
    if !path.exists() {
        return Ok(());
    }
    ordinary_existing(path)?;
    if path.file_name().and_then(|name| name.to_str()) != Some(expected_name) {
        return Err(storage(
            "cleanup refused a generation directory with an unexpected name",
        ));
    }
    read_ownership(&path.join(OWNERSHIP_FILE_NAME), project)?;
    fs::remove_dir_all(path)
}

fn replace_owned_dir(from: &Path, to: &Path, project: &Path, expected_to: &str) -> io::Result<()> {
    ordinary_existing(from)?;
    read_ownership(&from.join(OWNERSHIP_FILE_NAME), project)?;
    if to.exists() {
        remove_owned_dir(to, project, expected_to)?;
    }
    ordinary_chain(to)?;
    fs::rename(from, to)?;
    Ok(())
}

fn seed_empty_database(directory: &Path) -> io::Result<()> {
    let database = directory.join(DATABASE_FILE_NAME);
    if database.exists() {
        ordinary_existing(&database)?;
        return Ok(());
    }
    ordinary_chain(&database)?;
    drop(
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&database)?,
    );
    OpenOptions::new().write(true).open(&database)?.sync_all()
}

fn inspect_owned_dir(
    path: &Path,
    project: &Path,
    expected_role: GenerationRole,
) -> io::Result<Option<GenerationRecord>> {
    if !path.exists() {
        return Ok(None);
    }
    ordinary_existing(path)?;
    read_ownership(&path.join(OWNERSHIP_FILE_NAME), project)?;
    let meta = path.join(METADATA_FILE_NAME);
    if !meta.exists() {
        return Ok(None);
    }
    let record = read_generation(&meta, project)?;
    if record.role != expected_role {
        return Err(storage(
            "CodeGraph generation metadata is not current for this directory",
        ));
    }
    Ok(Some(record))
}

impl GenerationStore {
    pub fn open(project: &Path, limits: StorageLimits) -> io::Result<Self> {
        if limits.active_bytes == 0 || limits.project_bytes == 0 {
            return Err(invalid("CodeGraph storage limits must be nonzero"));
        }
        if limits.active_bytes > limits.project_bytes {
            return Err(invalid(
                "active CodeGraph storage cannot exceed the project limit",
            ));
        }
        Ok(Self {
            project: exact_existing_dir(project)?,
            limits,
        })
    }

    pub fn layout(&self) -> GenerationLayout {
        let store = self.project.join(STORE_DIR_NAME);
        GenerationLayout {
            project: self.project.clone(),
            committed: store.join(COMMITTED_DIR_NAME),
            retained: store.join(RETAINED_DIR_NAME),
            store,
            active: self.project.join(ACTIVE_DIR_NAME),
            stage: self.project.join(STAGE_DIR_NAME),
        }
    }

    pub fn prepare(&self) -> io::Result<GenerationLayout> {
        let layout = self.layout();
        create_owned_dir(&layout.store, &self.project)?;
        self.reconcile_checkpoint()?;
        create_owned_dir(&layout.committed, &self.project)?;
        Ok(layout)
    }

    // The database and its identity are published as a directory pair. A
    // stopped process between the two renames leaves the previous directory
    // intact; recovery restores that checkpoint and discards the unpublished one.
    fn reconcile_checkpoint(&self) -> io::Result<()> {
        let layout = self.layout();
        if !layout.store.exists() {
            return Ok(());
        }
        read_ownership(&layout.store.join(OWNERSHIP_FILE_NAME), &self.project)?;
        let previous = layout.store.join(PREVIOUS_COMMIT);
        if previous.exists() {
            read_ownership(&previous.join(OWNERSHIP_FILE_NAME), &self.project)?;
            if !layout.committed.exists() {
                fs::rename(&previous, &layout.committed)?;
            } else {
                self.read_checkpoint(&layout.committed)?
                    .ok_or_else(|| storage("published checkpoint has no committed metadata"))?;
                remove_owned_dir(&previous, &self.project, PREVIOUS_COMMIT)?;
            }
        }
        remove_owned_dir(
            &layout.store.join(PENDING_COMMIT),
            &self.project,
            PENDING_COMMIT,
        )?;
        Ok(())
    }

    /// Inspect and reclaim interrupted metadata. Does not restore committed
    /// over a live active database; call `startup_active` only after the
    /// worker is stopped.
    pub fn recover(&self) -> io::Result<Option<GenerationHandle>> {
        self.reconcile_checkpoint()?;
        let layout = self.layout();
        if layout.store.exists() {
            read_ownership(&layout.store.join(OWNERSHIP_FILE_NAME), &self.project)?;
        }
        if layout.committed.exists() {
            read_ownership(&layout.committed.join(OWNERSHIP_FILE_NAME), &self.project)?;
        }
        let temp = layout.store.join(METADATA_TEMP_NAME);
        if temp.exists() {
            ordinary_existing(&temp)?;
            fs::remove_file(&temp)?;
        }
        if layout.stage.exists() {
            match read_ownership(&layout.stage.join(OWNERSHIP_FILE_NAME), &self.project) {
                Ok(_) => {
                    inspect_owned_dir(&layout.stage, &self.project, GenerationRole::Stage)?;
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    remove_unmarked_if_empty(&layout.stage)?;
                }
                Err(error) => return Err(error),
            }
        }
        if !layout.active.exists() {
            return Ok(None);
        }
        match read_ownership(&layout.active.join(OWNERSHIP_FILE_NAME), &self.project) {
            Ok(_) => {
                let meta = layout.active.join(METADATA_FILE_NAME);
                if !meta.exists() {
                    return Ok(Some(handle_from(
                        GenerationRole::Active,
                        GenerationStatus::Failed,
                        layout.active.clone(),
                        ACTIVE_DIR_NAME.to_owned(),
                        None,
                    )));
                }
                match read_generation(&meta, &self.project) {
                    Ok(record) if record.role == GenerationRole::Active => Ok(Some(handle_from(
                        GenerationRole::Active,
                        record.status,
                        layout.active.clone(),
                        ACTIVE_DIR_NAME.to_owned(),
                        Some(&record),
                    ))),
                    Ok(_) => Err(storage(
                        "active CodeGraph generation metadata is not current",
                    )),
                    Err(_) => Ok(Some(handle_from(
                        GenerationRole::Active,
                        GenerationStatus::Failed,
                        layout.active.clone(),
                        ACTIVE_DIR_NAME.to_owned(),
                        None,
                    ))),
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                remove_unmarked_if_empty(&layout.active)?;
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }

    /// Restore the last committed snapshot into the active worker directory.
    /// Parent must invoke this only with no live worker.
    pub fn startup_active(&self) -> io::Result<GenerationHandle> {
        self.startup_active_bounded(
            Deadline::after(std::time::Duration::from_secs(WORKER_LEASE_SECS))?,
            &Cancellation::default(),
        )
    }

    pub fn startup_active_bounded(
        &self,
        deadline: Deadline,
        cancel: &Cancellation,
    ) -> io::Result<GenerationHandle> {
        let layout = self.prepare()?;
        self.recover()?;
        if let Some(committed) = self.committed_handle()? {
            if layout.active.exists() {
                remove_owned_dir(&layout.active, &self.project, ACTIVE_DIR_NAME)?;
            }
            return self
                .restore_active_from_committed(&committed, deadline, cancel)?
                .ok_or_else(|| {
                    storage("committed CodeGraph generation could not be restored to active")
                });
        }
        if let Some(active) = self.active_handle()? {
            return Ok(active);
        }
        self.create_empty_active()
    }

    pub fn stage_full_rebuild(&self) -> io::Result<GenerationHandle> {
        let layout = self.prepare()?;
        self.check_limits(None)?;
        remove_owned_dir(&layout.stage, &self.project, STAGE_DIR_NAME)?;
        create_owned_dir(&layout.stage, &self.project)?;
        seed_empty_database(&layout.stage)?;
        let record = GenerationRecord {
            schema: OWNERSHIP_SCHEMA,
            project: self.project.to_string_lossy().into_owned(),
            generation: unix_millis()?,
            role: GenerationRole::Stage,
            status: GenerationStatus::Staging,
            files: 0,
            nodes: 0,
            edges: 0,
        };
        write_generation(&layout.stage.join(METADATA_FILE_NAME), &record)?;
        self.check_limits(None)?;
        Ok(handle_from(
            GenerationRole::Stage,
            GenerationStatus::Staging,
            layout.stage.clone(),
            STAGE_DIR_NAME.to_owned(),
            Some(&record),
        ))
    }

    /// Publish a snapshot of a stopped generation. Never call this while the
    /// worker still holds the source database.
    pub fn commit_quiescent(&self, source: GenerationRole) -> io::Result<GenerationHandle> {
        self.commit_bounded(
            source,
            Deadline::after(std::time::Duration::from_secs(WORKER_LEASE_SECS))?,
            &Cancellation::default(),
        )
    }

    pub fn commit_bounded(
        &self,
        source: GenerationRole,
        deadline: Deadline,
        cancel: &Cancellation,
    ) -> io::Result<GenerationHandle> {
        let layout = self.prepare()?;
        let source_dir = match source {
            GenerationRole::Active => layout.active.clone(),
            GenerationRole::Stage => layout.stage.clone(),
            GenerationRole::Committed => {
                return Err(invalid("cannot commit the already committed generation"));
            }
        };
        if !source_dir.exists() {
            return Err(storage("quiescent CodeGraph generation is missing"));
        }
        read_ownership(&source_dir.join(OWNERSHIP_FILE_NAME), &self.project)?;
        let database = source_dir.join(DATABASE_FILE_NAME);
        if !database.is_file() {
            return Err(storage(
                "quiescent CodeGraph generation has no database to commit",
            ));
        }
        self.check_limits(Some(&source_dir))?;
        let (files, nodes, edges) = counts_or_zero(&database)?;
        let generation = unix_millis()?;
        let pending = layout.store.join(PENDING_COMMIT);
        create_owned_dir(&pending, &self.project)?;
        let staging_snapshot = pending.join(DATABASE_FILE_NAME);
        let available = self
            .limits
            .project_bytes
            .saturating_sub(self.usage()?.project_bytes);
        let copied = codegraph_store::snapshot(
            &database,
            &staging_snapshot,
            self.limits.active_bytes.min(available),
            self.limits.reserve_bytes,
            deadline,
            cancel,
        )?;
        if copied == 0 {
            let _ = fs::remove_file(&staging_snapshot);
            return Err(storage("committed CodeGraph snapshot was empty"));
        }
        let record = GenerationRecord {
            schema: OWNERSHIP_SCHEMA,
            project: self.project.to_string_lossy().into_owned(),
            generation,
            role: GenerationRole::Committed,
            status: GenerationStatus::Committed,
            files,
            nodes,
            edges,
        };
        write_generation(&pending.join(METADATA_FILE_NAME), &record)?;
        self.check_limits(None)?;
        if deadline.expired() || cancel.is_cancelled() {
            return Err(storage("checkpoint publication cancelled or expired"));
        }
        let previous = layout.store.join(PREVIOUS_COMMIT);
        fs::rename(&layout.committed, &previous)?;
        // Failure here retains the previous checkpoint. Recovery performs the
        // reverse rename; no delete-before-replace window exists.
        fs::rename(&pending, &layout.committed)?;
        if source == GenerationRole::Stage {
            if layout.active.exists() {
                replace_owned_dir(
                    &layout.active,
                    &layout.retained,
                    &self.project,
                    RETAINED_DIR_NAME,
                )?;
            }
            replace_owned_dir(
                &layout.stage,
                &layout.active,
                &self.project,
                ACTIVE_DIR_NAME,
            )?;
        }
        let active_record = GenerationRecord {
            role: GenerationRole::Active,
            status: GenerationStatus::Active,
            ..record.clone()
        };
        write_generation(&layout.active.join(METADATA_FILE_NAME), &active_record)?;
        remove_owned_dir(&layout.retained, &self.project, RETAINED_DIR_NAME)?;
        remove_owned_dir(&previous, &self.project, PREVIOUS_COMMIT)?;
        self.check_limits(None)?;
        Ok(handle_from(
            GenerationRole::Active,
            GenerationStatus::Active,
            layout.active.clone(),
            ACTIVE_DIR_NAME.to_owned(),
            Some(&active_record),
        ))
    }

    pub fn rollback_failed_stage(&self) -> io::Result<Option<GenerationHandle>> {
        let layout = self.layout();
        if layout.store.exists() {
            read_ownership(&layout.store.join(OWNERSHIP_FILE_NAME), &self.project)?;
        }
        remove_owned_dir(&layout.stage, &self.project, STAGE_DIR_NAME)?;
        self.recover()
    }

    pub fn usage(&self) -> io::Result<StorageUsage> {
        let layout = self.layout();
        let active_bytes = database_set_bytes(&layout.active, ACTIVE_DIR_NAME)?;
        let mut project_bytes = directory_bytes(&layout.store, STORE_DIR_NAME)?;
        for name in [ACTIVE_DIR_NAME, STAGE_DIR_NAME] {
            project_bytes = project_bytes
                .checked_add(directory_bytes(&self.project.join(name), name)?)
                .ok_or_else(|| storage("CodeGraph generation size overflow"))?;
        }
        Ok(StorageUsage {
            active_bytes,
            project_bytes,
            free_bytes: codegraph_store::free_bytes(&self.project)?,
            active_limit_bytes: self.limits.active_bytes,
            project_limit_bytes: self.limits.project_bytes,
            reserve_bytes: self.limits.reserve_bytes,
        })
    }

    /// Read-only storage monitor. Safe to attach as `Worker::start_monitored`
    /// because it never restores, deletes or snapshots.
    pub fn check_limits(&self, extra: Option<&Path>) -> io::Result<StorageUsage> {
        let usage = self.usage()?;
        let stage_bytes = database_set_bytes(&self.layout().stage, STAGE_DIR_NAME)?;
        let extra_bytes = match extra {
            Some(path) => {
                ordinary_chain(path)?;
                ordinary_existing(path)?;
                database_set_bytes(path, "extra")?
            }
            None => 0,
        };
        if extra_bytes > self.limits.active_bytes
            || usage.active_bytes > self.limits.active_bytes
            || stage_bytes > self.limits.active_bytes
        {
            return Err(storage(
                "CodeGraph active database/WAL exceeded the configured limit",
            ));
        }
        if usage.project_bytes > self.limits.project_bytes {
            return Err(storage(
                "CodeGraph project storage exceeded the configured limit",
            ));
        }
        if usage.free_bytes < self.limits.reserve_bytes {
            return Err(storage(
                "CodeGraph storage free-space reserve is not available",
            ));
        }
        Ok(usage)
    }

    pub fn cleanup_obsolete(&self) -> io::Result<()> {
        let layout = self.layout();
        if !layout.store.exists() {
            return Ok(());
        }
        read_ownership(&layout.store.join(OWNERSHIP_FILE_NAME), &self.project)?;
        remove_owned_dir(&layout.stage, &self.project, STAGE_DIR_NAME)?;
        remove_owned_dir(&layout.retained, &self.project, RETAINED_DIR_NAME)?;
        Ok(())
    }

    pub fn committed_handle(&self) -> io::Result<Option<GenerationHandle>> {
        let layout = self.layout();
        if let Some(handle) = self.read_checkpoint(&layout.committed)? {
            return Ok(Some(handle));
        }
        // Read-only callers can still identify the last saved generation during
        // interrupted publication; mutation/reconciliation requires admission.
        self.read_checkpoint(&layout.store.join(PREVIOUS_COMMIT))
    }

    fn read_checkpoint(&self, directory: &Path) -> io::Result<Option<GenerationHandle>> {
        let database = directory.join(DATABASE_FILE_NAME);
        let meta = directory.join(METADATA_FILE_NAME);
        if !database.is_file() || !meta.is_file() {
            return Ok(None);
        }
        read_ownership(&directory.join(OWNERSHIP_FILE_NAME), &self.project)?;
        let record = read_generation(&meta, &self.project)?;
        if record.role != GenerationRole::Committed || record.status != GenerationStatus::Committed
        {
            return Err(storage(
                "committed CodeGraph generation metadata is not current",
            ));
        }
        Ok(Some(handle_from(
            GenerationRole::Committed,
            GenerationStatus::Committed,
            directory.to_path_buf(),
            STORE_DIR_NAME.to_owned(),
            Some(&record),
        )))
    }

    pub fn active_handle(&self) -> io::Result<Option<GenerationHandle>> {
        let layout = self.layout();
        match inspect_owned_dir(&layout.active, &self.project, GenerationRole::Active)? {
            Some(record) => Ok(Some(handle_from(
                GenerationRole::Active,
                record.status,
                layout.active.clone(),
                ACTIVE_DIR_NAME.to_owned(),
                Some(&record),
            ))),
            None if layout.active.exists() => Ok(Some(handle_from(
                GenerationRole::Active,
                GenerationStatus::Failed,
                layout.active.clone(),
                ACTIVE_DIR_NAME.to_owned(),
                None,
            ))),
            None => Ok(None),
        }
    }

    fn restore_active_from_committed(
        &self,
        committed: &GenerationHandle,
        deadline: Deadline,
        cancel: &Cancellation,
    ) -> io::Result<Option<GenerationHandle>> {
        let layout = self.layout();
        create_owned_dir(&layout.active, &self.project)?;
        let destination = layout.active.join(DATABASE_FILE_NAME);
        if destination.exists() {
            ordinary_existing(&destination)?;
            fs::remove_file(&destination)?;
        }
        // Remaining project capacity includes leftover active files after the
        // destination is cleared, matching commit_bounded's active+project cap.
        let occupied = directory_bytes(&layout.store, STORE_DIR_NAME)?
            .saturating_add(directory_bytes(&layout.stage, STAGE_DIR_NAME)?)
            .saturating_add(directory_bytes(&layout.active, ACTIVE_DIR_NAME)?);
        let available = self.limits.project_bytes.saturating_sub(occupied);
        let copied = codegraph_store::snapshot(
            &committed.database,
            &destination,
            self.limits.active_bytes.min(available),
            self.limits.reserve_bytes,
            deadline,
            cancel,
        )?;
        if copied == 0 {
            return Err(storage("restored CodeGraph snapshot was empty"));
        }
        let record = GenerationRecord {
            schema: OWNERSHIP_SCHEMA,
            project: self.project.to_string_lossy().into_owned(),
            generation: committed.generation.unwrap_or(0),
            role: GenerationRole::Active,
            status: GenerationStatus::Active,
            files: committed.files,
            nodes: committed.nodes,
            edges: committed.edges,
        };
        write_generation(&layout.active.join(METADATA_FILE_NAME), &record)?;
        Ok(Some(handle_from(
            GenerationRole::Active,
            GenerationStatus::Active,
            layout.active.clone(),
            ACTIVE_DIR_NAME.to_owned(),
            Some(&record),
        )))
    }

    fn create_empty_active(&self) -> io::Result<GenerationHandle> {
        let layout = self.layout();
        create_owned_dir(&layout.active, &self.project)?;
        seed_empty_database(&layout.active)?;
        let record = GenerationRecord {
            schema: OWNERSHIP_SCHEMA,
            project: self.project.to_string_lossy().into_owned(),
            generation: unix_millis()?,
            role: GenerationRole::Active,
            status: GenerationStatus::Empty,
            files: 0,
            nodes: 0,
            edges: 0,
        };
        write_generation(&layout.active.join(METADATA_FILE_NAME), &record)?;
        Ok(handle_from(
            GenerationRole::Active,
            GenerationStatus::Empty,
            layout.active,
            ACTIVE_DIR_NAME.to_owned(),
            Some(&record),
        ))
    }
}

fn remove_unmarked_if_empty(path: &Path) -> io::Result<()> {
    ordinary_existing(path)?;
    if fs::read_dir(path)?.next().is_none() {
        fs::remove_dir(path)?;
    }
    Ok(())
}

pub fn data_name_for(role: GenerationRole) -> &'static str {
    match role {
        GenerationRole::Active => ACTIVE_DIR_NAME,
        GenerationRole::Stage => STAGE_DIR_NAME,
        GenerationRole::Committed => STORE_DIR_NAME,
    }
}
