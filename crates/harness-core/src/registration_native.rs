//! Windows registration and existing configuration protection. No path-based
//! deletion or unguarded write fallback is safe here.
//!
//! Sharing alone does NOT freeze reparse data: FILE_WRITE_ATTRIBUTES handles can
//! issue FSCTL_SET/DELETE_REPARSE_POINT despite a deny-write sharing mode. A TxF
//! modify-intent handle supplies that missing exclusion for the final object.
//! TxF is deprecated and optional; unavailable protection is an error, never a
//! reason to fall back to DeleteFile/RemoveDirectory. Only local NTFS is accepted.
//!
//! Ancestors are opened one component at a time relative to retained handles.
//! Each child denies delete sharing. After checking its parent again, the parent
//! is an ordinary, nonempty directory, which NTFS cannot turn into a reparse
//! point. Retained handles prevent emptying or renaming that chain. The final
//! transactional handle is reconciled with a root-relative open before use.
//!
//! Relevant Microsoft contracts (checked 2026-09-09):
//! - <https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilew>
//! - <https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-writefile>
//! - <https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-setendoffile>
//! - <https://learn.microsoft.com/en-us/windows/win32/fileio/programming-considerations-for-transacted-fileio->
//! - <https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/ns-ntifs-_file_rename_information>
//! - <https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-fsa/4aeefef8-92c3-4abc-af7a-a610caf8a165>

#![cfg(windows)]

use serde::{Deserialize, Serialize};
use std::ffi::{OsStr, OsString, c_void};
use std::fs::File;
use std::io::{self, Read, Seek, Write};
use std::mem::size_of;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::io::{AsHandle, AsRawHandle, BorrowedHandle, FromRawHandle, OwnedHandle};
use std::path::Path;
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::{
    FILETIME, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE, RtlNtStatusToDosError, UNICODE_STRING,
};
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, CREATE_NEW, CommitTransaction, CreateFileTransactedW, CreateFileW,
    CreateTransaction, DELETE, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT,
    FILE_DISPOSITION_INFO, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_NAME_OPENED, FILE_READ_ATTRIBUTES, FILE_READ_DATA, FILE_SHARE_DELETE, FILE_SHARE_READ,
    FILE_SHARE_WRITE, FileDispositionInfo, GetFileInformationByHandle, GetFinalPathNameByHandleW,
    GetVolumeInformationByHandleW, OPEN_EXISTING, SetFileInformationByHandle, SetFileTime,
    VOLUME_NAME_GUID,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateSymbolicLinkTransactedW, SYMBOLIC_LINK_FLAG_ALLOW_UNPRIVILEGED_CREATE,
    SYMBOLIC_LINK_FLAG_DIRECTORY,
};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_ID_INFO, FILE_RENAME_INFO, FILE_WRITE_ATTRIBUTES, FileIdInfo,
    GetFileInformationByHandleEx, TRANSACTION_DO_NOT_PROMOTE,
};
use windows_sys::Win32::System::IO::{DeviceIoControl, IO_STATUS_BLOCK};

// winioctl.h constants; this crate intentionally does not require another
// windows-sys feature for three constants or the WDK for one ABI declaration.
const FSCTL_GET_REPARSE_POINT: u32 = 0x0009_00a8;
const IO_REPARSE_TAG_SYMLINK: u32 = 0xa000_000c;
const SHARE_ALL: u32 = FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE;
const OPEN_FLAGS: u32 = FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT;
pub(super) const MAX_REGULAR_BYTES: usize = 16 * 1024 * 1024;

#[repr(C)]
struct ObjectAttributes {
    length: u32,
    root_directory: HANDLE,
    object_name: *const UNICODE_STRING,
    attributes: u32,
    security_descriptor: *const c_void,
    security_quality_of_service: *const c_void,
}

#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtOpenFile(
        file_handle: *mut HANDLE,
        desired_access: u32,
        object_attributes: *const ObjectAttributes,
        io_status_block: *mut IO_STATUS_BLOCK,
        share_access: u32,
        open_options: u32,
    ) -> i32;
    fn NtSetInformationFile(
        file_handle: HANDLE,
        io_status_block: *mut IO_STATUS_BLOCK,
        file_information: *const c_void,
        length: u32,
        file_information_class: i32,
    ) -> i32;
}

/// Delete the verified absolute symlink, including a dangling one, never its
/// target. Different lexical targets (including aliases) are ownership conflicts.
pub(super) fn remove_link(
    path: &Path,
    expected_target: &Path,
    directory: bool,
    expected_identity: &LinkIdentity,
) -> io::Result<()> {
    let guard = verified_link(path, expected_target, directory)?;
    guard.verify_identity(expected_identity)?;
    guard.remove()
}

/// Stable across a same-volume rename. The creation time supplements the NTFS
/// file reference's reuse sequence. This is object identity, not proof that a
/// caller created the object: persist it for a unique owned staging link first.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct LinkIdentity {
    volume_serial_number: u64,
    file_id: [u8; 16],
    creation_time: u64,
}

/// A newly created staging link remains isolated until the caller has durably
/// recorded its identity in an intent journal and explicitly commits it. Drop
/// rolls back creation. Existing names are never adopted or replaced.
pub(super) struct StagedLink {
    guard: FileGuard,
    identity: LinkIdentity,
}

/// Exclusive ordinary-file creation stays invisible until its identity has
/// been journaled. This uses the same namespace protection as staged links.
pub(super) struct StagedFile {
    guard: FileGuard,
    identity: LinkIdentity,
}

impl StagedFile {
    /// Stream a bounded dependency payload into a newly created, uncommitted
    /// object. Configuration callers retain their existing 16 MiB byte API.
    pub(super) fn from_reader(path: &Path, reader: &mut dyn Read, length: u64) -> io::Result<Self> {
        if length > 512 * 1024 * 1024 {
            return Err(invalid("dependency payload exceeds 512 MiB"));
        }
        let mut staged = Self::create(path, &[])?;
        let file = staged.guard.file.as_mut().expect("open staged file");
        let copied = io::copy(&mut reader.take(length + 1), file)?;
        if copied != length {
            return Err(conflict("dependency payload length changed"));
        }
        file.sync_all()?;
        Ok(staged)
    }

    /// Fill an uncommitted regular object after its identity is known (for a
    /// self-identifying commit decision). No bytes become visible before commit.
    pub(super) fn set_bytes(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.guard.stage_regular_bytes(bytes)?;
        if self.guard.regular_bytes()? != bytes {
            return Err(conflict("staged configuration bytes changed"));
        }
        Ok(())
    }

    pub(super) fn create(path: &Path, bytes: &[u8]) -> io::Result<Self> {
        if bytes.len() > MAX_REGULAR_BYTES {
            return Err(invalid("registration record exceeds 16 MiB"));
        }
        let parents = ParentGuard::open(path)?;
        let mut expected_name = final_name(parents.handle())?;
        expected_name.push(&parents.leaf);
        let transaction = new_transaction()?;
        let name = wide(expected_name.as_os_str());
        let file = owned(unsafe {
            CreateFileTransactedW(
                name.as_ptr(),
                DELETE | FILE_READ_ATTRIBUTES | FILE_READ_DATA | GENERIC_WRITE,
                0,
                null(),
                CREATE_NEW,
                OPEN_FLAGS,
                null_mut(),
                transaction.as_raw_handle(),
                null(),
                null(),
            )
        })?;
        if final_name(file.as_raw_handle())? != expected_name
            || info(file.as_raw_handle())?.nNumberOfLinks != 1
        {
            return Err(conflict("staging namespace changed during creation"));
        }
        ordinary(parents.handle())?;
        let mut guard = FileGuard {
            file: Some(File::from(file)),
            _parents: parents,
            transaction,
        };
        guard.stage_regular_bytes(bytes)?;
        if guard.regular_bytes()? != bytes {
            return Err(conflict("staged configuration bytes changed"));
        }
        let identity = identity(guard.handle())?;
        Ok(Self { guard, identity })
    }

    pub(super) fn identity(&self) -> LinkIdentity {
        self.identity.clone()
    }

    /// Enlist another resource before publishing this file. The caller must
    /// abandon the staged file if any participant fails; only `commit` publishes.
    pub(super) fn transaction(&self) -> BorrowedHandle<'_> {
        self.guard.transaction.as_handle()
    }

    pub(super) fn destination_name(&self, leaf: &OsStr) -> io::Result<std::path::PathBuf> {
        let mut name = final_name(self.guard._parents.handle())?;
        name.push(leaf);
        Ok(name)
    }

    pub(super) fn commit(self) -> io::Result<()> {
        self.guard.commit()
    }
}

impl StagedLink {
    pub(super) fn create(path: &Path, target: &Path, directory: bool) -> io::Result<Self> {
        Self::create_inner(
            path,
            target,
            directory,
            #[cfg(test)]
            || {},
        )
    }

