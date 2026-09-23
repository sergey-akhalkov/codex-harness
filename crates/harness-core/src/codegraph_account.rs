//! One reusable private broker location per Windows account, across Codex homes.
#![cfg(windows)]
use crate::{
    broker_state::{BrokerRoot, Generation},
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
    #[serde(default)]
    generations: Vec<Generation>,
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
    root_in(Path::new(&parent), None, deadline, cancel)
}

/// The location for one build generation: an existing entry, a free legacy
/// account root, or a freshly prepared root. Consumers of another generation
/// keep their own broker instead of being retired.
pub fn root_for(source: &str, deadline: Deadline, cancel: &Cancellation) -> io::Result<PathBuf> {
    let parent = std::env::var_os("LOCALAPPDATA")
        .ok_or_else(|| io::Error::other("CodeGraph local account storage is unavailable"))?;
    root_in(Path::new(&parent), Some(source), deadline, cancel)
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
    if bytes.len() > crate::broker_state::ANCHOR_LIMIT {
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

fn root_in(
    parent: &Path,
    source: Option<&str>,
    deadline: Deadline,
    cancel: &Cancellation,
) -> io::Result<PathBuf> {
    let parent = local_path(parent)?;
    if !parent.is_dir() {
        return Err(io::Error::other("CodeGraph account parent is missing"));
    }
    let account = crate::process_service::current_user()?;
    let _admission = Admission::acquire(&account, deadline, cancel)?;
    let anchor = parent.join("coding-agents-harness-codegraph.json");
    match FileGuard::read_regular(&anchor) {
        Ok((guard, bytes)) => {
            if bytes.len() > crate::broker_state::ANCHOR_LIMIT {
                return Err(io::Error::other(
                    "CodeGraph account record exceeds its bound",
                ));
            }
            let mut record: Record = serde_json::from_slice(&bytes)?;
            if record.owner != OWNER || record.account != account {
                return Err(io::Error::other(
                    "CodeGraph account record is not owned; preserving it",
                ));
            }
            let Some(source) = source else {
                let root = BrokerRoot::open(&record.root)?;
                return Ok(root.path().to_path_buf());
            };
            let identity = guard.object_identity()?;
            drop(guard);
            let (root, changed) = crate::broker_state::choose_generation(
                &record.root,
                &mut record.generations,
                source,
                || {
                    let prepared = BrokerRoot::prepare()?;
                    Ok(prepared.keep().path().to_path_buf())
                },
            )?;
            if changed {
                crate::registration_native::FileGuard::replace_regular(
                    &anchor,
                    &identity,
                    &bytes,
                    &serde_json::to_vec(&record)?,
                )?;
            }
            Ok(root)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let prepared = BrokerRoot::prepare()?;
            let root = prepared.keep().path().to_path_buf();
            let record = Record {
                owner: OWNER.into(),
                account,
                root: root.clone(),
                generations: source
                    .map(|source| Generation {
                        source: source.to_owned(),
                        root: root.clone(),
                    })
                    .into_iter()
                    .collect(),
            };
            StagedFile::create(&anchor, &serde_json::to_vec(&record)?)?.commit()?;
            Ok(root)
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
        let root = root_in(parent.path(), None, deadline, &cancel).unwrap();
        assert_eq!(
            root_in(parent.path(), None, deadline, &cancel).unwrap(),
            root
        );
        let anchor = parent.path().join("coding-agents-harness-codegraph.json");
        std::fs::write(&anchor, b"{\"owner\":\"foreign\"}").unwrap();
        assert!(root_in(parent.path(), None, deadline, &cancel).is_err());
        assert_eq!(std::fs::read(anchor).unwrap(), b"{\"owner\":\"foreign\"}");
        // Only this test's known private root is removed; no service was started.
        drop(BrokerRoot::open(&root).unwrap());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_second_generation_gets_its_own_location_and_reuses_it() {
        let parent = tempfile::tempdir().unwrap();
        let deadline = Deadline::after(Duration::from_secs(5)).unwrap();
        let cancel = Cancellation::default();
        // The first generation adopts the free legacy root.
        let first = root_in(parent.path(), Some("aaaa0000aaaa0000"), deadline, &cancel).unwrap();
        assert_eq!(
            root_in(parent.path(), Some("aaaa0000aaaa0000"), deadline, &cancel).unwrap(),
            first
        );
        // While that root's instance lease is held, the next generation is
        // prepared beside it instead of retiring the first broker.
        let lease = BrokerRoot::open(&first).unwrap();
        let _held = lease.try_instance().unwrap().unwrap();
        let second = root_in(parent.path(), Some("bbbb0000bbbb0000"), deadline, &cancel).unwrap();
        assert_ne!(second, first);
        assert_eq!(
            root_in(parent.path(), Some("bbbb0000bbbb0000"), deadline, &cancel).unwrap(),
            second
        );
        drop(_held);
        drop(lease);
        for root in [first, second] {
            drop(BrokerRoot::open(&root).unwrap());
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn grown_record_from_repeated_deliveries_is_pruned_and_reused() {
        let parent = tempfile::tempdir().unwrap();
        let deadline = Deadline::after(Duration::from_secs(10)).unwrap();
        let cancel = Cancellation::default();
        let mut roots = Vec::new();
        let mut generations = Vec::new();
        for seed in 0u32..30 {
            let root = BrokerRoot::prepare().unwrap().keep().path().to_path_buf();
            generations.push(Generation {
                source: format!("{seed:064x}"),
                root: root.clone(),
            });
            roots.push(root);
        }
        let anchor = parent.path().join("coding-agents-harness-codegraph.json");
        let record = serde_json::json!({
            "owner": OWNER,
            "account": crate::process_service::current_user().unwrap(),
            "root": roots[0],
            "generations": generations,
        });
        let bytes = serde_json::to_vec(&record).unwrap();
        assert!(
            bytes.len() > 4096,
            "the fixture must reproduce the grown live record"
        );
        std::fs::write(&anchor, &bytes).unwrap();
        let new_source = u64::MAX;
        let source = format!("{new_source:064x}");
        let resolved = root_in(parent.path(), Some(&source), deadline, &cancel).unwrap();
        assert!(
            roots.contains(&resolved),
            "a freed location must be reused instead of preparing a new one"
        );
        let rewritten: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&anchor).unwrap()).unwrap();
        let retained = rewritten["generations"].as_array().unwrap();
        assert_eq!(retained.len(), 1, "{retained:?}");
        assert_eq!(retained[0]["source"], source);
        assert!(
            std::fs::metadata(&anchor).unwrap().len() < 4096,
            "the rewritten record must be bounded"
        );
        for root in roots {
            drop(BrokerRoot::open(&root).ok());
            std::fs::remove_dir_all(root).unwrap();
        }
    }
}
