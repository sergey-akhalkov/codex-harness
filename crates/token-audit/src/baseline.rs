//! Local baseline snapshots and the measured diff loop.
//!
//! A baseline is one small JSON aggregate per audit under the resolved Codex
//! home at `harness/token-audit/baselines/`, with a `latest` pointer. It
//! stores aggregates and hashed identities only: no transcript content, no
//! raw workspace paths, no token estimates.
//!
//! Publication is uniquely named and immutable. A save reserves its own
//! create-new name derived from the report timestamp plus the writer's
//! publication identity, writes the complete snapshot under that name, and
//! only then replaces the `latest` pointer. Concurrent writers cannot
//! overwrite a successful snapshot or share a staging name, and a writer that
//! stops before the pointer update leaves the previous pointer naming a
//! complete snapshot. `latest` is defined by the last completed publication,
//! never by name or timestamp ordering.
//!
//! Loading keeps three questions separate: whether the file is a valid
//! baseline (format), whether it was written by comparable analyzer semantics
//! and population scope (comparability), and which format version it is
//! (version change). Legacy snapshots without comparison metadata stay
//! inspectable but cannot silently qualify for a controlled comparison.
use crate::{
    SCHEMA_VERSION,
    model::{Bucket, CoverageReport, Scan, SessionRow, TokenTotals},
    retention::Detail,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io,
    path::{Component, Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

/// Schema version of the baseline snapshot file.
pub const BASELINE_SCHEMA_VERSION: u32 = 1;

/// Accounting mode of the current analyzer: lifetime totals of the sessions
/// active in the scan window, with each session's recorded usage basis.
/// Interval usage accounting is a separate mode introduced with its own
/// change; a snapshot recorded under another mode is not comparable.
pub const MODE_ACTIVITY: &str = "activity";

/// Snapshot state: structurally validated with all comparison metadata.
pub const SNAPSHOT_VALID: &str = "valid";
/// Snapshot state: readable, but written before comparison metadata was
/// recorded; inspectable with a weaker explicit status.
pub const SNAPSHOT_LEGACY: &str = "legacy";
/// Snapshot state: the snapshot format version is not the current one.
pub const SNAPSHOT_UNSUPPORTED: &str = "unsupported_version";
/// Snapshot state: the file is not a structurally valid baseline.
pub const SNAPSHOT_INVALID: &str = "invalid";

/// Create-new attempts before a unique publication name is reported
/// unavailable.
const NAME_ATTEMPTS: u32 = 1000;

/// Digits of a zero-padded publication identity: epoch nanoseconds.
const IDENTITY_WIDTH: usize = 19;

/// One session digest: identities and recorded counters only.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionDigest {
    pub session_id: Option<String>,
    pub project: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub day: Option<String>,
    /// Where the recorded totals came from. Absent in legacy snapshots.
    #[serde(default)]
    pub usage_basis: Option<String>,
    /// Recorded subtotal status of this session. Absent in legacy snapshots.
    #[serde(default)]
    pub partial: bool,
    /// Recorded warning codes of this session. Absent in legacy snapshots.
    #[serde(default)]
    pub warnings: Vec<String>,
    pub usage: TokenTotals,
}

/// Recorded coverage of one scan, carried inside a snapshot so a later diff
/// can distinguish a real movement from a change in what was measured.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotCoverage {
    pub files_scanned: usize,
    pub sessions: usize,
    pub sessions_without_usage: usize,
    pub corrupt_lines: u64,
    pub oversized_lines: u64,
    pub unrecognized_events: u64,
    /// Sessions per recorded usage basis; `unavailable` counts sessions with
    /// no recorded usage.
    pub usage_basis: BTreeMap<String, usize>,
    /// Sessions carrying each warning code, plus analyzer-level warnings.
    pub warnings: BTreeMap<String, usize>,
    pub partial: bool,
}

impl SnapshotCoverage {
    /// Recorded coverage of one completed scan.
    pub fn from_coverage(coverage: &CoverageReport) -> Self {
        Self {
            files_scanned: coverage.files_scanned,
            sessions: coverage.sessions,
            sessions_without_usage: coverage.sessions_without_usage,
            corrupt_lines: coverage.corrupt_lines,
            oversized_lines: coverage.oversized_lines,
            unrecognized_events: coverage.unrecognized_events,
            usage_basis: coverage.usage_basis.clone(),
            warnings: coverage.warning_counts.clone(),
            partial: coverage.partial,
        }
    }
}

/// Aggregate snapshot of one completed scan.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BaselineSnapshot {
    pub schema_version: u32,
    pub created_at: String,
    pub sessions_root: String,
    pub window_days: Option<u32>,
    /// Accounting mode of the producing scan. Absent in legacy snapshots.
    #[serde(default)]
    pub mode: Option<String>,
    /// Analyzer report schema of the producing scan. Absent in legacy
    /// snapshots.
    #[serde(default)]
    pub analyzer_version: Option<u32>,
    /// Recorded coverage. Absent in legacy snapshots.
    #[serde(default)]
    pub coverage: Option<SnapshotCoverage>,
    pub sessions: Vec<SessionDigest>,
    pub by_project: Vec<Bucket>,
    pub by_model: Vec<Bucket>,
    pub by_effort: Vec<Bucket>,
    pub by_day: Vec<Bucket>,
    pub totals: Bucket,
}

