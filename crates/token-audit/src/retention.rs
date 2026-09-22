//! Bounded local retention of complete same-scan reports and bounded detail
//! reads over them.
//!
//! The interactive text presentation is bounded, so the complete machine
//! report of the same scan is retained once under the resolved Codex home at
//! `harness/token-audit/reports`. Retention keeps the newest
//! [`RETENTION_LIMIT`] files per kind; an evicted or expired record is an
//! explicit error. A detail read selects records from a retained file only:
//! it never rescans rollout sessions, calls a model or touches the network.
//!
//! Every run's locator carries that run's unique creation identity, so a name
//! is never reused: an evicted locator stays missing instead of resolving to a
//! later scan, and the scan timestamp lives only inside the record.

use serde_json::Value;
use std::{
    fs::{self, OpenOptions},
    io,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

/// Newest retained files kept per kind (`report` and `findings`).
pub const RETENTION_LIMIT: usize = 20;

/// Create-new attempts before a locator conflict is reported.
const IDENTITY_ATTEMPTS: u32 = 1000;

/// Digits of a zero-padded identity: epoch seconds plus nanoseconds.
const IDENTITY_WIDTH: usize = 19;

/// Unique identity of one retained run: epoch nanoseconds. Names sort by
/// identity, so retention order is creation order and an evicted name is never
/// reused.
fn run_identity() -> u64 {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    elapsed
        .as_secs()
        .saturating_mul(1_000_000_000)
        .saturating_add(u64::from(elapsed.subsec_nanos()))
}

/// Locator file name carrying one run's unique identity.
fn locator_name(kind: Kind, identity: u64) -> String {
    format!(
        "{}-{identity:0width$}.json",
        kind.name(),
        width = IDENTITY_WIDTH
    )
}

/// Kind of complete same-scan JSON retained for detail reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Report,
    Findings,
}

impl Kind {
    /// Command that produces this kind of presentation.
    pub fn name(self) -> &'static str {
        match self {
            Self::Report => "report",
            Self::Findings => "findings",
        }
    }

    fn record_array(self) -> &'static str {
        match self {
            Self::Report => "sessions",
            Self::Findings => "findings",
        }
    }

    fn record_field(self) -> &'static str {
        match self {
            Self::Report => "session_id",
            Self::Findings => "id",
        }
    }

    fn record_label(self) -> &'static str {
        match self {
            Self::Report => "session",
            Self::Findings => "finding",
        }
    }
}

/// Where the complete same-scan detail of one interactive presentation lives.
pub enum Detail {
    /// Complete same-scan JSON retained at this local path.
    Retained {
        path: PathBuf,
        /// Non-fatal retention error, for example a lock on an evicted file.
        warning: Option<String>,
    },
    /// Retention was refused; the presentation states the reason.
    Unavailable(String),
}

impl Detail {
    /// Presentation line naming the locator or the retention failure.
    pub fn note(&self, kind: Kind) -> String {
        match self {
            Self::Retained { path, warning } => {
                let mut note = format!(
                    "retained {}  (complete JSON; one record: token-audit detail --{} {} --{} ID)",
                    path.display(),
                    kind.name(),
                    path.display(),
                    kind.record_label()
                );
                if let Some(warning) = warning {
                    note.push_str(&format!("\nretention warning: {warning}"));
                }
                note
            }
            Self::Unavailable(reason) => format!("retained unavailable: {reason}"),
        }
    }
}

/// Default retention directory under the resolved Codex home.
///
/// This matches the session-root convention in `report::default_sessions_root`
/// and `harness_core::native_launcher::codex_home`: `CODEX_HOME` when set,
/// otherwise `USERPROFILE/.codex`. Existing baseline storage keeps its own
/// recorded path and is deliberately not migrated here.
pub fn default_directory() -> Option<PathBuf> {
    std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(|home| PathBuf::from(home).join(".codex")))
        .map(|root| root.join("harness").join("token-audit").join("reports"))
}

