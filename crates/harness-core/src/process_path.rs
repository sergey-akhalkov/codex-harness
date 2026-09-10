//! Windows process-local PATH intent. Unlike registry PATH, this environment
//! belongs only to one process and its descendants; it cannot alter a caller's
//! PowerShell environment. Recovery never writes another process's environment.
#![cfg(windows)]
use crate::path_plan;
use serde::{Deserialize, Serialize};
use std::{
    env, fmt, io,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    path::Path,
    sync::Mutex,
};
use windows_sys::Win32::{
    Foundation::{ERROR_INVALID_PARAMETER, FILETIME, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT},
    System::Threading::{
        GetCurrentProcess, GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        PROCESS_SYNCHRONIZE, WaitForSingleObject,
    },
};

// Serializes this module's writers. Callers must also serialize other PATH
// writers in an embedding process. The native manager owns its process scope.
static MUTATION: Mutex<()> = Mutex::new(());
const MAX_UNITS: usize = 32767;

#[cfg(test)]
pub(crate) fn with_test_environment<T>(run: impl FnOnce() -> T) -> T {
    tests::with_path(run)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Owner {
    pid: u32,
    creation_time: u64,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProcessPathChange {
    owner: Owner,
    before: Option<String>,
    after: Option<String>,
}

impl fmt::Debug for ProcessPathChange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProcessPathChange")
            .field("owner", &self.owner)
            .finish_non_exhaustive()
    }
}

pub(crate) struct ProcessPathSnapshot {
    value: Option<String>,
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::other(message)
}

fn validate_value(value: &Option<String>) -> io::Result<()> {
    if value
        .as_ref()
        .is_some_and(|value| value.contains('\0') || value.encode_utf16().count() > MAX_UNITS)
    {
        return Err(invalid("process PATH is invalid or exceeds its bound"));
    }
    Ok(())
}

fn current() -> io::Result<Option<String>> {
    let value = env::var_os("PATH")
        .map(|value| {
            value
                .into_string()
                .map_err(|_| invalid("process PATH is not valid Unicode; preserving it"))
        })
        .transpose()?;
    validate_value(&value)?;
    Ok(value)
}

fn created(handle: HANDLE) -> io::Result<u64> {
    let mut creation = FILETIME::default();
    let mut exit = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    if unsafe { GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok((u64::from(creation.dwHighDateTime) << 32) | u64::from(creation.dwLowDateTime))
}

impl Owner {
    fn current() -> io::Result<Self> {
        Ok(Self {
            pid: std::process::id(),
            creation_time: created(unsafe { GetCurrentProcess() })?,
        })
    }

    fn live(&self) -> io::Result<bool> {
        let handle = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                0,
                self.pid,
            )
        };
        if handle.is_null() {
            let error = io::Error::last_os_error();
            return if error.raw_os_error() == Some(ERROR_INVALID_PARAMETER as i32) {
                Ok(false)
            } else {
                Err(error)
            };
        }
        // The retained handle and creation time distinguish an exited owner
        // from a reused PID. Access/identity uncertainty never permits a write.
        let handle = unsafe { OwnedHandle::from_raw_handle(handle) };
        if created(handle.as_raw_handle())? != self.creation_time {
            return Ok(false);
        }
        match unsafe { WaitForSingleObject(handle.as_raw_handle(), 0) } {
            WAIT_OBJECT_0 => Ok(false),
            WAIT_TIMEOUT => Ok(true),
            _ => Err(io::Error::last_os_error()),
        }
    }
}

impl ProcessPathSnapshot {
    pub(crate) fn read() -> io::Result<Self> {
        Ok(Self { value: current()? })
    }

    /// Old PowerShell journals recorded values, with no process identity. Only
    /// an explicit recovery with a matching current value may restore them.
    pub(crate) fn legacy_restore(
        &self,
        before: &Option<String>,
        after: &Option<String>,
    ) -> io::Result<ProcessPathChange> {
        validate_value(before)?;
        validate_value(after)?;
        if &self.value != before && &self.value != after {
            return Err(invalid("legacy process PATH changed; preserving it"));
        }
        self.change(before.clone())
    }