/// Movement of one session between baseline and current scan.
#[derive(Clone, Debug, Serialize)]
pub struct SessionMovement {
    pub session_id: String,
    pub status: &'static str,
    pub baseline_usage_basis: Option<String>,
    pub current_usage_basis: Option<String>,
    pub baseline_partial: Option<bool>,
    pub current_partial: bool,
    pub baseline_warnings: Vec<String>,
    pub current_warnings: Vec<String>,
    pub baseline_total_tokens: Option<u64>,
    pub current_total_tokens: Option<u64>,
    pub delta_total_tokens: Option<i64>,
}

impl SessionMovement {
    /// Ranking significance: recorded deltas first, largest absolute movement
    /// first, then the largest recorded side.
    pub(crate) fn significance(&self) -> (bool, u64) {
        significance(
            self.delta_total_tokens,
            self.baseline_total_tokens,
            self.current_total_tokens,
        )
    }
}

/// Movement of one aggregate bucket.
#[derive(Clone, Debug, Serialize)]
pub struct BucketMovement {
    pub key: String,
    pub baseline_sessions: Option<usize>,
    pub current_sessions: usize,
    pub baseline_usage_basis: BTreeMap<String, usize>,
    pub current_usage_basis: BTreeMap<String, usize>,
    pub baseline_missing_usage_sessions: Option<usize>,
    pub current_missing_usage_sessions: usize,
    pub baseline_partial: Option<bool>,
    pub current_partial: bool,
    pub baseline_total_tokens: Option<u64>,
    pub current_total_tokens: Option<u64>,
    pub delta_total_tokens: Option<i64>,
}

impl BucketMovement {
    /// Ranking significance: recorded deltas first, largest absolute movement
    /// first, then the largest recorded side.
    pub(crate) fn significance(&self) -> (bool, u64) {
        significance(
            self.delta_total_tokens,
            self.baseline_total_tokens,
            self.current_total_tokens,
        )
    }
}

/// Coverage carried through a comparison.
#[derive(Clone, Debug, Serialize)]
pub struct CoverageMovement {
    /// Recorded coverage of the snapshot; `None` when it was not recorded
    /// (legacy) or the snapshot is not usable.
    pub baseline: Option<SnapshotCoverage>,
    pub current: SnapshotCoverage,
    /// A material coverage regression that makes a lower recorded subtotal
    /// unusable as a saving claim.
    pub degraded: bool,
    /// Deterministic reasons behind `degraded`.
    pub reasons: Vec<String>,
}

/// Complete diff of the current scan against one baseline.
#[derive(Clone, Debug, Serialize)]
pub struct BaselineDiff {
    pub schema_version: u32,
    pub command: &'static str,
    pub generated_at: String,
    /// Identifier of the compared snapshot, as it was resolved.
    pub baseline: String,
    pub current_root: String,
    /// `valid`, `legacy`, `unsupported_version` or `invalid`.
    pub snapshot_status: &'static str,
    /// Why the snapshot is not `valid`.
    pub snapshot_reason: Option<String>,
    pub snapshot_schema_version: Option<u32>,
    pub current_schema_version: u32,
    /// The snapshot and current analyzer schema versions differ.
    pub version_changed: bool,
    /// Structural format compatibility: schema and required fields validated.
    pub compatible: bool,
    /// Reason when the snapshot is not format-compatible.
    pub incompatibility: Option<String>,
    /// Scope and coverage comparability of the two populations.
    pub comparable: bool,
    /// Deterministic reasons the two populations are not comparable; empty
    /// when `comparable`.
    pub comparability: Vec<String>,
    pub coverage: CoverageMovement,
    pub sessions: Vec<SessionMovement>,
    pub by_project: Vec<BucketMovement>,
    pub by_model: Vec<BucketMovement>,
    pub by_effort: Vec<BucketMovement>,
    pub by_day: Vec<BucketMovement>,
    pub totals: BucketMovement,
    pub limitation: &'static str,
}

pub const BASELINE_LIMITATION: &str = "Baselines store aggregates and hashed identities only; diff reports recorded token movement without costs, quotas or transcript content.";

/// Movement comparison retained for stable detail reads.
pub const DIFF_RETENTION_LIMIT: usize = 20;

fn digest(row: &SessionRow) -> SessionDigest {
    SessionDigest {
        session_id: row.session_id.clone(),
        project: row.project.clone(),
        model: row.model.clone(),
        effort: row.effort.clone(),
        day: row.day.clone(),
        usage_basis: row.usage_basis.map(str::to_owned),
        partial: row.partial,
        warnings: row.warnings.clone(),
        usage: row.usage.clone(),
    }
}