/// Writes one complete same-scan JSON record and prunes older files of the
/// same kind beyond [`RETENTION_LIMIT`]. Each run reserves its own create-new
/// locator named by that run's unique identity, so no run overwrites another
/// run's record. Pruning only ever removes files whose name this
/// module created; unrelated files stay untouched.
pub fn retain(directory: &Path, kind: Kind, json: &str) -> io::Result<Detail> {
    fs::create_dir_all(directory)?;
    let path = reserve(directory, kind)?;
    if let Err(error) = write_complete(&path, json) {
        let _ = fs::remove_file(&path);
        return Err(error);
    }
    let warning = prune(directory, kind, &path)?;
    Ok(Detail::Retained { path, warning })
}

/// Reserves this run's own create-new locator. A concurrent run that already
/// holds the identity steps it forward until a free name is created.
fn reserve(directory: &Path, kind: Kind) -> io::Result<PathBuf> {
    let mut identity = run_identity();
    for _ in 0..IDENTITY_ATTEMPTS {
        let path = directory.join(locator_name(kind, identity));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(_) => return Ok(path),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                identity = identity.saturating_add(1);
            }
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::other(format!(
        "no free retained name for {} among {IDENTITY_ATTEMPTS} identities in {}",
        kind.name(),
        directory.display()
    )))
}

/// Publishes complete bytes atomically before returning the reserved locator.
fn write_complete(path: &Path, json: &str) -> io::Result<()> {
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::other("retained locator has no file name"))?
        .to_string_lossy()
        .into_owned();
    let temporary = path.with_file_name(format!("{name}.tmp"));
    if let Err(error) = fs::write(&temporary, json.as_bytes()) {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    Ok(())
}

/// Removes the oldest retained files of `kind` beyond [`RETENTION_LIMIT`],
/// always keeping the run that just wrote `protected`. Returns a warning for
/// the first file that could not be removed.
fn prune(directory: &Path, kind: Kind, protected: &Path) -> io::Result<Option<String>> {
    let older: Vec<PathBuf> = retained_files(directory, kind)?
        .into_iter()
        .filter(|(_, path)| path.as_path() != protected)
        .map(|(_, path)| path)
        .collect();
    let excess = older.len().saturating_sub(RETENTION_LIMIT - 1);
    let mut warning = None;
    for path in older.iter().take(excess) {
        if let Err(error) = fs::remove_file(path)
            && warning.is_none()
        {
            warning = Some(format!("could not evict {}: {error}", path.display()));
        }
    }
    Ok(warning)
}

/// Retained files of `kind`, oldest first by creation identity.
fn retained_files(directory: &Path, kind: Kind) -> io::Result<Vec<(u64, PathBuf)>> {
    let prefix = format!("{}-", kind.name());
    let mut files = Vec::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if let Some(identity) = retained_name(&name, &prefix) {
            files.push((identity, entry.path()));
        }
    }
    files.sort_by_key(|(identity, _)| *identity);
    Ok(files)
}

/// A retained name is `<kind>-<identity digits>.json` with the zero-padded
/// identity width; any other name in the directory belongs to someone else and
/// is never touched.
fn retained_name(name: &str, prefix: &str) -> Option<u64> {
    let identity = name.strip_prefix(prefix)?.strip_suffix(".json")?;
    if identity.len() < IDENTITY_WIDTH || !identity.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    identity.parse().ok()
}

/// Resolves a retained detail path, failing explicitly when the record has
/// expired or been evicted.
pub fn resolve(path: &Path) -> io::Result<PathBuf> {
    if path.is_file() {
        return Ok(path.to_owned());
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        format!(
            "retained detail not found: {}; retention keeps the newest {RETENTION_LIMIT} files per kind, so an evicted record needs a new interactive run (for example `token-audit report --format text`)",
            path.display()
        ),
    ))
}

/// Selects one session record from a retained session report without
/// rescanning sessions.
pub fn select_session(path: &Path, id: &str) -> io::Result<Value> {
    select(path, Kind::Report, id)
}

/// Selects one finding record from a retained findings report without
/// rescanning sessions.
pub fn select_finding(path: &Path, id: &str) -> io::Result<Value> {
    select(path, Kind::Findings, id)
}