    fn create_inner(
        path: &Path,
        target: &Path,
        directory: bool,
        #[cfg(test)] before_create: impl FnOnce(),
    ) -> io::Result<Self> {
        let expected = local_path(target)?;
        let parents = ParentGuard::open(path)?;
        let mut expected_name = final_name(parents.handle())?;
        expected_name.push(&parents.leaf);
        let transaction = new_transaction()?;
        let name = wide(expected_name.as_os_str());
        let target = wide(target.as_os_str());
        let flags = SYMBOLIC_LINK_FLAG_ALLOW_UNPRIVILEGED_CREATE
            | if directory {
                SYMBOLIC_LINK_FLAG_DIRECTORY
            } else {
                0
            };
        #[cfg(test)]
        before_create();
        if !unsafe {
            CreateSymbolicLinkTransactedW(
                name.as_ptr(),
                target.as_ptr(),
                flags,
                transaction.as_raw_handle(),
            )
        } {
            return Err(io::Error::last_os_error());
        }
        let file = owned(unsafe {
            CreateFileTransactedW(
                name.as_ptr(),
                DELETE | FILE_READ_ATTRIBUTES | FILE_READ_DATA,
                0,
                null(),
                OPEN_EXISTING,
                OPEN_FLAGS,
                null_mut(),
                transaction.as_raw_handle(),
                null(),
                null(),
            )
        })?;
        // An uncommitted entry cannot be opened by a nontransactional relative
        // lookup. Instead compare its kernel-resolved name with the retained
        // parent's resolved name. A racing parent reparse would resolve elsewhere
        // and rolls back here. The new TxF child now pins this namespace itself.
        if final_name(file.as_raw_handle())? != expected_name
            || info(file.as_raw_handle())?.nNumberOfLinks != 1
        {
            return Err(conflict("staging namespace changed during creation"));
        }
        ordinary(parents.handle())?;
        let guard = FileGuard {
            file: Some(File::from(file)),
            _parents: parents,
            transaction,
        };
        guard.verify_link(&expected, directory)?;
        let identity = identity(guard.handle())?;
        Ok(Self { guard, identity })
    }

    pub(super) fn identity(&self) -> LinkIdentity {
        self.identity.clone()
    }

    pub(super) fn link_description(&self) -> io::Result<(std::path::PathBuf, bool)> {
        self.guard.link_description()
    }

    /// Conflict validation only: this stage and its destination are siblings.
    /// The retained parent chain pins the resolved name until intent is durable.
    pub(super) fn destination_name(&self, leaf: &OsStr) -> io::Result<std::path::PathBuf> {
        let mut name = final_name(self.guard._parents.handle())?;
        name.push(leaf);
        Ok(name)
    }

    pub(super) fn commit(self) -> io::Result<()> {
        self.guard.commit()
    }
}

/// Resolve only the existing ordinary parent prefix for overlap validation.
/// Missing suffixes remain lexical. Never use this name to authorize or perform
/// a mutation: staging revalidates the requested path and pins its actual parent.
pub(super) fn destination_name(path: &Path) -> io::Result<std::path::PathBuf> {
    let normalized: std::path::PathBuf = path.components().collect();
    local_path(&normalized)?;
    let mut prefix = normalized.as_path();
    let mut suffix = Vec::new();
    loop {
        match ParentGuard::open(prefix) {
            Ok(parents) => {
                let mut name = final_name(parents.handle())?;
                name.push(&parents.leaf);
                for component in suffix.iter().rev() {
                    name.push(component);
                }
                return Ok(name);
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                suffix.push(prefix.file_name().ok_or_else(|| invalid("missing leaf"))?);
                prefix = prefix.parent().ok_or_else(|| invalid("missing parent"))?;
            }
            Err(error) => return Err(error),
        }
    }
}

pub(super) fn link_identity(
    path: &Path,
    expected_target: &Path,
    directory: bool,
) -> io::Result<LinkIdentity> {
    identity(verified_link(path, expected_target, directory)?.handle())
}

pub(super) fn verified_link(
    path: &Path,
    expected_target: &Path,
    directory: bool,
) -> io::Result<FileGuard> {
    let expected = local_path(expected_target)?;
    let guard = FileGuard::open(path)?;
    guard.verify_link(&expected, directory)?;
    Ok(guard)
}

/// Same non-resolving namespace comparison used by guarded link verification.
/// Filesystem aliases do not authorize a changed stored link target.
pub(super) fn targets_match(left: &Path, right: &Path) -> io::Result<bool> {
    Ok(local_path(left)? == local_path(right)?)
}

/// Publish the recorded staging object by handle, without replacing any existing
/// destination. No copy or link recreation is permitted, even across volumes.
pub(super) fn publish_link(
    staged_path: &Path,
    destination: &Path,
    expected_target: &Path,
    directory: bool,
    expected_identity: &LinkIdentity,
) -> io::Result<()> {
    publish_link_inner(
        staged_path,
        destination,
        expected_target,
        directory,
        expected_identity,
        #[cfg(test)]
        || {},
    )
}

fn publish_link_inner(
    staged_path: &Path,
    destination: &Path,
    expected_target: &Path,
    directory: bool,
    expected_identity: &LinkIdentity,
    #[cfg(test)] before_rename: impl FnOnce(),
) -> io::Result<()> {
    if local_path(staged_path)? == local_path(destination)? {
        return Err(invalid("staging and destination paths must differ"));
    }
    let expected = local_path(expected_target)?;
    let guard = FileGuard::open_with_write(staged_path, true)?;
    guard.verify_link(&expected, directory)?;
    guard.verify_identity(expected_identity)?;
    publish_guard(
        guard,
        destination,
        expected_identity,
        #[cfg(test)]
        before_rename,
    )
}

pub(super) fn publish_regular(
    staged_path: &Path,
    destination: &Path,
    bytes: &[u8],
    expected_identity: &LinkIdentity,
) -> io::Result<()> {
    if local_path(staged_path)? == local_path(destination)? {
        return Err(invalid("staging and destination paths must differ"));
    }
    let mut guard = FileGuard::open_with_write(staged_path, true)?;
    guard.verify_identity(expected_identity)?;
    if guard.regular_bytes()? != bytes {
        return Err(conflict("staged configuration bytes changed"));
    }
    publish_guard(
        guard,
        destination,
        expected_identity,
        #[cfg(test)]
        || {},
    )
}

fn publish_guard(
    guard: FileGuard,
    destination: &Path,
    expected_identity: &LinkIdentity,
    #[cfg(test)] before_rename: impl FnOnce(),
) -> io::Result<()> {
    let destination = ParentGuard::open(destination)?;
    if identity(destination.handle())?.volume_serial_number
        != expected_identity.volume_serial_number
    {
        return Err(invalid("publication must remain on the staging volume"));
    }
    // The destination can be empty, so acquire TxF modify intent on this
    // directory too: deny-delete sharing alone cannot prevent a junction there.
    let parent_name = wide(
        destination
            .full
            .parent()
            .expect("validated parent")
            .as_os_str(),
    );
    let parent = owned(unsafe {
        CreateFileTransactedW(
            parent_name.as_ptr(),
            FILE_READ_ATTRIBUTES | FILE_WRITE_ATTRIBUTES,
            FILE_SHARE_READ,
            null(),
            OPEN_EXISTING,
            OPEN_FLAGS,
            null_mut(),
            guard.transaction.as_raw_handle(),
            null(),
            null(),
        )
    })?;
    if identity(parent.as_raw_handle())? != identity(destination.handle())? {
        return Err(conflict("publication parent identity changed"));
    }
    ordinary(parent.as_raw_handle())?;
    #[cfg(test)]
    before_rename();
    rename_handle(guard.handle(), parent.as_raw_handle(), &destination.leaf).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("publish registration by handle: {error}"),
        )
    })?;
    // NTFS name tunneling affects file symlinks as well as ordinary files.
    // Retain the journaled creation time in this same TxF transaction, before
    // the renamed object becomes externally visible.
    let creation_time = FILETIME {
        dwLowDateTime: expected_identity.creation_time as u32,
        dwHighDateTime: (expected_identity.creation_time >> 32) as u32,
    };
    if unsafe { SetFileTime(guard.handle(), &creation_time, null(), null()) } == 0 {
        return Err(io::Error::last_os_error());
    }
    guard.verify_identity(expected_identity)?;
    // Both transactional handles must close before committing. The retained
    // nontransactional ancestor handles remain alive until after commit.
    drop(parent);
    guard.commit()
}

/// Holds one immutable registration record and its ordinary ancestor namespace.
/// Only `remove` commits a deletion. Drop (including unwinding) preserves it.
/// The record must be a single-link regular file no larger than 16 MiB.
/// A live guard may coexist with guards/operations on sibling paths.
pub(super) struct FileGuard {
    // Field order matters: close transacted files before closing the transaction.
    file: Option<File>,
    _parents: ParentGuard,
    transaction: OwnedHandle,
}

impl FileGuard {
    /// Captures the link itself, including its stored target, without following it.
    pub(super) fn capture_link(path: &Path) -> io::Result<Self> {
        let guard = Self::open(path)?;
        guard.link_description()?;
        Ok(guard)
    }

    pub(super) fn link_description(&self) -> io::Result<(std::path::PathBuf, bool)> {
        let metadata = info(self.handle())?;
        if metadata.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT == 0 {
            return Err(conflict("registration link type changed"));
        }
        let target = symlink_target(&reparse_data(self.handle())?)?;
        local_path(&target)?;
        Ok((
            target,
            metadata.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0,
        ))
    }

    pub(super) fn open_regular(path: &Path, expected: &[u8]) -> io::Result<Self> {
        if expected.len() > MAX_REGULAR_BYTES {
            return Err(invalid("registration record exceeds 16 MiB"));
        }
        let (guard, bytes) = Self::read_regular(path)?;
        if bytes != expected {
            return Err(conflict("registration record bytes changed"));
        }
        Ok(guard)
    }

    pub(super) fn read_regular(path: &Path) -> io::Result<(Self, Vec<u8>)> {
        let mut guard = Self::open(path)?;
        let bytes = guard.regular_bytes()?;
        Ok((guard, bytes))
    }

