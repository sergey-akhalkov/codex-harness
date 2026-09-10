//! Bounded native observation. Notifications coalesce to a counter, including
//! overflow; a finite published sync reconciles the actual source tree.
#![cfg(windows)]
use std::{
    io,
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
};
use windows_sys::Win32::{
    Foundation::{ERROR_NOTIFY_ENUM_DIR, INVALID_HANDLE_VALUE, WAIT_OBJECT_0, WAIT_TIMEOUT},
    Storage::FileSystem::{
        CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OVERLAPPED, FILE_LIST_DIRECTORY,
        FILE_NOTIFY_CHANGE_DIR_NAME, FILE_NOTIFY_CHANGE_FILE_NAME, FILE_NOTIFY_CHANGE_LAST_WRITE,
        FILE_NOTIFY_CHANGE_SIZE, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
        OPEN_EXISTING, ReadDirectoryChangesW,
    },
    System::{
        IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED},
        Threading::{CreateEventW, ResetEvent, WaitForSingleObject},
    },
};

pub struct Observer {
    changed: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<io::Result<()>>>,
}

impl Observer {
    pub fn start(root: &Path) -> io::Result<Self> {
        let path: Vec<_> = root.as_os_str().encode_wide().chain([0]).collect();
        let raw = unsafe {
            CreateFileW(
                path.as_ptr(),
                FILE_LIST_DIRECTORY,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OVERLAPPED,
                std::ptr::null_mut(),
            )
        };
        if raw == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        let directory = unsafe { OwnedHandle::from_raw_handle(raw) };
        let raw = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
        if raw.is_null() {
            return Err(io::Error::last_os_error());
        }
        let event = unsafe { OwnedHandle::from_raw_handle(raw) };
        let changed = Arc::new(AtomicU64::new(1)); // connect-time catch-up
        let stop = Arc::new(AtomicBool::new(false));
        let counter = changed.clone();
        let stopping = stop.clone();
        let (ready, armed) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("codegraph-observer".into())
            .spawn(move || observe(directory, event, &counter, &stopping, ready))?;
        let mut observer = Self {
            changed,
            stop,
            worker: Some(worker),
        };
        match armed.recv() {
            Ok(Ok(())) => Ok(observer),
            Ok(Err(error)) => {
                let _ = observer.close();
                Err(error)
            }
            Err(_) => {
                let _ = observer.close();
                Err(io::Error::other("CodeGraph observer failed before arming"))
            }
        }
    }
    pub fn revision(&self) -> io::Result<u64> {
        if self.worker.as_ref().is_none_or(JoinHandle::is_finished) {
            return Err(io::Error::other(
                "CodeGraph source observation stopped; deliberate reconnect required",
            ));
        }
        Ok(self.changed.load(Ordering::Acquire))
    }
    pub fn close(&mut self) -> io::Result<()> {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            worker
                .join()
                .map_err(|_| io::Error::other("CodeGraph observer panicked"))??;
        }
        Ok(())
    }
}
impl Drop for Observer {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

fn relevant(path: &str) -> bool {
    !path.split(['/', '\\']).any(|part| {
        matches!(
            part,
            ".git"
                | "node_modules"
                | "target"
                | ".codegraph-harness-active"
                | ".codegraph-harness-stage"
                | ".codegraph-harness-store"
        )
    })
}

fn changed_source(bytes: &[u8]) -> bool {
    // FILE_NOTIFY_INFORMATION is a 12-byte header followed by UTF-16. Parse
    // without alignment assumptions; malformed/overflow notifications reconcile.
    let mut offset = 0;
    loop {
        if bytes.len().saturating_sub(offset) < 12 {
            return true;
        }
        let next = u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
        let length =
            u32::from_le_bytes(bytes[offset + 8..offset + 12].try_into().unwrap()) as usize;
        let Some(name) = bytes.get(offset + 12..offset + 12 + length) else {
            return true;
        };
        if !length.is_multiple_of(2) {
            return true;
        }
        let name: Vec<_> = name
            .chunks_exact(2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
            .collect();
        if relevant(&String::from_utf16_lossy(&name)) {
            return true;
        }
        if next == 0 {
            return false;
        }
        if next < 12 || next > bytes.len() - offset {
            return true;
        }
        offset += next;
    }
}

fn observe(
    directory: OwnedHandle,
    event: OwnedHandle,
    counter: &AtomicU64,
    stop: &AtomicBool,
    ready: mpsc::SyncSender<io::Result<()>>,
) -> io::Result<()> {
    let mut ready = Some(ready);
    // ReadDirectoryChangesW requires DWORD alignment, including on overflow.
    let mut aligned = vec![0u32; 8192];
    let bytes = unsafe {
        std::slice::from_raw_parts_mut(aligned.as_mut_ptr().cast::<u8>(), aligned.len() * 4)
    };
    loop {
        if stop.load(Ordering::Acquire) {
            return Ok(());
        }
        let mut operation = OVERLAPPED {
            hEvent: event.as_raw_handle(),
            ..Default::default()
        };
        unsafe {
            ResetEvent(event.as_raw_handle());
        }
        let ok = unsafe {
            ReadDirectoryChangesW(
                directory.as_raw_handle(),
                bytes.as_mut_ptr().cast(),
                bytes.len() as u32,
                1,
                FILE_NOTIFY_CHANGE_DIR_NAME
                    | FILE_NOTIFY_CHANGE_FILE_NAME
                    | FILE_NOTIFY_CHANGE_LAST_WRITE
                    | FILE_NOTIFY_CHANGE_SIZE,
                std::ptr::null_mut(),
                &mut operation,
                None,
            )
        };
        if ok == 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(ERROR_NOTIFY_ENUM_DIR as i32) {
                counter.fetch_add(1, Ordering::AcqRel);
                thread::sleep(std::time::Duration::from_millis(100));
                continue;
            }
            if let Some(ready) = ready.take() {
                let _ = ready.send(Err(io::Error::other(error.to_string())));
            }
            return Err(error);
        }
        if let Some(ready) = ready.take() {
            let _ = ready.send(Ok(()));
        }
        loop {
            if stop.load(Ordering::Acquire) {
                // The buffer/OVERLAPPED must outlive cancellation completion.
                unsafe {
                    CancelIoEx(directory.as_raw_handle(), &operation);
                }
                let mut count = 0;
                unsafe {
                    GetOverlappedResult(directory.as_raw_handle(), &operation, &mut count, 1);
                }
                return Ok(());
            }
            match unsafe { WaitForSingleObject(event.as_raw_handle(), 100) } {
                WAIT_OBJECT_0 => break,
                WAIT_TIMEOUT => (),
                _ => {
                    let error = io::Error::last_os_error();
                    unsafe {
                        CancelIoEx(directory.as_raw_handle(), &operation);
                    }
                    let mut count = 0;
                    unsafe {
                        GetOverlappedResult(directory.as_raw_handle(), &operation, &mut count, 1);
                    }
                    return Err(error);
                }
            }
        }
        let mut count = 0;
        if unsafe { GetOverlappedResult(directory.as_raw_handle(), &operation, &mut count, 0) } == 0
        {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(ERROR_NOTIFY_ENUM_DIR as i32) {
                counter.fetch_add(1, Ordering::AcqRel);
                continue;
            }
            return Err(error);
        }
        if changed_source(&bytes[..count as usize]) {
            counter.fetch_add(1, Ordering::AcqRel);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        time::{Duration, Instant},
    };

    #[test]
    fn observes_add_rename_delete_and_ignores_owned_index_writes() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("src")).unwrap();
        fs::create_dir(root.path().join(".codegraph-harness-active")).unwrap();
        let mut observer = Observer::start(root.path()).unwrap();
        let before = observer.revision().unwrap();
        fs::write(
            root.path().join(".codegraph-harness-active/codegraph.db"),
            b"ignored",
        )
        .unwrap();
        thread::sleep(Duration::from_millis(250));
        assert_eq!(observer.revision().unwrap(), before);
        let mut previous = before;
        for operation in 0..3 {
            match operation {
                0 => fs::write(root.path().join("src/added.rs"), b"pub fn added() {}"),
                1 => fs::rename(
                    root.path().join("src/added.rs"),
                    root.path().join("src/renamed.rs"),
                ),
                _ => fs::remove_file(root.path().join("src/renamed.rs")),
            }
            .unwrap();
            let start = Instant::now();
            while observer.revision().unwrap() == previous {
                assert!(start.elapsed() < Duration::from_secs(3));
                thread::sleep(Duration::from_millis(20));
            }
            previous = observer.revision().unwrap();
        }
        observer.close().unwrap();
        assert!(observer.revision().is_err());
    }
}
