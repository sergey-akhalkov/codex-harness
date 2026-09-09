//! Nonblocking migration lock shared with `Invoke-HarnessInstall`.
//! The legacy contract is local to the Windows logon session and hashes the
//! lexical, absolute user-home name. It does not establish filesystem ownership.

use std::{io, path::Path};

#[cfg(windows)]
mod windows {
    use super::*;
    use std::{
        cell::RefCell,
        collections::BTreeSet,
        marker::PhantomData,
        os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
        rc::Rc,
    };
    use windows_sys::Win32::{
        Foundation::{WAIT_ABANDONED, WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT},
        System::Threading::{CreateMutexW, ReleaseMutex, WaitForSingleObject},
    };

    thread_local! {
        // Windows mutexes are recursive. Reject reentry on the owning thread;
        // another thread is excluded by the kernel mutex itself.
        static HELD: RefCell<BTreeSet<String>> = const { RefCell::new(BTreeSet::new()) };
    }

    fn name(home: &Path) -> io::Result<String> {
        if !home.is_absolute() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "absolute user home required",
            ));
        }
        let full = std::path::absolute(home)?;
        let text = full.to_str().filter(|s| !s.contains('\0')).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "user home is not valid Unicode",
            )
        })?;
        // .NET ToLowerInvariant uses simple casing: U+0130 stays unchanged,
        // unlike Rust's expanding lowercase mapping. No contextual sigma rule.
        let lower: String = text
            .trim_end_matches('\\')
            .chars()
            .map(|c| {
                let mut lower = c.to_lowercase();
                let first = lower.next().expect("lowercase is nonempty");
                if lower.next().is_none() { first } else { c }
            })
            .collect();
        Ok(format!(
            "Local\\CodexHarness-{}",
            crate::build_identity::hash_bytes(lower.as_bytes()).to_ascii_uppercase()
        ))
    }

    /// Held on the acquiring thread until drop; never transferable to another
    /// thread, because Windows requires that thread to release ownership.
    pub struct InstallationLock {
        handle: OwnedHandle,
        name: String,
        abandoned: bool,
        _thread: PhantomData<Rc<()>>,
    }

    impl InstallationLock {
        /// Does not create a directory or file, wait, or inspect installation
        /// state. Callers must validate/recover that state after acquiring.
        pub fn acquire(user_home: &Path) -> io::Result<Self> {
            let name = name(user_home)?;
            if HELD.with(|held| held.borrow().contains(&name)) {
                return Err(busy());
            }
            let wide: Vec<_> = name.encode_utf16().chain(Some(0)).collect();
            // Null security attributes keep the handle non-inheritable.
            let raw = unsafe { CreateMutexW(std::ptr::null(), 0, wide.as_ptr()) };
            if raw.is_null() {
                return Err(io::Error::last_os_error());
            }
            let handle = unsafe { OwnedHandle::from_raw_handle(raw) };
            let abandoned = match unsafe { WaitForSingleObject(handle.as_raw_handle(), 0) } {
                WAIT_OBJECT_0 => false,
                WAIT_ABANDONED => true,
                WAIT_TIMEOUT => return Err(busy()),
                WAIT_FAILED => return Err(io::Error::last_os_error()),
                _ => return Err(io::Error::other("unexpected installation mutex result")),
            };
            HELD.with(|held| {
                held.borrow_mut().insert(name.clone());
            });
            Ok(Self {
                handle,
                name,
                abandoned,
                _thread: PhantomData,
            })
        }

        /// A predecessor died while owning the mutex. This is an observation,
        /// not evidence that its installation state is safe to use or discard.
        pub fn was_abandoned(&self) -> bool {
            self.abandoned
        }
    }

    impl Drop for InstallationLock {
        fn drop(&mut self) {
            // !Send and private ownership prevent release on a different thread.
            unsafe {
                ReleaseMutex(self.handle.as_raw_handle());
            }
            HELD.with(|held| {
                held.borrow_mut().remove(&self.name);
            });
        }
    }

    fn busy() -> io::Error {
        io::Error::new(
            io::ErrorKind::WouldBlock,
            "another harness operation is active for this user",
        )
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn names_match_installed_powershell_invariant_casing_and_path_contract() {
            // Observed with the installed PowerShell 7.6.5 / Get-HarnessFullPath.
            for (path, hash) in [
                (
                    "C:\\Users\\USER\\",
                    "F46CB28AFC8C8A12A7C41C9F5E38FBA85E648C078A1507BF45D741382F4B13FF",
                ),
                (
                    "C:\\ΔΣ\\İIı\\K\\😀\\",
                    "7069C08D0659629F486DAD6FAFFA630CEFB4D7B1F7898BAA343E80FD65932B3F",
                ),
                (
                    "D:/Каталог/../Пользователь",
                    "4E2F715365DCA2A46FFF21D4F7D17ED909A1CA40EB0B316B26FF1F695C75A421",
                ),
            ] {
                assert_eq!(
                    name(Path::new(path)).unwrap(),
                    format!("Local\\CodexHarness-{hash}")
                );
            }
            assert!(name(Path::new("relative")).is_err());
            assert!(name(Path::new("C:\\bad\0path")).is_err());
        }

        #[test]
        fn reentry_and_competing_thread_fail_without_creating_home() {
            let root = tempfile::tempdir().unwrap();
            let home = root.path().join("Absent Юникод");
            let guard = InstallationLock::acquire(&home).unwrap();
            assert!(!guard.was_abandoned());
            assert_eq!(
                InstallationLock::acquire(&home).err().unwrap().kind(),
                io::ErrorKind::WouldBlock
            );
            let alias = std::path::PathBuf::from(home.to_str().unwrap().to_uppercase());
            assert_eq!(
                InstallationLock::acquire(&alias).err().unwrap().kind(),
                io::ErrorKind::WouldBlock
            );
            let other = home.clone();
            assert_eq!(
                std::thread::spawn(move || InstallationLock::acquire(&other).err().unwrap().kind())
                    .join()
                    .unwrap(),
                io::ErrorKind::WouldBlock
            );
            InstallationLock::acquire(&root.path().join("different-user")).unwrap();
            drop(guard);
            InstallationLock::acquire(&home).unwrap();
            assert!(!home.exists());
        }
    }
}

#[cfg(windows)]
pub use windows::InstallationLock;

#[cfg(not(windows))]
pub struct InstallationLock;

#[cfg(not(windows))]
impl InstallationLock {
    pub fn acquire(_: &Path) -> io::Result<Self> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "native installation requires Windows",
        ))
    }
}