/// Builds a snapshot from a completed scan.
pub fn snapshot(scan: &Scan) -> BaselineSnapshot {
    let report = &scan.report;
    BaselineSnapshot {
        schema_version: BASELINE_SCHEMA_VERSION,
        created_at: report.generated_at.clone(),
        sessions_root: report.sessions_root.clone(),
        window_days: report.window_days,
        mode: Some(MODE_ACTIVITY.to_owned()),
        analyzer_version: Some(SCHEMA_VERSION),
        coverage: Some(SnapshotCoverage::from_coverage(&report.coverage)),
        sessions: report.sessions.iter().map(digest).collect(),
        by_project: report.by_project.clone(),
        by_model: report.by_model.clone(),
        by_effort: report.by_effort.clone(),
        by_day: report.by_day.clone(),
        totals: report.totals.clone(),
    }
}

/// Default baseline directory under the resolved Codex home.
///
/// This matches the session-root and retention conventions: `CODEX_HOME`
/// when set, otherwise `USERPROFILE/.codex`.
pub fn default_directory() -> Option<PathBuf> {
    std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(|home| PathBuf::from(home).join(".codex")))
        .map(|root| root.join("harness").join("token-audit").join("baselines"))
}

/// Unique publication identity of this save: epoch nanoseconds.
fn publication_identity() -> u64 {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    elapsed
        .as_secs()
        .saturating_mul(1_000_000_000)
        .saturating_add(u64::from(elapsed.subsec_nanos()))
}

/// Writes one snapshot with its `latest` pointer and returns its file name.
///
/// The name is unique per publication and the snapshot file is never modified
/// after it is published: the only shared state is the `latest` pointer, which
/// is replaced only after the complete snapshot exists under its final name.
pub fn save(directory: &Path, scan: &Scan) -> io::Result<String> {
    fs::create_dir_all(directory)?;
    let encoded = serde_json::to_vec_pretty(&snapshot(scan)).map_err(io::Error::other)?;
    let stamp = scan
        .report
        .generated_at
        .replace([':', '-', 'T', 'Z', '.'], "");
    let mut identity = publication_identity();
    for _ in 0..NAME_ATTEMPTS {
        let name = format!(
            "baseline-{stamp}-{identity:0width$}.json",
            width = IDENTITY_WIDTH
        );
        let committed = directory.join(&name);
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&committed)
        {
            Ok(_) => {
                publish(&committed, &encoded, identity)?;
                return Ok(name);
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                identity = identity.saturating_add(1);
            }
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::other(format!(
        "no free baseline name among {NAME_ATTEMPTS} publication identities in {}",
        directory.display()
    )))
}

/// Completes one publication. The final name is already reserved by this
/// writer, so the complete bytes replace the empty reservation and only then
/// is the `latest` pointer replaced. A snapshot whose pointer update fails is
/// named in the error: it stays a complete, readable baseline.
fn publish(committed: &Path, encoded: &[u8], identity: u64) -> io::Result<()> {
    let directory = committed
        .parent()
        .ok_or_else(|| io::Error::other("baseline path has no parent directory"))?;
    let name = committed
        .file_name()
        .ok_or_else(|| io::Error::other("baseline path has no file name"))?
        .to_string_lossy()
        .into_owned();
    let staging = committed.with_file_name(format!("{name}.staging"));
    if let Err(error) = fs::write(&staging, encoded) {
        remove(&staging);
        remove(committed);
        return Err(error);
    }
    if let Err(error) = fs::rename(&staging, committed) {
        remove(&staging);
        remove(committed);
        return Err(error);
    }
    let pointer_temporary = directory.join(format!(
        "latest-{identity:0width$}.tmp",
        width = IDENTITY_WIDTH
    ));
    if let Err(error) = fs::write(&pointer_temporary, name.as_bytes()) {
        remove(&pointer_temporary);
        return Err(io::Error::new(
            error.kind(),
            format!(
                "snapshot {name} was published but the latest pointer could not be written: {error}"
            ),
        ));
    }
    if let Err(error) = fs::rename(&pointer_temporary, directory.join("latest")) {
        remove(&pointer_temporary);
        return Err(io::Error::new(
            error.kind(),
            format!(
                "snapshot {name} was published but the latest pointer could not be updated: {error}"
            ),
        ));
    }
    Ok(())
}

/// Best-effort removal of a publication artifact this writer owns.
fn remove(path: &Path) {
    let _ = fs::remove_file(path);
}

/// Resolves the requested baseline name, or the `latest` pointer.
///
/// Accepted forms are the published file name (`baseline-....json`) and its
/// basename without the `.json` suffix. A requested name or pointer that is
/// not a single plain file name inside `directory` is rejected instead of
/// being interpreted as a local path.
pub fn resolve(directory: &Path, requested: Option<&str>) -> io::Result<PathBuf> {
    match requested {
        Some(name) if name != "latest" => {
            let file = snapshot_file_name(name)?;
            let path = directory.join(&file);
            if path.is_file() {
                Ok(path)
            } else {
                Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("baseline {name} not found in {}", directory.display()),
                ))
            }
        }
        _ => {
            let pointer = directory.join("latest");
            let contents = fs::read_to_string(&pointer)?;
            let requested = contents.trim();
            let file = snapshot_file_name(requested).map_err(|error| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "the latest pointer {} does not name an owned baseline: {error}",
                        pointer.display()
                    ),
                )
            })?;
            let path = directory.join(&file);
            if path.is_file() {
                Ok(path)
            } else {
                Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    "the latest baseline pointer names no existing snapshot",
                ))
            }
        }
    }
}