    pub(crate) fn prepend(&self, bin: &Path) -> io::Result<(ProcessPathChange, bool)> {
        let (after, added) = path_plan::prepend(self.value.as_deref(), bin)?;
        let after = if added {
            Some(after)
        } else {
            self.value.clone()
        };
        Ok((self.change(after)?, added))
    }

    pub(crate) fn remove(&self, bin: &Path, was_added: bool) -> io::Result<ProcessPathChange> {
        let after = if was_added {
            self.value
                .as_ref()
                .map(|value| path_plan::remove(value, bin))
                .transpose()?
        } else {
            self.value.clone()
        };
        self.change(after)
    }

    fn change(&self, after: Option<String>) -> io::Result<ProcessPathChange> {
        let change = ProcessPathChange {
            owner: Owner::current()?,
            before: self.value.clone(),
            after,
        };
        change.validate()?;
        Ok(change)
    }
}

impl ProcessPathChange {
    pub(crate) fn validate(&self) -> io::Result<()> {
        if self.owner.pid == 0 || self.owner.creation_time == 0 {
            return Err(invalid("process PATH owner identity is invalid"));
        }
        validate_value(&self.before)?;
        validate_value(&self.after)
    }

    pub(crate) fn is_noop(&self) -> bool {
        self.before == self.after
    }

    pub(crate) fn publish(&self) -> io::Result<()> {
        self.validate()?;
        if self.owner != Owner::current()? {
            return Err(invalid(
                "process PATH publication belongs to another process",
            ));
        }
        let _guard = MUTATION
            .lock()
            .map_err(|_| invalid("process PATH mutation lock is poisoned"))?;
        if current()? != self.before {
            return Err(invalid(
                "process PATH changed before publication; preserving it",
            ));
        }
        set(&self.after);
        if current()? != self.after {
            return Err(invalid(
                "process PATH changed during publication; preserving it",
            ));
        }
        Ok(())
    }

    pub(crate) fn verify_published(&self) -> io::Result<()> {
        self.validate()?;
        if self.owner != Owner::current()? {
            return if self.owner.live()? {
                Err(invalid(
                    "process PATH owner is still alive; recover in that process",
                ))
            } else {
                Ok(())
            };
        }
        if current()? != self.after {
            return Err(invalid(
                "process PATH changed after publication; preserving it",
            ));
        }
        Ok(())
    }

    pub(crate) fn rollback(&self) -> io::Result<()> {
        self.rollback_inner(true)
    }

    pub(crate) fn check_undo(&self) -> io::Result<()> {
        self.rollback_inner(false)
    }

    fn rollback_inner(&self, apply: bool) -> io::Result<()> {
        self.validate()?;
        if self.owner != Owner::current()? {
            return if self.owner.live()? {
                Err(invalid(
                    "process PATH owner is still alive; recover in that process",
                ))
            } else {
                Ok(())
            };
        }
        let _guard = MUTATION
            .lock()
            .map_err(|_| invalid("process PATH mutation lock is poisoned"))?;
        let value = current()?;
        if value == self.before {
            return Ok(());
        }
        if value != self.after {
            return Err(invalid(
                "process PATH changed before rollback; preserving it",
            ));
        }
        if apply {
            set(&self.before);
        }
        Ok(())
    }
}

