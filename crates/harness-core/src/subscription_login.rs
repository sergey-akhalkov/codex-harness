//! Native subscription login: browser-only xAI OAuth and host-private Z.AI keys.
#![cfg(windows)]

use crate::{
    opencodex_login::{self, LoginStatus},
    process::{Cancellation, Deadline},
    subscription_lifecycle,
};
use serde_json::{Value, json};
use std::{
    fs::{self, OpenOptions},
    io::{self, Read, Write},
    net::TcpListener,
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE},
    Security::{
        ACCESS_ALLOWED_ACE, ACL, ACL_REVISION, AddAccessAllowedAceEx, DACL_SECURITY_INFORMATION,
        GetAce, GetFileSecurityW, GetSecurityDescriptorControl, GetSecurityDescriptorDacl,
        GetTokenInformation, InitializeAcl, InitializeSecurityDescriptor,
        PROTECTED_DACL_SECURITY_INFORMATION, SE_DACL_PROTECTED, SECURITY_DESCRIPTOR,
        SetFileSecurityW, SetSecurityDescriptorControl, SetSecurityDescriptorDacl, TOKEN_QUERY,
        TOKEN_USER, TokenUser,
    },
    Storage::FileSystem::FILE_ALL_ACCESS,
    System::Threading::{GetCurrentProcess, OpenProcessToken},
};

const LOGIN: Duration = Duration::from_secs(360);
const SECRET_LIMIT: usize = 8192;

fn other(message: impl Into<String>) -> io::Error {
    io::Error::other(message.into())
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

fn checked(ok: i32, message: &str) -> io::Result<()> {
    if ok == 0 { Err(other(message)) } else { Ok(()) }
}

fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

pub fn protect_owner_file(path: &Path) -> io::Result<()> {
    let mut token = std::ptr::null_mut();
    checked(
        unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) },
        "Z.AI key ACL identity unavailable",
    )?;
    struct Owned(HANDLE);
    impl Drop for Owned {
        fn drop(&mut self) {
            if !self.0.is_null() && self.0 != INVALID_HANDLE_VALUE {
                unsafe {
                    CloseHandle(self.0);
                }
            }
        }
    }
    let token = Owned(token);
    let mut needed = 0;
    unsafe {
        GetTokenInformation(token.0, TokenUser, std::ptr::null_mut(), 0, &mut needed);
    }
    if needed == 0 || needed > 65536 {
        return Err(other("Z.AI key ACL identity size limit"));
    }
    let mut user = vec![0u64; (needed as usize).div_ceil(8)];
    checked(
        unsafe {
            GetTokenInformation(
                token.0,
                TokenUser,
                user.as_mut_ptr().cast(),
                needed,
                &mut needed,
            )
        },
        "Z.AI key ACL identity unavailable",
    )?;
    let sid = unsafe { (*(user.as_ptr().cast::<TOKEN_USER>())).User.Sid };
    let mut acl = [0u32; 256];
    let acl_ptr = acl.as_mut_ptr().cast::<ACL>();
    checked(
        unsafe { InitializeAcl(acl_ptr, std::mem::size_of_val(&acl) as u32, ACL_REVISION) },
        "Z.AI key ACL failed",
    )?;
    checked(
        unsafe { AddAccessAllowedAceEx(acl_ptr, ACL_REVISION, 0, FILE_ALL_ACCESS, sid) },
        "Z.AI key ACL failed",
    )?;
    let mut descriptor = SECURITY_DESCRIPTOR::default();
    let sd = (&mut descriptor as *mut SECURITY_DESCRIPTOR).cast();
    checked(
        unsafe { InitializeSecurityDescriptor(sd, 1) },
        "Z.AI key ACL failed",
    )?;
    checked(
        unsafe { SetSecurityDescriptorDacl(sd, 1, acl_ptr, 0) },
        "Z.AI key ACL failed",
    )?;
    checked(
        unsafe { SetSecurityDescriptorControl(sd, SE_DACL_PROTECTED, SE_DACL_PROTECTED) },
        "Z.AI key ACL protection failed",
    )?;
    let wide = wide(path);
    checked(
        unsafe {
            SetFileSecurityW(
                wide.as_ptr(),
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                sd,
            )
        },
        "Z.AI key ACL failed",
    )?;
    let mut stored = [0u64; 1024];
    let stored_sd = stored.as_mut_ptr().cast();
    checked(
        unsafe {
            GetFileSecurityW(
                wide.as_ptr(),
                DACL_SECURITY_INFORMATION,
                stored_sd,
                std::mem::size_of_val(&stored) as u32,
                &mut needed,
            )
        },
        "Z.AI key ACL readback failed",
    )?;
    let (mut actual, mut present, mut defaulted) = (std::ptr::null_mut(), 0, 0);
    let (mut control, mut revision) = (0, 0);
    checked(
        unsafe { GetSecurityDescriptorDacl(stored_sd, &mut present, &mut actual, &mut defaulted) },
        "Z.AI key ACL readback failed",
    )?;
    checked(
        unsafe { GetSecurityDescriptorControl(stored_sd, &mut control, &mut revision) },
        "Z.AI key ACL readback failed",
    )?;
    if present == 0
        || actual.is_null()
        || control & SE_DACL_PROTECTED == 0
        || unsafe { (*actual).AceCount } != 1
    {
        return Err(other("Z.AI key ACL is not owner-only and protected"));
    }
    let mut entry = std::ptr::null_mut();
    checked(
        unsafe { GetAce(actual, 0, &mut entry) },
        "Z.AI key ACL entry unavailable",
    )?;
    let ace = unsafe { &*entry.cast::<ACCESS_ALLOWED_ACE>() };
    if ace.Header.AceType != 0 || ace.Mask != FILE_ALL_ACCESS {
        return Err(other("Z.AI key ACL does not match current account"));
    }
    Ok(())
}

