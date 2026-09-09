#![cfg(windows)]
use harness_core::{
    config_file::ConfigSnapshot,
    registration::{LinkChange, Registration},
};
use std::{
    fs,
    os::windows::fs::{symlink_dir, symlink_file},
    path::PathBuf,
};

fn fixture() -> PathBuf {
    let root = tempfile::Builder::new()
        .prefix("harness-link-change-")
        .tempdir()
        .unwrap()
        .keep();
    fs::create_dir(root.join("old-dir")).unwrap();
    fs::create_dir(root.join("new-dir")).unwrap();
    for name in ["old-file", "new-file", "old-dir/keep", "new-dir/keep"] {
        fs::write(root.join(name), name).unwrap();
    }
    println!("link change evidence: {}", root.display());
    root
}

#[test]
fn capture_refuses_a_different_stored_target_even_when_it_resolves_to_the_same_file() {
    let root = fixture();
    let alias = root.join("alias");
    let old = root.join("old-file");
    symlink_file(&old, &alias).unwrap();
    let path = root.join("managed");
    symlink_file(&alias, &path).unwrap();
    assert_eq!(
        fs::canonicalize(&path).unwrap(),
        fs::canonicalize(&old).unwrap()
    );
    assert!(LinkChange::remove(&path, &old).is_err());
    assert!(LinkChange::replace(&path, &old, &root.join("new-file")).is_err());
    assert_eq!(fs::read_link(&path).unwrap(), alias);
    assert_eq!(fs::read(&old).unwrap(), b"old-file");
}

