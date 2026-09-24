//! Named resource admission compatible with the transitional byte locks.
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
    BrokerStartup,
    BrokerInstance,
    Desktop,
    HeavyCommand,
}

impl Resource {
    fn filename(self) -> &'static str {
        match self {
            Self::BrokerStartup => "startup.lock",
            Self::BrokerInstance => "instance.lock",
            Self::Desktop => "desktop.lock",
            Self::HeavyCommand => HEAVY_COMMAND_LOCK,
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
    /// Nonblocking broker ownership probe. Busy is not evidence of an absent
    /// owner, even when no valid endpoint has been published yet.
    pub fn try_acquire(directory: &Path, resource: Resource) -> io::Result<Option<Self>> {
        let guard = Self::open(directory, resource)?;
        match guard.file.try_lock() {
            Ok(()) => Ok(Some(Self { _guard: guard })),
            Err(TryLockError::WouldBlock) => Ok(None),
            Err(TryLockError::Error(error)) => Err(error),
        }
    }

    pub fn acquire(
        directory: &Path,
        resource: Resource,
        deadline: Deadline,
        cancellation: &Cancellation,
    ) -> io::Result<Self> {
        Self::acquire_reporting(directory, resource, deadline, cancellation, || {})
    }

    /// Same accounting and lock semantics as `acquire`, but reports the first
    /// busy observation exactly once so a caller can print one queue diagnostic
    /// before it blocks.
    pub fn acquire_reporting(
        directory: &Path,
        resource: Resource,
        deadline: Deadline,
        cancellation: &Cancellation,
        mut waiting: impl FnMut(),
    ) -> io::Result<Self> {
        check_stop(deadline, cancellation)?;
        let guard = Self::open(directory, resource)?;
        let mut reported = false;
        loop {
            check_stop(deadline, cancellation)?;
            match guard.file.try_lock() {
                Ok(()) => return Ok(Self { _guard: guard }),
                Err(TryLockError::WouldBlock) => {
                    note_busy(&mut reported, &mut waiting, deadline);
                }
                Err(TryLockError::Error(error)) => return Err(error),
            }
        }
    }

    fn open(directory: &Path, resource: Resource) -> io::Result<ReadGuard> {
        open_named(directory, resource.filename())
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

const POLL_INTERVAL: Duration = Duration::from_millis(20);
const HEAVY_COMMAND_LOCK: &str = "heavy-command.lock";

fn note_busy(reported: &mut bool, waiting: &mut impl FnMut(), deadline: Deadline) {
    if !*reported {
        *reported = true;
        waiting();
    }
    std::thread::sleep(POLL_INTERVAL.min(deadline.remaining()));
}

fn open_named(directory: &Path, filename: &str) -> io::Result<ReadGuard> {
    let path = directory.join(filename);
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
    Ok(guard)
}

fn heavy_slot_filename(index: u32) -> String {
    format!("heavy-command.slot-{index}.lock")
}

/// Bounded heavy-command admission that shares the legacy `heavy-command.lock`.
///
/// A slot count of 1 takes an exclusive lock on that file and creates no slot
/// file. A larger count holds a shared lock on the same file for the whole
/// admission and an exclusive lock on exactly one `heavy-command.slot-N.lock`.
/// The shared lock overlaps the historical first-byte exclusive range, so an
/// old exclusive lock and a new admission exclude each other. Opening or
/// locking a slot file without that legacy lock is not admission. Drop releases
/// this admission's locks and does not delete or rewrite lock files.
pub struct HeavyAdmission {
    _slot: Option<ReadGuard>,
    _legacy: ReadGuard,
    slot_index: Option<u32>,
}

impl HeavyAdmission {
    /// Slot index `N` when this admission holds `heavy-command.slot-N.lock`.
    /// `None` for the exclusive single-slot legacy lock.
    pub fn slot_index(&self) -> Option<u32> {
        self.slot_index
    }

    /// Acquire one heavy-command admission in `directory`.
    ///
    /// `slot_count` of 0 fails before any lock file is created. Busy admission
    /// uses the existing 20 ms poll, invokes `waiting` once, and returns
    /// [`ErrorKind::TimedOut`](io::ErrorKind::TimedOut) at `deadline` without
    /// releasing other holders. Cancellation returns
    /// [`ErrorKind::Interrupted`](io::ErrorKind::Interrupted).
    pub fn acquire(
        directory: &Path,
        slot_count: u32,
        deadline: Deadline,
        cancellation: &Cancellation,
        mut waiting: impl FnMut(),
    ) -> io::Result<Self> {
        if slot_count == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "heavy-command slot count must be positive; no lock file created",
            ));
        }
        check_stop(deadline, cancellation)?;
        let legacy = open_named(directory, HEAVY_COMMAND_LOCK)?;
        let mut reported = false;
        if slot_count == 1 {
            loop {
                check_stop(deadline, cancellation)?;
                match legacy.file.try_lock() {
                    Ok(()) => {
                        return Ok(Self {
                            _slot: None,
                            _legacy: legacy,
                            slot_index: None,
                        });
                    }
                    Err(TryLockError::WouldBlock) => {
                        note_busy(&mut reported, &mut waiting, deadline);
                    }
                    Err(TryLockError::Error(error)) => return Err(error),
                }
            }
        }

        let mut slots = Vec::new();
        loop {
            check_stop(deadline, cancellation)?;
            match legacy.file.try_lock_shared() {
                Ok(()) => {}
                Err(TryLockError::WouldBlock) => {
                    note_busy(&mut reported, &mut waiting, deadline);
                    continue;
                }
                Err(TryLockError::Error(error)) => return Err(error),
            }
            if slots.is_empty() {
                for index in 0..slot_count {
                    slots.push(open_named(directory, &heavy_slot_filename(index))?);
                }
            }
            let mut chosen = None;
            for index in 0..slot_count {
                match slots[index as usize].file.try_lock() {
                    Ok(()) => {
                        chosen = Some(index);
                        break;
                    }
                    Err(TryLockError::WouldBlock) => {}
                    Err(TryLockError::Error(error)) => return Err(error),
                }
            }
            if let Some(index) = chosen {
                let slot = slots.swap_remove(index as usize);
                return Ok(Self {
                    _slot: Some(slot),
                    _legacy: legacy,
                    slot_index: Some(index),
                });
            }
            // Hold the shared lock only for a successful admission. Releasing
            // it before the queue poll keeps a full slot set from blocking a
            // legacy exclusive lock for the whole deadline.
            legacy.file.unlock()?;
            note_busy(&mut reported, &mut waiting, deadline);
        }
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
        let path = root.path().join("startup.lock");
        fs::write(&path, b"existing-private-marker").unwrap();
        let first = acquire(root.path(), Resource::BrokerStartup).unwrap();
        assert!(matches!(
            acquire(root.path(), Resource::BrokerStartup),
            Err(error) if error.kind() == io::ErrorKind::TimedOut
        ));
        let independent = acquire(root.path(), Resource::BrokerInstance).unwrap();
        assert!(fs::remove_file(&path).is_err());
        drop((first, independent));
        assert_eq!(fs::read(&path).unwrap(), b"existing-private-marker");
        assert!(acquire(root.path(), Resource::BrokerStartup).is_ok());
        assert!(path.exists());
    }

