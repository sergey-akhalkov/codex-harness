//! Private broker state. Runtime opens an explicitly prepared root and never
//! repairs its ACL or adopts an unmarked directory. Lock files are stable.
#![cfg(windows)]
use crate::{
    dependency_mcp_probe::{private_directory, strict_json},
    process::{Cancellation, Deadline},
    process_service::{current_default_owner, current_user},
    registration_native::{ReadGuard, StagedFile},
    resource_admission::{Lease, Resource},
};
use serde::{Deserialize, Serialize};
use std::{
    io::{self, Read},
    mem::{size_of, size_of_val},
    os::windows::{ffi::OsStrExt, io::AsRawHandle},
    path::{Path, PathBuf},
    ptr::null_mut,
};
use windows_sys::Win32::{Foundation::HANDLE, Security::*, Storage::FileSystem::FILE_ALL_ACCESS};

const OWNER: &str = "codex-harness/native-resource-broker/v1";
const MARKER: &str = "broker-owner.json";
fn invalid(reason: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, reason)
}
fn checked(value: i32) -> io::Result<()> {
    if value == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn sid_text(sid: PSID) -> io::Result<String> {
    checked(unsafe { IsValidSid(sid) })?;
    let size = unsafe { GetLengthSid(sid) } as usize;
    if !(8..=68).contains(&size) {
        return Err(invalid("broker SID exceeds its bound"));
    }
    let bytes = unsafe { std::slice::from_raw_parts(sid.cast::<u8>(), size) };
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn validate_descriptor(
    descriptor: PSECURITY_DESCRIPTOR,
    account: &str,
    directory: bool,
) -> io::Result<()> {
    let (mut owner, mut owner_default) = (null_mut(), 0);
    checked(unsafe { GetSecurityDescriptorOwner(descriptor, &mut owner, &mut owner_default) })?;
    if owner.is_null() {
        return Err(invalid("broker state has no owner; preserving state"));
    }
    let owner = sid_text(owner)?;
    if owner != account && owner != current_default_owner()? {
        return Err(invalid(
            "broker state owner differs from current account; preserving state",
        ));
    }
    let (mut acl, mut present, mut defaulted) = (null_mut(), 0, 0);
    let (mut control, mut revision) = (0, 0);
    checked(unsafe {
        GetSecurityDescriptorDacl(descriptor, &mut present, &mut acl, &mut defaulted)
    })?;
    checked(unsafe { GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) })?;
    if present == 0
        || acl.is_null()
        || (directory && control & SE_DACL_PROTECTED == 0)
        || unsafe { (*acl).AceCount } != 1
    {
        return Err(invalid("broker state ACL is not private; preserving state"));
    }
    let mut entry = null_mut();
    checked(unsafe { GetAce(acl, 0, &mut entry) })?;
    let header = unsafe { &*entry.cast::<ACE_HEADER>() };
    if header.AceType != 0 || usize::from(header.AceSize) < size_of::<ACCESS_ALLOWED_ACE>() {
        return Err(invalid(
            "broker state ACL entry is unsupported; preserving state",
        ));
    }
    let ace = unsafe { &*entry.cast::<ACCESS_ALLOWED_ACE>() };
    if ace.Header.AceType != 0
        || ace.Mask != FILE_ALL_ACCESS
        || u32::from(ace.Header.AceFlags) & INHERIT_ONLY_ACE != 0
        || (directory
            && u32::from(ace.Header.AceFlags) & (OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE)
                != OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE)
        || sid_text((&ace.SidStart as *const u32).cast_mut().cast())? != account
    {
        return Err(invalid(
            "broker state ACL grants unexpected access; preserving state",
        ));
    }
    Ok(())
}

fn verify_directory(directory: &Path, account: &str) -> io::Result<()> {
    let name: Vec<u16> = directory.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut data = [0u64; 1024];
    let mut needed = 0;
    checked(unsafe {
        GetFileSecurityW(
            name.as_ptr(),
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            data.as_mut_ptr().cast(),
            size_of_val(&data) as u32,
            &mut needed,
        )
    })?;
    validate_descriptor(data.as_mut_ptr().cast(), account, true)
}

fn verify_file(handle: HANDLE, account: &str) -> io::Result<()> {
    let mut data = [0u64; 1024];
    let mut needed = 0;
    checked(unsafe {
        GetKernelObjectSecurity(
            handle,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            data.as_mut_ptr().cast(),
            size_of_val(&data) as u32,
            &mut needed,
        )
    })?;
    validate_descriptor(data.as_mut_ptr().cast(), account, false)
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Marker {
    owner: String,
    account: String,
}

/// This file pins the ordinary, nonempty directory and its complete parent
/// chain. Its marker contains identity only; it is not a bearer credential.
pub struct BrokerRoot {
    path: PathBuf,
    account: String,
    _marker: ReadGuard,
}

/// Explicit preparation owns a fresh private directory until keep() transfers
/// it to the installation lifecycle. Dropping it closes the root guard first.
pub struct PreparedRoot {
    root: BrokerRoot,
    directory: tempfile::TempDir,
}
impl PreparedRoot {
    pub fn root(&self) -> &BrokerRoot {
        &self.root
    }
    pub fn keep(self) -> BrokerRoot {
        let Self { root, directory } = self;
        let _ = directory.keep();
        root
    }
}

impl BrokerRoot {
    /// Creates only a new random owner-protected directory; does not activate it.
    pub fn prepare() -> io::Result<PreparedRoot> {
        let directory = private_directory()?;
        let marker = Marker {
            owner: OWNER.into(),
            account: current_user()?,
        };
        let bytes =
            serde_json::to_vec(&marker).map_err(|_| invalid("broker marker encoding failed"))?;
        StagedFile::create(&directory.path().join(MARKER), &bytes)?.commit()?;
        let root = Self::open(directory.path())?;
        // Prepare stable lock objects before making this root available to
        // concurrent cold clients. Runtime never needs a first-creation race.
        for name in ["startup.lock", "instance.lock"] {
            StagedFile::create(&root.path().join(name), b"")?.commit()?;
        }
        Ok(PreparedRoot { root, directory })
    }
    /// No writes, acquisition, ACL repair or parent creation during runtime open.
    pub fn open(path: &Path) -> io::Result<Self> {
        let guard = ReadGuard::open_security(&path.join(MARKER))?;
        let mut bytes = Vec::new();
        (&guard.file).take(513).read_to_end(&mut bytes)?;
        if bytes.len() > 512 {
            return Err(invalid("broker marker exceeds its bound"));
        }
        let marker: Marker = serde_json::from_value(
            strict_json(&bytes)
                .map_err(|_| invalid("invalid broker owner marker; preserving state"))?,
        )
        .map_err(|_| invalid("invalid broker owner marker; preserving state"))?;
        let account = current_user()?;
        if marker.owner != OWNER || marker.account != account {
            return Err(invalid("broker owner marker differs; preserving state"));
        }
        // The held marker pins the nonempty directory before the descriptor
        // lookup by name. Reparse/rename replacement cannot redirect this read.
        verify_directory(path, &account)?;
        verify_file(guard.file.as_raw_handle(), &account)?;
        Ok(Self {
            path: path.canonicalize()?,
            account,
            _marker: guard,
        })
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub(crate) fn ensure_private(&self) -> io::Result<()> {
        verify_directory(&self.path, &self.account)?;
        verify_file(self._marker.file.as_raw_handle(), &self.account)
    }
    pub fn startup(&self, deadline: Deadline, cancel: &Cancellation) -> io::Result<Lease> {
        self.ensure_private()?;
        Lease::acquire(&self.path, Resource::BrokerStartup, deadline, cancel)
    }
    /// None means a live startup/instance lock holder must be preserved, even
    /// if its endpoint is absent, stale or unreadable.
    pub fn try_instance(&self) -> io::Result<Option<Lease>> {
        self.ensure_private()?;
        Lease::try_acquire(&self.path, Resource::BrokerInstance)
    }
    pub(crate) fn read_private(&self, name: &str, limit: usize) -> io::Result<Vec<u8>> {
        if !["endpoint.json", "service.log"].contains(&name) {
            return Err(invalid("unknown broker private record"));
        }
        self.ensure_private()?;
        let guard = ReadGuard::open_security(&self.path.join(name))?;
        verify_file(guard.file.as_raw_handle(), &self.account)?;
        let mut bytes = Vec::new();
        (&guard.file)
            .take(limit as u64 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() > limit {
            return Err(invalid("broker private record exceeds its bound"));
        }
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, time::Duration};
    #[test]
    fn private_root_reopens_without_mutation_and_owns_distinct_locks() {
        let prepared = BrokerRoot::prepare().unwrap();
        let root = prepared.root();
        let marker = fs::read(root.path().join(MARKER)).unwrap();
        let reopened = BrokerRoot::open(root.path()).unwrap();
        let startup = root
            .startup(
                Deadline::after(Duration::from_secs(1)).unwrap(),
                &Cancellation::default(),
            )
            .unwrap();
        let instance = root.try_instance().unwrap().unwrap();
        assert!(reopened.try_instance().unwrap().is_none());
        assert_eq!(
            root.startup(
                Deadline::after(Duration::from_millis(40)).unwrap(),
                &Cancellation::default()
            )
            .err()
            .unwrap()
            .kind(),
            io::ErrorKind::TimedOut
        );
        assert!(fs::remove_file(root.path().join("instance.lock")).is_err());
        drop((startup, instance));
        assert!(reopened.try_instance().unwrap().is_some());
        assert_eq!(fs::read(root.path().join(MARKER)).unwrap(), marker);
        assert!(root.path().join("startup.lock").exists());
        assert!(root.path().join("instance.lock").exists());
    }
    #[test]
    fn foreign_unmarked_and_replaced_markers_are_preserved() {
        let root = tempfile::tempdir().unwrap();
        let marker = root.path().join(MARKER);
        assert!(BrokerRoot::open(root.path()).is_err());
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
        let bytes = b"{\"owner\":\"foreign\",\"account\":\"private-sentinel\"}";
        fs::write(&marker, bytes).unwrap();
        let error = BrokerRoot::open(root.path()).err().unwrap();
        assert!(!error.to_string().contains("private-sentinel"));
        assert_eq!(fs::read(&marker).unwrap(), bytes);
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    }
    #[test]
    fn inherited_private_record_is_read_with_bounds_and_held_root_cannot_be_replaced() {
        let prepared = BrokerRoot::prepare().unwrap();
        let root = prepared.root();
        let path = root.path().join("endpoint.json");
        StagedFile::create(&path, b"owned record")
            .unwrap()
            .commit()
            .unwrap();
        assert_eq!(
            root.read_private("endpoint.json", 64).unwrap(),
            b"owned record"
        );
        assert!(root.read_private("endpoint.json", 4).is_err());
        assert!(root.read_private("../foreign", 64).is_err());
        assert!(fs::remove_file(root.path().join(MARKER)).is_err());
    }

    fn grant_public_access(path: &Path) {
        // This helper is restricted to the fresh owned test directory/record.
        let name: Vec<_> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let mut descriptor = SECURITY_DESCRIPTOR::default();
        let sd = (&mut descriptor as *mut SECURITY_DESCRIPTOR).cast();
        checked(unsafe { InitializeSecurityDescriptor(sd, 1) }).unwrap();
        checked(unsafe { SetSecurityDescriptorDacl(sd, 1, null_mut(), 0) }).unwrap();
        checked(unsafe {
            SetFileSecurityW(
                name.as_ptr(),
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                sd,
            )
        })
        .unwrap();
    }

    #[test]
    fn weakened_acl_is_refused_without_repair_or_lock_creation() {
        let prepared = BrokerRoot::prepare().unwrap();
        let root = prepared.root();
        let marker = std::fs::read(root.path().join(MARKER)).unwrap();
        std::fs::remove_file(root.path().join("startup.lock")).unwrap();
        std::fs::remove_file(root.path().join("instance.lock")).unwrap();
        grant_public_access(root.path());
        assert!(BrokerRoot::open(root.path()).is_err());
        assert!(
            root.startup(
                Deadline::after(Duration::from_secs(1)).unwrap(),
                &Cancellation::default()
            )
            .is_err()
        );
        assert!(root.try_instance().is_err());
        assert_eq!(std::fs::read(root.path().join(MARKER)).unwrap(), marker);
        assert!(!root.path().join("startup.lock").exists());
        assert!(!root.path().join("instance.lock").exists());
        // Opening did not repair the permissive descriptor.
        assert!(verify_directory(root.path(), &root.account).is_err());
    }

    #[test]
    fn private_parent_does_not_hide_a_public_endpoint_acl() {
        let prepared = BrokerRoot::prepare().unwrap();
        let root = prepared.root();
        let endpoint = root.path().join("endpoint.json");
        StagedFile::create(&endpoint, b"owned nonsecret fixture")
            .unwrap()
            .commit()
            .unwrap();
        grant_public_access(&endpoint);
        assert!(root.read_private("endpoint.json", 64).is_err());
        assert_eq!(std::fs::read(endpoint).unwrap(), b"owned nonsecret fixture");
    }
}
