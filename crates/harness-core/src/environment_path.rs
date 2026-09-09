//! Exact-value publication of the current user's registry PATH using TxR.
//! Callers own installation locking and durable intent. This module neither
//! changes the process environment nor broadcasts to other applications.
#![cfg(windows)]

use crate::path_plan;
use serde::{Deserialize, Serialize};
use std::{
    io,
    os::windows::io::{AsHandle, AsRawHandle, BorrowedHandle, FromRawHandle, OwnedHandle},
    path::Path,
    ptr::{null, null_mut},
};
use windows_sys::Win32::{
    Foundation::{
        ERROR_FILE_NOT_FOUND, HANDLE, INVALID_HANDLE_VALUE, RtlNtStatusToDosError, UNICODE_STRING,
    },
    Storage::FileSystem::{CommitTransaction, CreateTransaction, TRANSACTION_DO_NOT_PROMOTE},
    System::Registry::*,
};

const ENVIRONMENT: &str = "Environment";
const MAX_BYTES: usize = 32768 * 2;
const TRANSACTION_TIMEOUT_MS: u32 = 5000;

#[path = "environment_path_receipt.rs"]
pub(crate) mod receipt;

#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtQueryKey(
        key: HANDLE,
        class: i32,
        data: *mut core::ffi::c_void,
        size: u32,
        returned: *mut u32,
    ) -> i32;
    fn NtQueryValueKey(
        key: HANDLE,
        name: *const UNICODE_STRING,
        class: i32,
        data: *mut core::ffi::c_void,
        size: u32,
        returned: *mut u32,
    ) -> i32;
    #[cfg(test)]
    fn NtDeleteKey(key: HANDLE) -> i32;
    #[cfg(test)]
    fn NtSetValueKey(
        key: HANDLE,
        name: *const UNICODE_STRING,
        title: u32,
        kind: u32,
        data: *const core::ffi::c_void,
        size: u32,
    ) -> i32;
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn status(code: u32) -> io::Result<()> {
    if code == 0 {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(code as i32))
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

fn path_value_name(buffer: &mut [u16; 4]) -> UNICODE_STRING {
    UNICODE_STRING {
        Length: 8,
        MaximumLength: 8,
        Buffer: buffer.as_mut_ptr(),
    }
}

// No Debug: registry contents can contain private user paths or variables.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Value {
    kind: u32,
    bytes: Vec<u8>,
}

impl Value {
    fn text(&self) -> io::Result<String> {
        if !matches!(self.kind, REG_SZ | REG_EXPAND_SZ)
            || self.bytes.len() > MAX_BYTES
            || !self.bytes.len().is_multiple_of(2)
        {
            return Err(invalid("registry PATH has an unsupported type or size"));
        }
        let mut units: Vec<_> = self
            .bytes
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        // Fail before publication when Win32 string writing cannot promise an
        // exact round trip. Never normalize malformed foreign registry data.
        if units.pop() != Some(0) {
            return Err(invalid("registry PATH string lacks its terminator"));
        }
        if units.contains(&0) || units.len() > 32767 {
            return Err(invalid(
                "registry PATH has an invalid string representation",
            ));
        }
        String::from_utf16(&units).map_err(|_| invalid("registry PATH is not valid Unicode"))
    }

    fn from_text(kind: u32, text: &str) -> io::Result<Self> {
        let value = Self {
            kind,
            bytes: wide(text).into_iter().flat_map(u16::to_le_bytes).collect(),
        };
        value.text()?;
        Ok(value)
    }
}

/// Snapshot of `HKCU\Environment\Path`; absence and an empty string differ.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UserPathSnapshot {
    value: Option<Value>,
}

impl UserPathSnapshot {
    /// Reads the current user only, without creating a registry key or value.
    pub fn read() -> io::Result<Self> {
        Self::read_at(ENVIRONMENT)
    }

    fn read_at(subkey: &str) -> io::Result<Self> {
        let value = match Key::open(subkey, None, false)? {
            Some(key) => key.read()?,
            None => None,
        };
        let snapshot = Self { value };
        snapshot.text()?;
        Ok(snapshot)
    }

    /// Returns stored text, without expanding REG_EXPAND_SZ variables.
    pub fn text(&self) -> io::Result<Option<String>> {
        self.value.as_ref().map(Value::text).transpose()
    }

    pub fn prepend(&self, bin: &Path) -> io::Result<(UserPathChange, bool)> {
        let text = self.text()?;
        let (next, added) = path_plan::prepend(text.as_deref(), bin)?;
        let after = if added {
            Some(Value::from_text(
                self.value.as_ref().map_or(REG_EXPAND_SZ, |v| v.kind),
                &next,
            )?)
        } else {
            self.value.clone()
        };
        Ok((
            UserPathChange {
                before: self.value.clone(),
                after,
            },
            added,
        ))
    }

    /// The installer must establish ownership of an added entry first.
    pub fn remove(&self, bin: &Path, was_added: bool) -> io::Result<UserPathChange> {
        let text = self.text()?;
        let after = if was_added {
            match (&self.value, text) {
                (Some(value), Some(text)) => {
                    let next = path_plan::remove(&text, bin)?;
                    if next == text {
                        self.value.clone()
                    } else {
                        Some(Value::from_text(value.kind, &next)?)
                    }
                }
                _ => None,
            }
        } else {
            self.value.clone()
        };
        Ok(UserPathChange {
            before: self.value.clone(),
            after,
        })
    }
}

/// Serializable private values for future installer intent, validated on use.
/// Equality proves exact type/bytes, not the identity or history of a writer.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UserPathChange {
    before: Option<Value>,
    after: Option<Value>,
}

impl UserPathChange {
    pub fn is_noop(&self) -> bool {
        self.before == self.after
    }

    pub fn publish(&self) -> io::Result<bool> {
        self.exchange(ENVIRONMENT, false, || Ok(()))
    }

    pub fn rollback(&self) -> io::Result<bool> {
        self.exchange(ENVIRONMENT, true, || Ok(()))
    }

    fn exchange(
        &self,
        subkey: &str,
        undo: bool,
        mut before_commit: impl FnMut() -> io::Result<()>,
    ) -> io::Result<bool> {
        self.exchange_inner(subkey, undo, |phase| {
            if phase == "commit" {
                before_commit()
            } else {
                Ok(())
            }
        })
    }