fn private_snapshot(path: &Path) -> io::Result<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn preserved(path: &Path, before: Option<&[u8]>) -> io::Result<()> {
    let current = private_snapshot(path)?;
    if current.as_deref() == before {
        return Ok(());
    }
    // Private files are host-owned: restore the pre-login bytes exactly.
    match before {
        Some(bytes) => fs::write(path, bytes)?,
        None => {
            if path.is_file() {
                fs::remove_file(path)?;
            }
        }
    }
    Ok(())
}

fn unique_name() -> String {
    crate::broker_endpoint::random_key().unwrap_or_else(|_| {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|value| value.as_nanos())
            .unwrap_or_default();
        format!("{nanos:x}")
    })
}

fn open_local_page(path: &Path) -> io::Result<()> {
    Command::new("explorer")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|error| other(format!("cannot open local login page: {error}").as_str()))
}

fn read_stdin_key() -> io::Result<Option<String>> {
    let mut line = String::new();
    if std::io::stdin().read_line(&mut line)? == 0 {
        return Ok(None);
    }
    let key = line.trim();
    if key.is_empty() {
        Ok(None)
    } else {
        Ok(Some(key.to_string()))
    }
}

fn decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let Ok(byte) = u8::from_str_radix(
                std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap_or(""),
                16,
            )
        {
            out.push(byte);
            index += 3;
            continue;
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).replace('+', " ")
}

fn query_value(target: &str, name: &str) -> Option<String> {
    let query = target.split_once('?')?.1.split(' ').next()?;
    query.split('&').find_map(|part| {
        let (key, value) = part.split_once('=')?;
        (key == name).then(|| decode(value))
    })
}

fn receive_browser_key(page: &Path) -> io::Result<Option<String>> {
    const KEY_PORT: u16 = 56131;
    let listener = TcpListener::bind(("127.0.0.1", KEY_PORT))
        .map_err(|_| other("Z.AI key callback port 56131 unavailable"))?;
    fs::write(
        page,
        concat!(
            "<!doctype html><html lang=\"en\"><meta charset=\"utf-8\"><title>Z.AI key</title>",
            "<h1>Connect Z.AI</h1><p>Paste your Coding Plan key once.</p>",
            "<form action=\"http://127.0.0.1:56131/\" method=\"get\" autocomplete=\"off\">",
            "<input name=\"key\" type=\"password\" required autocomplete=\"off\">",
            "<button type=\"submit\">Send key</button></form></html>"
        ),
    )?;
    open_local_page(page)?;
    listener.set_nonblocking(true)?;
    let deadline = Deadline::after(Duration::from_secs(6 * 60))?;
    loop {
        if deadline.expired() {
            return Ok(None);
        }
        match listener.accept() {
            Ok((mut stream, _)) => {
                stream.set_read_timeout(Some(Duration::from_secs(5)))?;
                let mut request = Vec::new();
                let mut chunk = [0u8; 1024];
                while !request.windows(4).any(|window| window == b"\r\n\r\n")
                    && request.len() < 16 * 1024
                {
                    match stream.read(&mut chunk) {
                        Ok(0) => break,
                        Ok(size) => request.extend_from_slice(&chunk[..size]),
                        Err(_) => break,
                    }
                }
                let head = String::from_utf8_lossy(&request).into_owned();
                let target = head.lines().next().unwrap_or_default();
                let key = query_value(target, "key").unwrap_or_default();
                let body = if key.is_empty() {
                    "<html>missing key</html>"
                } else {
                    "<html>key received; you may close this tab.</html>"
                };
                let _ = stream.write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                );
                if key.is_empty() {
                    continue;
                }
                return Ok(Some(key));
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(error) => return Err(error),
        }
    }
}

