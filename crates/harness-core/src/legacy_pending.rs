//! Rollback-only consumer for the old PowerShell pending.json contract. The
//! old immutable intent stays held until every inverse is accepted. Repeating
//! recovery after an interruption uses that same intent, without a new format.
#![cfg(windows)]

use crate::{
    config_file::ConfigSnapshot,
    environment_path::UserPathSnapshot,
    installation_path::PathChange,
    installation_state::PathScope,
    inventory::ordinary_parents,
    legacy_pending_format::{Operation, Pending},
    process_path::ProcessPathSnapshot,
    registration_native::{self as native, FileGuard, StagedFile, StagedLink},
};
use std::{fs, io, path::Path};

fn conflict(message: &'static str) -> io::Error {
    io::Error::other(message)
}

/// Called under both installation-owner locks. Does not run old scripts or
/// consult a relocated source tree. A missing journal is a read-only no-op.
pub(crate) fn recover(
    codex_home: &Path,
    user_home: &Path,
    dependency_user_home: &Path,
) -> io::Result<bool> {
    recover_inner(codex_home, user_home, dependency_user_home, |_| Ok(()))
}

fn recover_inner(
    codex_home: &Path,
    user_home: &Path,
    dependency_user_home: &Path,
    checkpoint: impl FnMut(&str) -> io::Result<()>,
) -> io::Result<bool> {
    recover_mode(
        codex_home,
        user_home,
        dependency_user_home,
        false,
        checkpoint,
    )
}

pub(crate) fn preview(
    codex_home: &Path,
    user_home: &Path,
    dependency_user_home: &Path,
) -> io::Result<bool> {
    recover_mode(
        codex_home,
        user_home,
        dependency_user_home,
        true,
        |_| Ok(()),
    )
}

fn recover_mode(
    codex_home: &Path,
    user_home: &Path,
    dependency_user_home: &Path,
    preview: bool,
    mut checkpoint: impl FnMut(&str) -> io::Result<()>,
) -> io::Result<bool> {
    let journal_path = codex_home.join("harness/pending.json");
    ordinary_parents(&journal_path)?;
    let (journal_guard, bytes) = match FileGuard::read_regular(&journal_path) {
        Ok(pair) => pair,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    // Two independent pending operations have no defined ordering. Preserve
    // both instead of undoing one through the other's published objects.
    for relative in [
        "native-registration/journal.json",
        "native-registration/commit.json",
        "token-workflow-pending.json",
        "activation-pending.json",
    ] {
        let path = codex_home.join("harness").join(relative);
        ordinary_parents(&path)?;
        require_absent(&path)?;
    }
    let pending = Pending::parse(&bytes, codex_home, user_home, dependency_user_home)?;
    let metadata_path = codex_home.join("harness/installation.json");
    let controls = [
        &journal_path,
        &metadata_path,
        &codex_home.join("harness/native-registration"),
    ]
    .into_iter()
    .map(|path| native::destination_name(path))
    .collect::<io::Result<Vec<_>>>()?;
    let mut destinations = Vec::new();
    for operation in &pending.operations {
        let name = native::destination_name(&operation.destination)?;
        for other in controls.iter().chain(destinations.iter()) {
            if crate::registration::paths_overlap(&name, other)? {
                return Err(conflict(
                    "legacy recovery destinations overlap each other or recovery state",
                ));
            }
        }
        destinations.push(name);
    }
    let metadata = match ConfigSnapshot::read(&metadata_path) {
        Ok(snapshot) => Some(snapshot),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    pending.verify_state(metadata.as_ref().map(ConfigSnapshot::contents))?;
    let metadata_guard = metadata.as_ref().map(ConfigSnapshot::guard).transpose()?;
    let path = match pending.path_scope {
        PathScope::User => PathChange::User(
            UserPathSnapshot::for_registration()?
                .legacy_restore(&pending.path_before, &pending.path_after)?,
        ),
        PathScope::Process => PathChange::Process(
            ProcessPathSnapshot::read()?
                .legacy_restore(&pending.path_before, &pending.path_after)?,
        ),
    };
    let directory = |operation: &Operation| -> io::Result<bool> {
        Ok(native::targets_match(
            &operation.destination,
            &codex_home.join("agents/codex-harness"),
        )? || operation
            .destination
            .parent()
            .map(|parent| native::targets_match(parent, &user_home.join(".agents/skills")))
            .transpose()?
            .unwrap_or(false))
    };
    if preview {
        let _held = pending
            .operations
            .iter()
            .map(|operation| Action::inspect(operation, directory(operation)?))
            .collect::<io::Result<Vec<_>>>()?;
        return Ok(true);
    }
    let creation = if metadata.is_none() {
        pending
            .restore_state
            .as_deref()
            .map(|bytes| StagedFile::create(&metadata_path, bytes))
            .transpose()?
    } else {
        None
    };
    let mut actions = pending
        .operations
        .iter()
        .rev()
        .enumerate()
        .map(|(index, operation)| Action::prepare(operation, index, directory(operation)?))
        .collect::<io::Result<Vec<_>>>()?;
    checkpoint("prepared")?;
    let mut retained = Vec::new();
    for action in actions.drain(..) {
        let guard = action.restore(&mut checkpoint)?;
        retained.push(guard);
        checkpoint("operation")?;
    }
    match &path {
        PathChange::User(change) => {
            change.publish_legacy()?;
        }
        PathChange::Process(change) => change.publish()?,
    }
    checkpoint("path")?;
    let restored_metadata = match (metadata, metadata_guard, creation, &pending.restore_state) {
        (Some(snapshot), Some(guard), _, Some(bytes)) => {
            drop(guard);
            Some(snapshot.replace(bytes)?.guard()?)
        }
        (Some(_), Some(guard), _, None) => {
            guard.remove()?;
            None
        }
        (None, None, Some(stage), Some(bytes)) => {
            let identity = stage.identity();
            stage.commit()?;
            let guard = FileGuard::open_regular(&metadata_path, bytes)?;
            if guard.object_identity()? != identity {
                return Err(conflict("restored legacy metadata was replaced"));
            }
            Some(guard)
        }
        (None, None, None, None) => {
            require_absent(&metadata_path)?;
            None
        }
        _ => return Err(conflict("legacy metadata preparation is inconsistent")),
    };
    checkpoint("metadata")?;
    match &path {
        PathChange::User(change) => change.verify_published()?,
        PathChange::Process(change) => change.verify_published()?,
    }
    for operation in &pending.operations {
        if operation.old_source.is_none() {
            require_absent(&operation.destination)?;
        }
    }
    if restored_metadata.is_none() {
        require_absent(&metadata_path)?;
    }
    journal_guard.remove()?;
    checkpoint("journal")?;
    drop((restored_metadata, retained));
    Ok(true)
}

fn require_absent(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
        Ok(_) => Err(conflict(
            "legacy recovery found an unexpected object; preserving it",
        )),
    }
}

struct Action<'a> {
    operation: &'a Operation,
    current: Option<FileGuard>,
    already_restored: bool,
    // Never committed. The uncommitted child pins an ordinary nonempty parent
    // against rename and reparse changes, including gaps after link deletion.
    namespace: StagedFile,
}

