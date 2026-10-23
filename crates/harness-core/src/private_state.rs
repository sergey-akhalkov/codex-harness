//! Protection for control state that carries a capability token.
//!
//! An elevated lead and its app-server run at High integrity. A same-user
//! Medium process can otherwise read the recorded token and attach to that
//! session. When this process is elevated, endpoint records and token files
//! receive a High mandatory label so lower-integrity readers are denied.

use std::{io, path::Path};

/// Protect `path` when the current process is elevated. Ordinary sessions keep
/// their existing readable state.
#[cfg(windows)]
pub fn protect_if_elevated(path: &Path) -> io::Result<()> {
    if !crate::task_scheduler::is_elevated()? {
        return Ok(());
    }
    set_high_mandatory_label(path)
}

#[cfg(not(windows))]
pub fn protect_if_elevated(_path: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(windows)]
fn set_high_mandatory_label(path: &Path) -> io::Result<()> {
    use std::{
        ffi::c_void,
        mem::{size_of, zeroed},
        os::windows::ffi::OsStrExt,
        ptr,
    };
    use windows_sys::Win32::{
        Foundation::{
            CloseHandle, ERROR_NOT_ALL_ASSIGNED, GetLastError, HLOCAL, LUID, LocalFree,
            SetLastError,
        },
        Security::{
            ACL, ACL_REVISION, AddMandatoryAce, AdjustTokenPrivileges,
            Authorization::{ConvertStringSidToSidW, SE_FILE_OBJECT, SetNamedSecurityInfoW},
            GetLengthSid, InitializeAcl, LABEL_SECURITY_INFORMATION, LookupPrivilegeValueW, PSID,
            SE_PRIVILEGE_ENABLED, SE_SECURITY_NAME, SYSTEM_MANDATORY_LABEL_ACE,
            TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY,
        },
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };

    const NO_WRITE_UP: u32 = 1;
    const NO_READ_UP: u32 = 2;
    const NO_EXECUTE_UP: u32 = 4;
    const HIGH_MANDATORY_LEVEL: windows_sys::core::PCWSTR = windows_sys::core::w!("S-1-16-12288");

    fn checked(value: i32) -> io::Result<()> {
        if value == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    unsafe fn with_security_privilege<T>(
        operation: impl FnOnce() -> io::Result<T>,
    ) -> io::Result<T> {
        let mut token = ptr::null_mut();
        checked(unsafe {
            OpenProcessToken(
                GetCurrentProcess(),
                TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY,
                &mut token,
            )
        })?;
        let result = (|| {
            let mut luid = LUID {
                LowPart: 0,
                HighPart: 0,
            };
            checked(unsafe { LookupPrivilegeValueW(ptr::null(), SE_SECURITY_NAME, &mut luid) })?;
            let mut enable: TOKEN_PRIVILEGES = unsafe { zeroed() };
            enable.PrivilegeCount = 1;
            enable.Privileges[0].Luid = luid;
            enable.Privileges[0].Attributes = SE_PRIVILEGE_ENABLED;
            let mut previous: TOKEN_PRIVILEGES = unsafe { zeroed() };
            let mut returned = 0;
            unsafe { SetLastError(0) };
            let enabled = unsafe {
                AdjustTokenPrivileges(
                    token,
                    0,
                    &enable,
                    size_of::<TOKEN_PRIVILEGES>() as u32,
                    &mut previous,
                    &mut returned,
                )
            };
            if enabled == 0 {
                return Err(io::Error::last_os_error());
            }
            let error = unsafe { GetLastError() };
            if error == ERROR_NOT_ALL_ASSIGNED {
                return Err(io::Error::from_raw_os_error(error as i32));
            }
            let was_enabled = previous.PrivilegeCount > 0
                && previous.Privileges[0].Attributes & SE_PRIVILEGE_ENABLED != 0;
            let outcome = operation();
            if !was_enabled {
                let mut disable: TOKEN_PRIVILEGES = unsafe { zeroed() };
                disable.PrivilegeCount = 1;
                disable.Privileges[0].Luid = luid;
                let _ = unsafe {
                    AdjustTokenPrivileges(token, 0, &disable, 0, ptr::null_mut(), ptr::null_mut())
                };
            }
            outcome
        })();
        unsafe { CloseHandle(token) };
        result
    }

    unsafe fn apply_label(path: &Path) -> io::Result<()> {
        let mut sid: PSID = ptr::null_mut();
        if unsafe { ConvertStringSidToSidW(HIGH_MANDATORY_LEVEL, &mut sid) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let result = (|| {
            let sid_len = unsafe { GetLengthSid(sid) } as usize;
            let acl_len = size_of::<ACL>() + size_of::<SYSTEM_MANDATORY_LABEL_ACE>() + sid_len
                - size_of::<u32>();
            let words = acl_len.div_ceil(size_of::<u32>()) + 4;
            let mut storage = vec![0u32; words];
            let acl = storage.as_mut_ptr().cast::<ACL>();
            checked(unsafe { InitializeAcl(acl, acl_len as u32, ACL_REVISION) })?;
            checked(unsafe {
                AddMandatoryAce(
                    acl,
                    ACL_REVISION,
                    0,
                    NO_WRITE_UP | NO_READ_UP | NO_EXECUTE_UP,
                    sid,
                )
            })?;
            let wide: Vec<u16> = path
                .as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();
            let code = unsafe {
                SetNamedSecurityInfoW(
                    wide.as_ptr(),
                    SE_FILE_OBJECT,
                    LABEL_SECURITY_INFORMATION,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null(),
                    acl,
                )
            };
            if code != 0 {
                return Err(io::Error::from_raw_os_error(code as i32));
            }
            Ok(())
        })();
        unsafe { LocalFree(sid.cast::<c_void>() as HLOCAL) };
        result
    }

    unsafe { with_security_privilege(|| apply_label(path)) }
}

#[cfg(all(test, windows))]
mod tests {
    #[test]
    fn protection_is_a_readable_no_op_for_an_ordinary_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state");
        std::fs::write(&path, b"token").unwrap();
        super::protect_if_elevated(&path).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"token");
    }
}
