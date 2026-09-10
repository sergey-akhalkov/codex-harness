//! Read the audited CBM 0.10.8 configuration without starting its daemon.
#![cfg(windows)]
use crate::{native_build, registration_native::ReadGuard};
use serde::Serialize;
use std::{
    cell::Cell,
    collections::BTreeSet,
    ffi::{CStr, CString, c_char, c_int, c_void},
    fs::{self, File, OpenOptions},
    io::{self, Read, Seek, SeekFrom, Write},
    os::windows::fs::OpenOptionsExt,
    os::windows::io::AsRawHandle,
    path::Path,
    time::{Duration, Instant},
};
use windows_sys::Win32::Foundation::FreeLibrary;
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle, LOCKFILE_EXCLUSIVE_LOCK,
    LOCKFILE_FAIL_IMMEDIATELY, LockFileEx, UnlockFileEx,
};
use windows_sys::Win32::System::IO::OVERLAPPED;
use windows_sys::Win32::System::LibraryLoader::{
    GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
};
use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;

fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "CBM configuration is unavailable or incompatible",
    )
}

const MAX_DATABASE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_WAL_BYTES: u64 = 8 * 1024 * 1024;

/// A nonwaiting Windows byte lock, released before its borrowed file closes.
struct ByteLock<'a> {
    file: &'a File,
    offset: u32,
    length: u32,
}
impl<'a> ByteLock<'a> {
    fn acquire(file: &'a File, offset: u32, length: u32, exclusive: bool) -> io::Result<Self> {
        let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
        overlapped.Anonymous.Anonymous.Offset = offset;
        let flags = LOCKFILE_FAIL_IMMEDIATELY
            | if exclusive {
                LOCKFILE_EXCLUSIVE_LOCK
            } else {
                0
            };
        if unsafe { LockFileEx(file.as_raw_handle(), flags, 0, length, 0, &mut overlapped) } == 0 {
            return Err(invalid());
        }
        Ok(Self {
            file,
            offset,
            length,
        })
    }
}
impl Drop for ByteLock<'_> {
    fn drop(&mut self) {
        let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
        overlapped.Anonymous.Anonymous.Offset = self.offset;
        unsafe {
            UnlockFileEx(
                self.file.as_raw_handle(),
                0,
                self.length,
                0,
                &mut overlapped,
            )
        };
    }
}

fn single_link(guard: &ReadGuard) -> io::Result<()> {
    let mut information: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    if unsafe { GetFileInformationByHandle(guard.file.as_raw_handle(), &mut information) } == 0
        || information.nNumberOfLinks != 1
    {
        return Err(invalid());
    }
    Ok(())
}

fn bounded_bytes(file: &File, limit: u64) -> io::Result<Vec<u8>> {
    let length = file.metadata()?.len();
    if length > limit {
        return Err(invalid());
    }
    let mut reader = file;
    reader.seek(SeekFrom::Start(0))?;
    let mut bytes = Vec::new();
    reader.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 != length {
        return Err(invalid());
    }
    Ok(bytes)
}

/// Coherence comes from the Windows SQLite locking protocol, not hashes.
/// The main SHARED lock excludes rollback writes and journal-mode transitions.
/// For WAL, DMS prevents SHM reinitialization, and the first three SHM locks
/// exclude writers, checkpoints and recovery while both input files are copied.
/// https://www.sqlite.org/lockingv3.html
/// https://www.sqlite.org/walformat.html (locks 120..127; Windows DMS is 128)
/// Only ordinary, single-link files using this standard local VFS are supported.
struct Snapshot {
    main: Vec<u8>,
    wal: Option<Vec<u8>>,
}
impl Snapshot {
    fn capture(main: &ReadGuard, cache: &Path) -> io::Result<Self> {
        single_link(main)?;
        let pending = ByteLock::acquire(&main.file, 0x4000_0000, 1, false)?;
        let _shared = ByteLock::acquire(&main.file, 0x4000_0002, 510, false)?;
        drop(pending);
        // A hot journal would require recovery. Reject even inert/persistent
        // journals rather than copy potentially uncommitted rollback pages.
        match ReadGuard::open_shared(&cache.join("_config.db-journal")) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => (),
            _ => return Err(invalid()),
        }
        let mut header = [0; 20];
        let mut reader = &main.file;
        reader.seek(SeekFrom::Start(0))?;
        reader.read_exact(&mut header).map_err(|_| invalid())?;
        if &header[..16] != b"SQLite format 3\0" {
            return Err(invalid());
        }
        match (header[18], header[19]) {
            (1, 1) => Ok(Self {
                main: bounded_bytes(&main.file, MAX_DATABASE_BYTES)?,
                wal: None,
            }),
            (2, 2) => {
                // Missing sidecars fail closed: never let SQLite create them
                // in the source namespace, and never silently ignore a WAL.
                let shm =
                    ReadGuard::open_shared(&cache.join("_config.db-shm")).map_err(|_| invalid())?;
                single_link(&shm)?;
                let _dms = ByteLock::acquire(&shm.file, 128, 1, false)?;
                let _writers = ByteLock::acquire(&shm.file, 120, 3, true)?;
                let wal =
                    ReadGuard::open_shared(&cache.join("_config.db-wal")).map_err(|_| invalid())?;
                single_link(&wal)?;
                Ok(Self {
                    main: bounded_bytes(&main.file, MAX_DATABASE_BYTES)?,
                    wal: Some(bounded_bytes(&wal.file, MAX_WAL_BYTES)?),
                })
            }
            _ => Err(invalid()),
        }
    }

    fn materialize(&self) -> io::Result<tempfile::TempDir> {
        let directory = tempfile::Builder::new().prefix("harness-cbm-").tempdir()?;
        File::create_new(directory.path().join("_config.db"))?.write_all(&self.main)?;
        if let Some(wal) = &self.wal {
            File::create_new(directory.path().join("_config.db-wal"))?.write_all(wal)?;
        }
        Ok(directory)
    }
}

