//! Read-only native Codex sampling evidence, including ephemeral title threads.
//! Uses the installed Windows SQLite ABI (Windows SDK winsqlite3.h), not Python.
use serde_json::{Value, json};
use std::{
    ffi::{CStr, CString, c_char, c_int, c_void},
    io,
    path::Path,
    ptr::{null, null_mut},
    time::{Duration, Instant},
};

#[link(name = "winsqlite3")]
unsafe extern "system" {
    fn sqlite3_open_v2(
        name: *const c_char,
        db: *mut *mut c_void,
        flags: c_int,
        vfs: *const c_char,
    ) -> c_int;
    fn sqlite3_close(db: *mut c_void) -> c_int;
    fn sqlite3_prepare_v2(
        db: *mut c_void,
        sql: *const c_char,
        length: c_int,
        stmt: *mut *mut c_void,
        tail: *mut *const c_char,
    ) -> c_int;
    fn sqlite3_finalize(stmt: *mut c_void) -> c_int;
    fn sqlite3_step(stmt: *mut c_void) -> c_int;
    fn sqlite3_column_int64(stmt: *mut c_void, column: c_int) -> i64;
    fn sqlite3_column_text(stmt: *mut c_void, column: c_int) -> *const u8;
    fn sqlite3_column_bytes(stmt: *mut c_void, column: c_int) -> c_int;
    fn sqlite3_limit(db: *mut c_void, id: c_int, value: c_int) -> c_int;
    fn sqlite3_progress_handler(
        db: *mut c_void,
        instructions: c_int,
        callback: unsafe extern "system" fn(*mut c_void) -> c_int,
        context: *mut c_void,
    );
    fn sqlite3_errmsg(db: *mut c_void) -> *const c_char;
}

struct Database(*mut c_void);
impl Drop for Database {
    fn drop(&mut self) {
        unsafe {
            sqlite3_progress_handler(self.0, 0, elapsed, null_mut());
            sqlite3_close(self.0);
        }
    }
}
struct Statement(*mut c_void);
impl Drop for Statement {
    fn drop(&mut self) {
        unsafe {
            sqlite3_finalize(self.0);
        }
    }
}

fn failure(db: &Database) -> io::Error {
    let error = unsafe { CStr::from_ptr(sqlite3_errmsg(db.0)) }.to_string_lossy();
    io::Error::other(format!("native sampling query failed: {error}"))
}

unsafe extern "system" fn elapsed(context: *mut c_void) -> c_int {
    let started = unsafe { &*context.cast::<Instant>() };
    i32::from(started.elapsed() > Duration::from_secs(5))
}

pub fn read(database: &Path) -> io::Result<Vec<Value>> {
    if !database.is_absolute() || !database.is_file() {
        return Err(io::Error::other("native sampling database is unavailable"));
    }
    let name = CString::new(
        database
            .to_str()
            .ok_or_else(|| io::Error::other("database path is not UTF-8"))?,
    )?;
    let mut raw = null_mut();
    let status = unsafe { sqlite3_open_v2(name.as_ptr(), &mut raw, 1, null()) }; // READONLY, never create
    if raw.is_null() {
        return Err(io::Error::other("SQLite allocation/open failed"));
    }
    let db = Database(raw);
    if status != 0 {
        return Err(failure(&db));
    }
    let mut started = Instant::now();
    unsafe {
        sqlite3_limit(db.0, 0, 1024 * 1024); // maximum single value/row length
        sqlite3_progress_handler(db.0, 1000, elapsed, (&mut started as *mut Instant).cast());
    }
    let mut raw_stmt = null_mut();
    let status = unsafe {
        sqlite3_prepare_v2(db.0,
            c"select id,ts,ts_nanos,feedback_log_body from logs where target='feedback_tags' order by id".as_ptr(),
            -1, &mut raw_stmt, null_mut())
    };
    if status != 0 {
        return Err(failure(&db));
    }
    let stmt = Statement(raw_stmt);
    let mut result = Vec::new();
    let mut bytes_seen = 0usize;
    loop {
        match unsafe { sqlite3_step(stmt.0) } {
            101 => break,
            100 => {}
            _ => return Err(failure(&db)),
        }
        let text = unsafe { sqlite3_column_text(stmt.0, 3) };
        let count = unsafe { sqlite3_column_bytes(stmt.0, 3) };
        if text.is_null() || count < 0 {
            return Err(io::Error::other("sampling row is not text"));
        }
        bytes_seen += count as usize;
        if bytes_seen > 4 * 1024 * 1024 || result.len() >= 4096 {
            return Err(io::Error::other(
                "native sampling evidence exceeds its bound",
            ));
        }
        let message =
            std::str::from_utf8(unsafe { std::slice::from_raw_parts(text, count as usize) })
                .map_err(io::Error::other)?;
        if let Some(mut record) = parse(message)? {
            record["log_id"] = unsafe { sqlite3_column_int64(stmt.0, 0) }.into();
            record["unix_seconds"] = unsafe { sqlite3_column_int64(stmt.0, 1) }.into();
            record["unix_nanos"] = unsafe { sqlite3_column_int64(stmt.0, 2) }.into();
            result.push(record);
        }
    }
    Ok(result)
}

fn parse(message: &str) -> io::Result<Option<Value>> {
    let Some((_, model)) = message.split_once("model=\"") else {
        return Ok(None);
    };
    let model = model
        .split_once('"')
        .ok_or_else(|| io::Error::other("unclosed sampling model"))?
        .0;
    let Some((_, features)) = message.split_once("features=[") else {
        return Ok(None);
    };
    let features = features
        .split_once(']')
        .ok_or_else(|| io::Error::other("unclosed sampling features"))?
        .0;
    let context = features
        .split(',')
        .any(|flag| flag.trim().trim_matches('"') == "ContextManagement");
    Ok(Some(json!({"model":model,"context_management":context})))
}