impl<'a> Action<'a> {
    fn prepare(
        operation: &'a Operation,
        index: usize,
        expected_directory: bool,
    ) -> io::Result<Self> {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_nanos();
        let lease = operation
            .destination
            .parent()
            .ok_or_else(|| conflict("legacy destination has no parent"))?
            .join(format!(
                ".harness-recovery-{}-{stamp}-{index}",
                std::process::id()
            ));
        let namespace = StagedFile::create(&lease, &[])?;
        let (current, already_restored) = Self::inspect(operation, expected_directory)?;
        Ok(Self {
            operation,
            current,
            already_restored,
            namespace,
        })
    }

    fn inspect(
        operation: &Operation,
        expected_directory: bool,
    ) -> io::Result<(Option<FileGuard>, bool)> {
        ordinary_parents(&operation.destination)?;
        if operation.old_source.is_some() && operation.old_directory != expected_directory {
            return Err(conflict(
                "legacy recorded link type conflicts with its destination",
            ));
        }
        for source in [&operation.old_source, &operation.new_source]
            .into_iter()
            .flatten()
        {
            // Validate the guarded primitive's target contract even when the
            // destination is absent; discovering an unsupported target later
            // must not require undoing earlier recovery steps.
            native::targets_match(source, source)?;
        }
        let current = match FileGuard::capture_link(&operation.destination) {
            Ok(guard) => Some(guard),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error),
        };
        let already_restored = if let Some(guard) = &current {
            let (target, directory) = guard.link_description()?;
            let old = operation
                .old_source
                .as_ref()
                .map(|old| native::targets_match(&target, old))
                .transpose()?
                .unwrap_or(false);
            let new = operation
                .new_source
                .as_ref()
                .map(|new| native::targets_match(&target, new))
                .transpose()?
                .unwrap_or(false);
            if (!old && !new) || directory != expected_directory {
                return Err(conflict("legacy recovery link changed; preserving it"));
            }
            old
        } else {
            operation.old_source.is_none()
        };
        Ok((current, already_restored))
    }

    fn restore(
        self,
        checkpoint: &mut impl FnMut(&str) -> io::Result<()>,
    ) -> io::Result<(Option<FileGuard>, StagedFile)> {
        let Self {
            operation,
            current,
            already_restored,
            namespace,
        } = self;
        if already_restored {
            return Ok((current, namespace));
        }
        if let Some(guard) = current {
            guard.remove()?;
            checkpoint("removed")?;
        }
        let Some(old) = &operation.old_source else {
            return Ok((None, namespace));
        };
        let stage = StagedLink::create(&operation.destination, old, operation.old_directory)?;
        let identity = stage.identity();
        stage.commit()?;
        let guard = native::verified_link(&operation.destination, old, operation.old_directory)?;
        if guard.object_identity()? != identity {
            return Err(conflict("restored legacy link was replaced"));
        }
        Ok((Some(guard), namespace))
    }
}

#[cfg(test)]
#[path = "legacy_pending_tests.rs"]
mod tests;