type Database = *mut c_void;
type Statement = *mut c_void;
type Open = unsafe extern "C" fn(*const c_char, *mut Database, c_int, *const c_char) -> c_int;
type Close = unsafe extern "C" fn(Database) -> c_int;
type Prepare = unsafe extern "C" fn(
    Database,
    *const c_char,
    c_int,
    *mut Statement,
    *mut *const c_char,
) -> c_int;
type Step = unsafe extern "C" fn(Statement) -> c_int;
type Finalize = unsafe extern "C" fn(Statement) -> c_int;
type ColumnText = unsafe extern "C" fn(Statement, c_int) -> *const u8;
type ColumnBytes = unsafe extern "C" fn(Statement, c_int) -> c_int;
type ColumnType = unsafe extern "C" fn(Statement, c_int) -> c_int;
type BusyTimeout = unsafe extern "C" fn(Database, c_int) -> c_int;
type Progress = unsafe extern "C" fn(
    Database,
    c_int,
    Option<unsafe extern "C" fn(*mut c_void) -> c_int>,
    *mut c_void,
);
type Limit = unsafe extern "C" fn(Database, c_int, c_int) -> c_int;
type FileControl = unsafe extern "C" fn(Database, *const c_char, c_int, *mut c_void) -> c_int;

struct Api {
    library: *mut c_void,
    open: Open,
    close: Close,
    prepare: Prepare,
    step: Step,
    finalize: Finalize,
    text: ColumnText,
    bytes: ColumnBytes,
    kind: ColumnType,
    busy_timeout: BusyTimeout,
    progress: Progress,
    limit: Limit,
    file_control: FileControl,
}
impl Drop for Api {
    fn drop(&mut self) {
        unsafe {
            FreeLibrary(self.library);
        }
    }
}
impl Api {
    fn load() -> io::Result<Self> {
        let mut name = vec![0u16; 512];
        let length = unsafe { GetSystemDirectoryW(name.as_mut_ptr(), name.len() as u32) } as usize;
        if length == 0 || length >= name.len() {
            return Err(invalid());
        }
        name.truncate(length);
        name.extend("\\winsqlite3.dll\0".encode_utf16());
        let library = unsafe {
            LoadLibraryExW(
                name.as_ptr(),
                std::ptr::null_mut(),
                LOAD_LIBRARY_SEARCH_SYSTEM32,
            )
        };
        if library.is_null() {
            return Err(invalid());
        }
        // Resolve every required entry before constructing a usable API. The
        // temporary owner frees the DLL if any export is unavailable.
        struct Library(*mut c_void);
        impl Drop for Library {
            fn drop(&mut self) {
                unsafe {
                    FreeLibrary(self.0);
                }
            }
        }
        let owner = Library(library);
        unsafe fn symbol<T: Copy>(library: *mut c_void, name: &CStr) -> io::Result<T> {
            let pointer =
                unsafe { GetProcAddress(library, name.as_ptr().cast()) }.ok_or_else(invalid)?;
            if std::mem::size_of::<T>() != std::mem::size_of_val(&pointer) {
                return Err(invalid());
            }
            Ok(unsafe { std::mem::transmute_copy(&pointer) })
        }
        let version: unsafe extern "C" fn() -> c_int =
            unsafe { symbol(library, c"sqlite3_libversion_number")? };
        if unsafe { version() } < 3_041_000 {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "CBM resource inspection requires Windows SQLite 3.41 or newer",
            ));
        }
        let result = unsafe {
            Self {
                library,
                open: symbol(library, c"sqlite3_open_v2")?,
                close: symbol(library, c"sqlite3_close")?,
                prepare: symbol(library, c"sqlite3_prepare_v2")?,
                step: symbol(library, c"sqlite3_step")?,
                finalize: symbol(library, c"sqlite3_finalize")?,
                text: symbol(library, c"sqlite3_column_text")?,
                bytes: symbol(library, c"sqlite3_column_bytes")?,
                kind: symbol(library, c"sqlite3_column_type")?,
                busy_timeout: symbol(library, c"sqlite3_busy_timeout")?,
                progress: symbol(library, c"sqlite3_progress_handler")?,
                limit: symbol(library, c"sqlite3_limit")?,
                file_control: symbol(library, c"sqlite3_file_control")?,
            }
        };
        std::mem::forget(owner);
        Ok(result)
    }
}