/// The published file name of a requested baseline identifier.
fn snapshot_file_name(requested: &str) -> io::Result<String> {
    if requested.is_empty() {
        return Err(invalid_name(requested, "it is empty"));
    }
    if requested.contains('/') || requested.contains('\\') {
        return Err(invalid_name(requested, "it contains a path separator"));
    }
    let mut components = Path::new(requested).components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(_)), None) => {}
        _ => {
            return Err(invalid_name(
                requested,
                "it is not a plain file name inside the baseline directory",
            ));
        }
    }
    if requested.to_ascii_lowercase().ends_with(".json") {
        Ok(requested.to_owned())
    } else {
        Ok(format!("{requested}.json"))
    }
}

fn invalid_name(requested: &str, reason: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!(
            "baseline name {requested:?} is not accepted: {reason}; use the name returned by `token-audit baseline save` or `latest`"
        ),
    )
}

/// One loaded snapshot file, structurally validated.
enum LoadedSnapshot {
    Valid(BaselineSnapshot),
    Legacy(BaselineSnapshot),
    Unsupported {
        schema: u32,
        reason: String,
    },
    Invalid {
        /// Recorded schema version when the file carried a readable one.
        schema: Option<u32>,
        reason: String,
    },
}

/// Reads and validates one snapshot file. A malformed file is an explicit
/// state, not an error: the comparison reports it without inventing a
/// baseline.
fn load_snapshot(path: &Path) -> io::Result<LoadedSnapshot> {
    let raw = fs::read(path)?;
    let value: Value = match serde_json::from_slice(&raw) {
        Ok(value) => value,
        Err(error) => {
            return Ok(LoadedSnapshot::Invalid {
                schema: None,
                reason: format!("snapshot is not valid JSON: {error}"),
            });
        }
    };
    let Some(schema) = value.get("schema_version").and_then(Value::as_u64) else {
        return Ok(LoadedSnapshot::Invalid {
            schema: None,
            reason: "snapshot has no integer schema_version field".to_owned(),
        });
    };
    let schema = u32::try_from(schema).unwrap_or(u32::MAX);
    if schema != BASELINE_SCHEMA_VERSION {
        return Ok(LoadedSnapshot::Unsupported {
            schema,
            reason: format!("snapshot schema {schema} is not {BASELINE_SCHEMA_VERSION}"),
        });
    }
    let parsed: BaselineSnapshot = match serde_json::from_value(value) {
        Ok(parsed) => parsed,
        Err(error) => {
            return Ok(LoadedSnapshot::Invalid {
                schema: Some(schema),
                reason: format!("snapshot does not match the baseline schema: {error}"),
            });
        }
    };
    if parsed.totals.sessions != parsed.sessions.len() {
        return Ok(LoadedSnapshot::Invalid {
            schema: Some(schema),
            reason: format!(
                "snapshot records {} sessions but its totals count {}",
                parsed.sessions.len(),
                parsed.totals.sessions
            ),
        });
    }
    let legacy =
        parsed.mode.is_none() || parsed.analyzer_version.is_none() || parsed.coverage.is_none();
    Ok(if legacy {
        LoadedSnapshot::Legacy(parsed)
    } else {
        LoadedSnapshot::Valid(parsed)
    })
}