    #[test]
    fn conflicts_with_the_legacy_first_byte_lock() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("startup.lock");
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
            acquire(root.path(), Resource::BrokerStartup),
            Err(error) if error.kind() == io::ErrorKind::TimedOut
        ));
        drop(legacy);
        assert!(acquire(root.path(), Resource::BrokerStartup).is_ok());
    }

    #[test]
    fn cancellation_and_missing_parent_do_not_create_state() {
        let root = tempfile::tempdir().unwrap();
        let cancellation = Cancellation::default();
        cancellation.cancel();
        assert!(matches!(Lease::acquire(
            root.path(), Resource::BrokerStartup,
            Deadline::after(Duration::from_secs(1)).unwrap(), &cancellation
        ), Err(error) if error.kind() == io::ErrorKind::Interrupted));
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
        let absent = root.path().join("absent");
        assert!(acquire(&absent, Resource::BrokerStartup).is_err());
        assert!(!absent.exists());
    }

    #[test]
    fn aliased_lock_is_rejected_without_changing_sentinel() {
        let root = tempfile::tempdir().unwrap();
        let sentinel = root.path().join("sentinel");
        fs::write(&sentinel, b"PRIVATE-SENTINEL").unwrap();
        fs::hard_link(&sentinel, root.path().join("startup.lock")).unwrap();
        assert!(acquire(root.path(), Resource::BrokerStartup).is_err());
        assert_eq!(fs::read(&sentinel).unwrap(), b"PRIVATE-SENTINEL");
    }

    #[test]
    fn heavy_command_slot_serializes_and_reports_the_first_busy_observation() {
        let root = tempfile::tempdir().unwrap();
        let held = acquire(root.path(), Resource::HeavyCommand).unwrap();
        assert!(root.path().join("heavy-command.lock").is_file());
        let cancellation = Cancellation::default();
        let mut reported = 0;
        let waited = Lease::acquire_reporting(
            root.path(),
            Resource::HeavyCommand,
            Deadline::after(Duration::from_millis(120)).unwrap(),
            &cancellation,
            || reported += 1,
        );
        assert!(matches!(waited, Err(error) if error.kind() == io::ErrorKind::TimedOut));
        assert_eq!(reported, 1);
        drop(held);
        let mut reported = 0;
        assert!(
            Lease::acquire_reporting(
                root.path(),
                Resource::HeavyCommand,
                Deadline::after(Duration::from_secs(1)).unwrap(),
                &cancellation,
                || reported += 1,
            )
            .is_ok()
        );
        assert_eq!(reported, 0);
    }

    fn lock_first_byte_exclusive(file: &fs::File) -> bool {
        let mut offset: OVERLAPPED = unsafe { std::mem::zeroed() };
        let locked = unsafe {
            LockFileEx(
                file.as_raw_handle(),
                LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY,
                0,
                1,
                0,
                &mut offset,
            )
        };
        locked != 0
    }

    fn heavy(
        root: &Path,
        slot_count: u32,
        wait: Duration,
        cancellation: &Cancellation,
        waiting: impl FnMut(),
    ) -> io::Result<HeavyAdmission> {
        HeavyAdmission::acquire(
            root,
            slot_count,
            Deadline::after(wait).unwrap(),
            cancellation,
            waiting,
        )
    }

    #[test]
    fn slot_count_zero_fails_before_creating_files() {
        let root = tempfile::tempdir().unwrap();
        let Err(error) = heavy(
            root.path(),
            0,
            Duration::from_secs(1),
            &Cancellation::default(),
            || panic!("waiting must not run"),
        ) else {
            panic!("slot count 0 must fail before creating files");
        };
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
        let absent = root.path().join("absent");
        assert!(
            heavy(
                &absent,
                0,
                Duration::from_secs(1),
                &Cancellation::default(),
                || {},
            )
            .is_err()
        );
        assert!(!absent.exists());
    }

    #[test]
    fn heavy_cancellation_creates_no_lock_file() {
        let root = tempfile::tempdir().unwrap();
        let cancellation = Cancellation::default();
        cancellation.cancel();
        for slot_count in [1_u32, 2] {
            let Err(error) = heavy(
                root.path(),
                slot_count,
                Duration::from_secs(1),
                &cancellation,
                || panic!("waiting must not run"),
            ) else {
                panic!("cancelled admission must fail");
            };
            assert_eq!(error.kind(), io::ErrorKind::Interrupted);
        }
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
    }

    #[test]
    fn single_slot_uses_exclusive_legacy_lock_and_no_slot_file() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("heavy-command.lock");
        fs::write(&path, b"existing-private-marker").unwrap();
        let probe = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        let held = heavy(
            root.path(),
            1,
            Duration::from_secs(1),
            &Cancellation::default(),
            || panic!("waiting must not run"),
        )
        .unwrap();
        assert!(held.slot_index().is_none());
        assert!(!root.path().join("heavy-command.slot-0.lock").exists());
        assert!(
            !lock_first_byte_exclusive(&probe),
            "slot count 1 must take the legacy exclusive range"
        );
        let mut reported = 0;
        let waited = heavy(
            root.path(),
            1,
            Duration::from_millis(80),
            &Cancellation::default(),
            || reported += 1,
        );
        assert!(matches!(
            waited,
            Err(error) if error.kind() == io::ErrorKind::TimedOut
        ));
        assert_eq!(reported, 1);
        assert!(held.slot_index().is_none());
        drop(held);
        assert!(
            lock_first_byte_exclusive(&probe),
            "dropping the single-slot admission must release the legacy lock"
        );
        drop(probe);
        assert!(
            heavy(
                root.path(),
                1,
                Duration::from_secs(1),
                &Cancellation::default(),
                || panic!("waiting must not run"),
            )
            .is_ok()
        );
        assert!(!root.path().join("heavy-command.slot-0.lock").exists());
        assert_eq!(fs::read(&path).unwrap(), b"existing-private-marker");
    }

    #[test]
    fn bounded_slots_are_concurrent_and_release_exactly_their_lock() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("heavy-command.lock");
        fs::write(&path, b"legacy-marker").unwrap();
        let probe = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        let cancellation = Cancellation::default();
        let first = heavy(
            root.path(),
            2,
            Duration::from_secs(1),
            &cancellation,
            || panic!("waiting must not run"),
        )
        .unwrap();
        let second = heavy(
            root.path(),
            2,
            Duration::from_secs(1),
            &cancellation,
            || panic!("waiting must not run"),
        )
        .unwrap();
        assert_eq!(first.slot_index(), Some(0));
        assert_eq!(second.slot_index(), Some(1));
        assert!(root.path().join("heavy-command.slot-0.lock").is_file());
        assert!(root.path().join("heavy-command.slot-1.lock").is_file());
        assert!(!root.path().join("heavy-command.slot-2.lock").exists());
        assert!(
            !lock_first_byte_exclusive(&probe),
            "held admission must block the legacy exclusive lock"
        );

        let mut reported = 0;
        let waited = heavy(
            root.path(),
            2,
            Duration::from_millis(120),
            &cancellation,
            || reported += 1,
        );
        assert!(matches!(
            waited,
            Err(error) if error.kind() == io::ErrorKind::TimedOut
        ));
        assert_eq!(reported, 1);
        assert_eq!(first.slot_index(), Some(0));
        assert_eq!(second.slot_index(), Some(1));
        assert!(
            !lock_first_byte_exclusive(&probe),
            "a timed-out waiter must not release holders"
        );

        drop(first);
        let reacquired = heavy(
            root.path(),
            2,
            Duration::from_secs(1),
            &cancellation,
            || panic!("waiting must not run"),
        )
        .unwrap();
        assert_eq!(reacquired.slot_index(), Some(0));
        assert_eq!(second.slot_index(), Some(1));
        assert!(
            !lock_first_byte_exclusive(&probe),
            "releasing one slot must leave the other admission's legacy lock held"
        );
        let mut reported = 0;
        assert!(matches!(
            heavy(
                root.path(),
                2,
                Duration::from_millis(80),
                &cancellation,
                || reported += 1,
            ),
            Err(error) if error.kind() == io::ErrorKind::TimedOut
        ));
        assert_eq!(reported, 1);
        drop(second);
        drop(reacquired);
        assert!(
            lock_first_byte_exclusive(&probe),
            "dropping every admission must release the legacy lock"
        );
        drop(probe);
        assert_eq!(fs::read(&path).unwrap(), b"legacy-marker");
    }

    #[test]
    fn legacy_exclusive_lock_blocks_admission_while_slots_are_free() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("heavy-command.lock");
        fs::write(&path, b"legacy-marker").unwrap();
        fs::write(
            root.path().join("heavy-command.slot-0.lock"),
            b"slot-marker",
        )
        .unwrap();
        fs::write(root.path().join("heavy-command.slot-1.lock"), b"").unwrap();
        let legacy = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        assert!(lock_first_byte_exclusive(&legacy));
        let mut reported = 0;
        let waited = heavy(
            root.path(),
            2,
            Duration::from_millis(80),
            &Cancellation::default(),
            || reported += 1,
        );
        assert!(matches!(
            waited,
            Err(error) if error.kind() == io::ErrorKind::TimedOut
        ));
        assert_eq!(reported, 1);
        assert_eq!(
            fs::read(root.path().join("heavy-command.slot-0.lock")).unwrap(),
            b"slot-marker"
        );
        drop(legacy);
        assert_eq!(fs::read(&path).unwrap(), b"legacy-marker");
        let probe = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        let admitted = heavy(
            root.path(),
            2,
            Duration::from_secs(1),
            &Cancellation::default(),
            || panic!("waiting must not run"),
        )
        .unwrap();
        assert_eq!(admitted.slot_index(), Some(0));
        assert!(!lock_first_byte_exclusive(&probe));
        drop(admitted);
        assert!(lock_first_byte_exclusive(&probe));
    }

    #[test]
    fn opening_only_a_slot_file_is_not_admission() {
        let root = tempfile::tempdir().unwrap();
        let slot = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(root.path().join("heavy-command.slot-0.lock"))
            .unwrap();
        assert!(slot.try_lock().is_ok());
        let legacy_path = root.path().join("heavy-command.lock");
        let legacy = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&legacy_path)
            .unwrap();
        assert!(
            lock_first_byte_exclusive(&legacy),
            "a slot file lock must not block the legacy lock"
        );
        drop(legacy);
        let admitted = heavy(
            root.path(),
            2,
            Duration::from_secs(1),
            &Cancellation::default(),
            || panic!("waiting must not run"),
        )
        .unwrap();
        assert_eq!(admitted.slot_index(), Some(1));
        let probe = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&legacy_path)
            .unwrap();
        assert!(!lock_first_byte_exclusive(&probe));
        drop(probe);
        drop(admitted);
        let probe = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&legacy_path)
            .unwrap();
        assert!(
            lock_first_byte_exclusive(&probe),
            "the remaining slot-only lock must not count as admission"
        );
    }
}