struct Budget {
    started: Instant,
    callbacks: Cell<u32>,
}
unsafe extern "C" fn progress(pointer: *mut c_void) -> c_int {
    let budget = unsafe { &*pointer.cast::<Budget>() };
    let count = budget.callbacks.get().saturating_add(1);
    budget.callbacks.set(count);
    i32::from(count > 1000 || budget.started.elapsed() >= Duration::from_secs(2))
}
struct Connection<'a> {
    api: &'a Api,
    pointer: Database,
    _budget: Box<Budget>,
}
impl Drop for Connection<'_> {
    fn drop(&mut self) {
        unsafe {
            (self.api.close)(self.pointer);
        }
    }
}
struct Query<'q, 'a> {
    connection: &'q Connection<'a>,
    pointer: Statement,
}
impl Drop for Query<'_, '_> {
    fn drop(&mut self) {
        unsafe {
            (self.connection.api.finalize)(self.pointer);
        }
    }
}

impl<'a> Connection<'a> {
    fn verify_file(&self, guard: &ReadGuard) -> io::Result<()> {
        self.verify_open_file(&guard.file)
    }

    fn verify_open_file(&self, file: &File) -> io::Result<()> {
        fn identity(handle: *mut c_void) -> io::Result<(u32, u32, u32, u32, u32)> {
            let mut information: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
            if unsafe { GetFileInformationByHandle(handle, &mut information) } == 0 {
                return Err(invalid());
            }
            Ok((
                information.dwVolumeSerialNumber,
                information.nFileIndexHigh,
                information.nFileIndexLow,
                information.ftCreationTime.dwHighDateTime,
                information.ftCreationTime.dwLowDateTime,
            ))
        }
        let mut handle: *mut c_void = std::ptr::null_mut();
        // SQLITE_FCNTL_WIN32_GET_HANDLE. Borrow only; SQLite owns this handle.
        if unsafe {
            (self.api.file_control)(
                self.pointer,
                c"main".as_ptr(),
                29,
                (&mut handle as *mut *mut c_void).cast(),
            )
        } != 0
            || handle.is_null()
            || identity(handle)? != identity(file.as_raw_handle())?
        {
            return Err(invalid());
        }
        Ok(())
    }
    fn open(api: &'a Api, path: &Path, flags: c_int) -> io::Result<Self> {
        let name = CString::new(path.to_str().ok_or_else(invalid)?).map_err(|_| invalid())?;
        let mut pointer = std::ptr::null_mut();
        let code = unsafe { (api.open)(name.as_ptr(), &mut pointer, flags, std::ptr::null()) };
        if pointer.is_null() {
            return Err(invalid());
        }
        let mut budget = Box::new(Budget {
            started: Instant::now(),
            callbacks: Cell::new(0),
        });
        unsafe {
            (api.progress)(
                pointer,
                1000,
                Some(progress),
                (&mut *budget as *mut Budget).cast(),
            );
            (api.limit)(pointer, 0, 64 * 1024); // value/row length
            (api.limit)(pointer, 1, 4096); // SQL length
            (api.limit)(pointer, 2, 16); // columns
            (api.limit)(pointer, 5, 10_000); // compiled VDBE instructions
        }
        let connection = Self {
            api,
            pointer,
            _budget: budget,
        };
        if code != 0 || unsafe { (api.busy_timeout)(pointer, 2000) } != 0 {
            return Err(invalid());
        }
        Ok(connection)
    }
    fn prepare<'q>(&'q self, sql: &CStr) -> io::Result<Query<'q, 'a>> {
        let mut pointer = std::ptr::null_mut();
        let code = unsafe {
            (self.api.prepare)(
                self.pointer,
                sql.as_ptr(),
                -1,
                &mut pointer,
                std::ptr::null_mut(),
            )
        };
        if pointer.is_null() {
            return Err(invalid());
        }
        let query = Query {
            connection: self,
            pointer,
        };
        if code != 0 {
            return Err(invalid());
        }
        Ok(query)
    }
}
impl Query<'_, '_> {
    fn text(&self, column: c_int, limit: usize) -> io::Result<String> {
        let api = self.connection.api;
        if unsafe { (api.kind)(self.pointer, column) } != 3 {
            return Err(invalid());
        }
        let length = unsafe { (api.bytes)(self.pointer, column) };
        if length < 0 || length as usize > limit {
            return Err(invalid());
        }
        let pointer = unsafe { (api.text)(self.pointer, column) };
        if pointer.is_null() {
            return Err(invalid());
        }
        let bytes = unsafe { std::slice::from_raw_parts(pointer, length as usize) };
        Ok(std::str::from_utf8(bytes)
            .map_err(|_| invalid())?
            .to_owned())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Configuration {
    pub auto_index: bool,
    pub auto_watch: bool,
    pub ui_enabled: bool,
}
impl Configuration {
    pub fn bounded_policy_active(&self) -> bool {
        !self.auto_index && !self.auto_watch && !self.ui_enabled
    }
}

/// Seed only a fresh private probe cache. Never repairs an adopted cache or
/// overwrites settings. The caller owns this empty directory and its lifetime.
pub(crate) fn initialize_private(cache: &Path) -> io::Result<()> {
    native_build::ordinary_ancestors(cache)?;
    if fs::read_dir(cache)?.next().is_some() {
        return Err(invalid());
    }
    let path = cache.join("_config.db");
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .share_mode(3) // FILE_SHARE_READ | FILE_SHARE_WRITE, no namespace replacement.
        .open(&path)?;
    let api = Api::load()?;
    let connection = Connection::open(&api, &path, 2)?; // Existing owned file, READWRITE.
    connection.verify_open_file(&file)?;
    for sql in [
        c"CREATE TABLE config(key TEXT PRIMARY KEY,value TEXT)",
        c"INSERT INTO config VALUES ('auto_index','false'),('auto_watch','false')",
    ] {
        let query = connection.prepare(sql)?;
        if unsafe { (api.step)(query.pointer) } != 101 {
            return Err(invalid());
        }
    }
    drop(connection);
    file.sync_all()?;
    let mut ui = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(cache.join("config.json"))?;
    ui.write_all(b"{\"ui_enabled\":false}")?;
    ui.sync_all()?;
    Ok(())
}

/// Read a bounded coherent copy, including committed WAL data, without running
/// SQLite on the source cache. Busy locks, hot/persistent journals and WAL mode
/// without both ordinary sidecars are unavailable; no source setting is changed.
pub fn read(cache: &Path) -> io::Result<Configuration> {
    native_build::ordinary_ancestors(cache)?;
    let mut value = Configuration {
        auto_index: false,
        auto_watch: true,
        // The audited binary embeds UI assets: absent config.json enables UI.
        // SQLite's similarly named key is not consumed by cbm_ui_config_load.
        ui_enabled: true,
    };
    let path = cache.join("_config.db");
    match ReadGuard::open_shared(&path) {
        Ok(guard) => {
            let snapshot = Snapshot::capture(&guard, cache)?;
            drop(guard);
            let private = snapshot.materialize()?;
            let private_path = private.path().join("_config.db");
            let private_guard = ReadGuard::open_shared(&private_path)?;
            let api = Api::load()?;
            let connection = Connection::open(&api, &private_path, 1)?; // SQLITE_OPEN_READONLY
            connection.verify_file(&private_guard)?;
            let query = connection.prepare(
                c"SELECT key, value FROM config WHERE key IN ('auto_index','auto_watch')",
            )?;
            let mut seen = BTreeSet::new();
            loop {
                match unsafe { (api.step)(query.pointer) } {
                    101 => break, // SQLITE_DONE
                    100 => {
                        let key = query.text(0, 16)?;
                        if !seen.insert(key.clone()) {
                            return Err(invalid());
                        }
                        let setting = match query.text(1, 5)?.as_str() {
                            "true" => true,
                            "false" => false,
                            _ => return Err(invalid()),
                        };
                        match key.as_str() {
                            "auto_index" => value.auto_index = setting,
                            "auto_watch" => value.auto_watch = setting,
                            _ => return Err(invalid()),
                        }
                    }
                    _ => return Err(invalid()),
                }
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => (),
        Err(_) => return Err(invalid()),
    }
    match ReadGuard::open(&cache.join("config.json")) {
        Ok(mut guard) => {
            let mut bytes = Vec::new();
            (&mut guard.file)
                .take(64 * 1024 + 1)
                .read_to_end(&mut bytes)?;
            if bytes.len() > 64 * 1024 {
                return Err(invalid());
            }
            let json = crate::dependency_mcp_probe::strict_json(
                bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes),
            )
            .map_err(|_| invalid())?;
            let object = json.as_object().ok_or_else(invalid)?;
            value.ui_enabled = object
                .get("ui_enabled")
                .map(|value| value.as_bool().ok_or_else(invalid))
                .transpose()?
                .unwrap_or(false);
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => (),
        Err(_) => return Err(invalid()),
    }
    Ok(value)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::fs;

    pub(crate) fn policy_fixture(root: &Path, auto_watch: bool) {
        fs::write(root.join("config.json"), b"{\"ui_enabled\":false}").unwrap();
        let api = Api::load().unwrap();
        let connection = Connection::open(&api, &root.join("_config.db"), 6).unwrap();
        execute(
            &connection,
            "CREATE TABLE config(key TEXT PRIMARY KEY,value TEXT)",
        );
        execute(
            &connection,
            &format!(
                "INSERT INTO config VALUES ('auto_index','false'),('auto_watch','{auto_watch}'),('ui_enabled','false')"
            ),
        );
    }

    fn wal_fixture<'a>(api: &'a Api, root: &Path) -> Connection<'a> {
        fs::write(root.join("config.json"), b"{\"ui_enabled\":false}").unwrap();
        let connection = Connection::open(api, &root.join("_config.db"), 6).unwrap();
        execute(&connection, "PRAGMA journal_mode=WAL");
        execute(
            &connection,
            "CREATE TABLE config(key TEXT PRIMARY KEY,value TEXT)",
        );
        execute(
            &connection,
            "INSERT INTO config VALUES ('auto_watch','true')",
        );
        execute(&connection, "PRAGMA wal_checkpoint(TRUNCATE)");
        execute(
            &connection,
            "UPDATE config SET value='false' WHERE key='auto_watch'",
        );
        connection
    }

    // A closed, recovery-requiring WAL fixture. Copy an idle writer's committed
    // pair while it is still open, so closing it cannot checkpoint our fixture.
    fn detached_wal(root: &Path) {
        fs::write(root.join("config.json"), b"{\"ui_enabled\":false}").unwrap();
        let source = tempfile::tempdir().unwrap();
        let api = Api::load().unwrap();
        let _writer = wal_fixture(&api, source.path());
        for suffix in ["", "-wal"] {
            fs::copy(
                source.path().join(format!("_config.db{suffix}")),
                root.join(format!("_config.db{suffix}")),
            )
            .unwrap();
        }
        fs::write(root.join("_config.db-shm"), []).unwrap();
    }

    fn snapshot_values(snapshot: &Snapshot) -> Configuration {
        let directory = snapshot.materialize().unwrap();
        fs::write(
            directory.path().join("config.json"),
            b"{\"ui_enabled\":false}",
        )
        .unwrap();
        // capture() requires existing WAL sidecars. This fixture supplies the
        // ordinary empty SHM file so the private copy can reconstruct its index.
        if snapshot.wal.is_some() {
            fs::write(directory.path().join("_config.db-shm"), []).unwrap();
        }
        read(directory.path()).unwrap()
    }

    fn execute(connection: &Connection<'_>, sql: &str) {
        let sql = CString::new(sql).unwrap();
        let query = connection.prepare(&sql).unwrap();
        loop {
            match unsafe { (connection.api.step)(query.pointer) } {
                100 => (),
                101 => break,
                code => panic!("owned SQLite fixture returned {code}"),
            }
        }
    }

    #[test]
    fn absent_configuration_is_observed_without_creation() {
        let root = tempfile::tempdir().unwrap();
        let absent = root.path().join("absent");
        let observed = read(&absent).unwrap();
        assert_eq!(
            observed,
            Configuration {
                auto_index: false,
                auto_watch: true,
                ui_enabled: true
            }
        );
        assert!(!observed.bounded_policy_active());
        assert!(!absent.exists());
    }

    #[test]
    fn a_different_sqlite_file_cannot_satisfy_the_guarded_identity() {
        let root = tempfile::tempdir().unwrap();
        let api = Api::load().unwrap();
        let first = root.path().join("first.db");
        let second = root.path().join("second.db");
        for path in [&first, &second] {
            let connection = Connection::open(&api, path, 6).unwrap();
            execute(&connection, "CREATE TABLE config(key TEXT,value TEXT)");
        }
        let guard = ReadGuard::open_shared(&first).unwrap();
        let connection = Connection::open(&api, &second, 1).unwrap();
        assert!(connection.verify_file(&guard).is_err());
    }

    #[test]
    fn native_sqlite_reads_settings_and_json_ui_override_without_changing_values() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("_config.db");
        let api = Api::load().unwrap();
        {
            let connection = Connection::open(&api, &path, 6).unwrap();
            execute(
                &connection,
                "CREATE TABLE config(key TEXT PRIMARY KEY,value TEXT)",
            );
            execute(
                &connection,
                "INSERT INTO config VALUES ('auto_index','false'),('auto_watch','false'),('ui_enabled','true'),('unrelated','PRIVATE-IGNORE')",
            );
        }
        let original = fs::read(&path).unwrap();
        assert!(read(root.path()).unwrap().ui_enabled);
        fs::write(
            root.path().join("config.json"),
            b"\xef\xbb\xbf{\"ui_enabled\":false}",
        )
        .unwrap();
        assert!(read(root.path()).unwrap().bounded_policy_active());
        assert_eq!(fs::read(&path).unwrap(), original);
        fs::write(
            root.path().join("config.json"),
            b"{\"ui_enabled\":\"false\"}",
        )
        .unwrap();
        assert!(read(root.path()).is_err());
        assert_eq!(fs::read(&path).unwrap(), original);
        fs::write(
            root.path().join("config.json"),
            b"{\"ui_enabled\":true,\"ui_enabled\":false}",
        )
        .unwrap();
        assert!(read(root.path()).is_err());
        assert_eq!(fs::read(&path).unwrap(), original);
    }

    #[test]
    fn sqlite_ui_key_cannot_disable_the_embedded_ui_without_json_configuration() {
        let root = tempfile::tempdir().unwrap();
        policy_fixture(root.path(), false);
        fs::remove_file(root.path().join("config.json")).unwrap();
        let original = fs::read(root.path().join("_config.db")).unwrap();
        let observed = read(root.path()).unwrap();
        assert!(observed.ui_enabled);
        assert!(!observed.bounded_policy_active());
        assert_eq!(fs::read(root.path().join("_config.db")).unwrap(), original);
        assert!(!root.path().join("config.json").exists());
        // An existing empty object uses CBM_UI_DEFAULT_ENABLED=false.
        fs::write(root.path().join("config.json"), b"{}").unwrap();
        assert!(read(root.path()).unwrap().bounded_policy_active());
    }

    #[test]
    fn private_probe_initialization_disables_defaults_and_refuses_existing_inputs() {
        let root = tempfile::tempdir().unwrap();
        initialize_private(root.path()).unwrap();
        assert!(read(root.path()).unwrap().bounded_policy_active());
        let before = fs::read(root.path().join("_config.db")).unwrap();
        assert!(initialize_private(root.path()).is_err());
        assert_eq!(fs::read(root.path().join("_config.db")).unwrap(), before);
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 2);
        let foreign = tempfile::tempdir().unwrap();
        fs::write(foreign.path().join("sentinel"), b"KEEP").unwrap();
        assert!(initialize_private(foreign.path()).is_err());
        assert_eq!(fs::read_dir(foreign.path()).unwrap().count(), 1);
        assert_eq!(fs::read(foreign.path().join("sentinel")).unwrap(), b"KEEP");
    }

