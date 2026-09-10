//! Bounded SQLite snapshots through Windows' existing WinSQLite3 runtime.
//! Backup is a separate destination transaction; failed copies never replace a
//! committed generation. The provider decides when a completed refresh may be
//! published; a consistent SQLite snapshot alone is not a source-freshness proof.
#![cfg(windows)]
use crate::process::{Cancellation, Deadline};
use std::{
    ffi::{CString, c_char, c_int, c_void},
    fs, io,
    path::Path,
    ptr,
    time::Duration,
};

#[link(name = "winsqlite3")]
unsafe extern "C" {
    fn sqlite3_open_v2(
        name: *const c_char,
        db: *mut *mut c_void,
        flags: c_int,
        vfs: *const c_char,
    ) -> c_int;
    fn sqlite3_close(db: *mut c_void) -> c_int;
    fn sqlite3_backup_init(
        dst: *mut c_void,
        dst_name: *const c_char,
        src: *mut c_void,
        src_name: *const c_char,
    ) -> *mut c_void;
    fn sqlite3_backup_step(backup: *mut c_void, pages: c_int) -> c_int;
    fn sqlite3_backup_finish(backup: *mut c_void) -> c_int;
    fn sqlite3_backup_pagecount(backup: *mut c_void) -> c_int;
    fn sqlite3_prepare_v2(
        db: *mut c_void,
        sql: *const c_char,
        len: c_int,
        statement: *mut *mut c_void,
        tail: *mut *const c_char,
    ) -> c_int;
    fn sqlite3_step(statement: *mut c_void) -> c_int;
    fn sqlite3_column_int64(statement: *mut c_void, column: c_int) -> i64;
    fn sqlite3_column_text(statement: *mut c_void, column: c_int) -> *const u8;
    fn sqlite3_column_bytes(statement: *mut c_void, column: c_int) -> c_int;
    fn sqlite3_finalize(statement: *mut c_void) -> c_int;
    fn sqlite3_exec(
        db: *mut c_void,
        sql: *const c_char,
        callback: *const c_void,
        arg: *mut c_void,
        error: *mut *mut c_char,
    ) -> c_int;
    fn sqlite3_free(memory: *mut c_void);
    fn sqlite3_progress_handler(
        db: *mut c_void,
        steps: c_int,
        callback: Option<unsafe extern "C" fn(*mut c_void) -> c_int>,
        context: *mut c_void,
    );
}
const OK: c_int = 0;
const DONE: c_int = 101;
const ROW: c_int = 100;
const BUSY: c_int = 5;
const LOCKED: c_int = 6;
const INTERRUPT: c_int = 9;
const MAX_INDEXED_FILES: usize = 100_000;
const MAX_INDEXED_PATH_BYTES: usize = 32 * 1024 * 1024;

fn failed(operation: &str, code: c_int) -> io::Error {
    io::Error::other(format!("CodeGraph SQLite {operation} failed (code {code})"))
}

