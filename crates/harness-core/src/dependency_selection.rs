//! Journaled selection of retained dependency candidates. Package trees and
//! adopted/shared installations are never moved, overwritten or removed here.
#![cfg(windows)]
use crate::{
    build_identity, dependency_candidate, dependency_discovery, native_build,
    registration_native::{FileGuard, LinkIdentity, ReadGuard, StagedFile},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    io::{self, Read},
    path::{Path, PathBuf},
};

const MAX_RECORD: usize = 128 * 1024;

fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "dependency selection is invalid or changed; preserve the selection and recovery journal",
    )
}

fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn package(slot: &str) -> io::Result<&'static str> {
    match slot {
        "codebase-memory" => Ok("codebase-memory-mcp"),
        "nuphus" => Ok("@nuphus/nuphus-mcp-win32-x64"),
        "basedpyright" => Ok("basedpyright"),
        _ => Err(invalid()),
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Node {
    path: PathBuf,
    sha256: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Pointer {
    schema: u32,
    slot: String,
    stage: String,
    manifest_sha256: String,
    node: Option<Node>,
}

impl Pointer {
    fn parse(bytes: &[u8], slot: &str) -> io::Result<Self> {
        if bytes.len() > 8192 {
            return Err(invalid());
        }
        let value: Self = serde_json::from_slice(bytes).map_err(|_| invalid())?;
        package(slot)?;
        if value.schema != 1
            || value.slot != slot
            || !digest(&value.manifest_sha256)
            || !value.stage.starts_with("candidate-")
            || value.stage.len() > 128
            || value.stage.len() <= "candidate-".len()
            || !value
                .stage
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            || (slot == "basedpyright") != value.node.is_some()
        {
            return Err(invalid());
        }
        if let Some(node) = &value.node
            && (!digest(&node.sha256) || dependency_discovery::local_path(&node.path)? != node.path)
        {
            return Err(invalid());
        }
        Ok(value)
    }

    fn stage(&self, state: &Path) -> PathBuf {
        state.join("dependency-staging").join(&self.stage)
    }

    fn node(&self) -> Option<(&Path, &str)> {
        self.node
            .as_ref()
            .map(|node| (node.path.as_path(), node.sha256.as_str()))
    }

    fn inspect(&self, state: &Path) -> io::Result<dependency_candidate::InspectedCandidate> {
        let candidate =
            dependency_candidate::inspect(&self.stage(state), &self.manifest_sha256, self.node())?;
        if candidate.report()["package"] != package(&self.slot)? {
            return Err(invalid());
        }
        Ok(candidate)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Snapshot {
    identity: LinkIdentity,
    bytes: Vec<u8>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    schema: u32,
    slot: String,
    before: Option<Snapshot>,
    after: Option<Snapshot>,
}

impl Journal {
    fn parse(bytes: &[u8], slot: &str) -> io::Result<Self> {
        if bytes.len() > MAX_RECORD {
            return Err(invalid());
        }
        let value: Self = serde_json::from_slice(bytes).map_err(|_| invalid())?;
        if value.schema != 1 || value.slot != slot || value.before == value.after {
            return Err(invalid());
        }
        for snapshot in [&value.before, &value.after].into_iter().flatten() {
            Pointer::parse(&snapshot.bytes, slot)?;
        }
        // In-place replacements retain their original object identity.
        if let (Some(before), Some(after)) = (&value.before, &value.after)
            && before.identity != after.identity
        {
            return Err(invalid());
        }
        Ok(value)
    }
}

fn observe(path: &Path) -> io::Result<Option<Snapshot>> {
    match FileGuard::read_regular(path) {
        Ok((guard, bytes)) if bytes.len() <= MAX_RECORD => Ok(Some(Snapshot {
            identity: guard.object_identity()?,
            bytes,
        })),
        Ok(_) => Err(invalid()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            native_build::ordinary_ancestors(path)?;
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

fn guard_snapshot(path: &Path, expected: &Snapshot) -> io::Result<FileGuard> {
    let guard = FileGuard::open_regular(path, &expected.bytes)?;
    if guard.object_identity()? != expected.identity {
        return Err(invalid());
    }
    Ok(guard)
}

fn remove(path: &Path, expected: &Snapshot) -> io::Result<()> {
    guard_snapshot(path, expected)?.remove()
}

fn replace(path: &Path, before: &Snapshot, after: &Snapshot) -> io::Result<()> {
    if before.identity != after.identity {
        return Err(invalid());
    }
    FileGuard::replace_regular(path, &before.identity, &before.bytes, &after.bytes)
}

fn active(state: &Path, slot: &str) -> PathBuf {
    state.join(format!("dependency-{slot}.json"))
}
fn pending(state: &Path, slot: &str) -> PathBuf {
    state.join(format!("dependency-{slot}-journal.json"))
}
fn history(state: &Path, slot: &str, hash: &str) -> PathBuf {
    state.join(format!("dependency-{slot}-history-{hash}.json"))
}

fn ownership(state: &Path, slot: &str, snapshot: &Snapshot) -> io::Result<(PathBuf, Vec<u8>)> {
    let bytes = serde_json::to_vec(snapshot)?;
    let hash = build_identity::hash_bytes(&bytes);
    Ok((
        state.join(format!("dependency-{slot}-owned-{hash}.json")),
        bytes,
    ))
}

fn verify_ownership(state: &Path, slot: &str, snapshot: &Snapshot) -> io::Result<FileGuard> {
    let (path, bytes) = ownership(state, slot, snapshot)?;
    FileGuard::open_regular(&path, &bytes).map_err(|_| invalid())
}

fn retain_record(path: &Path, bytes: &[u8]) -> io::Result<FileGuard> {
    match observe(path)? {
        None => StagedFile::create(path, bytes)?.commit()?,
        Some(existing) if existing.bytes == bytes => (),
        Some(_) => return Err(invalid()),
    }
    FileGuard::open_regular(path, bytes)
}

fn removal_receipt(slot: &str, snapshot: &Snapshot) -> io::Result<(String, Vec<u8>)> {
    let bytes = serde_json::to_vec(&Journal {
        schema: 1,
        slot: slot.to_owned(),
        before: None,
        after: Some(snapshot.clone()),
    })?;
    Journal::parse(&bytes, slot)?;
    Ok((build_identity::hash_bytes(&bytes), bytes))
}

fn idle_rollback_receipt(state: &Path, slot: &str) -> io::Result<Option<String>> {
    let Some(snapshot) = observe(&active(state, slot))? else {
        return Ok(None);
    };
    let _owner = verify_ownership(state, slot, &snapshot)?;
    let (hash, bytes) = removal_receipt(slot, &snapshot)?;
    let found = match observe(&history(state, slot, &hash))? {
        Some(receipt) if receipt.bytes == bytes => {
            let _guard = guard_snapshot(&history(state, slot, &hash), &receipt)?;
            Some(hash)
        }
        Some(_) => return Err(invalid()),
        None => None,
    };
    if observe(&active(state, slot))? != Some(snapshot) {
        return Err(invalid());
    }
    Ok(found)
}

fn finish(
    state: &Path,
    slot: &str,
    receipt: &Snapshot,
    journal_guard: FileGuard,
    final_selection: Option<&Snapshot>,
) -> io::Result<String> {
    let hash = build_identity::hash_bytes(&receipt.bytes);
    let destination = history(state, slot, &hash);
    let _history_guard = retain_record(&destination, &receipt.bytes)?;
    let _ownership_guard = final_selection
        .map(|snapshot| {
            let (path, bytes) = ownership(state, slot, snapshot)?;
            retain_record(&path, &bytes)
        })
        .transpose()?;
    journal_guard.remove()?;
    Ok(hash)
}

fn ensure_idle(state: &Path, slot: &str) -> io::Result<()> {
    if observe(&pending(state, slot))?.is_some() {
        return Err(io::Error::other(
            "dependency selection has a pending journal; explicit recovery is required",
        ));
    }
    Ok(())
}

fn owned(state: &Path) -> io::Result<ReadGuard> {
    native_build::verify_owned_state(state)?;
    let mut guard = ReadGuard::open(&state.join("owner"))?;
    let mut bytes = Vec::new();
    (&mut guard.file)
        .take((native_build::OWNER.len() + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes != native_build::OWNER {
        return Err(invalid());
    }
    Ok(guard)
}

/// Caller retains the owned state lock and all candidate integrity leases.
fn transact(
    state: &Path,
    slot: &str,
    before: Option<Snapshot>,
    after: Option<Vec<u8>>,
) -> io::Result<String> {
    ensure_idle(state, slot)?;
    let destination = active(state, slot);
    let staged = match (&before, &after) {
        (None, Some(bytes)) => Some(StagedFile::create(&destination, bytes)?),
        _ => None,
    };
    let next = match after {
        Some(bytes) => Some(Snapshot {
            identity: match &before {
                Some(snapshot) => snapshot.identity.clone(),
                None => staged.as_ref().ok_or_else(invalid)?.identity(),
            },
            bytes,
        }),
        None => None,
    };
    let journal = Journal {
        schema: 1,
        slot: slot.to_owned(),
        before,
        after: next,
    };
    let bytes = serde_json::to_vec(&journal)?;
    Journal::parse(&bytes, slot)?;
    let prepared = StagedFile::create(&pending(state, slot), &bytes)?;
    let receipt = Snapshot {
        identity: prepared.identity(),
        bytes,
    };
    prepared.commit()?; // Durable before the active selection can change.
    let journal_guard = guard_snapshot(&pending(state, slot), &receipt)?;
    match (&journal.before, &journal.after, staged) {
        (None, Some(_), Some(staged)) => staged.commit()?,
        (Some(before), Some(after), None) => replace(&destination, before, after)?,
        (Some(before), None, None) => remove(&destination, before)?,
        _ => return Err(invalid()),
    }
    finish(state, slot, &receipt, journal_guard, journal.after.as_ref())
}

/// Read-only complete integrity inspection. No packages, hooks or probes run.
pub fn selected(state: &Path, slot: &str) -> io::Result<Value> {
    package(slot)?;
    let state = dependency_discovery::local_path(state)?;
    let _owner = owned(&state)?;
    ensure_idle(&state, slot)?;
    match observe(&active(&state, slot))? {
        None => Ok(
            json!({"schema_version":1,"operation":"dependency-selection-inspection","slot":slot,"status":"absent","package_code_executed":false}),
        ),
        Some(snapshot) => {
            let _selection_owner = verify_ownership(&state, slot, &snapshot)?;
            let pointer = Pointer::parse(&snapshot.bytes, slot)?;
            let candidate = pointer.inspect(&state)?;
            // A concurrent selection change cannot be reported as this one.
            if observe(&active(&state, slot))? != Some(snapshot) {
                return Err(invalid());
            }
            ensure_idle(&state, slot)?;
            Ok(
                json!({"schema_version":1,"operation":"dependency-selection-inspection","slot":slot,"status":"selected","candidate":candidate.report(),"package_code_executed":false}),
            )
        }
    }
}

/// Explicit activation runs the candidate's compatibility check and records only
/// a retained local selection. Global registration is a separate lifecycle step.
pub fn activate(
    state: &Path,
    slot: &str,
    stage: &Path,
    manifest_sha256: &str,
    node: Option<(&Path, &str)>,
) -> io::Result<Value> {
    package(slot)?;
    let state = dependency_discovery::local_path(state)?;
    let stage = dependency_discovery::local_path(stage)?;
    if stage.parent() != Some(state.join("dependency-staging").as_path()) {
        return Err(invalid());
    }
    let pointer = Pointer {
        schema: 1,
        slot: slot.to_owned(),
        stage: stage
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(invalid)?
            .to_owned(),
        manifest_sha256: manifest_sha256.to_owned(),
        node: node
            .map(|(path, sha256)| -> io::Result<Node> {
                Ok(Node {
                    path: dependency_discovery::local_path(path)?,
                    sha256: sha256.to_owned(),
                })
            })
            .transpose()?,
    };
    let after = serde_json::to_vec(&pointer)?;
    Pointer::parse(&after, slot)?;
    let _owner = owned(&state)?;
    let _lock = native_build::lock_owned_state(&state)?;
    ensure_idle(&state, slot)?;
    let before = observe(&active(&state, slot))?;
    let _selection_owner = before
        .as_ref()
        .map(|snapshot| verify_ownership(&state, slot, snapshot))
        .transpose()?;
    let previous = before
        .as_ref()
        .map(|value| Pointer::parse(&value.bytes, slot)?.inspect(&state))
        .transpose()?;
    let candidate = dependency_candidate::validate(&stage, manifest_sha256, pointer.node())?;
    if candidate.report()["package"] != package(slot)? {
        return Err(invalid());
    }
    if before.as_ref().is_some_and(|value| value.bytes == after) {
        if observe(&active(&state, slot))? != before {
            return Err(invalid());
        }
        return Ok(
            json!({"schema_version":1,"operation":"dependency-selection","slot":slot,"changed":false,"candidate":candidate.report(),"global_registration_changed":false}),
        );
    }
    let receipt = transact(&state, slot, before, Some(after))?;
    drop(previous);
    Ok(
        json!({"schema_version":1,"operation":"dependency-selection","slot":slot,"changed":true,"receipt_sha256":receipt,"candidate":candidate.report(),"global_registration_changed":false}),
    )
}

/// Restore the pre-operation selection after interruption. Unknown objects or
/// bytes are preserved, even if a replacement object contains identical JSON.
pub fn recover(state: &Path, slot: &str) -> io::Result<Value> {
    package(slot)?;
    let state = dependency_discovery::local_path(state)?;
    let _owner = owned(&state)?;
    let _lock = native_build::lock_owned_state(&state)?;
    let Some(mut receipt) = observe(&pending(&state, slot))? else {
        let usable = idle_rollback_receipt(&state, slot)?;
        return Ok(
            json!({"schema_version":1,"operation":"dependency-selection-recovery","slot":slot,"changed":false,"rollback_receipt_sha256":usable}),
        );
    };
    let mut journal = Journal::parse(&receipt.bytes, slot)?;
    let mut journal_guard = Some(guard_snapshot(&pending(&state, slot), &receipt)?);
    let current = observe(&active(&state, slot))?;
    if current != journal.before && current != journal.after {
        return Err(invalid());
    }
    let previous = journal
        .before
        .as_ref()
        .map(|snapshot| Pointer::parse(&snapshot.bytes, slot)?.inspect(&state))
        .transpose()?;
    if current != journal.before {
        match (&journal.after, &journal.before) {
            (Some(after), Some(before)) => replace(&active(&state, slot), after, before)?,
            (Some(after), None) => remove(&active(&state, slot), after)?,
            (None, Some(before)) => {
                // A committed deletion cannot recover the old file ID. Reserve
                // the absent name, journal its new ID before publication, then
                // commit. Another interruption safely repeats or recognizes it.
                let restored = StagedFile::create(&active(&state, slot), &before.bytes)?;
                journal.before.as_mut().ok_or_else(invalid)?.identity = restored.identity();
                let amended = Snapshot {
                    identity: receipt.identity.clone(),
                    bytes: serde_json::to_vec(&journal)?,
                };
                Journal::parse(&amended.bytes, slot)?;
                drop(journal_guard.take());
                replace(&pending(&state, slot), &receipt, &amended)?;
                receipt = amended;
                journal_guard = Some(guard_snapshot(&pending(&state, slot), &receipt)?);
                restored.commit()?;
            }
            _ => return Err(invalid()),
        }
    }
    // A restored deletion has a new ID. Retain a usable activation-equivalent
    // receipt so another explicit rollback is not stranded on the old object.
    let usable = if journal.after.is_none() {
        let (hash, bytes) = removal_receipt(slot, journal.before.as_ref().ok_or_else(invalid)?)?;
        Some((
            hash.clone(),
            retain_record(&history(&state, slot, &hash), &bytes)?,
        ))
    } else {
        None
    };
    let hash = finish(
        &state,
        slot,
        &receipt,
        journal_guard.ok_or_else(invalid)?,
        journal.before.as_ref(),
    )?;
    drop(previous);
    Ok(
        json!({"schema_version":1,"operation":"dependency-selection-recovery","slot":slot,"changed":true,"receipt_sha256":hash,"rollback_receipt_sha256":usable.as_ref().map(|(hash,_)| hash),"global_registration_changed":false}),
    )
}

/// Explicit rollback uses an exact completed activation receipt. The previous
/// candidate is inspected without network access or runtime provisioning.
pub fn rollback(state: &Path, slot: &str, receipt_sha256: &str) -> io::Result<Value> {
    package(slot)?;
    if !digest(receipt_sha256) {
        return Err(invalid());
    }
    let state = dependency_discovery::local_path(state)?;
    let _owner = owned(&state)?;
    let _lock = native_build::lock_owned_state(&state)?;
    ensure_idle(&state, slot)?;
    let receipt = observe(&history(&state, slot, receipt_sha256))?.ok_or_else(invalid)?;
    let _receipt_guard = guard_snapshot(&history(&state, slot, receipt_sha256), &receipt)?;
    if build_identity::hash_bytes(&receipt.bytes) != receipt_sha256 {
        return Err(invalid());
    }
    let journal = Journal::parse(&receipt.bytes, slot)?;
    if journal.after.is_none() {
        return Err(invalid());
    }
    let current = observe(&active(&state, slot))?;
    if current != journal.after {
        return Err(invalid());
    }
    let previous = journal
        .before
        .as_ref()
        .map(|snapshot| Pointer::parse(&snapshot.bytes, slot)?.inspect(&state))
        .transpose()?;
    let hash = transact(
        &state,
        slot,
        current,
        journal.before.map(|snapshot| snapshot.bytes),
    )?;
    drop(previous);
    Ok(
        json!({"schema_version":1,"operation":"dependency-selection-rollback","slot":slot,"changed":true,"receipt_sha256":hash,"global_registration_changed":false}),
    )
}

#[cfg(test)]
#[path = "dependency_selection_tests.rs"]
mod tests;