    fn exchange_inner(
        &self,
        subkey: &str,
        undo: bool,
        checkpoint: impl FnMut(&str) -> io::Result<()>,
    ) -> io::Result<bool> {
        let tx = Transaction::new()?;
        let changed = self.stage_in(subkey, undo, false, tx.0.as_handle(), checkpoint)?;
        if changed {
            tx.commit()?;
        }
        Ok(changed)
    }

    /// Enlist in the receipt file's transaction, without committing it. Strict
    /// mode accepts only the recorded baseline, even if a foreign writer has
    /// already installed identical candidate bytes. Any error requires abort.
    fn stage_in(
        &self,
        subkey: &str,
        undo: bool,
        strict: bool,
        transaction: BorrowedHandle<'_>,
        mut checkpoint: impl FnMut(&str) -> io::Result<()>,
    ) -> io::Result<bool> {
        for value in [&self.before, &self.after].into_iter().flatten() {
            value.text()?;
        }
        let (expected, desired) = if undo {
            (&self.after, &self.before)
        } else {
            (&self.before, &self.after)
        };
        // Only a publication that needs a value may create the missing key.
        let key = Key::open(subkey, Some(transaction), desired.is_some())?;
        checkpoint("opened")?;
        let Some(key) = key else {
            return if desired.is_none() && (!strict || expected.is_none()) {
                Ok(false)
            } else {
                Err(invalid("registry PATH key disappeared"))
            };
        };
        // Reads alone do not enlist a name against RegRenameKey. Stage a PATH
        // value first, then check the actual name under write participation.
        // An absent candidate uses an uncommitted empty placeholder. Every
        // failure aborts it; a successful no-op restores absence before a
        // caller can commit another participant such as the receipt file.
        let empty = Value::from_text(REG_SZ, "")?;
        key.write(Some(desired.as_ref().unwrap_or(&empty)))?;
        checkpoint("staged")?;
        key.verify_name(subkey)?;
        // A separate nontransacted handle observes the committed baseline,
        // not our candidate. This closes the read-before-first-write race too.
        // A later foreign write/rename aborts the enlisted transaction.
        let baseline = Key::open(subkey, None, false)?;
        let current = baseline.as_ref().map(Key::read).transpose()?.flatten();
        drop(baseline);
        if &current != expected && (strict || &current != desired) {
            return Err(invalid(
                "registry PATH changed; preserving the foreign value",
            ));
        }
        key.write(desired.as_ref())?;
        if &key.read()? != desired {
            return Err(invalid("registry PATH candidate verification failed"));
        }
        let changed = &current != desired;
        if changed {
            checkpoint("commit")?;
        }
        // After write enlistment, a foreign rename can move the object but
        // causes commit to fail. There is no nontransactional write fallback.
        drop(key);
        Ok(changed)
    }
}

struct Transaction(OwnedHandle);

impl Transaction {
    fn new() -> io::Result<Self> {
        let handle = unsafe {
            CreateTransaction(
                null_mut(),
                null_mut(),
                TRANSACTION_DO_NOT_PROMOTE,
                0,
                0,
                TRANSACTION_TIMEOUT_MS,
                null(),
            )
        };
        if handle.is_null() || handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        // Unique noninheritable handle. Closing its last reference aborts an
        // uncommitted transaction, including process termination without Drop.
        Ok(Self(unsafe { OwnedHandle::from_raw_handle(handle) }))
    }

    fn handle(&self) -> HANDLE {
        self.0.as_raw_handle()
    }

    fn commit(self) -> io::Result<()> {
        if unsafe { CommitTransaction(self.handle()) } == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
}

struct Key(HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        unsafe {
            RegCloseKey(self.0);
        }
    }
}

impl Key {
    fn verify_name(&self, subkey: &str) -> io::Result<()> {
        let mut root = null_mut();
        status(unsafe { RegOpenCurrentUser(KEY_QUERY_VALUE, &mut root) })?;
        let root = Key(root);
        let expected = format!("{}\\{subkey}", root.name()?);
        if !self.name()?.eq_ignore_ascii_case(&expected) {
            return Err(invalid("registry PATH key is redirected; preserving it"));
        }
        Ok(())
    }
    fn open(
        subkey: &str,
        transaction: Option<BorrowedHandle<'_>>,
        create: bool,
    ) -> io::Result<Option<Self>> {
        let mut root = null_mut();
        status(unsafe { RegOpenCurrentUser(KEY_QUERY_VALUE, &mut root) })?;
        let root = Key(root);
        let expected = format!("{}\\{subkey}", root.name()?);
        let name = wide(subkey);
        let mut handle = null_mut();
        let access = KEY_QUERY_VALUE
            | if transaction.is_some() {
                KEY_SET_VALUE
            } else {
                0
            };
        let code = match transaction {
            Some(tx) if create => unsafe {
                RegCreateKeyTransactedW(
                    root.0,
                    name.as_ptr(),
                    0,
                    null(),
                    REG_OPTION_NON_VOLATILE,
                    access,
                    null(),
                    &mut handle,
                    null_mut(),
                    tx.as_raw_handle(),
                    null(),
                )
            },
            Some(tx) => unsafe {
                RegOpenKeyTransactedW(
                    root.0,
                    name.as_ptr(),
                    0,
                    access,
                    &mut handle,
                    tx.as_raw_handle(),
                    null(),
                )
            },
            None => unsafe { RegOpenKeyExW(root.0, name.as_ptr(), 0, access, &mut handle) },
        };
        if code == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        status(code)?;
        let key = Self(handle);
        // Reject registry symbolic-link redirection using the opened handle's
        // actual native name, before reading or changing a PATH value.
        if !key.name()?.eq_ignore_ascii_case(&expected) {
            return Err(invalid("registry PATH key is redirected; preserving it"));
        }
        Ok(Some(key))
    }