/// Loads and compares one baseline against a fresh scan.
///
/// The result keeps snapshot validity (format), version change, population
/// comparability and coverage degradation separate. A snapshot that is not
/// usable contributes no fabricated baseline: its movements are omitted, its
/// totals stay unknown and every reason is explicit.
pub fn diff(baseline_path: &Path, name: &str, scan: &Scan) -> io::Result<BaselineDiff> {
    let report = &scan.report;
    let loaded = load_snapshot(baseline_path)?;
    let current_coverage = SnapshotCoverage::from_coverage(&report.coverage);
    let (snapshot_status, snapshot_reason, snapshot_schema_version, parsed) = match &loaded {
        LoadedSnapshot::Valid(snapshot) => (
            SNAPSHOT_VALID,
            None,
            Some(snapshot.schema_version),
            Some(snapshot),
        ),
        LoadedSnapshot::Legacy(snapshot) => (
            SNAPSHOT_LEGACY,
            Some(
                "legacy snapshot: comparison metadata (mode, analyzer version, coverage) was not recorded"
                    .to_owned(),
            ),
            Some(snapshot.schema_version),
            Some(snapshot),
        ),
        LoadedSnapshot::Unsupported { schema, reason } => {
            (SNAPSHOT_UNSUPPORTED, Some(reason.clone()), Some(*schema), None)
        }
        LoadedSnapshot::Invalid { schema, reason } => {
            (SNAPSHOT_INVALID, Some(reason.clone()), *schema, None)
        }
    };
    let compatible = parsed.is_some();
    let incompatibility = (!compatible).then(|| snapshot_reason.clone()).flatten();
    let version_changed =
        snapshot_schema_version.is_some_and(|schema| schema != BASELINE_SCHEMA_VERSION);
    let mut comparability = Vec::new();
    if matches!(loaded, LoadedSnapshot::Legacy(_)) {
        comparability.push(
            "legacy snapshot: coverage, usage basis and accounting mode were not recorded"
                .to_owned(),
        );
    }
    if let Some(snapshot) = parsed {
        if snapshot.sessions_root != report.sessions_root {
            comparability.push("recorded sessions root identity differs".to_owned());
        }
        if snapshot.window_days != report.window_days {
            comparability.push(format!(
                "window differs: baseline {} vs current {}",
                window_text(snapshot.window_days),
                window_text(report.window_days)
            ));
        }
        if matches!(loaded, LoadedSnapshot::Valid(_)) {
            if snapshot.mode.as_deref() != Some(MODE_ACTIVITY) {
                comparability.push(format!(
                    "accounting mode {} differs from {MODE_ACTIVITY}",
                    snapshot.mode.as_deref().unwrap_or("unknown")
                ));
            }
            if snapshot.analyzer_version != Some(SCHEMA_VERSION) {
                comparability.push(format!(
                    "analyzer semantics version {:?} differs from {SCHEMA_VERSION}",
                    snapshot.analyzer_version
                ));
            }
        }
    }
    if !compatible {
        comparability.push(match &loaded {
            LoadedSnapshot::Unsupported { .. } => {
                "snapshot version is not comparable with the current analyzer".to_owned()
            }
            _ => "snapshot is not structurally valid".to_owned(),
        });
    }
    let coverage = coverage_movement(
        parsed.and_then(|snapshot| snapshot.coverage.clone()),
        &current_coverage,
    );
    for reason in &coverage.reasons {
        comparability.push(format!("coverage: {reason}"));
    }
    let comparable = comparability.is_empty();
    let mut sessions = Vec::new();
    let mut by_project = Vec::new();
    let mut by_model = Vec::new();
    let mut by_effort = Vec::new();
    let mut by_day = Vec::new();
    if let Some(snapshot) = parsed {
        let baseline_sessions: BTreeMap<&str, &SessionDigest> = snapshot
            .sessions
            .iter()
            .filter_map(|digest| digest.session_id.as_deref().map(|id| (id, digest)))
            .collect();
        let current_sessions: BTreeMap<&str, &SessionRow> = report
            .sessions
            .iter()
            .filter_map(|row| row.session_id.as_deref().map(|id| (id, row)))
            .collect();
        for (id, digest) in &baseline_sessions {
            let current = current_sessions.get(id).copied();
            sessions.push(SessionMovement {
                session_id: (*id).to_owned(),
                status: if current.is_some() { "same" } else { "gone" },
                baseline_usage_basis: digest.usage_basis.clone(),
                current_usage_basis: current.and_then(|row| row.usage_basis.map(str::to_owned)),
                baseline_partial: Some(digest.partial),
                current_partial: current.is_some_and(|row| row.partial),
                baseline_warnings: digest.warnings.clone(),
                current_warnings: current.map(|row| row.warnings.clone()).unwrap_or_default(),
                baseline_total_tokens: digest.usage.total_tokens,
                current_total_tokens: current.and_then(|row| row.usage.total_tokens),
                delta_total_tokens: delta(
                    digest.usage.total_tokens,
                    current.and_then(|row| row.usage.total_tokens),
                ),
            });
        }
        for (id, row) in &current_sessions {
            if !baseline_sessions.contains_key(id) {
                sessions.push(SessionMovement {
                    session_id: (*id).to_owned(),
                    status: "new",
                    baseline_usage_basis: None,
                    current_usage_basis: row.usage_basis.map(str::to_owned),
                    baseline_partial: None,
                    current_partial: row.partial,
                    baseline_warnings: Vec::new(),
                    current_warnings: row.warnings.clone(),
                    baseline_total_tokens: None,
                    current_total_tokens: row.usage.total_tokens,
                    delta_total_tokens: delta(None, row.usage.total_tokens),
                });
            }
        }
        sessions.sort_by(|left, right| left.session_id.cmp(&right.session_id));
        by_project = bucket_moves(&snapshot.by_project, &report.by_project);
        by_model = bucket_moves(&snapshot.by_model, &report.by_model);
        by_effort = bucket_moves(&snapshot.by_effort, &report.by_effort);
        by_day = bucket_moves(&snapshot.by_day, &report.by_day);
    }
    let totals = BucketMovement {
        key: "totals".to_owned(),
        baseline_sessions: parsed.map(|snapshot| snapshot.totals.sessions),
        current_sessions: report.totals.sessions,
        baseline_usage_basis: parsed
            .map(|snapshot| snapshot.totals.usage_basis.clone())
            .unwrap_or_default(),
        current_usage_basis: report.totals.usage_basis.clone(),
        baseline_missing_usage_sessions: parsed
            .map(|snapshot| snapshot.totals.missing_usage_sessions),
        current_missing_usage_sessions: report.totals.missing_usage_sessions,
        baseline_partial: parsed.map(|snapshot| snapshot.totals.partial),
        current_partial: report.totals.partial,
        baseline_total_tokens: parsed.and_then(|snapshot| snapshot.totals.usage.total_tokens),
        current_total_tokens: report.totals.usage.total_tokens,
        delta_total_tokens: delta(
            parsed.and_then(|snapshot| snapshot.totals.usage.total_tokens),
            report.totals.usage.total_tokens,
        ),
    };
    Ok(BaselineDiff {
        schema_version: BASELINE_SCHEMA_VERSION,
        command: "baseline diff",
        generated_at: report.generated_at.clone(),
        baseline: name.to_owned(),
        current_root: report.sessions_root.clone(),
        snapshot_status,
        snapshot_reason,
        snapshot_schema_version,
        current_schema_version: BASELINE_SCHEMA_VERSION,
        version_changed,
        compatible,
        incompatibility,
        comparable,
        comparability,
        coverage,
        sessions,
        by_project,
        by_model,
        by_effort,
        by_day,
        totals,
        limitation: BASELINE_LIMITATION,
    })
}

