//! Bounded local retention of complete same-scan reports and bounded detail
//! reads over them.
//!
//! The interactive text presentation is bounded, so the complete machine
//! report of the same scan is retained once under
//! `CODEX_HOME/harness/token-audit/reports`. Retention keeps the newest
//! [`RETENTION_LIMIT`] files per kind; an evicted or expired record is an
//! explicit error. A detail read selects records from a retained file only:
//! it never rescans rollout sessions, calls a model or touches the network.

use serde_json::Value;
use std::{
    fs, io,
    path::{Path, PathBuf},
};

/// Newest retained files kept per kind (`report` and `findings`).
pub const RETENTION_LIMIT: usize = 20;

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

/// Default retention directory under the resolved CODEX_HOME.
pub fn default_directory() -> Option<PathBuf> {
    std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
        .map(|root| root.join("harness").join("token-audit").join("reports"))
}

/// Writes one complete same-scan JSON record and prunes older files of the
/// same kind beyond [`RETENTION_LIMIT`]. Pruning only ever removes files whose
/// name this module created; unrelated files stay untouched.
pub fn retain(directory: &Path, kind: Kind, generated_at: &str, json: &str) -> io::Result<Detail> {
    fs::create_dir_all(directory)?;
    let stamp: String = generated_at.chars().filter(char::is_ascii_digit).collect();
    if stamp.is_empty() {
        return Err(io::Error::other(
            "generated_at carries no timestamp digits for retention",
        ));
    }
    let name = format!("{}-{stamp}.json", kind.name());
    let path = directory.join(&name);
    let temporary = directory.join(format!("{name}.tmp"));
    fs::write(&temporary, json.as_bytes())?;
    fs::rename(&temporary, &path)?;
    let warning = prune(directory, kind)?;
    Ok(Detail::Retained { path, warning })
}

/// Removes the oldest retained files of `kind` beyond [`RETENTION_LIMIT`].
/// Returns a warning for the first file that could not be removed.
fn prune(directory: &Path, kind: Kind) -> io::Result<Option<String>> {
    let prefix = format!("{}-", kind.name());
    let mut files = Vec::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if is_retained_name(&name, &prefix) {
            files.push(entry.path());
        }
    }
    files.sort();
    let excess = files.len().saturating_sub(RETENTION_LIMIT);
    let mut warning = None;
    for path in files.iter().take(excess) {
        if let Err(error) = fs::remove_file(path)
            && warning.is_none()
        {
            warning = Some(format!("could not evict {}: {error}", path.display()));
        }
    }
    Ok(warning)
}

/// A retained name is exactly `<kind>-<timestamp digits>.json`.
fn is_retained_name(name: &str, prefix: &str) -> bool {
    name.strip_prefix(prefix)
        .and_then(|rest| rest.strip_suffix(".json"))
        .is_some_and(|stamp| !stamp.is_empty() && stamp.chars().all(|c| c.is_ascii_digit()))
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

    #[test]
    fn retention_keeps_newest_files_per_kind_and_ignores_foreign_files() {
        let directory = tempfile::tempdir().unwrap();
        let foreign = directory.path().join("notes.json");
        fs::write(&foreign, "keep").unwrap();
        let stamps: Vec<String> = (0..RETENTION_LIMIT + 3)
            .map(|index| format!("2026-09-22T10:00:{index:02}.000000000Z"))
            .collect();
        for generated_at in &stamps {
            retain(directory.path(), Kind::Report, generated_at, "{}").unwrap();
        }
        retain(
            directory.path(),
            Kind::Findings,
            "2026-09-22T10:00:00.000000000Z",
            "{}",
        )
        .unwrap();
        let retained = names(directory.path());
        let reports: Vec<&String> = retained
            .iter()
            .filter(|name| name.starts_with("report-"))
            .collect();
        assert_eq!(reports.len(), RETENTION_LIMIT);
        assert_eq!(
            retained
                .iter()
                .filter(|name| name.starts_with("findings-"))
                .count(),
            1
        );
        assert!(retained.contains(&"notes.json".to_owned()));
        for evicted in &stamps[..3] {
            let name = format!("report-{}.json", digits(evicted));
            assert!(!retained.contains(&name), "{name} should be evicted");
        }
        let oldest_kept = format!("report-{}.json", digits(&stamps[3]));
        assert!(retained.contains(&oldest_kept), "{oldest_kept} missing");
    }

    fn digits(value: &str) -> String {
        value.chars().filter(char::is_ascii_digit).collect()
    }

    #[test]
    fn expired_detail_is_an_explicit_error() {
        let directory = tempfile::tempdir().unwrap();
        let missing = directory.path().join("report-20260922100000000000000.json");
        let error = resolve(&missing).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        let message = error.to_string();
        assert!(message.contains("not found"), "{message}");
        assert!(message.contains("--format text"), "{message}");
    }
}
