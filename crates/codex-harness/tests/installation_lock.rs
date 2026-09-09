#![cfg(windows)]

use harness_core::installation_lock::InstallationLock;
use std::{
    fs, io,
    os::windows::io::{FromRawHandle, OwnedHandle},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
use windows_sys::Win32::System::Threading::CreateMutexW;

#[test]
#[ignore = "owned Rust child; HARNESS_INSTALL_LOCK_ROOT must identify its test root"]
fn installation_lock_fixture() {
    let root = PathBuf::from(std::env::var_os("HARNESS_INSTALL_LOCK_ROOT").unwrap());
    let _guard = InstallationLock::acquire(&root.join("User Юникод")).unwrap();
    fs::write(root.join("ready"), b"locked").unwrap();
    // Bound even a manually invoked fixture. Parent termination tests kill and
    // reap this exact descendant-free process before the fallback expires.
    std::thread::sleep(Duration::from_secs(20));
    std::process::exit(79);
}

struct FixtureChild(Child);

impl Drop for FixtureChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn fixture(root: &Path) -> FixtureChild {
    let mut child = FixtureChild(
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "installation_lock_fixture",
                "--nocapture",
            ])
            .env("HARNESS_INSTALL_LOCK_ROOT", root)
            .stdin(Stdio::null())
            .stdout(fs::File::create(root.join("child.stdout")).unwrap())
            .stderr(fs::File::create(root.join("child.stderr")).unwrap())
            .spawn()
            .unwrap(),
    );
    let until = Instant::now() + Duration::from_secs(5);
    while !root.join("ready").exists() && Instant::now() < until {
        assert!(
            child.0.try_wait().unwrap().is_none(),
            "child failed: {}",
            root.display()
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        root.join("ready").exists(),
        "child not ready: {}",
        root.display()
    );
    child
}

fn root() -> PathBuf {
    let root = tempfile::Builder::new()
        .prefix("harness-install-lock-")
        .tempdir()
        .unwrap()
        .keep();
    println!("lock evidence: {}", root.display());
    root
}

#[test]
fn killed_owner_is_excluded_and_abandonment_is_observed_without_home_writes() {
    let root = root();
    let home = root.join("User Юникод");
    let mut child = fixture(&root);
    assert_eq!(
        InstallationLock::acquire(&home).err().unwrap().kind(),
        io::ErrorKind::WouldBlock
    );
    // Keep the existing kernel object alive across child death so its abandoned
    // state is observable. This exact name follows the old PowerShell contract.
    let text = home.to_str().unwrap().replace('/', "\\").to_lowercase();
    let name = format!(
        "Local\\CodexHarness-{}",
        harness_core::build_identity::hash_bytes(text.as_bytes()).to_ascii_uppercase()
    );
    let wide: Vec<_> = name.encode_utf16().chain(Some(0)).collect();
    let raw = unsafe { CreateMutexW(std::ptr::null(), 0, wide.as_ptr()) };
    assert!(!raw.is_null());
    let _retained = unsafe { OwnedHandle::from_raw_handle(raw) };
    child.0.kill().unwrap();
    let exit = child.0.wait().unwrap();
    assert!(!exit.success());
    assert_ne!(exit.code(), Some(79));
    let lock = InstallationLock::acquire(&home).unwrap();
    assert!(lock.was_abandoned());
    assert!(!home.exists());
    drop(lock);
    assert!(!InstallationLock::acquire(&home).unwrap().was_abandoned());
}

#[test]
fn actual_inventory_entrypoint_refuses_concurrent_installation_before_reading_sources() {
    let root = root();
    let home = root.join("User Юникод");
    let mut child = fixture(&root);
    let invoke = || {
        Command::new(env!("CARGO_BIN_EXE_codex-harness"))
            .arg("inventory")
            .arg("--source")
            .arg(root.join("missing-source"))
            .arg("--codex-home")
            .arg(root.join("missing-codex-home"))
            .arg("--user-home")
            .arg(&home)
            .current_dir(&root)
            .output()
            .unwrap()
    };
    let blocked = invoke();
    assert_eq!(blocked.status.code(), Some(2));
    assert!(blocked.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&blocked.stderr).contains("another harness operation is active")
    );
    assert!(!home.exists());
    assert!(!root.join("missing-codex-home").exists());
    child.0.kill().unwrap();
    child.0.wait().unwrap();
    let free = invoke();
    assert_eq!(free.status.code(), Some(2));
    assert!(!String::from_utf8_lossy(&free.stderr).contains("another harness operation is active"));
    assert!(free.stdout.is_empty());
}