/// Coverage carried through a comparison, with the material regressions that
/// make a lower recorded subtotal unusable as a saving claim.
fn coverage_movement(
    baseline: Option<SnapshotCoverage>,
    current: &SnapshotCoverage,
) -> CoverageMovement {
    let mut reasons = Vec::new();
    if let Some(baseline) = &baseline {
        if current.sessions_without_usage > baseline.sessions_without_usage {
            reasons.push(format!(
                "sessions without recorded usage increased {} -> {}",
                baseline.sessions_without_usage, current.sessions_without_usage
            ));
        }
        if current.corrupt_lines > baseline.corrupt_lines {
            reasons.push(format!(
                "corrupt lines increased {} -> {}; unreadable usage may be omitted",
                baseline.corrupt_lines, current.corrupt_lines
            ));
        }
        if current.oversized_lines > baseline.oversized_lines {
            reasons.push(format!(
                "oversized lines increased {} -> {}",
                baseline.oversized_lines, current.oversized_lines
            ));
        }
    }
    CoverageMovement {
        baseline,
        current: current.clone(),
        degraded: !reasons.is_empty(),
        reasons,
    }
}

fn bucket_index(buckets: &[Bucket]) -> BTreeMap<&str, &Bucket> {
    buckets
        .iter()
        .map(|bucket| (bucket.key.as_str(), bucket))
        .collect()
}

fn bucket_moves(baseline: &[Bucket], current: &[Bucket]) -> Vec<BucketMovement> {
    let baseline = bucket_index(baseline);
    let current = bucket_index(current);
    let mut keys: Vec<&str> = baseline.keys().chain(current.keys()).copied().collect();
    keys.sort();
    keys.dedup();
    keys.into_iter()
        .map(|key| {
            let baseline_bucket = baseline.get(key).copied();
            let current_bucket = current.get(key).copied();
            BucketMovement {
                key: key.to_owned(),
                baseline_sessions: baseline_bucket.map(|bucket| bucket.sessions),
                current_sessions: current_bucket.map_or(0, |bucket| bucket.sessions),
                baseline_usage_basis: baseline_bucket
                    .map(|bucket| bucket.usage_basis.clone())
                    .unwrap_or_default(),
                current_usage_basis: current_bucket
                    .map(|bucket| bucket.usage_basis.clone())
                    .unwrap_or_default(),
                baseline_missing_usage_sessions: baseline_bucket
                    .map(|bucket| bucket.missing_usage_sessions),
                current_missing_usage_sessions: current_bucket
                    .map_or(0, |bucket| bucket.missing_usage_sessions),
                baseline_partial: baseline_bucket.map(|bucket| bucket.partial),
                current_partial: current_bucket.is_some_and(|bucket| bucket.partial),
                baseline_total_tokens: baseline_bucket.and_then(|bucket| bucket.usage.total_tokens),
                current_total_tokens: current_bucket.and_then(|bucket| bucket.usage.total_tokens),
                delta_total_tokens: delta(
                    baseline_bucket.and_then(|bucket| bucket.usage.total_tokens),
                    current_bucket.and_then(|bucket| bucket.usage.total_tokens),
                ),
            }
        })
        .collect()
}

fn delta(baseline: Option<u64>, current: Option<u64>) -> Option<i64> {
    baseline.zip(current).and_then(|(baseline, current)| {
        current
            .checked_sub(baseline)
            .map(|value| value as i64)
            .or_else(|| baseline.checked_sub(current).map(|value| -(value as i64)))
    })
}