// This explicit mutation oracle needs an existing account privilege beyond
// Developer Mode's unprivileged CreateSymbolicLink flag. It enables it only in
// the short-lived Rust test process, never in an account or global policy.
fn retarget_link(path: &std::path::Path, target: &std::path::Path) -> std::io::Result<()> {
    use std::os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    };
    use windows_sys::Win32::{
        Foundation::{GetLastError, INVALID_HANDLE_VALUE, SetLastError},
        Security::{
            AdjustTokenPrivileges, LUID_AND_ATTRIBUTES, LookupPrivilegeValueW,
            SE_PRIVILEGE_ENABLED, TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY,
        },
        Storage::FileSystem::{
            CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
            FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_WRITE_ATTRIBUTES,
            OPEN_EXISTING,
        },
        System::{
            IO::DeviceIoControl,
            Threading::{GetCurrentProcess, OpenProcessToken},
        },
    };
    let mut token = std::ptr::null_mut();
    if unsafe {
        OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY,
            &mut token,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error());
    }
    let token = unsafe { OwnedHandle::from_raw_handle(token) };
    let privilege: Vec<u16> = "SeCreateSymbolicLinkPrivilege"
        .encode_utf16()
        .chain([0])
        .collect();
    let mut entry = LUID_AND_ATTRIBUTES::default();
    if unsafe { LookupPrivilegeValueW(std::ptr::null(), privilege.as_ptr(), &mut entry.Luid) } == 0
    {
        return Err(std::io::Error::last_os_error());
    }
    entry.Attributes = SE_PRIVILEGE_ENABLED;
    let state = TOKEN_PRIVILEGES {
        PrivilegeCount: 1,
        Privileges: [entry],
    };
    unsafe {
        SetLastError(0);
    }
    if unsafe {
        AdjustTokenPrivileges(
            token.as_raw_handle(),
            0,
            &state,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error());
    }
    let error = unsafe { GetLastError() };
    if error != 0 {
        return Err(std::io::Error::from_raw_os_error(error as i32));
    }
    let name: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    let file = unsafe {
        CreateFileW(
            name.as_ptr(),
            FILE_WRITE_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS,
            std::ptr::null_mut(),
        )
    };
    if file == INVALID_HANDLE_VALUE {
        return Err(std::io::Error::last_os_error());
    }
    let file = unsafe { OwnedHandle::from_raw_handle(file) };
    let substitute: Vec<u16> = "\\??\\"
        .encode_utf16()
        .chain(target.as_os_str().encode_wide())
        .collect();
    let print: Vec<u16> = target.as_os_str().encode_wide().collect();
    let length = u16::try_from(12 + (substitute.len() + print.len() + 2) * 2)
        .map_err(std::io::Error::other)?;
    let mut data = Vec::new();
    data.extend(0xa000000cu32.to_le_bytes());
    data.extend(length.to_le_bytes());
    data.extend(0u16.to_le_bytes());
    data.extend(0u16.to_le_bytes());
    data.extend(((substitute.len() * 2) as u16).to_le_bytes());
    data.extend((((substitute.len() + 1) * 2) as u16).to_le_bytes());
    data.extend(((print.len() * 2) as u16).to_le_bytes());
    data.extend(0u32.to_le_bytes());
    for unit in substitute.into_iter().chain([0]).chain(print).chain([0]) {
        data.extend(unit.to_le_bytes());
    }
    let mut used = 0;
    if unsafe {
        DeviceIoControl(
            file.as_raw_handle(),
            0x900a4,
            data.as_ptr().cast(),
            data.len() as u32,
            std::ptr::null_mut(),
            0,
            &mut used,
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[test]
#[ignore = "requires existing SeCreateSymbolicLinkPrivilege; explicit owned reparse mutation"]
fn retargeted_backup_alias_is_rejected_before_any_configuration_or_link_undo() {
    let root = fixture();
    let old = root.join("old-file");
    let next = root.join("new-file");
    let alias = root.join("alias");
    symlink_file(&old, &alias).unwrap();
    let path = root.join("managed");
    symlink_file(&old, &path).unwrap();
    let created = root.join("created.toml");
    let reg = Registration::open(&root.join("state")).unwrap();
    reg.apply_with_changes(
        &[],
        &[],
        &[harness_core::config_create::ConfigCreation::new(&created, b"owned config").unwrap()],
        &[LinkChange::replace(&path, &old, &next).unwrap()],
    )
    .unwrap();
    let journal = fs::read(reg.journal_path()).unwrap();
    let parsed: serde_json::Value = serde_json::from_slice(&journal).unwrap();
    let backup = PathBuf::from(parsed["link_changes"][0]["backup"].as_str().unwrap());
    retarget_link(&backup, &alias)
        .expect("owned reparse mutation privilege must be available for this explicit test");
    assert_eq!(
        fs::canonicalize(&backup).unwrap(),
        fs::canonicalize(&old).unwrap()
    );
    assert!(reg.disconnect().is_err());
    assert_eq!(fs::read_link(&backup).unwrap(), alias);
    assert_eq!(fs::read_link(&path).unwrap(), next);
    assert_eq!(fs::read(&created).unwrap(), b"owned config");
    assert_eq!(fs::read(reg.journal_path()).unwrap(), journal);
    retarget_link(&backup, &old).unwrap();
    reg.disconnect().unwrap();
    assert_eq!(fs::read_link(&path).unwrap(), old);
    assert!(!created.exists());
    assert_eq!(fs::read(&old).unwrap(), b"old-file");
    assert_eq!(fs::read(&next).unwrap(), b"new-file");
}

#[test]
fn replacement_and_removal_restore_original_file_directory_and_dangling_links() {
    for old_dir in [false, true] {
        for next_dir in [None, Some(false), Some(true)] {
            let root = fixture();
            let target = root.join(if old_dir { "old-dir" } else { "old-file" });
            let path = root.join("managed");
            if old_dir {
                symlink_dir(&target, &path).unwrap();
            } else {
                symlink_file(&target, &path).unwrap();
            }
            // Capture must work even when the former checkout moved away.
            if next_dir.is_none() {
                fs::rename(&target, root.join("preserved-source")).unwrap();
            }
            let change = match next_dir {
                Some(directory) => LinkChange::replace(
                    &path,
                    &target,
                    &root.join(if directory { "new-dir" } else { "new-file" }),
                )
                .unwrap(),
                None => LinkChange::remove(&path, &target).unwrap(),
            };
            let reg = Registration::open(&root.join("state")).unwrap();
            reg.apply_with_changes(&[], &[], &[], std::slice::from_ref(&change))
                .unwrap();
            if let Some(directory) = next_dir {
                assert_eq!(
                    fs::read_link(&path).unwrap(),
                    root.join(if directory { "new-dir" } else { "new-file" })
                );
            } else {
                assert!(fs::symlink_metadata(&path).is_err());
            }
            reg.apply_with_changes(&[], &[], &[], std::slice::from_ref(&change))
                .unwrap();
            let undo = reg.disconnect().unwrap();
            assert_eq!(undo.restored, vec![path.clone()]);
            assert_eq!(fs::read_link(&path).unwrap(), target);
            assert!(!reg.journal_path().exists());
            assert!(!fs::read_dir(&root).unwrap().any(|e| {
                e.unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".codex-harness-")
            }));
            assert_eq!(fs::read(root.join("new-file")).unwrap(), b"new-file");
        }
    }
}

#[test]
fn capture_identity_and_foreign_rollback_conflicts_preserve_all_versions() {
    let root = fixture();
    let path = root.join("managed");
    let old = root.join("old-file");
    symlink_file(&old, &path).unwrap();
    let change = LinkChange::replace(&path, &old, &root.join("new-file")).unwrap();
    fs::rename(&path, root.join("retained-original")).unwrap();
    symlink_file(&old, &path).unwrap();
    let reg = Registration::open(&root.join("state")).unwrap();
    assert!(reg.apply_with_changes(&[], &[], &[], &[change]).is_err());
    assert!(!reg.journal_path().exists());
    assert_eq!(fs::read_link(&path).unwrap(), old);
    fs::remove_file(&path).unwrap();
    fs::rename(root.join("retained-original"), &path).unwrap();
    let change = LinkChange::replace(&path, &old, &root.join("new-file")).unwrap();
    let config = root.join("config.toml");
    fs::write(&config, b"before").unwrap();
    reg.apply_with_changes(
        &[],
        &[ConfigSnapshot::read(&config)
            .unwrap()
            .plan_replace(b"after")
            .unwrap()],
        &[],
        &[change],
    )
    .unwrap();
    fs::rename(&path, root.join("retained-published")).unwrap();
    fs::write(&path, b"foreign").unwrap();
    let journal = fs::read(reg.journal_path()).unwrap();
    assert!(reg.disconnect().is_err());
    assert_eq!(fs::read(&config).unwrap(), b"after");
    assert_eq!(fs::read(&path).unwrap(), b"foreign");
    assert_eq!(fs::read(reg.journal_path()).unwrap(), journal);
    fs::remove_file(&path).unwrap(); // resolves this owned actor's foreign object
    reg.disconnect().unwrap();
    assert_eq!(fs::read(&config).unwrap(), b"before");
    assert_eq!(fs::read_link(&path).unwrap(), old);
    assert_eq!(
        fs::read_link(root.join("retained-published")).unwrap(),
        root.join("new-file")
    );
}
