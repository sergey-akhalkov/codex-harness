//! One reusable private broker location per Windows account, across Codex homes.
#![cfg(windows)]
use crate::{
    broker_state::BrokerRoot,
    dependency_discovery::local_path,
    process::{Cancellation, Deadline},
    registration_native::{FileGuard, StagedFile},
};
use std::{
    io,
    path::{Path, PathBuf},
};

const OWNER: &str = "coding-agents-harness/codegraph-account/v1";

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    owner: String,
    account: String,
    root: PathBuf,
}

struct Admission(std::os::windows::io::OwnedHandle);
impl Admission {
    fn acquire(account: &str, deadline: Deadline, cancel: &Cancellation) -> io::Result<Self> {
        use std::os::windows::io::FromRawHandle;
        use windows_sys::Win32::{
            Foundation::{WAIT_ABANDONED, WAIT_OBJECT_0, WAIT_TIMEOUT},
            System::Threading::{CreateMutexW, WaitForSingleObject},
        };
        let name: Vec<_> = format!("Global\\CodingAgentsHarness.CodeGraph.Location.{account}")
            .encode_utf16()
            .chain([0])
            .collect();
        let raw = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
        if raw.is_null() {
            return Err(io::Error::last_os_error());
        }
        let handle = unsafe { std::os::windows::io::OwnedHandle::from_raw_handle(raw) };
        loop {
            if cancel.is_cancelled() || deadline.expired() {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "CodeGraph location admission cancelled or expired",
                ));
            }
            match unsafe {
                WaitForSingleObject(raw, deadline.remaining().as_millis().min(50) as u32)
            } {
                WAIT_OBJECT_0 | WAIT_ABANDONED => return Ok(Self(handle)),
                WAIT_TIMEOUT => (),
                _ => return Err(io::Error::last_os_error()),
            }
        }
    }
}
impl Drop for Admission {
    fn drop(&mut self) {
        use std::os::windows::io::AsRawHandle;
        unsafe {
            windows_sys::Win32::System::Threading::ReleaseMutex(self.0.as_raw_handle());
        }
    }
}

/// The anchor contains only a private runtime location. Consumer identities and
/// authentication records remain in the protected broker directory. No network,
/// package installation or project discovery happens here.
pub fn root(deadline: Deadline, cancel: &Cancellation) -> io::Result<PathBuf> {
    let parent = std::env::var_os("LOCALAPPDATA")
        .ok_or_else(|| io::Error::other("CodeGraph local account storage is unavailable"))?;
    root_in(Path::new(&parent), deadline, cancel)
}

/// Read-only lookup for Check/Disconnect; an unused installation creates no
/// account anchor, service directory, lock or process.
pub fn existing_root() -> io::Result<Option<PathBuf>> {
    let parent = std::env::var_os("LOCALAPPDATA")
        .ok_or_else(|| io::Error::other("CodeGraph local account storage is unavailable"))?;
    let anchor = local_path(Path::new(&parent))?.join("coding-agents-harness-codegraph.json");
    let (_guard, bytes) = match FileGuard::read_regular(&anchor) {
        Ok(value) => value,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if bytes.len() > 4096 {
        return Err(io::Error::other(
            "CodeGraph account record exceeds its bound",
        ));
    }
    let record: Record = serde_json::from_slice(&bytes)?;
    if record.owner != OWNER || record.account != crate::process_service::current_user()? {
        return Err(io::Error::other(
            "CodeGraph account record is not owned; preserving it",
        ));
    }
    Ok(Some(BrokerRoot::open(&record.root)?.path().to_path_buf()))
}

fn root_in(parent: &Path, deadline: Deadline, cancel: &Cancellation) -> io::Result<PathBuf> {
    let parent = local_path(parent)?;
    if !parent.is_dir() {
        return Err(io::Error::other("CodeGraph account parent is missing"));
    }
    let account = crate::process_service::current_user()?;
    let _admission = Admission::acquire(&account, deadline, cancel)?;
    let anchor = parent.join("coding-agents-harness-codegraph.json");
    match FileGuard::read_regular(&anchor) {
        Ok((_guard, bytes)) => {
            if bytes.len() > 4096 {
                return Err(io::Error::other(
                    "CodeGraph account record exceeds its bound",
                ));
            }
            let record: Record = serde_json::from_slice(&bytes)?;
            if record.owner != OWNER || record.account != account {
                return Err(io::Error::other(
                    "CodeGraph account record is not owned; preserving it",
                ));
            }
            let root = BrokerRoot::open(&record.root)?;
            Ok(root.path().to_path_buf())
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let prepared = BrokerRoot::prepare()?;
            let record = Record {
                owner: OWNER.into(),
                account,
                root: prepared.root().path().into(),
            };
            StagedFile::create(&anchor, &serde_json::to_vec(&record)?)?.commit()?;
            let root = prepared.keep();
            Ok(root.path().to_path_buf())
        }
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    #[test]
    fn stable_account_location_reopens_and_foreign_anchor_is_preserved() {
        let parent = tempfile::tempdir().unwrap();
        let deadline = Deadline::after(Duration::from_secs(5)).unwrap();
        let cancel = Cancellation::default();
        let root = root_in(parent.path(), deadline, &cancel).unwrap();
        assert_eq!(root_in(parent.path(), deadline, &cancel).unwrap(), root);
        let anchor = parent.path().join("coding-agents-harness-codegraph.json");
        std::fs::write(&anchor, b"{\"owner\":\"foreign\"}").unwrap();
        assert!(root_in(parent.path(), deadline, &cancel).is_err());
        assert_eq!(std::fs::read(anchor).unwrap(), b"{\"owner\":\"foreign\"}");
        // Only this test's known private root is removed; no service was started.
        drop(BrokerRoot::open(&root).unwrap());
        std::fs::remove_dir_all(root).unwrap();
    }
}
