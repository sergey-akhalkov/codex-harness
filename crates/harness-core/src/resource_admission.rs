//! Account-scoped resource admission compatible with the transitional byte locks.
#![cfg(windows)]

use crate::{
    process::{Cancellation, Deadline},
    registration_native::{ReadGuard, StagedFile},
};
use std::{fs::TryLockError, io, os::windows::io::AsRawHandle, path::Path, time::Duration};
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
};

#[derive(Clone, Copy, Debug)]
pub enum Resource {
    CodebaseIndex,
    CodebaseCatalogue,
}

impl Resource {
    fn filename(self) -> &'static str {
        match self {
            Self::CodebaseIndex => "cbm-index.lock",
            Self::CodebaseCatalogue => "cbm-catalogue.lock",
        }
    }
}

/// The caller supplies the account directory, independently of CODEX_HOME.
/// Provisioning creates that directory; admission never creates parent trees,
/// removes a lock file, changes its contents, or starts a worker.
pub struct Lease {
    _guard: ReadGuard,
}

impl Lease {
    pub fn acquire(
        directory: &Path,
        resource: Resource,
        deadline: Deadline,
        cancellation: &Cancellation,
    ) -> io::Result<Self> {
        check_stop(deadline, cancellation)?;
        let path = directory.join(resource.filename());
        let guard = match ReadGuard::open_shared(&path) {
            Ok(guard) => guard,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                // TxF reserves the absent destination; competing creation is
                // left intact. A later invocation may join that new lock.
                StagedFile::create(&path, b"")?.commit()?;
                ReadGuard::open_shared(&path)?
            }
            Err(error) => return Err(error),
        };
        let mut information = BY_HANDLE_FILE_INFORMATION::default();
        if unsafe { GetFileInformationByHandle(guard.file.as_raw_handle(), &mut information) } == 0
            || information.nNumberOfLinks != 1
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "resource lock is not an unaliased ordinary file",
            ));
        }
        loop {
            check_stop(deadline, cancellation)?;
            match guard.file.try_lock() {
                Ok(()) => return Ok(Self { _guard: guard }),
                Err(TryLockError::WouldBlock) => {
                    std::thread::sleep(Duration::from_millis(20).min(deadline.remaining()));
                }
                Err(TryLockError::Error(error)) => return Err(error),
            }
        }
    }
}

fn check_stop(deadline: Deadline, cancellation: &Cancellation) -> io::Result<()> {
    if cancellation.is_cancelled() {
        Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "resource admission cancelled; no worker started",
        ))
    } else if deadline.expired() {
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "resource busy; no additional worker started",
        ))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, os::windows::io::AsRawHandle};
    use windows_sys::Win32::{
        Storage::FileSystem::{LOCKFILE_EXCLUSIVE_LOCK, LOCKFILE_FAIL_IMMEDIATELY, LockFileEx},
        System::IO::OVERLAPPED,
    };

    fn acquire(root: &Path, resource: Resource) -> io::Result<Lease> {
        Lease::acquire(
            root,
            resource,
            Deadline::after(Duration::from_millis(80)).unwrap(),
            &Cancellation::default(),
        )
    }

    #[test]
    fn serializes_account_slot_and_preserves_existing_bytes_and_file() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("cbm-index.lock");
        fs::write(&path, b"existing-private-marker").unwrap();
        let first = acquire(root.path(), Resource::CodebaseIndex).unwrap();
        assert!(matches!(
            acquire(root.path(), Resource::CodebaseIndex),
            Err(error) if error.kind() == io::ErrorKind::TimedOut
        ));
        let independent = acquire(root.path(), Resource::CodebaseCatalogue).unwrap();
        assert!(fs::remove_file(&path).is_err());
        drop((first, independent));
        assert_eq!(fs::read(&path).unwrap(), b"existing-private-marker");
        assert!(acquire(root.path(), Resource::CodebaseIndex).is_ok());
        assert!(path.exists());
    }

    #[test]
    fn conflicts_with_the_legacy_first_byte_lock() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("cbm-index.lock");
        let legacy = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        let mut offset: OVERLAPPED = unsafe { std::mem::zeroed() };
        assert_ne!(
            unsafe {
                LockFileEx(
                    legacy.as_raw_handle(),
                    LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY,
                    0,
                    1,
                    0,
                    &mut offset,
                )
            },
            0
        );
        assert!(matches!(
            acquire(root.path(), Resource::CodebaseIndex),
            Err(error) if error.kind() == io::ErrorKind::TimedOut
        ));
        drop(legacy);
        assert!(acquire(root.path(), Resource::CodebaseIndex).is_ok());
    }

    #[test]
    fn cancellation_and_missing_parent_do_not_create_state() {
        let root = tempfile::tempdir().unwrap();
        let cancellation = Cancellation::default();
        cancellation.cancel();
        assert!(matches!(Lease::acquire(
            root.path(), Resource::CodebaseIndex,
            Deadline::after(Duration::from_secs(1)).unwrap(), &cancellation
        ), Err(error) if error.kind() == io::ErrorKind::Interrupted));
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
        let absent = root.path().join("absent");
        assert!(acquire(&absent, Resource::CodebaseIndex).is_err());
        assert!(!absent.exists());
    }

    #[test]
    fn aliased_lock_is_rejected_without_changing_sentinel() {
        let root = tempfile::tempdir().unwrap();
        let sentinel = root.path().join("sentinel");
        fs::write(&sentinel, b"PRIVATE-SENTINEL").unwrap();
        fs::hard_link(&sentinel, root.path().join("cbm-index.lock")).unwrap();
        assert!(acquire(root.path(), Resource::CodebaseIndex).is_err());
        assert_eq!(fs::read(&sentinel).unwrap(), b"PRIVATE-SENTINEL");
    }
}