struct Database(*mut c_void);
impl Database {
    fn open(path: &Path, write: bool) -> io::Result<Self> {
        let name = CString::new(
            path.to_str()
                .ok_or_else(|| io::Error::other("SQLite path is not UTF-8"))?,
        )
        .map_err(io::Error::other)?;
        let mut db = ptr::null_mut();
        let code = unsafe {
            sqlite3_open_v2(
                name.as_ptr(),
                &mut db,
                if write { 0x0002 | 0x0004 } else { 0x0001 },
                ptr::null(),
            )
        };
        let database = Self(db);
        if code != OK {
            return Err(failed("open", code));
        }
        Ok(database)
    }
    fn scalar(&self, sql: &str) -> io::Result<u64> {
        let text = CString::new(sql).map_err(io::Error::other)?;
        let mut statement = ptr::null_mut();
        let code = unsafe {
            sqlite3_prepare_v2(self.0, text.as_ptr(), -1, &mut statement, ptr::null_mut())
        };
        let statement = Statement(statement);
        if code != OK {
            return Err(failed("prepare", code));
        }
        let code = unsafe { sqlite3_step(statement.0) };
        if code != ROW {
            return Err(failed("step", code));
        }
        u64::try_from(unsafe { sqlite3_column_int64(statement.0, 0) })
            .map_err(|_| io::Error::other("negative SQLite count"))
    }
    fn exec(&self, sql: &str) -> io::Result<()> {
        let text = CString::new(sql).map_err(io::Error::other)?;
        let mut message = ptr::null_mut();
        let code = unsafe {
            sqlite3_exec(
                self.0,
                text.as_ptr(),
                ptr::null(),
                ptr::null_mut(),
                &mut message,
            )
        };
        if !message.is_null() {
            unsafe { sqlite3_free(message.cast()) };
        }
        if code != OK {
            return Err(failed("exec", code));
        }
        Ok(())
    }
}
impl Drop for Database {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                sqlite3_close(self.0);
            }
        }
    }
}
struct Statement(*mut c_void);
impl Drop for Statement {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                sqlite3_finalize(self.0);
            }
        }
    }
}
struct Backup(*mut c_void);
impl Drop for Backup {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                sqlite3_backup_finish(self.0);
            }
        }
    }
}

fn ordinary(path: &Path) -> io::Result<()> {
    use std::os::windows::fs::MetadataExt;
    let mut current = Some(path);
    while let Some(path) = current {
        let meta = fs::symlink_metadata(path)?;
        if meta.file_attributes()
            & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT
            != 0
        {
            return Err(io::Error::other(
                "CodeGraph storage path contains a link/reparse point",
            ));
        }
        current = path.parent();
    }
    Ok(())
}

pub fn free_bytes(directory: &Path) -> io::Result<u64> {
    use std::os::windows::ffi::OsStrExt;
    ordinary(directory)?;
    let path: Vec<u16> = directory.as_os_str().encode_wide().chain([0]).collect();
    let mut available = 0;
    if unsafe {
        windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW(
            path.as_ptr(),
            &mut available,
            ptr::null_mut(),
            ptr::null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(available)
}

/// Native accounting against SQLite itself, independent of upstream text. This
/// does not establish extraction completeness or matching on-disk source hashes.
pub fn counts(database: &Path) -> io::Result<serde_json::Value> {
    ordinary(database)?;
    let db = Database::open(database, false)?;
    Ok(
        serde_json::json!({"files":db.scalar("SELECT count(*) FROM files")?,"nodes":db.scalar("SELECT count(*) FROM nodes")?,"edges":db.scalar("SELECT count(*) FROM edges")?}),
    )
}

struct Stop {
    deadline: Deadline,
    cancel: Cancellation,
}

unsafe extern "C" fn interrupted(context: *mut c_void) -> c_int {
    let stop = unsafe { &*(context.cast::<Stop>()) };
    i32::from(stop.deadline.expired() || stop.cancel.is_cancelled())
}

fn attach_progress(db: &Database, stop: &mut Stop) {
    unsafe {
        sqlite3_progress_handler(db.0, 1000, Some(interrupted), (stop as *mut Stop).cast());
    }
}

fn stopped(deadline: Deadline, cancel: &Cancellation) -> io::Result<()> {
    if cancel.is_cancelled() {
        return Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "CodeGraph inventory cancelled",
        ));
    }
    if deadline.expired() {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "CodeGraph inventory deadline exceeded",
        ));
    }
    Ok(())
}

fn column_text(statement: *mut c_void, column: c_int) -> io::Result<String> {
    let pointer = unsafe { sqlite3_column_text(statement, column) };
    let count = unsafe { sqlite3_column_bytes(statement, column) };
    if pointer.is_null() || count < 0 {
        return Err(io::Error::other("CodeGraph files.path is not text"));
    }
    let bytes = unsafe { std::slice::from_raw_parts(pointer, count as usize) };
    std::str::from_utf8(bytes)
        .map(str::to_owned)
        .map_err(io::Error::other)
}