    fn name(&self) -> io::Result<String> {
        // ULONG-aligned KEY_NAME_INFORMATION: four-byte count + WCHAR array.
        let mut data = vec![0u32; 4096];
        let mut returned = 0;
        let code = unsafe {
            NtQueryKey(
                self.0,
                3,
                data.as_mut_ptr().cast(),
                (data.len() * 4) as u32,
                &mut returned,
            )
        };
        if code < 0 {
            return Err(io::Error::from_raw_os_error(
                unsafe { RtlNtStatusToDosError(code) } as i32,
            ));
        }
        let size = data[0] as usize;
        if size > data.len() * 4 - 4
            || !size.is_multiple_of(2)
            || returned < 4
            || size > returned as usize - 4
        {
            return Err(invalid("registry key name exceeds its bound"));
        }
        let units =
            unsafe { std::slice::from_raw_parts(data.as_ptr().add(1).cast::<u16>(), size / 2) };
        String::from_utf16(units).map_err(|_| invalid("registry key name is not valid Unicode"))
    }

    fn read(&self) -> io::Result<Option<Value>> {
        let value = self.read_raw(MAX_BYTES)?;
        if let Some(value) = &value {
            value.text()?;
        }
        Ok(value)
    }

    fn read_raw(&self, maximum: usize) -> io::Result<Option<Value>> {
        // One bounded API read returns a coherent type/data pair; no size-query
        // allocation race or Win32 string-termination normalization. The native
        // KEY_VALUE_PARTIAL_INFORMATION is ULONG-aligned, with a 12-byte header.
        let mut data = vec![0u32; (maximum + 12).div_ceil(4)];
        let mut returned = 0;
        let mut name = [80u16, 97, 116, 104];
        let code = unsafe {
            NtQueryValueKey(
                self.0,
                &path_value_name(&mut name),
                2,
                data.as_mut_ptr().cast(),
                (data.len() * 4) as u32,
                &mut returned,
            )
        };
        let error = if code < 0 {
            unsafe { RtlNtStatusToDosError(code) }
        } else {
            0
        };
        if error == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        if returned as usize > maximum + 12 {
            return Err(invalid("registry PATH exceeds its bound"));
        }
        status(error)?;
        let size = data[2] as usize;
        if returned < 12 || size > maximum || size > returned as usize - 12 {
            return Err(invalid("registry PATH has an invalid data extent"));
        }
        let bytes =
            unsafe { std::slice::from_raw_parts(data.as_ptr().add(3).cast::<u8>(), size) }.to_vec();
        Ok(Some(Value {
            kind: data[1],
            bytes,
        }))
    }

    fn write(&self, value: Option<&Value>) -> io::Result<()> {
        let name = wide("Path");
        let code = match value {
            Some(value) => unsafe {
                RegSetValueExW(
                    self.0,
                    name.as_ptr(),
                    0,
                    value.kind,
                    value.bytes.as_ptr(),
                    value.bytes.len() as u32,
                )
            },
            None => unsafe { RegDeleteValueW(self.0, name.as_ptr()) },
        };
        if value.is_none() && code == ERROR_FILE_NOT_FOUND {
            return Ok(());
        }
        status(code)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        path::PathBuf,
        process::{Command, Stdio},
        time::{Duration, Instant},
    };

    const TEST_ROOT: &str = "Software\\CodexHarnessAcceptance\\";

    struct Fixture {
        subkey: String,
        logs: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let logs = tempfile::Builder::new()
                .prefix("harness-environment-path-")
                .tempdir()
                .unwrap()
                .keep();
            let subkey = format!("{TEST_ROOT}{}", logs.file_name().unwrap().to_str().unwrap());
            assert!(Key::open(&subkey, None, false).unwrap().is_none());
            Self { subkey, logs }
        }

        fn raw(&self, value: Option<&Value>) -> io::Result<()> {
            let mut handle = null_mut();
            status(unsafe {
                RegCreateKeyExW(
                    HKEY_CURRENT_USER,
                    wide(&self.subkey).as_ptr(),
                    0,
                    null(),
                    REG_OPTION_NON_VOLATILE,
                    KEY_QUERY_VALUE | KEY_SET_VALUE,
                    null(),
                    &mut handle,
                    null_mut(),
                )
            })?;
            let key = Key(handle);
            if let Some(value) = value {
                // Native fixture can store deliberately malformed byte arrays;
                // it does not depend on RegSetValueEx string normalization.
                let mut name = [80u16, 97, 116, 104];
                let code = unsafe {
                    NtSetValueKey(
                        key.0,
                        &path_value_name(&mut name),
                        0,
                        value.kind,
                        value.bytes.as_ptr().cast(),
                        value.bytes.len() as u32,
                    )
                };
                status(if code < 0 {
                    unsafe { RtlNtStatusToDosError(code) }
                } else {
                    0
                })
            } else {
                key.write(None)
            }
        }

        fn snapshot(&self) -> UserPathSnapshot {
            UserPathSnapshot::read_at(&self.subkey).unwrap()
        }