    #[test]
    fn committed_wal_settings_are_not_replaced_by_a_stale_main_file() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("config.json"), b"{\"ui_enabled\":false}").unwrap();
        let path = root.path().join("_config.db");
        let api = Api::load().unwrap();
        let connection = Connection::open(&api, &path, 6).unwrap();
        execute(&connection, "PRAGMA journal_mode=WAL");
        execute(
            &connection,
            "CREATE TABLE config(key TEXT PRIMARY KEY,value TEXT)",
        );
        execute(
            &connection,
            "INSERT INTO config VALUES ('auto_watch','true')",
        );
        execute(&connection, "PRAGMA wal_checkpoint(TRUNCATE)");
        execute(
            &connection,
            "UPDATE config SET value='false' WHERE key='auto_watch'",
        );
        assert!(root.path().join("_config.db-wal").metadata().unwrap().len() > 0);
        assert!(read(root.path()).unwrap().bounded_policy_active());
    }

    #[test]
    fn shm_and_wal_aliases_preserve_external_sentinels() {
        for suffix in ["-shm", "-wal"] {
            for symbolic in [false, true] {
                let root = tempfile::tempdir().unwrap();
                let external = tempfile::tempdir().unwrap();
                detached_wal(root.path());
                let sentinel = external.path().join("sentinel");
                let original = b"PRIVATE FOREIGN CONTENT MUST SURVIVE";
                fs::write(&sentinel, original).unwrap();
                let alias = root.path().join(format!("_config.db{suffix}"));
                fs::remove_file(&alias).unwrap();
                if symbolic {
                    std::os::windows::fs::symlink_file(&sentinel, &alias).unwrap();
                } else {
                    fs::hard_link(&sentinel, &alias).unwrap();
                }
                let main = fs::read(root.path().join("_config.db")).unwrap();
                assert_eq!(
                    read(root.path()).unwrap_err().to_string(),
                    invalid().to_string()
                );
                assert_eq!(fs::read(&sentinel).unwrap(), original);
                assert_eq!(fs::read(root.path().join("_config.db")).unwrap(), main);
                fs::remove_file(alias).unwrap();
            }
        }
    }

    #[test]
    fn missing_wal_sidecars_fail_without_creation() {
        for suffix in ["-shm", "-wal"] {
            let root = tempfile::tempdir().unwrap();
            detached_wal(root.path());
            let missing = root.path().join(format!("_config.db{suffix}"));
            fs::remove_file(&missing).unwrap();
            assert!(read(root.path()).is_err());
            assert!(!missing.exists());
        }
    }

    #[test]
    fn source_redirect_after_capture_cannot_reach_sqlite() {
        let root = tempfile::tempdir().unwrap();
        let external = tempfile::tempdir().unwrap();
        detached_wal(root.path());
        let guard = ReadGuard::open_shared(&root.path().join("_config.db")).unwrap();
        let snapshot = Snapshot::capture(&guard, root.path()).unwrap();
        let sentinel = external.path().join("sentinel");
        fs::write(&sentinel, b"KEEP").unwrap();
        let shm = root.path().join("_config.db-shm");
        fs::remove_file(&shm).unwrap();
        std::os::windows::fs::symlink_file(&sentinel, &shm).unwrap();
        assert!(snapshot_values(&snapshot).bounded_policy_active());
        assert_eq!(fs::read(&sentinel).unwrap(), b"KEEP");
        fs::remove_file(shm).unwrap();
    }

    #[test]
    fn recovery_uses_bounded_wal_and_preserves_original_files() {
        let root = tempfile::tempdir().unwrap();
        detached_wal(root.path());
        let paths = ["_config.db", "_config.db-wal", "_config.db-shm"];
        let before = paths.map(|name| fs::read(root.path().join(name)).unwrap());
        assert!(read(root.path()).unwrap().bounded_policy_active());
        for (name, original) in paths.into_iter().zip(before) {
            assert_eq!(fs::read(root.path().join(name)).unwrap(), original);
        }
        let wal = fs::OpenOptions::new()
            .write(true)
            .open(root.path().join("_config.db-wal"))
            .unwrap();
        // Exact cap is accepted; invalid padding terminates WAL recovery after
        // the valid committed frames. One additional byte must fail before SQL.
        wal.set_len(MAX_WAL_BYTES).unwrap();
        assert!(read(root.path()).unwrap().bounded_policy_active());
        wal.set_len(MAX_WAL_BYTES + 1).unwrap();
        let started = Instant::now();
        assert_eq!(
            read(root.path()).unwrap_err().to_string(),
            invalid().to_string()
        );
        assert!(started.elapsed() < Duration::from_secs(2));
        assert_eq!(wal.metadata().unwrap().len(), MAX_WAL_BYTES + 1);
        assert!(
            fs::read(root.path().join("_config.db-shm"))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn oversized_valid_committed_wal_fails_before_recovery() {
        let root = tempfile::tempdir().unwrap();
        let api = Api::load().unwrap();
        let writer = wal_fixture(&api, root.path());
        execute(&writer, "PRAGMA wal_autocheckpoint=0");
        execute(&writer, "CREATE TABLE padding(value BLOB)");
        execute(
            &writer,
            "WITH RECURSIVE rows(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM rows WHERE x<600) INSERT INTO padding SELECT zeroblob(16384) FROM rows",
        );
        let main = fs::read(root.path().join("_config.db")).unwrap();
        let wal_path = root.path().join("_config.db-wal");
        let wal = fs::read(&wal_path).unwrap();
        assert!(wal.len() as u64 > MAX_WAL_BYTES);
        assert!(main.len() as u64 <= MAX_DATABASE_BYTES);
        assert!(read(root.path()).is_err());
        assert_eq!(fs::read(root.path().join("_config.db")).unwrap(), main);
        assert_eq!(fs::read(&wal_path).unwrap(), wal);
    }

    #[test]
    fn copy_locks_conflict_with_actual_sqlite_writer_and_checkpoint() {
        let root = tempfile::tempdir().unwrap();
        let api = Api::load().unwrap();
        let writer = wal_fixture(&api, root.path());
        assert_eq!(unsafe { (api.busy_timeout)(writer.pointer, 0) }, 0);
        let shm = ReadGuard::open_shared(&root.path().join("_config.db-shm")).unwrap();
        let locks = ByteLock::acquire(&shm.file, 120, 3, true).unwrap();
        {
            let query = writer.prepare(c"BEGIN IMMEDIATE").unwrap();
            assert_eq!(unsafe { (api.step)(query.pointer) }, 5); // SQLITE_BUSY
        }
        {
            let query = writer.prepare(c"PRAGMA wal_checkpoint(TRUNCATE)").unwrap();
            assert_eq!(unsafe { (api.step)(query.pointer) }, 100);
            let busy = unsafe { (api.text)(query.pointer, 0) };
            assert!(!busy.is_null());
            assert_eq!(unsafe { CStr::from_ptr(busy.cast()) }.to_bytes(), b"1");
        }
        drop(locks);
        execute(
            &writer,
            "UPDATE config SET value='true' WHERE key='auto_watch'",
        );
        execute(&writer, "PRAGMA wal_checkpoint(TRUNCATE)");
        assert!(read(root.path()).unwrap().auto_watch);
    }

    #[test]
    fn wal_write_checkpoint_and_recovery_locks_fail_closed_and_release() {
        let root = tempfile::tempdir().unwrap();
        let api = Api::load().unwrap();
        let writer = wal_fixture(&api, root.path());
        let shm = ReadGuard::open_shared(&root.path().join("_config.db-shm")).unwrap();
        for offset in [120, 121, 122] {
            let locked = ByteLock::acquire(&shm.file, offset, 1, true).unwrap();
            assert!(read(root.path()).is_err());
            drop(locked);
            assert!(read(root.path()).unwrap().bounded_policy_active());
        }
        execute(&writer, "BEGIN IMMEDIATE");
        execute(
            &writer,
            "UPDATE config SET value='true' WHERE key='auto_watch'",
        );
        assert!(read(root.path()).is_err());
        execute(&writer, "ROLLBACK");
        assert!(read(root.path()).unwrap().bounded_policy_active());
        let detached = tempfile::tempdir().unwrap();
        detached_wal(detached.path());
        let detached_shm = ReadGuard::open_shared(&detached.path().join("_config.db-shm")).unwrap();
        let _initializing = ByteLock::acquire(&detached_shm.file, 128, 1, true).unwrap();
        assert!(read(detached.path()).is_err());
    }

    #[test]
    fn snapshot_is_committed_and_isolated_from_later_writer_and_checkpoint() {
        let root = tempfile::tempdir().unwrap();
        let api = Api::load().unwrap();
        let writer = wal_fixture(&api, root.path());
        let guard = ReadGuard::open_shared(&root.path().join("_config.db")).unwrap();
        let snapshot = Snapshot::capture(&guard, root.path()).unwrap();
        execute(
            &writer,
            "UPDATE config SET value='true' WHERE key='auto_watch'",
        );
        execute(&writer, "PRAGMA wal_checkpoint(TRUNCATE)");
        assert!(snapshot_values(&snapshot).bounded_policy_active());
        assert!(read(root.path()).unwrap().auto_watch);
    }

    #[test]
    fn rollback_journal_and_exclusive_writer_are_unavailable() {
        let root = tempfile::tempdir().unwrap();
        let api = Api::load().unwrap();
        let writer = Connection::open(&api, &root.path().join("_config.db"), 6).unwrap();
        execute(&writer, "CREATE TABLE config(key TEXT,value TEXT)");
        execute(&writer, "BEGIN EXCLUSIVE");
        assert!(read(root.path()).is_err());
        execute(&writer, "ROLLBACK");
        assert!(read(root.path()).is_ok());
        let journal = root.path().join("_config.db-journal");
        fs::write(&journal, b"UNKNOWN JOURNAL").unwrap();
        assert!(read(root.path()).is_err());
        assert_eq!(fs::read(&journal).unwrap(), b"UNKNOWN JOURNAL");
    }

    #[test]
    fn malformed_schema_values_and_recursive_views_fail_with_fixed_private_errors() {
        for statement in [
            "CREATE TABLE wrong(key TEXT,value TEXT)",
            "CREATE VIEW config AS WITH RECURSIVE loop(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM loop) SELECT 'auto_watch' AS key, printf('%d',x) AS value FROM loop WHERE x<0",
        ] {
            let root = tempfile::tempdir().unwrap();
            let path = root.path().join("_config.db");
            let api = Api::load().unwrap();
            {
                let connection = Connection::open(&api, &path, 6).unwrap();
                execute(&connection, statement);
            }
            let started = Instant::now();
            assert_eq!(
                read(root.path()).unwrap_err().to_string(),
                invalid().to_string()
            );
            assert!(started.elapsed() < Duration::from_secs(5));
        }
    }
}