fn select(path: &Path, kind: Kind, id: &str) -> io::Result<Value> {
    let resolved = resolve(path)?;
    let raw = fs::read(&resolved)?;
    let value: Value = serde_json::from_slice(&raw).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("retained file {} is not JSON: {error}", resolved.display()),
        )
    })?;
    let records = value
        .get(kind.record_array())
        .and_then(Value::as_array)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "retained file {} carries no `{}` array; pass the path printed by `token-audit {} --format text`",
                    resolved.display(),
                    kind.record_array(),
                    kind.name()
                ),
            )
        })?;
    let matches: Vec<&Value> = records
        .iter()
        .filter(|record| record[kind.record_field()].as_str().unwrap_or("unknown") == id)
        .collect();
    match matches.as_slice() {
        [] => Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!(
                "{} {id} is not among the {} recorded in {}; nothing was rescanned",
                kind.record_label(),
                records.len(),
                resolved.display()
            ),
        )),
        [record] => Ok((*record).clone()),
        many => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "{id} matches {} records in {}; select a distinct identity",
                many.len(),
                resolved.display()
            ),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(directory: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(directory)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    fn retained_path(detail: Detail) -> PathBuf {
        match detail {
            Detail::Retained { path, .. } => path,
            Detail::Unavailable(reason) => panic!("retention unavailable: {reason}"),
        }
    }

    #[test]
    fn retention_keeps_newest_files_per_kind_and_ignores_foreign_files() {
        let directory = tempfile::tempdir().unwrap();
        let foreign = directory.path().join("notes.json");
        fs::write(&foreign, "keep").unwrap();
        let mut written = Vec::new();
        for index in 0..RETENTION_LIMIT + 3 {
            let payload = format!("{{\"scan\":{index}}}");
            let path = retained_path(retain(directory.path(), Kind::Report, &payload).unwrap());
            // A new locator is readable immediately: retention never evicts
            // the run that just wrote its record.
            assert_eq!(fs::read_to_string(&path).unwrap(), payload);
            written.push(path);
        }
        retain(directory.path(), Kind::Findings, "{}").unwrap();
        let retained = names(directory.path());
        let reports: Vec<&String> = retained
            .iter()
            .filter(|name| name.starts_with("report-"))
            .collect();
        assert_eq!(reports.len(), RETENTION_LIMIT, "{reports:?}");
        assert_eq!(
            retained
                .iter()
                .filter(|name| name.starts_with("findings-"))
                .count(),
            1
        );
        assert!(retained.contains(&"notes.json".to_owned()));
        for (index, path) in written.iter().enumerate() {
            // The three oldest runs were evicted and stay evicted.
            assert_eq!(path.exists(), index >= 3, "{} at {index}", path.display());
        }
    }

    #[test]
    fn identical_scan_timestamps_keep_each_run_until_its_locator_expires() {
        let directory = tempfile::tempdir().unwrap();
        let payload = |index: usize| {
            format!("{{\"generated_at\":\"2026-09-22T10:00:00Z\",\"scan\":{index}}}")
        };
        let mut written = Vec::new();
        for index in 0..RETENTION_LIMIT + 5 {
            let path =
                retained_path(retain(directory.path(), Kind::Report, &payload(index)).unwrap());
            assert!(
                !written.contains(&path),
                "locator names are never reused: {}",
                path.display()
            );
            assert_eq!(
                fs::read_to_string(&path).unwrap(),
                payload(index),
                "each run reads its own exact scan immediately"
            );
            written.push(path);
        }
        for (index, path) in written.iter().enumerate() {
            if index < 5 {
                // An evicted locator stays missing instead of resolving to a
                // later scan.
                assert!(!path.exists(), "{} at {index}", path.display());
                assert_eq!(resolve(path).unwrap_err().kind(), io::ErrorKind::NotFound);
            } else {
                assert_eq!(
                    fs::read_to_string(path).unwrap(),
                    payload(index),
                    "surviving locators keep their own scan"
                );
            }
        }
        // Name order is creation order, so eviction always removes the oldest
        // retained run.
        let created: Vec<String> = written
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(created, names(directory.path()));
    }

    #[test]
    fn expired_detail_is_an_explicit_error() {
        let directory = tempfile::tempdir().unwrap();
        let missing = directory.path().join("report-1789000000000000000.json");
        let error = resolve(&missing).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        let message = error.to_string();
        assert!(message.contains("not found"), "{message}");
        assert!(message.contains("--format text"), "{message}");
    }
}