        fn change(&self) -> UserPathChange {
            self.snapshot()
                .prepend(Path::new("C:\\harness-owned-test\\bin"))
                .unwrap()
                .0
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let leaf = self
                .subkey
                .strip_prefix(TEST_ROOT)
                .expect("owned acceptance key");
            assert!(!leaf.is_empty() && !leaf.contains(['\\', '/']));
            let mut handle = null_mut();
            let code = unsafe {
                RegOpenKeyExW(
                    HKEY_CURRENT_USER,
                    wide(&self.subkey).as_ptr(),
                    REG_OPTION_OPEN_LINK,
                    windows_sys::Win32::Storage::FileSystem::DELETE,
                    &mut handle,
                )
            };
            if code == ERROR_FILE_NOT_FOUND {
                return;
            }
            status(code).expect("open exact owned acceptance key for cleanup");
            let key = Key(handle);
            // Single captured leaf, including an unexpected symbolic link;
            // never recursively enumerate/delete its target or the shared root.
            assert!(
                unsafe { NtDeleteKey(key.0) } >= 0,
                "owned key cleanup failed"
            );
        }
    }

    fn value(kind: u32, text: &str) -> Value {
        Value::from_text(kind, text).unwrap()
    }

    #[test]
    fn exact_raw_types_absence_and_empty_survive_publish_and_rollback() {
        for baseline in [
            None,
            Some(value(REG_SZ, "")),
            Some(value(REG_EXPAND_SZ, "%USERPROFILE%\\私的;;")),
            Some(value(REG_SZ, "C:\\旧路径;C:\\👋")),
        ] {
            let fixture = Fixture::new();
            if baseline.is_some() {
                fixture.raw(baseline.as_ref()).unwrap();
            }
            assert!(fixture.snapshot().value == baseline);
            if baseline.is_none() {
                assert!(Key::open(&fixture.subkey, None, false).unwrap().is_none());
            }
            let change = fixture.change();
            assert!(change.exchange(&fixture.subkey, false, || Ok(())).unwrap());
            assert!(fixture.snapshot().value == change.after);
            assert!(!change.exchange(&fixture.subkey, false, || Ok(())).unwrap());
            assert!(change.exchange(&fixture.subkey, true, || Ok(())).unwrap());
            assert!(fixture.snapshot().value == baseline);
            assert!(!change.exchange(&fixture.subkey, true, || Ok(())).unwrap());
            println!("exact-value evidence: {}", fixture.logs.display());
        }
    }

    #[test]
    fn planner_preserves_noop_representation_and_only_removes_owned_entries() {
        let fixture = Fixture::new();
        let bin = Path::new("C:\\harness-owned-test\\bin");
        let original = value(REG_SZ, "C:\\unrelated;;c:\\HARNESS-owned-test\\bin;C:\\尾;");
        fixture.raw(Some(&original)).unwrap();
        let snapshot = fixture.snapshot();
        let (noop, added) = snapshot.prepend(bin).unwrap();
        assert!(!added && noop.is_noop());
        assert!(
            !noop
                .exchange(&fixture.subkey, false, || panic!("no write expected"))
                .unwrap()
        );
        assert!(snapshot.remove(bin, false).unwrap().is_noop());
        let remove = snapshot.remove(bin, true).unwrap();
        assert!(remove.exchange(&fixture.subkey, false, || Ok(())).unwrap());
        assert_eq!(
            fixture.snapshot().text().unwrap().as_deref(),
            Some("C:\\unrelated;;C:\\尾;")
        );
        remove.exchange(&fixture.subkey, true, || Ok(())).unwrap();
        assert!(fixture.snapshot().value == Some(original));
    }

    #[test]
    fn foreign_type_and_bytes_before_publish_or_rollback_are_preserved() {
        for during_undo in [false, true] {
            for type_only in [false, true] {
                let fixture = Fixture::new();
                fixture.raw(Some(&value(REG_SZ, "C:\\original"))).unwrap();
                let change = fixture.change();
                if during_undo {
                    change.exchange(&fixture.subkey, false, || Ok(())).unwrap();
                }
                let mut foreign = fixture.snapshot().value.unwrap();
                if type_only {
                    foreign.kind = REG_EXPAND_SZ;
                } else {
                    foreign = value(REG_SZ, "C:\\private-foreign-canary");
                }
                fixture.raw(Some(&foreign)).unwrap();
                let error = change
                    .exchange(&fixture.subkey, during_undo, || {
                        panic!("foreign input must stop before write")
                    })
                    .unwrap_err();
                assert!(!error.to_string().contains("private-foreign-canary"));
                assert!(fixture.snapshot().value == Some(foreign));
            }
        }
    }

    #[test]
    fn malformed_values_refuse_privately_without_any_write() {
        let malformed = [
            Value {
                kind: REG_SZ,
                bytes: Vec::new(),
            },
            Value {
                kind: REG_SZ,
                bytes: vec![65, 0],
            },
            Value {
                kind: REG_BINARY,
                bytes: b"private-path-canary".to_vec(),
            },
            Value {
                kind: REG_SZ,
                bytes: vec![7],
            },
            Value {
                kind: REG_SZ,
                bytes: vec![0, 0, 65, 0, 0, 0],
            },
            Value {
                kind: REG_SZ,
                bytes: vec![0, 0xd8, 0, 0],
            },
            Value {
                kind: REG_SZ,
                bytes: vec![65; MAX_BYTES + 2],
            },
        ];
        for bad in malformed {
            let fixture = Fixture::new();
            fixture.raw(Some(&bad)).unwrap();
            let error = match UserPathSnapshot::read_at(&fixture.subkey) {
                Ok(_) => panic!("malformed value accepted"),
                Err(error) => error,
            };
            assert!(!error.to_string().contains("private-path-canary"));
            let key = Key::open(&fixture.subkey, None, false).unwrap().unwrap();
            assert!(key.read_raw(MAX_BYTES + 2).unwrap() == Some(bad.clone()));
            // Deserialization is not permission to publish unchecked bytes.
            let invalid_change = UserPathChange {
                before: None,
                after: Some(bad),
            };
            assert!(
                invalid_change
                    .exchange(&fixture.subkey, false, || panic!("invalid input written"))
                    .is_err()
            );
        }
    }

    #[test]
    fn failure_before_commit_aborts_candidate_and_preserves_other_values() {
        let fixture = Fixture::new();
        let before = value(REG_EXPAND_SZ, "%USERPROFILE%\\original");
        fixture.raw(Some(&before)).unwrap();
        let tx = Transaction::new().unwrap();
        let key = Key::open(&fixture.subkey, Some(tx.0.as_handle()), false)
            .unwrap()
            .unwrap();
        let unrelated = wide("private-unrelated-value");
        status(unsafe {
            RegSetValueExW(
                key.0,
                wide("Unrelated").as_ptr(),
                0,
                REG_SZ,
                unrelated.as_ptr().cast(),
                (unrelated.len() * 2) as u32,
            )
        })
        .unwrap();
        drop(key);
        tx.commit().unwrap();
        let change = fixture.change();
        assert!(
            change
                .exchange(&fixture.subkey, false, || Err(invalid(
                    "injected boundary failure"
                )))
                .is_err()
        );
        assert!(fixture.snapshot().value == Some(before.clone()));
        change.exchange(&fixture.subkey, false, || Ok(())).unwrap();
        change.exchange(&fixture.subkey, true, || Ok(())).unwrap();
        assert!(fixture.snapshot().value == Some(before));
        let key = Key::open(&fixture.subkey, None, false).unwrap().unwrap();
        let mut bytes = vec![0u8; unrelated.len() * 2];
        let mut size = bytes.len() as u32;
        status(unsafe {
            RegQueryValueExW(
                key.0,
                wide("Unrelated").as_ptr(),
                null(),
                null_mut(),
                bytes.as_mut_ptr(),
                &mut size,
            )
        })
        .unwrap();
        assert!(
            bytes
                == unrelated
                    .into_iter()
                    .flat_map(u16::to_le_bytes)
                    .collect::<Vec<_>>()
        );
    }

    #[test]
    fn competing_nontransactional_write_cannot_be_silently_overwritten() {
        let fixture = Fixture::new();
        fixture.raw(Some(&value(REG_SZ, "C:\\original"))).unwrap();
        let change = fixture.change();
        let foreign = value(REG_SZ, "C:\\foreign-after-staging");
        let mut foreign_won = false;
        let result = change.exchange(&fixture.subkey, false, || {
            fixture.raw(Some(&foreign))?;
            foreign_won = true;
            Ok(())
        });
        assert!(
            result.is_err(),
            "both competing writes unexpectedly committed"
        );
        if foreign_won {
            assert!(fixture.snapshot().value == Some(foreign));
        } else {
            assert!(fixture.snapshot().value == change.before);
        }
        println!(
            "competing write succeeded: {foreign_won}; evidence: {}",
            fixture.logs.display()
        );
    }

    #[test]
    fn redirected_registry_key_is_refused_before_read_or_write() {
        let fixture = Fixture::new();
        fixture
            .raw(Some(&value(REG_SZ, "C:\\foreign-target")))
            .unwrap();
        let alias = Fixture::new();
        let mut handle = null_mut();
        status(unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                wide(&alias.subkey).as_ptr(),
                0,
                null(),
                REG_OPTION_CREATE_LINK,
                KEY_ALL_ACCESS,
                null(),
                &mut handle,
                null_mut(),
            )
        })
        .unwrap();
        struct OwnedLink(Key);
        impl Drop for OwnedLink {
            fn drop(&mut self) {
                // The retained creating handle addresses the link itself,
                // never its target. Do not recursively delete through aliases.
                let code = unsafe { NtDeleteKey(self.0.0) };
                assert!(code >= 0, "owned registry link deletion failed");
            }
        }
        let link = OwnedLink(Key(handle));
        let target = Key::open(&fixture.subkey, None, false)
            .unwrap()
            .unwrap()
            .name()
            .unwrap();
        let bytes: Vec<_> = target.encode_utf16().flat_map(u16::to_le_bytes).collect();
        status(unsafe {
            RegSetValueExW(
                link.0.0,
                wide("SymbolicLinkValue").as_ptr(),
                0,
                REG_LINK,
                bytes.as_ptr(),
                bytes.len() as u32,
            )
        })
        .unwrap();
        let before = fixture.snapshot().value;
        assert!(UserPathSnapshot::read_at(&alias.subkey).is_err());
        let change = fixture.change();
        assert!(
            change
                .exchange(&alias.subkey, false, || panic!("redirected key written"))
                .is_err()
        );
        assert!(fixture.snapshot().value == before);
        drop(link);
        assert!(Key::open(&alias.subkey, None, false).unwrap().is_none());
        println!("registry alias evidence: {}", fixture.logs.display());
    }

    #[test]
    fn renamed_key_cannot_move_a_transactional_write_outside_its_verified_name() {
        for (mode, undo, noop) in [
            ("publish", false, false),
            ("rollback", true, false),
            ("noop", false, true),
        ] {
            for requested in ["opened", "staged", "commit"] {
                if noop && requested == "commit" {
                    continue;
                }
                let fixture = Fixture::new();
                let moved = Fixture::new();
                fixture.raw(Some(&value(REG_SZ, "C:\\original"))).unwrap();
                let mut change = fixture.change();
                if undo || noop {
                    change.exchange(&fixture.subkey, false, || Ok(())).unwrap();
                }
                if noop {
                    change = fixture.change();
                    assert!(change.is_noop());
                }
                let original = fixture.snapshot().value;
                let mut renamed = None;
                let result = change.exchange_inner(&fixture.subkey, undo, |phase| {
                    if phase == requested {
                        let code = unsafe {
                            RegRenameKey(
                                HKEY_CURRENT_USER,
                                wide(&fixture.subkey).as_ptr(),
                                wide(moved.subkey.strip_prefix(TEST_ROOT).unwrap()).as_ptr(),
                            )
                        };
                        renamed = Some(code);
                        status(code)?;
                    }
                    Ok(())
                });
                assert!(renamed.is_some(), "rename checkpoint not reached");
                assert!(result.is_err(), "{mode}/{requested} escaped its name");
                if renamed == Some(0) {
                    assert!(moved.snapshot().value == original);
                    assert!(Key::open(&fixture.subkey, None, false).unwrap().is_none());
                } else {
                    assert!(fixture.snapshot().value == original);
                    assert!(Key::open(&moved.subkey, None, false).unwrap().is_none());
                }
                println!(
                    "{mode}/{requested}: rename={renamed:?}, error={:?}; {}",
                    result.err().and_then(|e| e.raw_os_error()),
                    fixture.logs.display()
                );
            }
        }
    }

    #[test]
    fn foreign_value_before_or_after_first_stage_is_preserved() {
        for requested in ["opened", "staged"] {
            let fixture = Fixture::new();
            fixture.raw(Some(&value(REG_SZ, "C:\\original"))).unwrap();
            let change = fixture.change();
            let foreign = value(REG_EXPAND_SZ, "C:\\foreign-at-first-stage");
            let mut reached = false;
            let result = change.exchange_inner(&fixture.subkey, false, |phase| {
                if phase == requested {
                    fixture.raw(Some(&foreign))?;
                    reached = true;
                }
                Ok(())
            });
            assert!(reached && result.is_err());
            assert!(fixture.snapshot().value == Some(foreign));
        }
    }

    #[test]
    fn transacted_stage_can_read_committed_baseline_without_losing_ownership() {
        for same in [false, true] {
            let fixture = Fixture::new();
            let original = value(REG_SZ, "C:\\original");
            let candidate = if same {
                original.clone()
            } else {
                value(REG_SZ, "C:\\candidate")
            };
            fixture.raw(Some(&original)).unwrap();
            let tx = Transaction::new().unwrap();
            let key = Key::open(&fixture.subkey, Some(tx.0.as_handle()), false)
                .unwrap()
                .unwrap();
            key.write(Some(&candidate)).unwrap();
            assert!(fixture.snapshot().value == Some(original));
            drop(key);
            tx.commit().unwrap();
            assert!(fixture.snapshot().value == Some(candidate));
            println!("committed baseline same={same}: {}", fixture.logs.display());
        }
    }

    #[test]
    fn same_value_stage_still_detects_a_later_key_rename() {
        let fixture = Fixture::new();
        let moved = Fixture::new();
        let original = value(REG_SZ, "C:\\original");
        fixture.raw(Some(&original)).unwrap();
        let tx = Transaction::new().unwrap();
        let key = Key::open(&fixture.subkey, Some(tx.0.as_handle()), false)
            .unwrap()
            .unwrap();
        key.write(Some(&original)).unwrap();
        let renamed = unsafe {
            RegRenameKey(
                HKEY_CURRENT_USER,
                wide(&fixture.subkey).as_ptr(),
                wide(moved.subkey.strip_prefix(TEST_ROOT).unwrap()).as_ptr(),
            )
        };
        drop(key);
        let committed = tx.commit();
        println!(
            "identical stage rename={renamed}, commit={:?}",
            committed.as_ref().err().and_then(io::Error::raw_os_error)
        );
        if renamed == 0 {
            assert!(committed.is_err());
            assert!(moved.snapshot().value == Some(original));
        } else {
            assert!(fixture.snapshot().value == Some(original));
        }
    }

    #[test]
    #[ignore = "explicit read-only observation of the current user's PATH"]
    fn actual_user_path_snapshot_is_readonly() {
        let key = Key::open(ENVIRONMENT, None, false).unwrap();
        let before = key
            .as_ref()
            .map(|k| k.read_raw(MAX_BYTES))
            .transpose()
            .unwrap()
            .flatten();
        let snapshot = UserPathSnapshot::read().unwrap();
        assert!(snapshot.value == before);
        let after = UserPathSnapshot::read().unwrap();
        assert!(after.value == before);
        let bytes = serde_json::to_vec(&snapshot).unwrap();
        println!(
            "public snapshot: present={}, representation_sha256={}",
            before.is_some(),
            crate::build_identity::hash_bytes(&bytes)
        );
    }

    #[test]
    #[ignore = "owned child of killed_transaction_restores_without_rust_destructors"]
    fn path_process_fixture() {
        let subkey = std::env::var("HARNESS_PATH_TEST_KEY").unwrap();
        let leaf = subkey.strip_prefix(TEST_ROOT).unwrap();
        assert!(leaf.starts_with("harness-environment-path-") && !leaf.contains(['\\', '/']));
        let logs = PathBuf::from(std::env::var_os("HARNESS_PATH_TEST_LOGS").unwrap());
        let snapshot = UserPathSnapshot::read_at(&subkey).unwrap();
        let (change, _) = snapshot
            .prepend(Path::new("C:\\harness-owned-test\\bin"))
            .unwrap();
        change
            .exchange(&subkey, false, || {
                fs::write(logs.join("ready"), b"transacted-value-verified")?;
                std::thread::sleep(Duration::from_secs(20));
                std::process::exit(79);
            })
            .unwrap();
        panic!("kill boundary not reached");
    }

    #[test]
    fn file_and_registry_publish_or_abort_in_one_transaction() {
        use crate::registration_native::StagedFile;
        for commit in [false, true] {
            for initial in [None, Some(value(REG_SZ, "C:\\original"))] {
                let fixture = Fixture::new();
                fixture.raw(initial.as_ref()).unwrap();
                let change = fixture.change();
                let receipt = fixture.logs.join("atomic-receipt.json");
                let staged = StagedFile::create(&receipt, b"owned-receipt").unwrap();
                assert!(
                    change
                        .stage_in(&fixture.subkey, false, true, staged.transaction(), |_| Ok(
                            ()
                        ))
                        .unwrap()
                );
                assert!(fixture.snapshot().value == initial);
                assert!(fs::read(&receipt).is_err());
                if commit {
                    staged.commit().unwrap();
                    assert!(fixture.snapshot().value == change.after);
                    assert_eq!(fs::read(&receipt).unwrap(), b"owned-receipt");
                } else {
                    drop(staged);
                    assert!(fixture.snapshot().value == initial);
                    assert!(!receipt.exists());
                }
            }
        }
    }

    fn receipt_intent(
        fixture: &Fixture,
        change: &UserPathChange,
    ) -> crate::config_file::ConfigSnapshot {
        let path = fixture.logs.join("intent.json");
        fs::write(&path, serde_json::to_vec(change).unwrap()).unwrap();
        crate::config_file::ConfigSnapshot::read(&path).unwrap()
    }

    #[test]
    fn receipts_bind_repeat_and_undo_to_the_original_intent() {
        for initial in [
            None,
            Some(value(REG_SZ, "")),
            Some(value(REG_EXPAND_SZ, "%TEMP%;C:\\Unicode-λ")),
        ] {
            let fixture = Fixture::new();
            fixture.raw(initial.as_ref()).unwrap();
            let change = fixture.change();
            let intent = receipt_intent(&fixture, &change);
            let receipts = receipt::PathReceipts::new(&intent, &change);
            assert!(
                receipts
                    .exchange(&fixture.subkey, false, |_| Ok(()))
                    .unwrap()
            );
            assert!(fixture.snapshot().value == change.after);
            assert!(
                !receipts
                    .exchange(&fixture.subkey, false, |_| Ok(()))
                    .unwrap()
            );
            // Reload both persisted intent and change, as a fresh process would.
            let reloaded =
                crate::config_file::ConfigSnapshot::read(&fixture.logs.join("intent.json"))
                    .unwrap();
            let resumed: UserPathChange = serde_json::from_slice(reloaded.contents()).unwrap();
            let receipts = receipt::PathReceipts::new(&reloaded, &resumed);
            assert!(
                receipts
                    .exchange(&fixture.subkey, true, |_| Ok(()))
                    .unwrap()
            );
            assert!(fixture.snapshot().value == initial);
            assert!(
                !receipts
                    .exchange(&fixture.subkey, true, |_| Ok(()))
                    .unwrap()
            );
            assert!(
                receipts
                    .exchange(&fixture.subkey, false, |_| Ok(()))
                    .is_err()
            );
            assert!(fixture.logs.join("path-applied.json").is_file());
            assert!(fixture.logs.join("path-undone.json").is_file());
        }
    }

    #[test]
    fn receipted_noop_does_not_commit_a_temporary_value() {
        for existing_key in [false, true] {
            let fixture = Fixture::new();
            if existing_key {
                fixture.raw(None).unwrap();
            }
            let change = UserPathChange {
                before: None,
                after: None,
            };
            let intent = receipt_intent(&fixture, &change);
            let receipts = receipt::PathReceipts::new(&intent, &change);
            assert!(
                !receipts
                    .exchange(&fixture.subkey, false, |_| Ok(()))
                    .unwrap()
            );
            assert!(fixture.snapshot().value.is_none());
            assert!(
                !receipts
                    .exchange(&fixture.subkey, true, |_| Ok(()))
                    .unwrap()
            );
            assert!(fixture.snapshot().value.is_none());
            assert_eq!(
                Key::open(&fixture.subkey, None, false).unwrap().is_some(),
                existing_key
            );
        }
    }

    #[test]
    fn absent_receipts_never_adopt_identical_foreign_changes() {
        for undo in [false, true] {
            let fixture = Fixture::new();
            fixture.raw(Some(&value(REG_SZ, "C:\\original"))).unwrap();
            let change = fixture.change();
            let intent = receipt_intent(&fixture, &change);
            let receipts = receipt::PathReceipts::new(&intent, &change);
            if undo {
                receipts
                    .exchange(&fixture.subkey, false, |_| Ok(()))
                    .unwrap();
            }
            let foreign = if undo { &change.before } else { &change.after };
            fixture.raw(foreign.as_ref()).unwrap();
            assert!(
                receipts
                    .exchange(&fixture.subkey, undo, |_| Ok(()))
                    .is_err()
            );
            assert!(fixture.snapshot().value == *foreign);
            assert!(
                !fixture
                    .logs
                    .join(if undo {
                        "path-undone.json"
                    } else {
                        "path-applied.json"
                    })
                    .exists()
            );
        }
    }

    #[test]
    fn foreign_receipt_and_intent_objects_or_bytes_are_preserved() {
        for case in [
            "intent-bytes",
            "intent-copy",
            "receipt-bytes",
            "receipt-copy",
            "receipt-schema",
            "receipt-missing",
            "receipt-other-change",
            "receipt-other-user",
            "receipt-oversize",
        ] {
            let fixture = Fixture::new();
            fixture.raw(Some(&value(REG_SZ, "C:\\original"))).unwrap();
            let change = fixture.change();
            let intent = receipt_intent(&fixture, &change);
            let receipts = receipt::PathReceipts::new(&intent, &change);
            receipts
                .exchange(&fixture.subkey, false, |_| Ok(()))
                .unwrap();
            let path = fixture.logs.join(if case.starts_with("intent-") {
                "intent.json"
            } else {
                "path-applied.json"
            });
            let original = fs::read(&path).unwrap();
            match case {
                "intent-bytes" | "receipt-bytes" => {
                    fs::write(&path, b"private-invalid-body").unwrap()
                }
                "intent-copy" | "receipt-copy" => {
                    fs::rename(&path, path.with_extension("original")).unwrap();
                    fs::write(&path, &original).unwrap();
                }
                "receipt-missing" => fs::rename(&path, path.with_extension("moved")).unwrap(),
                "receipt-oversize" => fs::write(&path, vec![b'x'; 65537]).unwrap(),
                _ => {
                    let mut doc: serde_json::Value = serde_json::from_slice(&original).unwrap();
                    match case {
                        "receipt-schema" => doc["schema"] = 999.into(),
                        "receipt-other-change" => doc["scope"]["change_sha256"] = "foreign".into(),
                        "receipt-other-user" => doc["scope"]["registry"] = "foreign-user".into(),
                        _ => unreachable!(),
                    }
                    fs::write(&path, serde_json::to_vec(&doc).unwrap()).unwrap();
                }
            }
            let foreign = fs::read(&path).ok();
            let err = receipts
                .exchange(&fixture.subkey, true, |_| Ok(()))
                .expect_err(case);
            assert!(!err.to_string().contains("private-invalid-body"));
            assert!(fixture.snapshot().value == change.after, "{case}");
            assert_eq!(fs::read(&path).ok(), foreign, "{case}");
            assert!(!fixture.logs.join("path-undone.json").exists(), "{case}");
        }
    }

    #[test]
    fn receipt_failure_or_foreign_registry_write_aborts_both_participants() {
        for failure in ["injected", "foreign-value", "foreign-file"] {
            let fixture = Fixture::new();
            fixture.raw(Some(&value(REG_SZ, "C:\\original"))).unwrap();
            let change = fixture.change();
            let intent = receipt_intent(&fixture, &change);
            let receipts = receipt::PathReceipts::new(&intent, &change);
            let foreign = value(REG_EXPAND_SZ, "C:\\foreign");
            if failure == "foreign-file" {
                fs::write(fixture.logs.join("path-applied.json"), b"foreign receipt").unwrap();
            }
            let result = receipts.exchange(&fixture.subkey, false, |phase| {
                if phase == "receipt-staged" {
                    if failure == "injected" {
                        return Err(io::Error::other("injected before commit"));
                    }
                    fixture.raw(Some(&foreign))?;
                }
                Ok(())
            });
            assert!(result.is_err(), "{failure}");
            assert!(
                fixture.snapshot().value
                    == if failure == "foreign-value" {
                        Some(foreign)
                    } else {
                        change.before
                    }
            );
            if failure == "foreign-file" {
                assert_eq!(
                    fs::read(fixture.logs.join("path-applied.json")).unwrap(),
                    b"foreign receipt"
                );
            } else {
                assert!(!fixture.logs.join("path-applied.json").exists());
            }
        }
    }

    #[test]
    fn valid_receipt_cannot_be_reused_for_another_change_or_registry_scope() {
        let first = Fixture::new();
        let second = Fixture::new();
        first.raw(Some(&value(REG_SZ, "C:\\original"))).unwrap();
        let change = first.change();
        let intent = receipt_intent(&first, &change);
        let receipts = receipt::PathReceipts::new(&intent, &change);
        receipts.exchange(&first.subkey, false, |_| Ok(())).unwrap();
        second.raw(change.after.as_ref()).unwrap();
        assert!(receipts.exchange(&second.subkey, true, |_| Ok(())).is_err());
        let different = UserPathChange {
            before: Some(value(REG_SZ, "C:\\different")),
            after: change.after.clone(),
        };
        assert!(
            receipt::PathReceipts::new(&intent, &different)
                .exchange(&first.subkey, true, |_| Ok(()))
                .is_err()
        );
        assert!(first.snapshot().value == change.after);
        assert!(second.snapshot().value == change.after);
        assert!(!first.logs.join("path-undone.json").exists());
    }

    #[test]
    #[ignore = "owned child of killed_receipt_transaction_has_a_durable_atomic_outcome"]
    fn receipt_process_fixture() {
        let subkey = std::env::var("HARNESS_PATH_TEST_KEY").unwrap();
        let leaf = subkey.strip_prefix(TEST_ROOT).unwrap();
        assert!(leaf.starts_with("harness-environment-path-") && !leaf.contains(['\\', '/']));
        let logs = PathBuf::from(std::env::var_os("HARNESS_PATH_TEST_LOGS").unwrap());
        let phase = std::env::var("HARNESS_PATH_RECEIPT_PHASE").unwrap();
        assert!(matches!(
            phase.as_str(),
            "receipt-staged" | "receipt-committed"
        ));
        let undo = std::env::var("HARNESS_PATH_RECEIPT_UNDO").unwrap() == "true";
        let intent = crate::config_file::ConfigSnapshot::read(&logs.join("intent.json")).unwrap();
        let change: UserPathChange = serde_json::from_slice(intent.contents()).unwrap();
        receipt::PathReceipts::new(&intent, &change)
            .exchange(&subkey, undo, |at| {
                if at == phase {
                    fs::write(logs.join("ready"), b"receipt-boundary")?;
                    std::thread::sleep(Duration::from_secs(20));
                    std::process::exit(79);
                }
                Ok(())
            })
            .unwrap();
        panic!("kill boundary not reached");
    }

    #[test]
    fn killed_receipt_transaction_has_a_durable_atomic_outcome() {
        for undo in [false, true] {
            for committed in [false, true] {
                for absent in [false, true] {
                    let fixture = Fixture::new();
                    if !absent {
                        fixture.raw(Some(&value(REG_SZ, "C:\\original"))).unwrap();
                    }
                    let change = fixture.change();
                    let intent = receipt_intent(&fixture, &change);
                    let receipts = receipt::PathReceipts::new(&intent, &change);
                    if undo {
                        receipts
                            .exchange(&fixture.subkey, false, |_| Ok(()))
                            .unwrap();
                    }
                    let mut child = Command::new(std::env::current_exe().unwrap())
                        .args([
                            "--exact",
                            "environment_path::tests::receipt_process_fixture",
                            "--ignored",
                            "--nocapture",
                        ])
                        .env("HARNESS_PATH_TEST_KEY", &fixture.subkey)
                        .env("HARNESS_PATH_TEST_LOGS", &fixture.logs)
                        .env("HARNESS_PATH_RECEIPT_UNDO", undo.to_string())
                        .env(
                            "HARNESS_PATH_RECEIPT_PHASE",
                            if committed {
                                "receipt-committed"
                            } else {
                                "receipt-staged"
                            },
                        )
                        .stdin(Stdio::null())
                        .stdout(fs::File::create(fixture.logs.join("child.stdout")).unwrap())
                        .stderr(fs::File::create(fixture.logs.join("child.stderr")).unwrap())
                        .spawn()
                        .unwrap();
                    let until = Instant::now() + Duration::from_secs(10);
                    while !fixture.logs.join("ready").exists() && Instant::now() < until {
                        if child.try_wait().unwrap().is_some() {
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    let ready = fs::read(fixture.logs.join("ready")).ok().as_deref()
                        == Some(b"receipt-boundary");
                    let _ = child.kill();
                    let exit = child.wait().unwrap();
                    assert!(ready, "child missed boundary: {}", fixture.logs.display());
                    assert!(!exit.success() && exit.code() != Some(79));
                    let expected = if committed == undo {
                        &change.before
                    } else {
                        &change.after
                    };
                    assert!(&fixture.snapshot().value == expected);
                    let receipt_path = fixture.logs.join(if undo {
                        "path-undone.json"
                    } else {
                        "path-applied.json"
                    });
                    assert_eq!(receipt_path.is_file(), committed);
                    let repeated = receipts
                        .exchange(&fixture.subkey, undo, |_| Ok(()))
                        .unwrap();
                    assert_eq!(repeated, !committed);
                    assert!(
                        fixture.snapshot().value == if undo { change.before } else { change.after }
                    );
                    println!(
                        "receipt kill undo={undo} committed={committed} absent={absent}: {}",
                        fixture.logs.display()
                    );
                }
            }
        }
    }

    #[test]
    fn killed_transaction_restores_without_rust_destructors() {
        for absent in [false, true] {
            let fixture = Fixture::new();
            if !absent {
                fixture.raw(Some(&value(REG_SZ, "C:\\original"))).unwrap();
            }
            let before = fixture.snapshot().value;
            let mut child = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--ignored",
                    "--exact",
                    "environment_path::tests::path_process_fixture",
                    "--nocapture",
                ])
                .env("HARNESS_PATH_TEST_KEY", &fixture.subkey)
                .env("HARNESS_PATH_TEST_LOGS", &fixture.logs)
                .stdin(Stdio::null())
                .stdout(fs::File::create(fixture.logs.join("child.stdout")).unwrap())
                .stderr(fs::File::create(fixture.logs.join("child.stderr")).unwrap())
                .spawn()
                .unwrap();
            let until = Instant::now() + Duration::from_secs(10);
            while !fixture.logs.join("ready").exists() && Instant::now() < until {
                if child.try_wait().unwrap().is_some() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            let ready = fs::read(fixture.logs.join("ready")).ok().as_deref()
                == Some(b"transacted-value-verified");
            let _ = child.kill();
            let exit = child.wait().unwrap();
            assert!(
                ready,
                "child did not reach boundary: {}",
                fixture.logs.display()
            );
            assert!(!exit.success() && exit.code() != Some(79));
            assert!(fixture.snapshot().value == before);
            if absent {
                assert!(Key::open(&fixture.subkey, None, false).unwrap().is_none());
            }
            println!("killed transaction evidence: {}", fixture.logs.display());
        }
    }
}