fn window_text(days: Option<u32>) -> String {
    days.map_or_else(|| "all".to_owned(), |days| format!("last {days} days"))
}

/// Ranking significance: recorded deltas first, largest absolute movement
/// first, then the largest recorded side.
fn significance(delta: Option<i64>, baseline: Option<u64>, current: Option<u64>) -> (bool, u64) {
    match delta {
        Some(delta) => (true, delta.unsigned_abs()),
        None => (false, baseline.unwrap_or(0).max(current.unwrap_or(0))),
    }
}

/// Movements in the documented presentation order. The bounded text
/// presentation and the detail paging share this order, so a presented row
/// and a paged row never disagree.
fn rank_movements(movements: &[Value]) -> Vec<&Value> {
    let mut ranked: Vec<&Value> = movements.iter().collect();
    ranked.sort_by(|left, right| {
        movement_significance(right)
            .cmp(&movement_significance(left))
            .then_with(|| movement_id(left).cmp(movement_id(right)))
    });
    ranked
}

fn movement_significance(movement: &Value) -> (bool, u64) {
    significance(
        movement.get("delta_total_tokens").and_then(Value::as_i64),
        movement
            .get("baseline_total_tokens")
            .and_then(Value::as_u64),
        movement.get("current_total_tokens").and_then(Value::as_u64),
    )
}

fn movement_id(movement: &Value) -> &str {
    movement
        .get("session_id")
        .or_else(|| movement.get("key"))
        .and_then(Value::as_str)
        .unwrap_or("unknown")
}

fn movement_array<'a>(record: &'a Value, field: &'static str) -> io::Result<&'a [Value]> {
    record
        .get(field)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "retained comparison carries no `{field}` array; pass the path printed by `token-audit baseline diff --format text`"
                ),
            )
        })
}

fn group_field(kind: &str) -> io::Result<&'static str> {
    match kind {
        "project" => Ok("by_project"),
        "model" => Ok("by_model"),
        "effort" => Ok("by_effort"),
        "day" => Ok("by_day"),
        other => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("unknown group kind {other}; expected project, model, effort or day"),
        )),
    }
}

fn detail_page(
    kind: &str,
    selector: Option<&str>,
    offset: usize,
    limit: usize,
    total: usize,
    movements: &[&Value],
) -> Value {
    json!({
        "schema_version": SCHEMA_VERSION,
        "command": "baseline diff detail",
        "kind": kind,
        "selector": selector,
        "offset": offset,
        "limit": limit,
        "total": total,
        "count": movements.len(),
        "movements": movements,
    })
}

impl BaselineDiff {
    /// Default movement page of one diff detail read.
    pub const DETAIL_LIMIT_DEFAULT: usize = 100;

    /// Largest movement page one diff detail read returns.
    pub const DETAIL_LIMIT_MAX: usize = 1000;

    /// Retains this complete comparison for stable detail reads and returns
    /// its locator, or an explicit unavailable state.
    pub fn retain_complete(&self) -> Detail {
        let Some(directory) = default_directory().map(|directory| directory.join("diffs")) else {
            return Detail::Unavailable(
                "CODEX_HOME or USERPROFILE is required to retain the complete comparison"
                    .to_owned(),
            );
        };
        let json = match serde_json::to_string_pretty(self) {
            Ok(json) => json,
            Err(error) => return Detail::Unavailable(error.to_string()),
        };
        retain_comparison(&directory, &json)
            .unwrap_or_else(|error| Detail::Unavailable(error.to_string()))
    }