    fn regular_bytes(&mut self) -> io::Result<Vec<u8>> {
        let metadata = info(self.handle())?;
        let size = u64::from(metadata.nFileSizeHigh) << 32 | u64::from(metadata.nFileSizeLow);
        if metadata.dwFileAttributes & (FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_REPARSE_POINT)
            != 0
            || size > MAX_REGULAR_BYTES as u64
        {
            return Err(conflict(
                "registration record type or size is not supported",
            ));
        }
        let mut bytes = Vec::with_capacity(size as usize);
        let file = self.file.as_mut().expect("open guard");
        file.rewind()?;
        file.take(size + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 != size {
            return Err(conflict("registration record size changed"));
        }
        Ok(bytes)
    }

    pub(super) fn object_identity(&self) -> io::Result<LinkIdentity> {
        identity(self.handle())
    }

    pub(super) fn replace_regular(
        path: &Path,
        expected_identity: &LinkIdentity,
        before: &[u8],
        after: &[u8],
    ) -> io::Result<()> {
        if before.len() > MAX_REGULAR_BYTES || after.len() > MAX_REGULAR_BYTES {
            return Err(invalid("registration record exceeds 16 MiB"));
        }
        let mut guard = Self::open_with_write(path, true)?;
        guard.verify_identity(expected_identity)?;
        if guard.regular_bytes()? != before {
            return Err(conflict("registration record bytes changed"));
        }
        guard.stage_regular_bytes(after)?;
        guard.commit()
    }

    fn stage_regular_bytes(&mut self, bytes: &[u8]) -> io::Result<()> {
        if bytes.len() > MAX_REGULAR_BYTES {
            return Err(invalid("registration record exceeds 16 MiB"));
        }
        // Both writes and truncation use the same transacted handle. Dropping
        // an uncommitted guard rolls back partial writes and the length change.
        let file = self.file.as_mut().expect("open guard");
        file.rewind()?;
        file.write_all(bytes)?;
        file.set_len(bytes.len() as u64)?;
        file.sync_all()
    }

    fn open(path: &Path) -> io::Result<Self> {
        Self::open_with_write(path, false)
    }

    fn open_with_write(path: &Path, write: bool) -> io::Result<Self> {
        // No timer: expiring a live guard would silently drop its protection.
        // Last transaction-handle closure rolls back; it is never distributed.
        Self::open_in(path, write, new_transaction()?)
    }

    fn open_in(path: &Path, write: bool, transaction: OwnedHandle) -> io::Result<Self> {
        Self::open_from_parents(ParentGuard::open(path)?, write, transaction)
    }

    fn open_from_parents(
        parents: ParentGuard,
        write: bool,
        transaction: OwnedHandle,
    ) -> io::Result<Self> {
        let name = wide(parents.full.as_os_str());
        // DELETE is modify intent for TxF, excluding even preexisting attribute
        // writers. No read/write/delete sharing excludes data readers/writers and
        // name replacement. OPEN_REPARSE_POINT never opens the source target.
        let file = owned(unsafe {
            CreateFileTransactedW(
                name.as_ptr(),
                DELETE
                    | FILE_READ_ATTRIBUTES
                    | FILE_READ_DATA
                    | if write { GENERIC_WRITE } else { 0 },
                0,
                null(),
                OPEN_EXISTING,
                OPEN_FLAGS,
                null_mut(),
                transaction.as_raw_handle(),
                null(),
                null(),
            )
        })?;
        let parent = parents.handle();
        let child = relative_open(parent, &parents.leaf, SHARE_ALL)?;
        let live = info(file.as_raw_handle())?;
        if live.nNumberOfLinks != 1
            || identity(file.as_raw_handle())? != identity(child.as_raw_handle())?
        {
            return Err(conflict("registration object or namespace changed"));
        }
        ordinary(parent)?;
        // The pinned, single-name leaf now proves this parent is nonempty too.
        drop(child);
        Ok(Self {
            file: Some(File::from(file)),
            _parents: parents,
            transaction,
        })
    }

    fn handle(&self) -> HANDLE {
        self.file.as_ref().expect("open guard").as_raw_handle()
    }

    fn verify_link(&self, expected: &LocalPath, directory: bool) -> io::Result<()> {
        let metadata = info(self.handle())?;
        if metadata.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT == 0
            || (metadata.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0) != directory
        {
            return Err(conflict("registration link type changed"));
        }
        let bytes = reparse_data(self.handle())?;
        let target = symlink_target(&bytes)?;
        if &local_path(&target)? != expected {
            return Err(conflict("registration link target changed"));
        }
        Ok(())
    }

    fn verify_identity(&self, expected: &LinkIdentity) -> io::Result<()> {
        if &identity(self.handle())? != expected {
            return Err(conflict("registration link object identity changed"));
        }
        Ok(())
    }

    pub(super) fn remove(self) -> io::Result<()> {
        self.stage_removal()?;
        self.commit()
    }

    fn stage_removal(&self) -> io::Result<()> {
        // TxF stages this exact handle's deletion. It pins the namespace through
        // commit, including the interval after the transacted file handle closes.
        let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
        if unsafe {
            SetFileInformationByHandle(
                self.handle(),
                FileDispositionInfo,
                (&disposition as *const FILE_DISPOSITION_INFO).cast(),
                size_of::<FILE_DISPOSITION_INFO>() as u32,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// Atomically remove exact regular siblings while this separate retained
    /// anchor pins their ordinary parent. No file is removed on abort/crash.
    pub(super) fn remove_siblings(
        &self,
        mut guards: Vec<Self>,
        mut checkpoint: impl FnMut(&'static str, usize) -> io::Result<()>,
    ) -> io::Result<()> {
        if guards.is_empty() {
            return Ok(());
        }
        if guards.len() > 16 {
            return Err(conflict("registration cleanup exceeds its bound"));
        }
        let mut claims = Vec::new();
        for guard in &mut guards {
            if guard._parents.full.parent() != self._parents.full.parent()
                || guard._parents.full == self._parents.full
            {
                return Err(conflict(
                    "registration cleanup requires retained sibling ownership",
                ));
            }
            claims.push((
                guard.object_identity()?,
                crate::build_identity::hash_bytes(&guard.regular_bytes()?),
            ));
        }
        // The retained anchor prevents a parent rename/reparse change during
        // this handoff. Reacquisition verifies each exact ID and byte hash.
        let claims = guards
            .into_iter()
            .zip(claims)
            .map(|(guard, (identity, sha256))| {
                let Self {
                    file,
                    _parents,
                    transaction,
                } = guard;
                drop(file);
                drop(transaction);
                (_parents, identity, sha256)
            })
            .collect::<Vec<_>>();
        checkpoint("released", 0)?;
        let transaction = new_transaction()?;
        let mut enlisted = Vec::new();
        for (index, (parents, identity, sha256)) in claims.into_iter().enumerate() {
            let mut guard = Self::open_from_parents(parents, false, transaction.try_clone()?)?;
            if guard.object_identity()? != identity
                || crate::build_identity::hash_bytes(&guard.regular_bytes()?) != sha256
            {
                return Err(conflict(
                    "registration cleanup ownership changed; preserving files",
                ));
            }
            guard.stage_removal()?;
            enlisted.push(guard);
            checkpoint("staged", index)?;
        }
        for guard in &mut enlisted {
            drop(guard.file.take());
        }
        checkpoint("before-commit", 0)?;
        if unsafe { CommitTransaction(transaction.as_raw_handle()) } == 0 {
            return Err(io::Error::last_os_error());
        }
        checkpoint("committed", 0)
    }

    fn commit(mut self) -> io::Result<()> {
        drop(self.file.take());
        if unsafe { CommitTransaction(self.transaction.as_raw_handle()) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

struct ParentGuard {
    handles: Vec<OwnedHandle>,
    full: std::path::PathBuf,
    leaf: OsString,
}

#[cfg(test)]
#[path = "../tests/registration_native/atomic_cleanup.rs"]
mod atomic_cleanup_tests;

impl ParentGuard {
    fn open(path: &Path) -> io::Result<Self> {
        let path = local_path(path)?;
        let (leaf, names) = path
            .names
            .split_last()
            .ok_or_else(|| invalid("missing leaf"))?;
        let root_name = wide(OsStr::new(&format!("\\\\?\\{}:\\", char::from(path.drive))));
        let root = owned(unsafe {
            CreateFileW(
                root_name.as_ptr(),
                FILE_READ_ATTRIBUTES,
                FILE_SHARE_READ,
                null(),
                OPEN_EXISTING,
                OPEN_FLAGS,
                null_mut(),
            )
        })?;
        ordinary(root.as_raw_handle())?;
        let mut full = volume_root(root.as_raw_handle())?;
        let mut handles = vec![root];
        for name in names {
            let parent = handles.last().expect("root retained").as_raw_handle();
            let child = relative_open(parent, name, FILE_SHARE_READ)?;
            ordinary(child.as_raw_handle())?;
            // After a root-relative open the child pins its parent's contents;
            // reject a reparse installed before the child became nonremovable.
            ordinary(parent)?;
            handles.push(child);
            full.push(name);
        }
        full.push(leaf);
        Ok(Self {
            handles,
            full,
            leaf: leaf.clone(),
        })
    }

    fn handle(&self) -> HANDLE {
        self.handles.last().expect("root retained").as_raw_handle()
    }
}

fn identity(handle: HANDLE) -> io::Result<LinkIdentity> {
    let mut id = FILE_ID_INFO::default();
    if unsafe {
        GetFileInformationByHandleEx(
            handle,
            FileIdInfo,
            (&mut id as *mut FILE_ID_INFO).cast(),
            size_of::<FILE_ID_INFO>() as u32,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let created = info(handle)?.ftCreationTime;
    Ok(LinkIdentity {
        volume_serial_number: id.VolumeSerialNumber,
        file_id: id.FileId.Identifier,
        creation_time: u64::from(created.dwHighDateTime) << 32 | u64::from(created.dwLowDateTime),
    })
}

fn rename_handle(source: HANDLE, parent: HANDLE, leaf: &OsStr) -> io::Result<()> {
    let name = wide(leaf);
    let length = size_of::<FILE_RENAME_INFO>() + name.len() * 2;
    // u64 alignment suffices for FILE_RENAME_INFO on supported Windows targets.
    let mut buffer = vec![0u64; length.div_ceil(size_of::<u64>())];
    let rename = buffer.as_mut_ptr().cast::<FILE_RENAME_INFO>();
    // SAFETY: aligned initialized allocation contains the full variable-length
    // structure. The counted UTF-16 leaf has no separators or embedded NULs.
    unsafe {
        (*rename).Anonymous.ReplaceIfExists = false;
        (*rename).RootDirectory = parent;
        (*rename).FileNameLength = ((name.len() - 1) * 2) as u32;
        std::ptr::copy_nonoverlapping(
            name.as_ptr(),
            std::ptr::addr_of_mut!((*rename).FileName).cast(),
            name.len(),
        );
        let result = NtSetInformationFile(
            source,
            &mut IO_STATUS_BLOCK::default(),
            rename.cast(),
            length as u32,
            10,
        );
        if result < 0 {
            return Err(io::Error::from_raw_os_error(
                RtlNtStatusToDosError(result) as i32
            ));
        }
    }
    Ok(())
}

fn new_transaction() -> io::Result<OwnedHandle> {
    // No ambient transaction, distributed participant, or timeout that could
    // expire while a caller still relies on the guard. Last-handle close aborts.
    owned(unsafe {
        CreateTransaction(
            null_mut(),
            null_mut(),
            TRANSACTION_DO_NOT_PROMOTE,
            0,
            0,
            0,
            null(),
        )
    })
}

fn final_name(handle: HANDLE) -> io::Result<std::path::PathBuf> {
    let mut name = vec![0u16; 32768];
    let length = unsafe {
        GetFinalPathNameByHandleW(
            handle,
            name.as_mut_ptr(),
            name.len() as u32,
            VOLUME_NAME_GUID,
        )
    };
    if length == 0 {
        return Err(io::Error::last_os_error());
    }
    if length as usize >= name.len() {
        return Err(invalid("resolved registration name too long"));
    }
    Ok(OsString::from_wide(&name[..length as usize]).into())
}

fn owned(handle: HANDLE) -> io::Result<OwnedHandle> {
    if handle == INVALID_HANDLE_VALUE || handle.is_null() {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: a successful creating API transfers this unique handle to us.
    Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
}

fn info(handle: HANDLE) -> io::Result<BY_HANDLE_FILE_INFORMATION> {
    let mut metadata = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: the output is correctly sized and the caller retains the handle.
    if unsafe { GetFileInformationByHandle(handle, &mut metadata) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(metadata)
}

fn ordinary(handle: HANDLE) -> io::Result<()> {
    let flags = info(handle)?.dwFileAttributes;
    if flags & (FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_DIRECTORY) != FILE_ATTRIBUTE_DIRECTORY
    {
        return Err(conflict(
            "registration ancestor is not an ordinary directory",
        ));
    }
    Ok(())
}

fn relative_open(parent: HANDLE, name: &OsStr, sharing: u32) -> io::Result<OwnedHandle> {
    relative_open_access(parent, name, sharing, FILE_READ_ATTRIBUTES)
}

fn relative_open_access(
    parent: HANDLE,
    name: &OsStr,
    sharing: u32,
    access: u32,
) -> io::Result<OwnedHandle> {
    let mut text: Vec<u16> = name.encode_wide().collect();
    let length = u16::try_from(text.len() * 2).map_err(|_| invalid("component too long"))?;
    let name = UNICODE_STRING {
        Length: length,
        MaximumLength: length,
        Buffer: text.as_mut_ptr(),
    };
    let attributes = ObjectAttributes {
        length: size_of::<ObjectAttributes>() as u32,
        root_directory: parent,
        object_name: &name,
        attributes: 0, // exact spelling, also safe for case-sensitive directories
        security_descriptor: null(),
        security_quality_of_service: null(),
    };
    let mut handle = null_mut();
    let mut status = IO_STATUS_BLOCK::default();
    // SYNCHRONIZE, FILE_SYNCHRONOUS_IO_NONALERT, FILE_OPEN_REPARSE_POINT.
    // One validated component: no ancestor or leaf reparse traversal is possible.
    let result = unsafe {
        NtOpenFile(
            &mut handle,
            access | 0x0010_0000,
            &attributes,
            &mut status,
            sharing,
            0x0020_0020,
        )
    };
    if result < 0 {
        return Err(io::Error::from_raw_os_error(
            unsafe { RtlNtStatusToDosError(result) } as i32,
        ));
    }
    owned(handle)
}

/// Read-only package observation with the same root-relative, nontraversing
/// namespace protection as registration. Unlike FileGuard, this is not a TxF
/// snapshot or mutation authority: a preexisting writable mapping may still
/// change bytes. Consumers must describe hashes as observations, not activation
/// evidence, and must use FileGuard for protected publication/removal.
pub(crate) struct ReadGuard {
    pub(crate) file: File,
    _parents: ParentGuard,
}

impl ReadGuard {
    pub(crate) fn open(path: &Path) -> io::Result<Self> {
        Self::from_parents(ParentGuard::open(path)?)
    }

    /// SQLite read transactions must coexist with its WAL writer. This pins
    /// the ordinary namespace but intentionally permits data writers; callers
    /// must use SQLite's snapshot protocol and verify its opened file identity.
    pub(crate) fn open_shared(path: &Path) -> io::Result<Self> {
        Self::from_parents_with_share(ParentGuard::open(path)?, FILE_SHARE_READ | FILE_SHARE_WRITE)
    }

    /// Immutable broker credentials also require descriptor inspection on the
    /// retained file object. Ordinary source/package readers retain their
    /// narrower original access mask.
    pub(crate) fn open_security(path: &Path) -> io::Result<Self> {
        Self::from_parents_with_access(ParentGuard::open(path)?, FILE_SHARE_READ, 0x00020000)
    }

    fn from_parents(parents: ParentGuard) -> io::Result<Self> {
        Self::from_parents_with_share(parents, FILE_SHARE_READ)
    }

    fn from_parents_with_share(parents: ParentGuard, share: u32) -> io::Result<Self> {
        Self::from_parents_with_access(parents, share, 0)
    }

    fn from_parents_with_access(parents: ParentGuard, share: u32, extra: u32) -> io::Result<Self> {
        let handle = relative_open_access(
            parents.handle(),
            &parents.leaf,
            share,
            FILE_READ_ATTRIBUTES | FILE_READ_DATA | extra,
        )?;
        let metadata = info(handle.as_raw_handle())?;
        if metadata.dwFileAttributes & (FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_REPARSE_POINT)
            != 0
        {
            return Err(conflict("package observation requires an ordinary file"));
        }
        // The opened leaf pins a nonempty parent. Recheck after that pin, so a
        // preceding attribute-only reparse change fails without target traversal.
        ordinary(parents.handle())?;
        Ok(Self {
            file: File::from(handle),
            _parents: parents,
        })
    }
}

fn volume_root(handle: HANDLE) -> io::Result<std::path::PathBuf> {
    let mut filesystem = [0u16; 32];
    if unsafe {
        GetVolumeInformationByHandleW(
            handle,
            null_mut(),
            0,
            null_mut(),
            null_mut(),
            null_mut(),
            filesystem.as_mut_ptr(),
            filesystem.len() as u32,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    if !filesystem.starts_with(&[78, 84, 70, 83, 0]) {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "guarded unlink requires local NTFS",
        ));
    }
    let mut name = [0u16; 64];
    let length = unsafe {
        GetFinalPathNameByHandleW(
            handle,
            name.as_mut_ptr(),
            name.len() as u32,
            VOLUME_NAME_GUID | FILE_NAME_OPENED,
        )
    };
    if length == 0 {
        return Err(io::Error::last_os_error());
    }
    if length != 49 {
        return Err(invalid("drive does not identify an ordinary volume root"));
    }
    let root = String::from_utf16(&name[..49]).map_err(|_| invalid("invalid volume root"))?;
    if !root.starts_with("\\\\?\\Volume{") || !root.ends_with("}\\") {
        return Err(invalid("unsupported volume namespace"));
    }
    Ok(root.into())
}

#[derive(Debug, PartialEq, Eq)]
struct LocalPath {
    drive: u8,
    names: Vec<OsString>,
}

/// Lexical validation only: reject UNC/device/ambiguous paths before any I/O.
pub(crate) fn validate_local_path(path: &Path) -> io::Result<()> {
    local_path(path).map(|_| ())
}

fn local_path(path: &Path) -> io::Result<LocalPath> {
    let mut text: Vec<u16> = path.as_os_str().encode_wide().collect();
    if text.len() > 32760 || text.contains(&0) {
        return Err(invalid("invalid or oversized registration path"));
    }
    for ch in &mut text {
        if *ch == u16::from(b'/') {
            *ch = u16::from(b'\\');
        }
    }
    let text = text.strip_prefix(&[92, 92, 63, 92]).unwrap_or(&text);
    if text.len() < 3
        || text[0] > 127
        || !(text[0] as u8).is_ascii_alphabetic()
        || text[1..3] != [58, 92]
    {
        return Err(invalid(
            "registration paths must be absolute local drive paths",
        ));
    }
    let mut names = Vec::new();
    if text.len() > 3 {
        for name in text[3..].split(|ch| *ch == 92) {
            if name.is_empty()
                || matches!(name.last(), Some(32 | 46))
                || name
                    .iter()
                    .any(|ch| *ch < 32 || [58, 42, 63, 34, 60, 62, 124].contains(ch))
            {
                return Err(invalid("ambiguous registration path component"));
            }
            let stem: String = name
                .iter()
                .take_while(|ch| **ch != 46)
                .map(|ch| char::from_u32(u32::from(*ch)).unwrap_or('\u{fffd}'))
                .collect::<String>()
                .to_ascii_uppercase();
            if matches!(
                stem.as_str(),
                "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
            ) || ((stem.starts_with("COM") || stem.starts_with("LPT"))
                && stem.len() == 4
                && stem.as_bytes()[3].is_ascii_digit())
            {
                return Err(invalid("device names are not registration paths"));
            }
            names.push(OsString::from_wide(name));
        }
    }
    Ok(LocalPath {
        drive: (text[0] as u8).to_ascii_uppercase(),
        names,
    })
}

fn wide(text: &OsStr) -> Vec<u16> {
    text.encode_wide().chain(Some(0)).collect()
}

fn reparse_data(handle: HANDLE) -> io::Result<Vec<u8>> {
    let mut bytes = vec![0u8; 16384];
    let mut length = 0;
    if unsafe {
        DeviceIoControl(
            handle,
            FSCTL_GET_REPARSE_POINT,
            null(),
            0,
            bytes.as_mut_ptr().cast(),
            bytes.len() as u32,
            &mut length,
            null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    if length as usize > bytes.len() {
        return Err(invalid("invalid reparse data length"));
    }
    bytes.truncate(length as usize);
    Ok(bytes)
}

fn symlink_target(bytes: &[u8]) -> io::Result<std::path::PathBuf> {
    if bytes.len() < 20
        || u32::from_le_bytes(bytes[..4].try_into().unwrap()) != IO_REPARSE_TAG_SYMLINK
        || usize::from(u16::from_le_bytes(bytes[4..6].try_into().unwrap())) + 8 != bytes.len()
        || bytes[16..20] != [0; 4]
    {
        return Err(conflict("not a supported absolute symbolic link"));
    }
    let range = |offset: usize| -> io::Result<&[u8]> {
        let start = usize::from(u16::from_le_bytes(
            bytes[offset..offset + 2].try_into().unwrap(),
        ));
        let size = usize::from(u16::from_le_bytes(
            bytes[offset + 2..offset + 4].try_into().unwrap(),
        ));
        if start % 2 != 0 || size % 2 != 0 {
            return Err(invalid("unaligned reparse name"));
        }
        bytes
            .get(20 + start..20 + start + size)
            .ok_or_else(|| invalid("invalid reparse name bounds"))
    };
    let target: Vec<u16> = range(8)?
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    range(12)?; // Validate the ignored print name too; only the substitute controls traversal.
    let target = target.strip_prefix(&[92, 63, 63, 92]).unwrap_or(&target);
    Ok(OsString::from_wide(target).into())
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

fn conflict(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::windows::fs::{symlink_dir, symlink_file};
    use windows_sys::Win32::Storage::FileSystem::{FILE_WRITE_ATTRIBUTES, FILE_WRITE_DATA};

    fn fixture() -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix("harness-native-unlink-")
            .tempdir()
            .unwrap()
    }

    fn native_open(path: &Path, access: u32) -> io::Result<OwnedHandle> {
        let path = wide(path.as_os_str());
        owned(unsafe {
            CreateFileW(
                path.as_ptr(),
                access,
                SHARE_ALL,
                null(),
                OPEN_EXISTING,
                OPEN_FLAGS,
                null_mut(),
            )
        })
    }

    fn control(handle: HANDLE, code: u32, bytes: &[u8]) -> io::Result<()> {
        if unsafe {
            DeviceIoControl(
                handle,
                code,
                bytes.as_ptr().cast(),
                bytes.len() as u32,
                null_mut(),
                0,
                &mut 0,
                null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    fn remove_tag(path: &Path) -> io::Result<()> {
        let writer = native_open(path, FILE_WRITE_ATTRIBUTES)?;
        control(
            writer.as_raw_handle(),
            0x0009_00ac,
            &[12, 0, 0, 160, 0, 0, 0, 0],
        )
    }

    fn junction(path: &Path, target: &Path) -> io::Result<()> {
        let substitute: Vec<u8> = OsString::from(format!("\\??\\{}", target.display()))
            .encode_wide()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect();
        let mut bytes = Vec::from(0xa000_0003u32.to_le_bytes());
        bytes.extend(((8 + substitute.len() + 2) as u16).to_le_bytes());
        bytes.extend(0u16.to_le_bytes());
        bytes.extend(0u16.to_le_bytes());
        bytes.extend(((substitute.len() - 2) as u16).to_le_bytes());
        bytes.extend((substitute.len() as u16).to_le_bytes());
        bytes.extend(0u16.to_le_bytes());
        bytes.extend(substitute);
        bytes.extend(0u16.to_le_bytes());
        let writer = native_open(path, FILE_WRITE_ATTRIBUTES)?;
        control(writer.as_raw_handle(), 0x0009_00a4, &bytes)
    }

    fn sharing_error(error: io::Error) {
        assert!(
            matches!(error.raw_os_error(), Some(32 | 6800 | 6824)),
            "{error:?}"
        );
    }

    // Scenarios with no intervening writer; ownership races retain the staging
    // identity explicitly and call super::remove_link below.
    fn remove_link(path: &Path, target: &Path, directory: bool) -> io::Result<()> {
        let id = link_identity(path, target, directory)?;
        super::remove_link(path, target, directory, &id)
    }

    #[test]
    fn staged_creation_is_exclusive_and_drop_rolls_back() {
        let root = fixture();
        for directory in [false, true] {
            let target = root.path().join("missing");
            let path = root.path().join(format!("stage-{directory}"));
            let staged = StagedLink::create(&path, &target, directory).unwrap();
            let id = staged.identity();
            assert_eq!(identity(staged.guard.handle()).unwrap(), id);
            // It is invisible, but its name is reserved against competing creates.
            assert!(fs::symlink_metadata(&path).is_err());
            assert!(StagedLink::create(&path, &target, directory).is_err());
            if directory {
                assert!(symlink_dir(&target, &path).is_err());
            } else {
                assert!(symlink_file(&target, &path).is_err());
            }
            assert_eq!(
                remove_tag(&path).unwrap_err().kind(),
                io::ErrorKind::NotFound
            );
            drop(staged);
            assert!(fs::symlink_metadata(&path).is_err());
            if directory {
                symlink_dir(&target, &path).unwrap();
            } else {
                symlink_file(&target, &path).unwrap();
            }
            let foreign = link_identity(&path, &target, directory).unwrap();
            assert!(StagedLink::create(&path, &target, directory).is_err());
            assert_eq!(link_identity(&path, &target, directory).unwrap(), foreign);
        }
    }

    #[test]
    fn sibling_stages_coexist_with_journal_then_commit_and_publish() {
        let root = fixture();
        let target = root.path().join("missing");
        let mut stages = Vec::new();
        for index in 0..8 {
            let path = root.path().join(format!("stage-{index}"));
            let stage = StagedLink::create(&path, &target, index % 2 == 0).unwrap();
            stages.push((path, index % 2 == 0, stage.identity(), stage));
        }
        let journal = root.path().join("journal");
        let bytes =
            serde_json::to_vec(&stages.iter().map(|(_, _, id, _)| id).collect::<Vec<_>>()).unwrap();
        fs::write(&journal, &bytes).unwrap();
        let journal = FileGuard::open_regular(&journal, &bytes).unwrap();
        let mut committed = Vec::new();
        for (path, directory, id, stage) in stages {
            stage.commit().unwrap();
            assert_eq!(link_identity(&path, &target, directory).unwrap(), id);
            committed.push((path, directory, id));
        }
        for (index, (path, directory, id)) in committed.into_iter().enumerate() {
            let destination = root.path().join(format!("published-{index}"));
            publish_link(&path, &destination, &target, directory, &id).unwrap();
            assert_eq!(link_identity(&destination, &target, directory).unwrap(), id);
            super::remove_link(&destination, &target, directory, &id).unwrap();
        }
        journal.remove().unwrap();
    }

    #[test]
    fn uncommitted_child_blocks_empty_parent_redirect_and_rename() {
        for regular in [false, true] {
            let root = fixture();
            let parent = root.path().join("empty");
            fs::create_dir(&parent).unwrap();
            let target = root.path().join("missing");
            let file = regular.then(|| StagedFile::create(&parent.join("stage"), &[]).unwrap());
            let link = (!regular)
                .then(|| StagedLink::create(&parent.join("stage"), &target, false).unwrap());
            assert!(junction(&parent, root.path()).is_err());
            sharing_error(fs::rename(&parent, root.path().join("moved")).unwrap_err());
            drop((file, link));
            assert!(!parent.join("stage").exists());
            junction(&parent, root.path()).unwrap(); // Positive control: now really empty.
            fs::remove_dir(&parent).unwrap();
        }
    }

    #[test]
    fn creation_rolls_back_if_parent_is_redirected_after_verification() {
        let root = fixture();
        let parent = root.path().join("empty");
        let foreign = root.path().join("foreign");
        fs::create_dir(&parent).unwrap();
        fs::create_dir(&foreign).unwrap();
        fs::write(foreign.join("keep"), b"foreign").unwrap();
        let stage = parent.join("stage");
        let result = StagedLink::create_inner(&stage, &root.path().join("missing"), false, || {
            junction(&parent, &foreign).unwrap();
        });
        assert!(result.is_err());
        assert!(fs::symlink_metadata(foreign.join("stage")).is_err());
        assert_eq!(fs::read(foreign.join("keep")).unwrap(), b"foreign");
        fs::remove_dir(&parent).unwrap();
    }

    #[test]
    fn relative_lookup_does_not_follow_a_changed_root_handles_reparse() {
        let root = fixture();
        let parent = root.path().join("empty");
        let target = root.path().join("foreign");
        fs::create_dir(&parent).unwrap();
        fs::create_dir(&target).unwrap();
        fs::write(target.join("keep"), b"foreign").unwrap();
        let held = native_open(&parent, FILE_READ_ATTRIBUTES).unwrap();
        ordinary(held.as_raw_handle()).unwrap();
        junction(&parent, &target).unwrap();
        assert!(relative_open(held.as_raw_handle(), OsStr::new("keep"), FILE_SHARE_READ).is_err());
        assert!(ordinary(held.as_raw_handle()).is_err());
        assert_eq!(fs::read(target.join("keep")).unwrap(), b"foreign");
        drop(held);
        fs::remove_dir(&parent).unwrap();
    }

    #[test]
    fn read_guard_refuses_a_parent_redirected_after_its_capture() {
        let root = fixture();
        let parent = root.path().join("empty");
        let target = root.path().join("foreign");
        fs::create_dir(&parent).unwrap();
        fs::create_dir(&target).unwrap();
        fs::write(target.join("keep"), b"foreign content must not be read").unwrap();
        let held = ParentGuard::open(&parent.join("keep")).unwrap();
        junction(&parent, &target).unwrap();
        assert!(ReadGuard::from_parents(held).is_err());
        fs::remove_dir(&parent).unwrap();
        assert_eq!(
            fs::read(target.join("keep")).unwrap(),
            b"foreign content must not be read"
        );
    }

    #[test]
    fn foreign_create_at_final_publication_boundary_survives() {
        let root = fixture();
        let target = root.path().join("missing");
        let path = root.path().join("stage");
        let stage = StagedLink::create(&path, &target, false).unwrap();
        let id = stage.identity();
        stage.commit().unwrap();
        let parent = root.path().join("empty");
        fs::create_dir(&parent).unwrap();
        let destination = parent.join("foreign");
        let result = publish_link_inner(&path, &destination, &target, false, &id, || {
            // The parent itself cannot be redirected, but a competitor is free
            // to create a child. The kernel rename must refuse that occupied name.
            assert!(junction(&parent, root.path()).is_err());
            sharing_error(fs::rename(&parent, root.path().join("moved")).unwrap_err());
            symlink_file(&target, &destination).unwrap();
        });
        assert!(result.is_err());
        let foreign = link_identity(&destination, &target, false).unwrap();
        assert_ne!(foreign, id);
        assert!(super::remove_link(&destination, &target, false, &id).is_err());
        assert_eq!(link_identity(&path, &target, false).unwrap(), id);
        assert_eq!(
            link_identity(&destination, &target, false).unwrap(),
            foreign
        );
    }

    #[test]
    fn publication_preserves_identity_for_files_directories_and_dangling_targets() {
        let root = fixture();
        for directory in [false, true] {
            for dangling in [false, true] {
                let source = root.path().join(format!("source-{directory}-{dangling}"));
                if !dangling {
                    if directory {
                        fs::create_dir(&source).unwrap();
                    } else {
                        fs::write(&source, b"keep").unwrap();
                    }
                }
                let stage = root.path().join(format!("stage-{directory}-{dangling}"));
                if directory {
                    symlink_dir(&source, &stage).unwrap();
                } else {
                    symlink_file(&source, &stage).unwrap();
                }
                let parent = root.path().join(format!("empty-{directory}-{dangling}"));
                fs::create_dir(&parent).unwrap();
                let destination = parent.join("published");
                let id = link_identity(&stage, &source, directory).unwrap();
                let encoded = serde_json::to_vec(&id).unwrap();
                let decoded: LinkIdentity = serde_json::from_slice(&encoded).unwrap();
                assert_eq!(id, decoded);
                publish_link(&stage, &destination, &source, directory, &id).unwrap();
                assert!(fs::symlink_metadata(&stage).is_err());
                assert_eq!(link_identity(&destination, &source, directory).unwrap(), id);
                super::remove_link(&destination, &source, directory, &id).unwrap();
                assert!(fs::symlink_metadata(&destination).is_err());
                assert_eq!(source.exists(), !dangling);
            }
        }
    }

    #[test]
    fn matching_foreign_destination_is_neither_replaced_nor_removed() {
        let root = fixture();
        for directory in [false, true] {
            let source = root.path().join("missing");
            let stage = root.path().join(format!("stage-{directory}"));
            let destination = root.path().join(format!("destination-{directory}"));
            if directory {
                symlink_dir(&source, &stage).unwrap();
            } else {
                symlink_file(&source, &stage).unwrap();
            }
            let own = link_identity(&stage, &source, directory).unwrap();
            // Competing creation after intent records our staging identity.
            if directory {
                symlink_dir(&source, &destination).unwrap();
            } else {
                symlink_file(&source, &destination).unwrap();
            }
            let foreign = link_identity(&destination, &source, directory).unwrap();
            assert_ne!(own, foreign);
            assert!(publish_link(&stage, &destination, &source, directory, &own).is_err());
            assert!(super::remove_link(&destination, &source, directory, &own).is_err());
            assert_eq!(
                link_identity(&destination, &source, directory).unwrap(),
                foreign
            );
            assert_eq!(link_identity(&stage, &source, directory).unwrap(), own);
            super::remove_link(&stage, &source, directory, &own).unwrap();
        }
    }

    #[test]
    fn replaced_stage_and_replaced_published_link_are_preserved() {
        let root = fixture();
        let source = root.path().join("missing");
        let stage = root.path().join("stage");
        let destination = root.path().join("destination");
        symlink_file(&source, &stage).unwrap();
        let own = link_identity(&stage, &source, false).unwrap();
        let saved = root.path().join("saved");
        fs::rename(&stage, &saved).unwrap(); // Original stays alive: deterministic different ID.
        symlink_file(&source, &stage).unwrap();
        let foreign = link_identity(&stage, &source, false).unwrap();
        assert_ne!(own, foreign);
        assert!(publish_link(&stage, &destination, &source, false, &own).is_err());
        assert!(super::remove_link(&stage, &source, false, &own).is_err());
        assert!(!destination.exists());
        assert_eq!(link_identity(&stage, &source, false).unwrap(), foreign);
        publish_link(&saved, &destination, &source, false, &own).unwrap();
        fs::rename(&destination, &saved).unwrap();
        symlink_file(&source, &destination).unwrap();
        assert!(super::remove_link(&destination, &source, false, &own).is_err());
        assert_eq!(fs::read_link(&destination).unwrap(), source);
    }

    #[test]
    fn publication_refuses_existing_regular_or_reparse_destination_parent() {
        let root = fixture();
        let source = root.path().join("missing");
        let stage = root.path().join("stage");
        symlink_file(&source, &stage).unwrap();
        let own = link_identity(&stage, &source, false).unwrap();
        let occupied = root.path().join("occupied");
        fs::write(&occupied, b"foreign").unwrap();
        assert!(publish_link(&stage, &occupied, &source, false, &own).is_err());
        assert_eq!(fs::read(&occupied).unwrap(), b"foreign");
        let target = root.path().join("directory");
        fs::create_dir(&target).unwrap();
        let redirect = root.path().join("redirect");
        symlink_dir(&target, &redirect).unwrap();
        assert!(publish_link(&stage, &redirect.join("new"), &source, false, &own).is_err());
        assert!(!target.join("new").exists());
        assert_eq!(link_identity(&stage, &source, false).unwrap(), own);
    }

    #[test]
    #[ignore = "set HARNESS_NATIVE_OTHER_VOLUME_TMP to an owned writable directory on a second volume"]
    fn cross_volume_publication_is_refused_without_copying() {
        let root = fixture();
        let second = tempfile::Builder::new()
            .prefix("harness-native-unlink-other-")
            .tempdir_in(
                std::env::var_os("HARNESS_NATIVE_OTHER_VOLUME_TMP")
                    .expect("second volume fixture root"),
            )
            .unwrap();
        let source = root.path().join("missing");
        let stage = root.path().join("stage");
        symlink_file(&source, &stage).unwrap();
        let own = link_identity(&stage, &source, false).unwrap();
        let destination = second.path().join("destination");
        let parent = ParentGuard::open(&destination).unwrap();
        assert_ne!(
            identity(parent.handle()).unwrap().volume_serial_number,
            own.volume_serial_number
        );
        drop(parent);
        assert!(publish_link(&stage, &destination, &source, false, &own).is_err());
        assert_eq!(link_identity(&stage, &source, false).unwrap(), own);
        assert!(fs::symlink_metadata(&destination).is_err());
    }

    #[test]
    fn sharing_only_baseline_allows_attribute_only_reparse_mutation() {
        let root = fixture();
        let link = root.path().join("link");
        symlink_dir(root.path().join("missing"), &link).unwrap();
        let name = wide(link.as_os_str());
        let handle = owned(unsafe {
            CreateFileW(
                name.as_ptr(),
                DELETE | FILE_READ_ATTRIBUTES,
                FILE_SHARE_READ,
                null(),
                OPEN_EXISTING,
                OPEN_FLAGS,
                null_mut(),
            )
        })
        .unwrap();
        // Counterexample: share protection is held, but the actual tag is removed.
        remove_tag(&link).unwrap();
        assert_eq!(
            info(handle.as_raw_handle()).unwrap().dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT,
            0
        );
        drop(handle);
    }

    #[test]
    fn verified_link_blocks_swap_tag_removal_and_parent_rename_until_unlink() {
        let root = fixture();
        for directory in [false, true] {
            let ancestor = root.path().join(format!("ancestor-{directory}"));
            let parent = ancestor.join("parent");
            fs::create_dir_all(&parent).unwrap();
            let source = root.path().join("original-missing");
            let foreign = root.path().join("foreign-missing");
            let link = parent.join("link");
            let replacement = root.path().join(format!("replacement-{directory}"));
            if directory {
                symlink_dir(&source, &link).unwrap();
                symlink_dir(&foreign, &replacement).unwrap();
            } else {
                symlink_file(&source, &link).unwrap();
                symlink_file(&foreign, &replacement).unwrap();
            }
            let guard = FileGuard::open(&link).unwrap();
            guard
                .verify_link(&local_path(&source).unwrap(), directory)
                .unwrap();
            // Deterministic adversarial interval: the same verified handle remains
            // alive until the exact production disposition/commit implementation.
            sharing_error(fs::rename(&link, parent.join("saved")).unwrap_err());
            assert!(fs::rename(&replacement, &link).is_err());
            sharing_error(remove_tag(&link).unwrap_err());
            sharing_error(fs::rename(&parent, ancestor.join("moved")).unwrap_err());
            sharing_error(fs::rename(&ancestor, root.path().join("moved")).unwrap_err());
            // Attribute-only opens on ancestors are allowed. The locked child
            // makes the directory nonempty, so NTFS rejects the actual redirect.
            assert_eq!(
                junction(&parent, root.path()).unwrap_err().raw_os_error(),
                Some(145)
            );
            guard.remove().unwrap();
            assert!(fs::symlink_metadata(&link).is_err());
            assert_eq!(fs::read_link(&replacement).unwrap(), foreign);
            fs::rename(&parent, ancestor.join("moved")).unwrap();
        }
    }

    #[test]
    fn preexisting_attribute_or_data_writer_aborts_and_preserves_link() {
        let root = fixture();
        let source = root.path().join("missing");
        for directory in [false, true] {
            for access in [FILE_WRITE_ATTRIBUTES, FILE_WRITE_DATA, DELETE] {
                let link = root.path().join(format!("link-{directory}-{access}"));
                if directory {
                    symlink_dir(&source, &link).unwrap();
                } else {
                    symlink_file(&source, &link).unwrap();
                }
                let writer = native_open(&link, access).unwrap();
                sharing_error(remove_link(&link, &source, directory).unwrap_err());
                assert_eq!(fs::read_link(&link).unwrap(), source);
                drop(writer);
                remove_link(&link, &source, directory).unwrap();
            }
        }
    }

    #[test]
    fn changed_targets_types_relative_links_and_ancestor_reparses_are_preserved() {
        let root = fixture();
        let source = root.path().join("source");
        let foreign = root.path().join("foreign");
        fs::write(&source, b"owned source").unwrap();
        fs::write(&foreign, b"foreign source").unwrap();
        let link = root.path().join("link");
        symlink_file(&foreign, &link).unwrap();
        assert!(remove_link(&link, &source, false).is_err());
        assert_eq!(fs::read_link(&link).unwrap(), foreign);
        assert!(remove_link(&link, &foreign, true).is_err());
        assert!(remove_link(&source, &source, false).is_err());
        let relative = root.path().join("relative");
        symlink_file("source", &relative).unwrap();
        assert!(remove_link(&relative, &source, false).is_err());
        let dir = root.path().join("directory");
        fs::create_dir(&dir).unwrap();
        let child = dir.join("child");
        symlink_file(&source, &child).unwrap();
        let redirect = root.path().join("redirect");
        symlink_dir(&dir, &redirect).unwrap();
        assert!(remove_link(&redirect.join("child"), &source, false).is_err());
        let mount = root.path().join("junction");
        fs::create_dir(&mount).unwrap();
        junction(&mount, &dir).unwrap(); // Positive control for the mutation probe.
        assert!(remove_link(&mount.join("child"), &source, false).is_err());
        assert!(remove_link(&mount, &dir, true).is_err());
        assert_eq!(fs::read_link(&child).unwrap(), source);
        assert_eq!(fs::read(&source).unwrap(), b"owned source");
        assert_eq!(fs::read(&foreign).unwrap(), b"foreign source");
        fs::remove_dir(&mount).unwrap();
    }

    #[test]
    fn regular_guard_refuses_foreign_type_hardlinks_and_open_writers() {
        let root = fixture();
        let record = root.path().join("record");
        fs::write(&record, b"original").unwrap();
        assert!(FileGuard::open_regular(&record, b"modified").is_err());
        assert!(FileGuard::open_regular(&record, b"short").is_err());
        assert!(FileGuard::open_regular(root.path(), b"").is_err());
        let link = root.path().join("symlink");
        symlink_file(&record, &link).unwrap();
        assert!(FileGuard::open_regular(&link, b"original").is_err());
        let writer = native_open(&record, FILE_WRITE_ATTRIBUTES).unwrap();
        assert!(FileGuard::open_regular(&record, b"original").is_err());
        drop(writer);
        let alias = root.path().join("hardlink");
        fs::hard_link(&record, &alias).unwrap();
        assert!(FileGuard::open_regular(&record, b"original").is_err());
        assert_eq!(fs::read(&alias).unwrap(), b"original");
    }

    #[test]
    fn regular_guard_refuses_a_writable_mapping_after_its_file_handle_closes() {
        use windows_sys::Win32::System::Memory::{
            CreateFileMappingW, FILE_MAP_WRITE, MapViewOfFile, PAGE_READWRITE, UnmapViewOfFile,
        };
        let root = fixture();
        let record = root.path().join("record");
        fs::write(&record, b"original").unwrap();
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&record)
            .unwrap();
        let mapping = owned(unsafe {
            CreateFileMappingW(file.as_raw_handle(), null(), PAGE_READWRITE, 0, 0, null())
        })
        .unwrap();
        let view = unsafe { MapViewOfFile(mapping.as_raw_handle(), FILE_MAP_WRITE, 0, 0, 0) };
        assert!(!view.Value.is_null());
        drop(file);
        let guarded = FileGuard::open_regular(&record, b"original");
        // Close the view before assertions, so even a failed exclusion probe
        // cannot retain a mapped writer to an abandoned test fixture.
        unsafe {
            view.Value.cast::<u8>().write(b'X');
            assert_ne!(UnmapViewOfFile(view), 0);
        }
        drop(mapping);
        assert!(guarded.is_err());
        assert_eq!(fs::read(&record).unwrap(), b"Xriginal");
        FileGuard::open_regular(&record, b"Xriginal")
            .unwrap()
            .remove()
            .unwrap();
    }

    #[test]
    fn committed_removal_with_an_existing_metadata_reader() {
        let root = fixture();
        let record = root.path().join("record");
        fs::write(&record, b"owned").unwrap();
        let reader = native_open(&record, FILE_READ_ATTRIBUTES).unwrap();
        let guard = FileGuard::open_regular(&record, b"owned").unwrap();
        guard.remove().unwrap();
        assert!(fs::symlink_metadata(&record).is_err());
        drop(reader);
        assert!(fs::symlink_metadata(&record).is_err());
    }

    fn posix_replace(source: &Path, destination: &Path) -> io::Result<()> {
        let file = native_open(source, DELETE)?;
        let parent = native_open(destination.parent().unwrap(), FILE_READ_ATTRIBUTES)?;
        let name = wide(destination.file_name().unwrap());
        let length = size_of::<FILE_RENAME_INFO>() + name.len() * 2;
        let mut buffer = vec![0u64; length.div_ceil(8)];
        let rename = buffer.as_mut_ptr().cast::<FILE_RENAME_INFO>();
        unsafe {
            (*rename).Anonymous.Flags = 3; // REPLACE_IF_EXISTS | POSIX_SEMANTICS.
            (*rename).RootDirectory = parent.as_raw_handle();
            (*rename).FileNameLength = ((name.len() - 1) * 2) as u32;
            std::ptr::copy_nonoverlapping(
                name.as_ptr(),
                std::ptr::addr_of_mut!((*rename).FileName).cast(),
                name.len(),
            );
            let result = NtSetInformationFile(
                file.as_raw_handle(),
                &mut IO_STATUS_BLOCK::default(),
                rename.cast(),
                length as u32,
                65,
            ); // FileRenameInformationEx.
            if result < 0 {
                return Err(io::Error::from_raw_os_error(
                    RtlNtStatusToDosError(result) as i32
                ));
            }
        }
        Ok(())
    }

    #[test]
    fn posix_replacement_cannot_bypass_a_verified_record_or_link_guard() {
        let root = fixture();
        for link in [false, true] {
            let path = root.path().join(format!("owned-{link}"));
            let replacement = root.path().join(format!("foreign-{link}"));
            let target = root.path().join("missing");
            let guard = if link {
                symlink_file(&target, &path).unwrap();
                symlink_file(root.path().join("other-missing"), &replacement).unwrap();
                verified_link(&path, &target, false).unwrap()
            } else {
                fs::write(&path, b"owned").unwrap();
                fs::write(&replacement, b"foreign").unwrap();
                FileGuard::open_regular(&path, b"owned").unwrap()
            };
            assert!(posix_replace(&replacement, &path).is_err());
            guard.remove().unwrap();
            assert!(fs::symlink_metadata(&replacement).is_ok());
            posix_replace(&replacement, &path).unwrap(); // Positive control.
            if link {
                assert_eq!(
                    fs::read_link(&path).unwrap(),
                    root.path().join("other-missing")
                );
            } else {
                assert_eq!(fs::read(&path).unwrap(), b"foreign");
            }
        }
    }

    #[test]
    fn exact_extended_unicode_target_is_accepted_without_resolving_it() {
        let root = fixture();
        let source = root.path().join("источник отсутствует");
        let link = root.path().join("ссылка");
        symlink_file(&source, &link).unwrap();
        let extended = std::path::PathBuf::from(format!("\\\\?\\{}", source.display()));
        remove_link(&link, &extended, false).unwrap();
        assert!(!source.exists());
    }

    #[test]
    fn ambiguous_paths_and_malformed_reparse_buffers_are_refused() {
        for path in [
            "relative",
            "C:relative",
            "\\rooted",
            "C:\\a\\..\\b",
            "C:\\a\\.\\b",
            "C:\\a.\\b",
            "C:\\a \\b",
            "C:\\a:stream",
            "C:\\NUL",
            "C:\\COM1.txt",
            "\\\\server\\share\\a",
            "\\\\?\\UNC\\server\\share\\a",
            "C:\\a\0b",
        ] {
            assert!(local_path(Path::new(path)).is_err(), "{path:?}");
        }
        assert!(symlink_target(&[]).is_err());
        let mut bytes = vec![0u8; 20];
        bytes[..4].copy_from_slice(&IO_REPARSE_TAG_SYMLINK.to_le_bytes());
        bytes[4..6].copy_from_slice(&12u16.to_le_bytes());
        bytes[16] = 1; // Relative even if the buffer's text would look absolute.
        assert!(symlink_target(&bytes).is_err());
        bytes[16] = 0;
        bytes[8] = 1; // Unaligned substitute-name offset.
        assert!(symlink_target(&bytes).is_err());
        bytes[8] = 0;
        bytes[10] = 200; // Out-of-bounds substitute name.
        assert!(symlink_target(&bytes).is_err());
    }

    #[test]
    fn removes_only_file_directory_and_dangling_links() {
        let root = fixture();
        for directory in [false, true] {
            for dangling in [false, true] {
                let source = root.path().join(format!("source-{directory}-{dangling}"));
                if !dangling {
                    if directory {
                        fs::create_dir(&source).unwrap();
                        fs::write(source.join("keep"), b"source").unwrap();
                    } else {
                        fs::write(&source, b"source").unwrap();
                    }
                }
                let link = root.path().join(format!("link-{directory}-{dangling}"));
                if directory {
                    symlink_dir(&source, &link).unwrap();
                } else {
                    symlink_file(&source, &link).unwrap();
                }
                remove_link(&link, &source, directory).unwrap();
                assert!(fs::symlink_metadata(&link).is_err());
                if !dangling {
                    assert_eq!(
                        fs::read(if directory {
                            source.join("keep")
                        } else {
                            source
                        })
                        .unwrap(),
                        b"source"
                    );
                }
            }
        }
    }

    #[test]
    fn staged_regular_bytes_are_isolated_and_drop_restores_contents_and_length() {
        let root = fixture();
        let path = root.path().join("config.toml");
        let before = b"original configuration";
        fs::write(&path, before).unwrap();
        let original = FileGuard::read_regular(&path)
            .unwrap()
            .0
            .object_identity()
            .unwrap();
        for after in [vec![], b"short".to_vec(), vec![b'x'; 128 * 1024]] {
            let mut guard = FileGuard::open_with_write(&path, true).unwrap();
            guard.stage_regular_bytes(&after).unwrap();
            assert_eq!(guard.regular_bytes().unwrap(), after);
            assert!(fs::write(&path, b"foreign").is_err());
            assert!(fs::rename(&path, root.path().join("foreign")).is_err());
            // Exercise the interval used by commit: closing the data handle
            // must not publish the staged content or release the TxF exclusion.
            drop(guard.file.take());
            assert_eq!(fs::read(&path).unwrap(), before);
            assert!(fs::write(&path, b"foreign").is_err());
            drop(guard);
            let (restored, bytes) = FileGuard::read_regular(&path).unwrap();
            assert_eq!(bytes, before);
            assert_eq!(restored.object_identity().unwrap(), original);
        }
    }

    #[test]
    fn staged_regular_creation_is_exclusive_and_publication_preserves_identity() {
        let root = fixture();
        let stage = root.path().join("stage");
        let destination = root.path().join("config.toml");
        let data = b"private configuration";
        let staged = StagedFile::create(&stage, data).unwrap();
        assert!(fs::read(&stage).is_err());
        assert!(fs::write(&stage, b"foreign").is_err());
        drop(staged);
        assert!(!stage.exists());
        let staged = StagedFile::create(&stage, data).unwrap();
        let identity = staged.identity();
        staged.commit().unwrap();
        assert_eq!(fs::read(&stage).unwrap(), data);
        assert!(StagedFile::create(&stage, data).is_err());
        fs::write(&destination, data).unwrap();
        assert!(publish_regular(&stage, &destination, data, &identity).is_err());
        assert_eq!(fs::read(&stage).unwrap(), data);
        assert_eq!(fs::read(&destination).unwrap(), data);
        fs::remove_file(&destination).unwrap(); // owned test actor's object
        publish_regular(&stage, &destination, data, &identity).unwrap();
        assert!(!stage.exists());
        let guard = FileGuard::open_regular(&destination, data).unwrap();
        assert_eq!(guard.object_identity().unwrap(), identity);
        guard.remove().unwrap();
        assert!(!destination.exists());
    }

    #[test]
    #[ignore = "owned child of killed_regular_write_rolls_back_without_rust_destructors"]
    fn regular_write_process_fixture() {
        let root =
            std::path::PathBuf::from(std::env::var_os("HARNESS_REGULAR_WRITE_ROOT").unwrap());
        let mut guard = if std::env::var_os("HARNESS_REGULAR_WRITE_CREATE").is_some() {
            StagedFile::create(&root.join("config.toml"), &vec![b'x'; 128 * 1024])
                .unwrap()
                .guard
        } else {
            let mut guard = FileGuard::open_with_write(&root.join("config.toml"), true).unwrap();
            guard.stage_regular_bytes(&vec![b'x'; 128 * 1024]).unwrap();
            guard
        };
        assert_eq!(guard.regular_bytes().unwrap(), vec![b'x'; 128 * 1024]);
        if std::env::var_os("HARNESS_REGULAR_WRITE_CLOSE_DATA").is_some() {
            drop(guard.file.take());
        }
        fs::write(root.join("staged"), b"ready").unwrap();
        std::thread::sleep(std::time::Duration::from_secs(20));
        // No implicit success: the parent must terminate this process while the
        // transaction is live. If it fails to do so, exit without Rust Drop.
        std::process::exit(79);
    }

    #[test]
    fn killed_regular_write_rolls_back_without_rust_destructors() {
        use std::process::{Command, Stdio};
        use std::time::{Duration, Instant};
        for (create, close_data) in [(false, false), (false, true), (true, false), (true, true)] {
            let root = fixture();
            let path = root.path().join("config.toml");
            let original = if create {
                None
            } else {
                fs::write(&path, b"original configuration").unwrap();
                Some(
                    FileGuard::read_regular(&path)
                        .unwrap()
                        .0
                        .object_identity()
                        .unwrap(),
                )
            };
            let mut command = Command::new(std::env::current_exe().unwrap());
            command
                .args([
                    "--ignored",
                    "--exact",
                    "registration_native::tests::regular_write_process_fixture",
                    "--nocapture",
                ])
                .env("HARNESS_REGULAR_WRITE_ROOT", root.path())
                .env_remove("HARNESS_REGULAR_WRITE_CLOSE_DATA")
                .env_remove("HARNESS_REGULAR_WRITE_CREATE")
                .stdin(Stdio::null())
                .stdout(File::create(root.path().join("stdout")).unwrap())
                .stderr(File::create(root.path().join("stderr")).unwrap());
            if close_data {
                command.env("HARNESS_REGULAR_WRITE_CLOSE_DATA", "1");
            }
            if create {
                command.env("HARNESS_REGULAR_WRITE_CREATE", "1");
            }
            let mut child = command.spawn().unwrap();
            let until = Instant::now() + Duration::from_secs(10);
            let marker = root.path().join("staged");
            while !marker.exists() && Instant::now() < until {
                if child.try_wait().unwrap().is_some() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            let observed = marker.exists();
            // This retained handle targets only the owned Rust fixture, which
            // never creates descendants. Reap even if the setup timed out.
            let _ = child.kill();
            let exit = child.wait().unwrap();
            assert!(
                observed,
                "staging was not observed: {}",
                root.path().display()
            );
            assert!(!exit.success());
            assert_ne!(exit.code(), Some(79), "fixture reached its own deadline");
            if let Some(original) = original {
                let (guard, bytes) = FileGuard::read_regular(&path).unwrap();
                assert_eq!(bytes, b"original configuration");
                assert_eq!(guard.object_identity().unwrap(), original);
            } else {
                assert!(
                    fs::symlink_metadata(&path).is_err_and(|e| e.kind() == io::ErrorKind::NotFound)
                );
            }
        }
    }

    #[test]
    fn regular_guard_preserves_on_drop_removes_explicitly_and_allows_siblings() {
        let root = fixture();
        let journal = root.path().join("journal");
        fs::write(&journal, b"expected").unwrap();
        let guard = FileGuard::open_regular(&journal, b"expected").unwrap();
        assert!(fs::write(&journal, b"foreign").is_err());
        assert!(fs::rename(&journal, root.path().join("moved")).is_err());
        let marker = root.path().join("marker");
        fs::write(&marker, b"bound").unwrap();
        FileGuard::open_regular(&marker, b"bound")
            .unwrap()
            .remove()
            .unwrap();
        let link = root.path().join("link");
        let source = root.path().join("missing");
        symlink_file(&source, &link).unwrap();
        remove_link(&link, &source, false).unwrap();
        drop(guard);
        assert_eq!(fs::read(&journal).unwrap(), b"expected");
        assert!(FileGuard::open_regular(&journal, b"foreign!").is_err());
        FileGuard::open_regular(&journal, b"expected")
            .unwrap()
            .remove()
            .unwrap();
        assert!(!journal.exists());
    }
}