/// Read-only inventory of extracted `files.path` values from a published 1.6.0
/// database. Paths are returned as stored; callers normalize. Bounded to 100k
/// rows and 32 MiB of UTF-8. No arbitrary SQL API.
pub fn indexed_files(
    database: &Path,
    deadline: Deadline,
    cancel: &Cancellation,
) -> io::Result<Vec<String>> {
    read_paths(
        database,
        "SELECT path FROM files ORDER BY path",
        deadline,
        cancel,
    )
}

/// Bounded read-only source locations for an exact symbol in a committed graph.
/// Used to verify automatic refresh without a provider query triggering work.
pub fn symbol_files(
    database: &Path,
    symbol: &str,
    deadline: Deadline,
    cancel: &Cancellation,
) -> io::Result<Vec<String>> {
    if symbol.len() > 1024 || symbol.contains('\0') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid exact symbol",
        ));
    }
    read_paths(
        database,
        &format!(
            "SELECT DISTINCT file_path FROM nodes WHERE name='{}' ORDER BY file_path",
            symbol.replace('\'', "''")
        ),
        deadline,
        cancel,
    )
}

fn read_paths(
    database: &Path,
    query: &str,
    deadline: Deadline,
    cancel: &Cancellation,
) -> io::Result<Vec<String>> {
    ordinary(database)?;
    stopped(deadline, cancel)?;
    let mut stop = Stop {
        deadline,
        cancel: cancel.clone(),
    };
    let db = Database::open(database, false)?;
    attach_progress(&db, &mut stop);
    let sql = CString::new(query).map_err(io::Error::other)?;
    let mut statement = ptr::null_mut();
    let code =
        unsafe { sqlite3_prepare_v2(db.0, sql.as_ptr(), -1, &mut statement, ptr::null_mut()) };
    let statement = Statement(statement);
    if code != OK {
        return Err(failed("prepare", code));
    }
    let mut paths = Vec::new();
    let mut bytes = 0usize;
    loop {
        stopped(deadline, cancel)?;
        let code = unsafe { sqlite3_step(statement.0) };
        match code {
            ROW => {
                let path = column_text(statement.0, 0)?;
                bytes = bytes
                    .checked_add(path.len())
                    .ok_or_else(|| io::Error::other("CodeGraph inventory size overflow"))?;
                if paths.len() >= MAX_INDEXED_FILES || bytes > MAX_INDEXED_PATH_BYTES {
                    return Err(io::Error::other(
                        "CodeGraph inventory exceeded 100000 paths or 32MiB",
                    ));
                }
                paths.push(path);
            }
            DONE => break,
            INTERRUPT => {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "CodeGraph inventory cancelled or expired",
                ));
            }
            BUSY | LOCKED => {
                std::thread::sleep(Duration::from_millis(20).min(deadline.remaining()))
            }
            code => return Err(failed("step", code)),
        }
    }
    Ok(paths)
}

/// A stopped published 1.6.0 database must be structurally readable and have no
/// never-attempted resolution work before the owner can publish a checkpoint.
/// This is not proof that every language or semantic edge is supported.
pub fn validate_completion(
    database: &Path,
    deadline: Deadline,
    cancel: &Cancellation,
) -> io::Result<serde_json::Value> {
    ordinary(database)?;
    let mut stop = Stop {
        deadline,
        cancel: cancel.clone(),
    };
    let db = Database::open(database, false)?;
    attach_progress(&db, &mut stop);
    if deadline.expired() || cancel.is_cancelled() {
        return Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "CodeGraph completion check cancelled or expired",
        ));
    }
    let damage = db.scalar("SELECT count(*) FROM pragma_quick_check WHERE quick_check <> 'ok'")?;
    let pending = db.scalar("SELECT count(*) FROM unresolved_refs WHERE status = 'pending'")?;
    if damage != 0 || pending != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "CodeGraph index is incomplete: {damage} integrity errors, {pending} pending references"
            ),
        ));
    }
    Ok(
        serde_json::json!({"files":db.scalar("SELECT count(*) FROM files")?,
        "nodes":db.scalar("SELECT count(*) FROM nodes")?,"edges":db.scalar("SELECT count(*) FROM edges")?,
        "pending_references":pending,"integrity_errors":damage}),
    )
}