    /// Reads one retained complete comparison for stable detail selection.
    /// Expired or evicted records are explicit errors; nothing is rescanned.
    pub fn open(path: &Path) -> io::Result<Value> {
        if !path.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!(
                    "retained baseline comparison not found: {}; retention keeps the newest {DIFF_RETENTION_LIMIT} comparisons, so an evicted record needs a new `token-audit baseline diff --format text` run",
                    path.display()
                ),
            ));
        }
        let raw = fs::read(path)?;
        let value: Value = serde_json::from_slice(&raw).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("retained file {} is not JSON: {error}", path.display()),
            )
        })?;
        let command = value
            .get("command")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        if command != "baseline diff" {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "retained file {} is a {command} record, not a baseline diff; pass the path printed by `token-audit baseline diff --format text`",
                    path.display()
                ),
            ));
        }
        Ok(value)
    }

    /// One session movement of a retained comparison.
    pub fn select_session(record: &Value, id: &str) -> io::Result<Value> {
        let movements = movement_array(record, "sessions")?;
        let selected: Vec<&Value> = movements
            .iter()
            .filter(|movement| movement["session_id"].as_str() == Some(id))
            .collect();
        match selected.as_slice() {
            [] => Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!(
                    "session {id} is not among the {} recorded movements in this comparison; nothing was rescanned",
                    movements.len()
                ),
            )),
            [movement] => Ok(detail_page("session", Some(id), 0, 1, 1, &[*movement])),
            many => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "{id} matches {} movements in this comparison; select a distinct identity",
                    many.len()
                ),
            )),
        }
    }

    /// One aggregate-group movement of a retained comparison.
    pub fn select_group(record: &Value, kind: &str, key: &str) -> io::Result<Value> {
        let field = group_field(kind)?;
        let movements = movement_array(record, field)?;
        let selected: Vec<&Value> = movements
            .iter()
            .filter(|movement| movement["key"].as_str() == Some(key))
            .collect();
        match selected.as_slice() {
            [] => Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!(
                    "group {kind}:{key} is not among the {} recorded movements in this comparison",
                    movements.len()
                ),
            )),
            [movement] => Ok(detail_page(
                "group",
                Some(&format!("{kind}:{key}")),
                0,
                1,
                1,
                &[*movement],
            )),
            many => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "{kind}:{key} matches {} movements in this comparison; select a distinct identity",
                    many.len()
                ),
            )),
        }
    }

    /// A deterministic page of session movements in presentation order.
    pub fn page_sessions(record: &Value, offset: usize, limit: usize) -> io::Result<Value> {
        let movements = movement_array(record, "sessions")?;
        let total = movements.len();
        let page: Vec<&Value> = rank_movements(movements)
            .into_iter()
            .skip(offset)
            .take(limit)
            .collect();
        Ok(detail_page("sessions", None, offset, limit, total, &page))
    }

    /// A deterministic page of one group kind in presentation order.
    pub fn page_groups(
        record: &Value,
        kind: &str,
        offset: usize,
        limit: usize,
    ) -> io::Result<Value> {
        let field = group_field(kind)?;
        let movements = movement_array(record, field)?;
        let total = movements.len();
        let page: Vec<&Value> = rank_movements(movements)
            .into_iter()
            .skip(offset)
            .take(limit)
            .collect();
        Ok(detail_page(
            "groups",
            Some(kind),
            offset,
            limit,
            total,
            &page,
        ))
    }
}

/// Retains one complete comparison under `diffs/`, keeping the newest
/// [`DIFF_RETENTION_LIMIT`] records and never touching foreign files.
fn retain_comparison(directory: &Path, json: &str) -> io::Result<Detail> {
    fs::create_dir_all(directory)?;
    let path = reserve_comparison(directory)?;
    if let Err(error) = write_complete(&path, json) {
        remove(&path);
        return Err(error);
    }
    let warning = prune_comparisons(directory, &path)?;
    Ok(Detail::Retained { path, warning })
}

/// Reserves this comparison's own create-new locator, stepping the identity
/// forward while a concurrent read holds the same name.
fn reserve_comparison(directory: &Path) -> io::Result<PathBuf> {
    let mut identity = publication_identity();
    for _ in 0..NAME_ATTEMPTS {
        let path = directory.join(comparison_name(identity));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(_) => return Ok(path),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                identity = identity.saturating_add(1);
            }
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::other(format!(
        "no free retained comparison name among {NAME_ATTEMPTS} identities in {}",
        directory.display()
    )))
}

fn comparison_name(identity: u64) -> String {
    format!("diff-{identity:0width$}.json", width = IDENTITY_WIDTH)
}

/// Publishes complete bytes over the reserved locator before it is returned.
fn write_complete(path: &Path, json: &str) -> io::Result<()> {
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::other("retained comparison has no file name"))?
        .to_string_lossy()
        .into_owned();
    let temporary = path.with_file_name(format!("{name}.tmp"));
    if let Err(error) = fs::write(&temporary, json.as_bytes()) {
        remove(&temporary);
        return Err(error);
    }
    if let Err(error) = fs::rename(&temporary, path) {
        remove(&temporary);
        return Err(error);
    }
    Ok(())
}

/// Removes the oldest retained comparisons beyond [`DIFF_RETENTION_LIMIT`],
/// always keeping the record that was just written.
fn prune_comparisons(directory: &Path, protected: &Path) -> io::Result<Option<String>> {
    let older: Vec<PathBuf> = retained_comparisons(directory)?
        .into_iter()
        .filter(|(_, path)| path.as_path() != protected)
        .map(|(_, path)| path)
        .collect();
    let excess = older.len().saturating_sub(DIFF_RETENTION_LIMIT - 1);
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

/// Retained comparisons, oldest first by creation identity.
fn retained_comparisons(directory: &Path) -> io::Result<Vec<(u64, PathBuf)>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if let Some(identity) = comparison_identity(&name) {
            files.push((identity, entry.path()));
        }
    }
    files.sort_by_key(|(identity, _)| *identity);
    Ok(files)
}

/// A retained name is `diff-<identity digits>.json` with the zero-padded
/// identity width; any other name in the directory belongs to someone else.
fn comparison_identity(name: &str) -> Option<u64> {
    let identity = name.strip_prefix("diff-")?.strip_suffix(".json")?;
    if identity.len() < IDENTITY_WIDTH || !identity.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    identity.parse().ok()
}