fn store_zai_key(path: &Path, key: &str) -> io::Result<()> {
    if key.is_empty() || key.contains(['\r', '\n']) || key.len() > SECRET_LIMIT {
        return Err(other("Z.AI key must be a single non-empty line."));
    }
    if key.to_ascii_lowercase().contains("zai.config.toml") {
        return Err(other(
            "Login must not use the local zai profile as the secret source.",
        ));
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension(format!("{}.tmp", unique_name()));
    {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(key.as_bytes())?;
        file.flush()?;
    }
    protect_owner_file(&temporary)?;
    fs::rename(&temporary, path).or_else(|_| {
        fs::copy(&temporary, path)?;
        fs::remove_file(&temporary)
    })?;
    protect_owner_file(path)?;
    Ok(())
}

pub struct LoginRequest {
    pub provider: String,
    pub source: PathBuf,
    pub codex_home: PathBuf,
    pub user_home: PathBuf,
    pub package_root: Option<PathBuf>,
    pub key_file: Option<PathBuf>,
    pub open_browser: bool,
    pub evidence: Option<PathBuf>,
}
pub fn login(request: &LoginRequest) -> io::Result<Value> {
    let source = std::path::absolute(&request.source)?;
    let home = std::path::absolute(&request.codex_home)?;
    let user = std::path::absolute(&request.user_home)?;
    let paths = subscription_lifecycle::service_paths(&source, &user, &home)?;
    let before_profile = private_snapshot(&paths.zai_profile)?;
    let before_catalog = private_snapshot(&paths.zai_catalog)?;
    let report = match request.provider.as_str() {
        "xai" => login_xai(request, &paths)?,
        "zai" => login_zai(request, &paths)?,
        _ => return Err(invalid("subscription-login provider must be xai or zai")),
    };
    preserved(&paths.zai_profile, before_profile.as_deref())?;
    preserved(&paths.zai_catalog, before_catalog.as_deref())?;
    Ok(report)
}

fn login_xai(
    request: &LoginRequest,
    paths: &subscription_lifecycle::ServicePaths,
) -> io::Result<Value> {
    let package = request
        .package_root
        .clone()
        .unwrap_or_else(|| paths.home.clone());
    let evidence = request.evidence.clone().unwrap_or_else(|| {
        paths
            .home
            .join("harness/runtime/subscription-login")
            .join(unique_name())
    });
    fs::create_dir_all(&evidence)?;
    let authorization = evidence.join("authorization.json");
    let cancel = Cancellation::default();
    let deadline = Deadline::after(LOGIN)?;
    let page_path = PathBuf::from(format!("{}.html", authorization.display()));
    let store_home = paths.home.clone();
    let worker = {
        let package = package.clone();
        let authorization = authorization.clone();
        let cancel = cancel.clone();
        std::thread::spawn(move || {
            opencodex_login::login_xai_browser_only(
                &package,
                &store_home,
                &authorization,
                deadline,
                &cancel,
            )
        })
    };
    if request.open_browser {
        let started = std::time::Instant::now();
        while started.elapsed() < Duration::from_secs(8) {
            if page_path.is_file() {
                let _ = open_local_page(&page_path);
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    let probe = worker
        .join()
        .map_err(|_| other("Grok login worker failed"))??;
    if probe.status != LoginStatus::Saved {
        return Err(other(format!(
            "Grok login was not confirmed ({:?}). Evidence: {}",
            probe.status,
            evidence.display()
        )));
    }
    Ok(json!({
        "authenticated": true,
        "provider": "xai",
        "evidence": evidence
    }))
}

fn login_zai(
    request: &LoginRequest,
    paths: &subscription_lifecycle::ServicePaths,
) -> io::Result<Value> {
    let key = if let Some(file) = &request.key_file {
        if !file.is_absolute() {
            return Err(invalid("KeyFile must be an absolute path."));
        }
        let text = String::from_utf8(fs::read(file)?)
            .map_err(|_| other("Z.AI key must be UTF-8"))?
            .trim()
            .to_owned();
        Some(text)
    } else {
        read_stdin_key()?
    };
    let key = if let Some(key) = key.filter(|value| !value.is_empty()) {
        key
    } else {
        fs::create_dir_all(&paths.runtime)?;
        let page = paths
            .runtime
            .join(format!("zai-login-{}.html", unique_name()));
        let received = receive_browser_key(&page);
        let _ = fs::remove_file(&page);
        received?.ok_or_else(|| other("Z.AI key must be a single non-empty line."))?
    };
    store_zai_key(&paths.zai_key, &key)?;
    Ok(json!({
        "authorized": true,
        "provider": "zai",
        "store": paths.zai_key,
        "profileUnchanged": true
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_source(root: &Path) {
        let source = root.join("source");
        fs::create_dir_all(source.join("global/opencodex/agents")).unwrap();
        // The retired OpenCodex kit sources are gone; login tests only need
        // ordinary fixture files at the historical paths.
        fs::write(
            source.join("global/opencodex/config.json"),
            b"{\"hostname\":\"127.0.0.1\",\"port\":10100}",
        )
        .unwrap();
        fs::write(source.join("global/opencodex/agents/middle.toml"), b"role").unwrap();
        fs::write(source.join("global/opencodex/dependency.json"), b"{}").unwrap();
    }
    #[test]
    fn zai_login_from_key_file_protects_store_and_preserves_profile() {
        let root = tempfile::tempdir().unwrap();
        write_source(root.path());
        let source = std::path::absolute(root.path().join("source")).unwrap();
        let home = std::path::absolute(root.path().join("codex")).unwrap();
        let user = std::path::absolute(root.path().join("user")).unwrap();
        fs::create_dir_all(user.join(".opencodex")).unwrap();
        std::os::windows::fs::symlink_file(
            source.join("global/opencodex/config.json"),
            user.join(".opencodex/config.json"),
        )
        .unwrap();
        fs::create_dir_all(&home).unwrap();
        fs::write(home.join("zai.config.toml"), b"keep-profile").unwrap();
        fs::write(home.join("zai.models.json"), b"keep-catalog").unwrap();
        let key_file = std::path::absolute(root.path().join("key.txt")).unwrap();
        fs::write(&key_file, b"fixture-zai-secret\n").unwrap();
        let report = login(&LoginRequest {
            provider: "zai".into(),
            source,
            codex_home: home.clone(),
            user_home: user,
            package_root: None,
            key_file: Some(key_file),
            open_browser: false,
            evidence: None,
        })
        .unwrap();
        assert_eq!(report["authorized"], true);
        assert_eq!(
            fs::read(home.join("harness/subscriptions/zai-key.txt")).unwrap(),
            b"fixture-zai-secret"
        );
        assert_eq!(
            fs::read(home.join("zai.config.toml")).unwrap(),
            b"keep-profile"
        );
        assert_eq!(
            fs::read(home.join("zai.models.json")).unwrap(),
            b"keep-catalog"
        );
        protect_owner_file(&home.join("harness/subscriptions/zai-key.txt")).unwrap();
    }

    #[test]
    fn zai_login_rejects_profile_path_as_secret_without_writing_store() {
        let root = tempfile::tempdir().unwrap();
        write_source(root.path());
        let source = std::path::absolute(root.path().join("source")).unwrap();
        let home = std::path::absolute(root.path().join("codex")).unwrap();
        let user = std::path::absolute(root.path().join("user")).unwrap();
        fs::create_dir_all(user.join(".opencodex")).unwrap();
        std::os::windows::fs::symlink_file(
            source.join("global/opencodex/config.json"),
            user.join(".opencodex/config.json"),
        )
        .unwrap();
        let key_file = std::path::absolute(root.path().join("key.txt")).unwrap();
        fs::write(&key_file, b"C:\\private\\zai.config.toml").unwrap();
        let error = login(&LoginRequest {
            provider: "zai".into(),
            source,
            codex_home: home.clone(),
            user_home: user,
            package_root: None,
            key_file: Some(key_file),
            open_browser: false,
            evidence: None,
        })
        .unwrap_err();
        assert!(error.to_string().contains("local zai profile"));
        assert!(!home.join("harness/subscriptions/zai-key.txt").exists());
        assert!(!home.join("zai.config.toml").exists());
    }
}