fn set(value: &Option<String>) {
    // std's installed Windows contract permits set_var/remove_var even with
    // multiple threads. This module serializes its own logical PATH mutations.
    unsafe {
        match value {
            Some(value) => env::set_var("PATH", value),
            None => env::remove_var("PATH"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Restore(Option<String>);
    impl Drop for Restore {
        fn drop(&mut self) {
            set(&self.0);
        }
    }
    pub(super) fn with_path<T>(run: impl FnOnce() -> T) -> T {
        let _restore = Restore(current().unwrap());
        run()
    }

    #[test]
    #[ignore = "owned child of foreign_owner_is_refused_until_exit"]
    fn foreign_owner_fixture() {
        let output = env::var_os("HARNESS_PROCESS_PATH_INTENT").unwrap();
        let (change, _) = ProcessPathSnapshot::read()
            .unwrap()
            .prepend(Path::new("C:\\owned-child-path\\bin"))
            .unwrap();
        change.publish().unwrap();
        std::fs::write(output, serde_json::to_vec(&change).unwrap()).unwrap();
        std::thread::sleep(std::time::Duration::from_secs(20));
        std::process::exit(79);
    }

    #[test]
    fn foreign_owner_is_refused_until_exit() {
        use std::{
            fs,
            process::{Command, Stdio},
            time::{Duration, Instant},
        };
        let _restore = Restore(current().unwrap());
        let root = tempfile::tempdir().unwrap();
        let intent = root.path().join("private-intent.json");
        let mut child = Command::new(env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "process_path::tests::foreign_owner_fixture",
                "--nocapture",
            ])
            .env("HARNESS_PROCESS_PATH_INTENT", &intent)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(fs::File::create(root.path().join("child.stderr")).unwrap())
            .spawn()
            .unwrap();
        let until = Instant::now() + Duration::from_secs(10);
        while !intent.exists() && Instant::now() < until {
            if child.try_wait().unwrap().is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let reached = intent.exists();
        if !reached {
            let _ = child.kill();
            let _ = child.wait();
            panic!("owned PATH fixture did not publish intent");
        }
        let change: ProcessPathChange =
            serde_json::from_slice(&fs::read(&intent).unwrap()).unwrap();
        assert_eq!(change.owner.pid, child.id());
        let foreign = Some("C:\\parent-owned-current".into());
        set(&foreign);
        assert!(change.publish().is_err());
        assert!(change.verify_published().is_err());
        assert!(change.rollback().is_err());
        assert_eq!(current().unwrap(), foreign);
        child.kill().unwrap();
        let exit = child.wait().unwrap();
        assert!(!exit.success());
        assert_ne!(exit.code(), Some(79));
        change.verify_published().unwrap();
        change.rollback().unwrap();
        assert!(change.publish().is_err());
        assert_eq!(current().unwrap(), foreign);
        let mut reused = ProcessPathSnapshot::read()
            .unwrap()
            .prepend(Path::new("C:\\owned-new-process\\bin"))
            .unwrap()
            .0;
        reused.owner.creation_time -= 1;
        reused.rollback().unwrap();
        assert_eq!(current().unwrap(), foreign);
    }

    #[test]
    fn process_path_roundtrip_preserves_absence_empty_unicode_and_foreign_edits() {
        let _restore = Restore(current().unwrap());
        for baseline in [
            None,
            Some(String::new()),
            Some("C:\\исходный;C:\\foreign".into()),
        ] {
            set(&baseline);
            let (change, added) = ProcessPathSnapshot::read()
                .unwrap()
                .prepend(Path::new("C:\\owned-native-test\\bin"))
                .unwrap();
            assert!(added);
            assert!(!change.is_noop());
            change.publish().unwrap();
            change.verify_published().unwrap();
            let adopted = ProcessPathSnapshot::read()
                .unwrap()
                .remove(Path::new("C:\\owned-native-test\\bin"), false)
                .unwrap();
            assert!(adopted.is_noop());
            let removal = ProcessPathSnapshot::read()
                .unwrap()
                .remove(Path::new("C:\\owned-native-test\\bin"), true)
                .unwrap();
            removal.publish().unwrap();
            removal.rollback().unwrap();
            let foreign = Some("C:\\concurrent-foreign".into());
            set(&foreign);
            assert!(change.rollback().is_err());
            assert!(change.verify_published().is_err());
            assert_eq!(current().unwrap(), foreign);
            set(&change.after);
            change.rollback().unwrap();
            change.rollback().unwrap();
            assert_eq!(current().unwrap(), baseline);
        }
    }
}