/// Execute SQL against a new or existing owned database. Tests and generation
/// fixtures use this instead of a foreign-language SQLite bridge.
pub fn exec(database: &Path, sql: &str) -> io::Result<()> {
    let parent = database
        .parent()
        .ok_or_else(|| io::Error::other("SQLite database needs an owned parent"))?;
    ordinary(parent)?;
    if !database.is_absolute() {
        return Err(io::Error::other("SQLite database path must be absolute"));
    }
    if database.exists() {
        ordinary(database)?;
    } else {
        ordinary(parent)?;
    }
    Database::open(database, true)?.exec(sql)
}

/// Create a new destination only. Existing files and linked ancestors are
/// rejected. max_bytes bounds the source's logical database; reserve_bytes is
/// checked before and during page copying. No backend query or rebuild runs.
pub fn snapshot(
    source: &Path,
    destination: &Path,
    max_bytes: u64,
    reserve_bytes: u64,
    deadline: Deadline,
    cancel: &Cancellation,
) -> io::Result<u64> {
    ordinary(source)?;
    let parent = destination
        .parent()
        .ok_or_else(|| io::Error::other("snapshot destination needs an owned parent"))?;
    ordinary(parent)?;
    if !destination.is_absolute() || destination.exists() {
        return Err(io::Error::other(
            "snapshot destination must be a new absolute owned file",
        ));
    }
    let source_db = Database::open(source, false)?;
    let pages = source_db.scalar("PRAGMA page_count")?;
    let page_size = source_db.scalar("PRAGMA page_size")?;
    let size = pages
        .checked_mul(page_size)
        .ok_or_else(|| io::Error::other("SQLite size overflow"))?;
    if size > max_bytes || free_bytes(parent)? < reserve_bytes.saturating_add(size) {
        return Err(io::Error::other(
            "CodeGraph snapshot storage allowance/free-space reserve exceeded",
        ));
    }
    if deadline.expired() || cancel.is_cancelled() {
        return Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "CodeGraph snapshot cancelled before creation",
        ));
    }
    drop(
        fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(destination)?,
    );
    // Failed destinations are retained for owned recovery inspection; callers
    // must not publish them merely because the file exists.
    let target = Database::open(destination, true)?;
    let pointer =
        unsafe { sqlite3_backup_init(target.0, c"main".as_ptr(), source_db.0, c"main".as_ptr()) };
    if pointer.is_null() {
        return Err(io::Error::other("CodeGraph SQLite backup could not start"));
    }
    let mut backup = Backup(pointer);
    loop {
        if cancel.is_cancelled() {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "CodeGraph snapshot cancelled",
            ));
        }
        if deadline.expired() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "CodeGraph snapshot deadline exceeded",
            ));
        }
        if free_bytes(parent)? < reserve_bytes {
            return Err(io::Error::other(
                "CodeGraph snapshot free-space reserve reached",
            ));
        }
        let code = unsafe { sqlite3_backup_step(backup.0, 128) };
        let copied_size =
            (unsafe { sqlite3_backup_pagecount(backup.0) } as u64).saturating_mul(page_size);
        if copied_size > max_bytes {
            return Err(io::Error::other(
                "CodeGraph snapshot grew beyond its storage allowance",
            ));
        }
        match code {
            DONE => break,
            OK => {}
            BUSY | LOCKED => {
                std::thread::sleep(Duration::from_millis(20).min(deadline.remaining()))
            }
            code => return Err(failed("backup step", code)),
        }
    }
    let code = unsafe { sqlite3_backup_finish(backup.0) };
    backup.0 = ptr::null_mut();
    if code != OK {
        return Err(failed("backup finish", code));
    }
    drop(target);
    fs::OpenOptions::new()
        .write(true)
        .open(destination)?
        .sync_all()?;
    Ok(fs::metadata(destination)?.len())
}
