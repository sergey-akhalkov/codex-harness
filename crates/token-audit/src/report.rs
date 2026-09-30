//! Sessions-directory discovery, streaming scan and aggregation.
//!
//! Each rollout file is read once through the shared tolerant reader, reduced
//! to one session row and dropped; only small aggregates stay in memory.
use crate::{
    LIMITATION, SCHEMA_VERSION, UNAVAILABLE_COUNTER_OVERFLOW,
    UNAVAILABLE_INCOMPLETE_RESPONSE_RECORDS, UNAVAILABLE_INCOMPLETE_TURN_INPUT,
    UNAVAILABLE_MISSING_CACHE_FIELDS, UNAVAILABLE_MISSING_TURN_IDENTITY,
    UNAVAILABLE_NO_TURN_RECORDS, UNAVAILABLE_ZERO_FINAL_INPUT, UNAVAILABLE_ZERO_INPUT_TOKENS,
    WARN_COUNTER_TOTAL_OVERFLOW, WARN_MISSING_SESSION_USAGE, WARN_MIXED_USAGE_BASIS,
    WARN_SESSION_TIMESTAMP_MISSING,
    identity::{directory_identity, project_identity},
    model::{
        ACCOUNTING_VERSION, ATTRIBUTION_UNASSIGNED, Accounting, BASIS_CUMULATIVE_INCREMENT,
        BASIS_RESPONSE_DELTA, BASIS_RESPONSE_SUM, BASIS_THREAD_CUMULATIVE, BASIS_UNAVAILABLE,
        Bucket, ContextAggregate, CoverageReport, DAY_BASIS_SESSION_START, FORMAT_TOKEN_COUNT,
        FORMAT_USAGE_RECORD, IncrementalStats, IntervalBucket, IntervalCoverage, IntervalPolicy,
        IntervalUsage, IntervalWindow, MODE_INTERVAL, Report, Scan, SessionContext,
        SessionInterval, SessionRow, TokenTotals,
    },
};
use chrono::{DateTime, Duration, NaiveDate, SecondsFormat, Utc};
use harness_core::rollout_reader::{
    self, CheckpointError, Coverage, InstructionBytes, ParserCheckpoint, SessionSummary, TurnUsage,
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs, io,
    path::{Path, PathBuf},
    time::{Instant, SystemTime},
};

/// The reader's sentinel for sessions whose events name more than one project.
const MIXED_PROJECT: &str = "mixed";

/// Inputs of one analyzer scan.
#[derive(Clone, Debug)]
pub struct ScanOptions {
    /// Directory holding rollout session files, or a single rollout file.
    pub sessions_root: PathBuf,
    /// Bounds the scan to sessions whose recorded activity is within this many
    /// days of `generated_at`.
    pub days: Option<u32>,
    /// Timestamp used as the scan time and written into the report.
    pub generated_at: DateTime<Utc>,
    /// Interval accounting window, half-open `[start, end)`, in UTC. `None`
    /// keeps the activity view: lifetime totals of sessions active in
    /// `days`.
    pub interval: Option<(DateTime<Utc>, DateTime<Utc>)>,
    /// Disposable checkpoint directory enabling incremental reuse. `None`
    /// parses every discovered file.
    pub checkpoints: Option<PathBuf>,
}

/// Default disposable checkpoint directory under the resolved Codex home.
///
/// This matches the session-root and retention conventions: `CODEX_HOME` when
/// set, otherwise `USERPROFILE/.codex`.
pub fn default_checkpoint_directory() -> Option<PathBuf> {
    std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(|home| PathBuf::from(home).join(".codex")))
        .map(|home| home.join("harness").join("token-audit").join("checkpoints"))
}

/// Checkpoints kept in one directory; older ones are pruned after a write.
const CHECKPOINT_LIMIT: usize = 512;
/// Total bytes of checkpoints kept in one directory. The store is a
/// disposable cache, so it is bounded in size, not only in file count; a
/// history larger than this bound is partially reused and the remainder is
/// parsed in full.
const CHECKPOINT_TOTAL_BYTES: u64 = 256 * 1024 * 1024;
/// Largest checkpoint this analyzer stores; larger parser state falls back to
/// full parsing on the next scan instead of growing unbounded local state.
const CHECKPOINT_BYTES_LIMIT: usize = 16 * 1024 * 1024;

/// Stable local name of one file's checkpoint: the hash of its resolved path.
fn checkpoint_name(path: &Path) -> io::Result<String> {
    let resolved = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let mut name = Sha256::new();
    name.update(resolved.as_os_str().as_encoded_bytes());
    Ok(format!("{:x}.json", name.finalize()))
}

/// A stored checkpoint is only touched when its name matches this shape, so
/// unrelated files in the directory are never read or removed.
fn checkpoint_stored_name(name: &str) -> bool {
    name.len() == 69
        && name.ends_with(".json")
        && name.as_bytes().iter().take(64).all(u8::is_ascii_hexdigit)
}

/// Loads one file's checkpoint, naming why it could not be used.
fn load_checkpoint(
    directory: &Path,
    path: &Path,
) -> (Option<ParserCheckpoint>, Option<&'static str>) {
    let Ok(name) = checkpoint_name(path) else {
        return (None, Some("checkpoint_unreadable"));
    };
    let bytes = match fs::read(directory.join(name)) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return (None, Some("no_checkpoint"));
        }
        Err(_) => return (None, Some("checkpoint_unreadable")),
    };
    match ParserCheckpoint::from_bytes(&bytes) {
        Ok(checkpoint) => (Some(checkpoint), None),
        Err(CheckpointError::VersionMismatch) => (None, Some("checkpoint_version_mismatch")),
        Err(CheckpointError::Corrupt) => (None, Some("checkpoint_corrupt")),
    }
}

/// Writes one checkpoint atomically; a failure leaves the previous state.
fn store_checkpoint(
    directory: &Path,
    path: &Path,
    checkpoint: &ParserCheckpoint,
) -> io::Result<()> {
    let bytes = checkpoint.to_bytes()?;
    if bytes.len() > CHECKPOINT_BYTES_LIMIT {
        return Err(io::Error::other("checkpoint exceeds the local size bound"));
    }
    fs::create_dir_all(directory)?;
    let name = checkpoint_name(path)?;
    let staging = directory.join(format!("{name}.{}.tmp", std::process::id()));
    fs::write(&staging, &bytes)?;
    if let Err(error) = fs::rename(&staging, directory.join(&name)) {
        let _ = fs::remove_file(&staging);
        return Err(error);
    }
    Ok(())
}

/// Removes the oldest checkpoints beyond [`CHECKPOINT_LIMIT`], keeping
/// `protected`. Returns how many were pruned.
fn prune_checkpoints(directory: &Path, protected: Option<&Path>) -> usize {
    let Ok(entries) = fs::read_dir(directory) else {
        return 0;
    };
    let mut stored: Vec<(SystemTime, u64, PathBuf)> = Vec::new();
    let mut total = 0u64;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !checkpoint_stored_name(&name) {
            continue;
        }
        let path = entry.path();
        if protected.is_some_and(|protected| path == protected) {
            continue;
        }
        let metadata = entry.metadata().ok();
        let modified = metadata
            .as_ref()
            .and_then(|metadata| metadata.modified().ok())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        let bytes = metadata.map(|metadata| metadata.len()).unwrap_or(0);
        total = total.saturating_add(bytes);
        stored.push((modified, bytes, path));
    }
    stored.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.2.cmp(&right.2)));
    let excess = stored
        .len()
        .saturating_sub(CHECKPOINT_LIMIT.saturating_sub(1));
    let mut pruned = 0;
    for (index, (_, bytes, path)) in stored.into_iter().enumerate() {
        if index >= excess && total <= CHECKPOINT_TOTAL_BYTES {
            break;
        }
        if fs::remove_file(&path).is_ok() {
            total = total.saturating_sub(bytes);
            pruned += 1;
        }
    }
    pruned
}

/// The configured Codex home sessions directory.
pub fn default_sessions_root() -> Option<PathBuf> {
    std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(|home| PathBuf::from(home).join(".codex")))
        .map(|home| home.join("sessions"))
}

/// Current time from the system clock. The crate does not enable chrono's
/// `clock` feature, matching the rest of the workspace.
pub fn now() -> DateTime<Utc> {
    let elapsed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    DateTime::from_timestamp(elapsed.as_secs() as i64, elapsed.subsec_nanos()).unwrap_or_default()
}

/// Sorted rollout files under a sessions directory, or the single given file.
pub fn discover(root: &Path) -> io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    if root.is_file() {
        files.push(root.to_owned());
    } else {
        walk(root, &mut files)?;
    }
    files.sort();
    Ok(files)
}

fn walk(directory: &Path, files: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            walk(&path, files)?;
        } else if path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("jsonl"))
        {
            files.push(path);
        }
    }
    Ok(())
}

/// Scans the sessions directory and aggregates recorded usage.
///
/// Every discovered rollout file is read once through the shared reader and
/// reduced to one session row; malformed, unknown and missing records only
/// increase coverage counters.
pub fn scan(options: &ScanOptions) -> io::Result<Scan> {
    let started = Instant::now();
    let files = discover(&options.sessions_root)?;
    let files_discovered = files.len();
    let cutoff = options
        .days
        .map(|days| options.generated_at - Duration::days(i64::from(days)));
    let mut coverage = CoverageReport::default();
    let mut sessions = Vec::new();
    let mut projects = BTreeMap::new();
    let mut inputs = Vec::new();
    let mut incremental = IncrementalStats::disabled();
    incremental.enabled = options.checkpoints.is_some();
    let mut interval = IntervalAggregate::default();
    for path in files {
        let read = match &options.checkpoints {
            Some(directory) => {
                let (checkpoint, load_reason) = load_checkpoint(directory, &path);
                let read = rollout_reader::read_incremental(&path, checkpoint);
                if let Some(reason) = read.invalidation.or(load_reason) {
                    *incremental
                        .invalidations
                        .entry(reason.to_owned())
                        .or_default() += 1;
                }
                if let Some(next) = &read.checkpoint {
                    match store_checkpoint(directory, &path, next) {
                        Ok(()) => incremental.checkpoints_written += 1,
                        Err(_) => incremental.checkpoint_write_failures += 1,
                    }
                }
                read
            }
            None => rollout_reader::read_incremental(&path, None),
        };
        incremental.bytes_read = incremental.bytes_read.saturating_add(read.bytes_read);
        incremental.events_parsed = incremental.events_parsed.saturating_add(read.events_parsed);
        if read.reused {
            incremental.files_reused += 1;
        } else {
            incremental.files_full_parsed += 1;
        }
        incremental.bytes_discovered = incremental.bytes_discovered.saturating_add(
            read.summary
                .source
                .as_ref()
                .and_then(|file| file.metadata().ok())
                .map_or(0, |metadata| metadata.len()),
        );
        let summary = read.summary;
        coverage.files_scanned += 1;
        add_file_coverage(&mut coverage, &summary.coverage);
        inputs.push(path.clone());
        let first = rollout_reader::timestamp(&summary.row["first_timestamp"]);
        let last = rollout_reader::timestamp(&summary.row["last_timestamp"]);
        if cutoff.is_some_and(|cutoff| last.is_some_and(|last| last < cutoff)) {
            coverage.sessions_excluded_by_window += 1;
            continue;
        }
        let mut row = FileScan {
            path: &path,
            summary: &summary,
            first,
            last,
        }
        .session_row(&mut projects);
        if let Some((start, end)) = options.interval {
            let outcome = analyze_interval(&summary, start, end);
            interval.absorb(&outcome);
            row.interval = Some(outcome.row);
        }
        for code in &row.warnings {
            *coverage.warning_counts.entry(code.clone()).or_default() += 1;
        }
        for format in &row.formats {
            *coverage.formats.entry(*format).or_default() += 1;
        }
        *coverage
            .usage_basis
            .entry(row.usage_basis.unwrap_or(BASIS_UNAVAILABLE).to_owned())
            .or_default() += 1;
        coverage.sessions_without_project += usize::from(row.project.is_none());
        coverage.sessions_without_model += usize::from(row.model.is_none());
        coverage.sessions_without_effort += usize::from(row.effort.is_none());
        coverage.sessions_without_day += usize::from(row.day.is_none());
        coverage.sessions_without_usage += usize::from(row.usage_basis.is_none());
        coverage.sessions_without_turn_metrics +=
            usize::from(row.context.turn_metrics_unavailable.is_some());
        sessions.push(row);
    }
    if options.checkpoints.is_some()
        && incremental.checkpoints_written > 0
        && let Some(directory) = &options.checkpoints
    {
        incremental.checkpoints_pruned = prune_checkpoints(directory, None);
    }
    incremental.scan_millis = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    coverage.sessions = sessions.len();
    let totals_accumulator = accumulator_of(&sessions);
    let mut overflow = totals_accumulator.overflowed();
    let totals = totals_accumulator.finish("all");
    let (by_project, project_overflow) = bucketize(&sessions, |row| row.project.clone());
    let (by_model, model_overflow) = bucketize(&sessions, |row| row.model.clone());
    let (by_effort, effort_overflow) = bucketize(&sessions, |row| row.effort.clone());
    let (by_day, day_overflow) = bucketize(&sessions, |row| row.day.clone());
    overflow |= project_overflow || model_overflow || effort_overflow || day_overflow;
    coverage.mixed_usage_basis = totals.usage_basis.contains_key(BASIS_THREAD_CUMULATIVE)
        && totals.usage_basis.contains_key(BASIS_RESPONSE_SUM);
    if coverage.mixed_usage_basis {
        *coverage
            .warning_counts
            .entry(WARN_MIXED_USAGE_BASIS.to_owned())
            .or_default() += 1;
    }
    if overflow {
        *coverage
            .warning_counts
            .entry(WARN_COUNTER_TOTAL_OVERFLOW.to_owned())
            .or_default() += 1;
    }
    coverage.partial = !coverage.warning_counts.is_empty();
    let accounting = match options.interval {
        Some((start, end)) => Accounting {
            version: ACCOUNTING_VERSION,
            mode: MODE_INTERVAL.to_owned(),
            window_days: options.days,
            day_basis: DAY_BASIS_SESSION_START,
            interval: Some(IntervalWindow {
                start: window_stamp(start),
                end: window_stamp(end),
                basis: "recorded_event_timestamp",
                timestamp_source: "rollout_jsonl_event_timestamp_field",
                boundary: "start_inclusive_end_exclusive_utc",
                policy: IntervalPolicy {
                    duplication: "stable response identity counted once; conflicting records stay unknown",
                    cumulative: "only increments measured between two recorded observations are attributed",
                    reset: "a counter decrease or a missing endpoint leaves the increment unknown",
                    boundary_crossing: "an increment spanning a window boundary is unallocated",
                    missing_timestamp: "a recorded amount without a usable timestamp is unallocated",
                    attribution: "recorded turn context per usage unit; otherwise unassigned",
                },
            }),
        },
        None => Accounting::activity(options.days),
    };
    let interval = options.interval.map(|(_, _)| interval.finish());
    let report = Report {
        schema_version: SCHEMA_VERSION,
        command: "report",
        generated_at: options
            .generated_at
            .to_rfc3339_opts(SecondsFormat::Secs, true),
        sessions_root: directory_identity(&options.sessions_root),
        window_days: options.days,
        files_discovered,
        sessions,
        by_project,
        by_model,
        by_effort,
        by_day,
        totals,
        coverage,
        accounting,
        interval,
        incremental,
        limitation: LIMITATION,
    };
    Ok(Scan {
        report,
        projects,
        inputs,
    })
}

fn add_file_coverage(coverage: &mut CoverageReport, file: &Coverage) {
    coverage.lines = coverage.lines.saturating_add(file.lines);
    coverage.events = coverage.events.saturating_add(file.events);
    coverage.recognized_events = coverage
        .recognized_events
        .saturating_add(file.recognized_events);
    coverage.unrecognized_events = coverage
        .unrecognized_events
        .saturating_add(file.unrecognized_events);
    coverage.corrupt_lines = coverage.corrupt_lines.saturating_add(file.corrupt_lines);
    coverage.oversized_lines = coverage
        .oversized_lines
        .saturating_add(file.oversized_lines);
}

fn bucketize<F>(rows: &[SessionRow], key_of: F) -> (Vec<Bucket>, bool)
where
    F: Fn(&SessionRow) -> Option<String>,
{
    let mut buckets: BTreeMap<String, Accumulator> = BTreeMap::new();
    for row in rows {
        if let Some(key) = key_of(row) {
            buckets.entry(key).or_default().push(row);
        }
    }
    let mut overflow = false;
    let mut finished = Vec::with_capacity(buckets.len());
    for (key, accumulator) in buckets {
        overflow |= accumulator.overflowed();
        finished.push(accumulator.finish(&key));
    }
    (finished, overflow)
}

fn accumulator_of(rows: &[SessionRow]) -> Accumulator {
    let mut accumulator = Accumulator::default();
    for row in rows {
        accumulator.push(row);
    }
    accumulator
}

/// Recorded session totals with the basis they came from.
fn recorded_usage(turns: &[TurnUsage], raw: &Value) -> (Option<&'static str>, TokenTotals) {
    if let Some(thread) = turns
        .iter()
        .rev()
        .find_map(|turn| turn.thread_usage.as_ref())
    {
        return (
            Some(BASIS_THREAD_CUMULATIVE),
            TokenTotals::from_usage(thread),
        );
    }
    let row_totals = TokenTotals::from_row(raw);
    if row_totals.recorded() > 0 {
        return (Some(BASIS_THREAD_CUMULATIVE), row_totals);
    }
    if turns.is_empty() {
        return (None, TokenTotals::default());
    }
    let summed = response_delta_totals(turns);
    if summed.recorded() > 0 {
        return (Some(BASIS_RESPONSE_SUM), summed);
    }
    (None, TokenTotals::default())
}

/// Sum of the counters recorded for individual responses; a counter stays
/// unknown only when no response recorded it.
fn response_delta_totals(turns: &[TurnUsage]) -> TokenTotals {
    let mut fields = [FieldTotal::default(); 5];
    for turn in turns {
        for (slot, value) in fields
            .iter_mut()
            .zip(TokenTotals::from_usage(&turn.usage).fields())
        {
            slot.push(value);
        }
    }
    let rows = turns.len();
    TokenTotals {
        input_tokens: fields[0].value(rows),
        cached_input_tokens: fields[1].value(rows),
        output_tokens: fields[2].value(rows),
        reasoning_output_tokens: fields[3].value(rows),
        total_tokens: fields[4].value(rows),
    }
}

fn session_context(
    turns: &[TurnUsage],
    totals: &TokenTotals,
    warnings: &BTreeSet<String>,
    instructions: &InstructionBytes,
) -> SessionContext {
    let (cached_input_ratio, cache_efficiency_unavailable) =
        match (totals.cached_input_tokens, totals.input_tokens) {
            (Some(cached), Some(input)) if input > 0 => (Some(cached as f64 / input as f64), None),
            (Some(_), Some(_)) => (None, Some(UNAVAILABLE_ZERO_INPUT_TOKENS.to_owned())),
            _ => (None, Some(UNAVAILABLE_MISSING_CACHE_FIELDS.to_owned())),
        };
    let (summed, final_input, repayment, turn_metrics_unavailable) =
        turn_economics(turns, warnings);
    SessionContext {
        cached_input_ratio,
        cache_efficiency_unavailable,
        summed_turn_input_tokens: summed,
        final_turn_input_tokens: final_input,
        repayment_multiplier: repayment,
        turn_metrics_unavailable,
        instruction_base_bytes: instructions.base_bytes,
        instruction_developer_bytes: instructions.developer_bytes,
    }
}

/// Summed per-turn input and the final turn's input, or an explicit marker.
fn turn_economics(
    turns: &[TurnUsage],
    warnings: &BTreeSet<String>,
) -> (Option<u64>, Option<u64>, Option<f64>, Option<String>) {
    fn unavailable(reason: &str) -> (Option<u64>, Option<u64>, Option<f64>, Option<String>) {
        (None, None, None, Some(reason.to_owned()))
    }
    if turns.is_empty() {
        return unavailable(UNAVAILABLE_NO_TURN_RECORDS);
    }
    if warnings.contains("missing_response_id") || warnings.contains("conflicting_response_id") {
        return unavailable(UNAVAILABLE_INCOMPLETE_RESPONSE_RECORDS);
    }
    if turns.iter().any(|turn| turn.turn_id.is_none()) {
        return unavailable(UNAVAILABLE_MISSING_TURN_IDENTITY);
    }
    let mut summed: Option<u64> = Some(0);
    let mut final_input = None;
    for turn in turns {
        let Some(input) = turn.usage.get("input_tokens").copied().flatten() else {
            return unavailable(UNAVAILABLE_INCOMPLETE_TURN_INPUT);
        };
        final_input = Some(input);
        summed = summed.and_then(|total| total.checked_add(input));
    }
    if summed.is_none() {
        return unavailable(UNAVAILABLE_COUNTER_OVERFLOW);
    }
    let repayment = match (summed, final_input) {
        (Some(sum), Some(final_input)) if final_input > 0 => Some(sum as f64 / final_input as f64),
        _ => None,
    };
    let unavailable = repayment
        .is_none()
        .then(|| UNAVAILABLE_ZERO_FINAL_INPUT.to_owned());
    (summed, final_input, repayment, unavailable)
}

/// Session date from the rollout file name, used for the day bucket when the
/// session records no timestamps.
fn path_date(path: &Path) -> Option<NaiveDate> {
    let name = path.file_name().and_then(|name| name.to_str())?;
    for (index, _) in name.char_indices() {
        let Some(window) = name.get(index..index + 10) else {
            continue;
        };
        let bytes = window.as_bytes();
        let shape = bytes.iter().enumerate().all(|(offset, byte)| {
            if matches!(offset, 4 | 7) {
                *byte == b'-'
            } else {
                byte.is_ascii_digit()
            }
        });
        if shape && let Ok(date) = NaiveDate::parse_from_str(window, "%Y-%m-%d") {
            return Some(date);
        }
    }
    None
}

struct FileScan<'a> {
    path: &'a Path,
    summary: &'a SessionSummary,
    first: Option<DateTime<Utc>>,
    last: Option<DateTime<Utc>>,
}

impl FileScan<'_> {
    fn session_row(&self, projects: &mut BTreeMap<String, String>) -> SessionRow {
        let mut warnings: BTreeSet<String> = self.summary.warnings.iter().cloned().collect();
        if self.first.is_none() {
            warnings.insert(WARN_SESSION_TIMESTAMP_MISSING.to_owned());
        }
        let day = self
            .first
            .map(|time| time.date_naive())
            .or_else(|| path_date(self.path));
        let project = self.summary.row["project"]
            .as_str()
            .filter(|label| *label != MIXED_PROJECT)
            .map(|label| {
                let identity = project_identity(label);
                projects
                    .entry(identity.clone())
                    .or_insert_with(|| label.to_owned());
                identity
            });
        let (usage_basis, usage) = recorded_usage(&self.summary.turns, &self.summary.row);
        if usage_basis.is_none() {
            warnings.insert(WARN_MISSING_SESSION_USAGE.to_owned());
        }
        let mut formats = Vec::new();
        if !self.summary.turns.is_empty() {
            formats.push(FORMAT_USAGE_RECORD);
        }
        if TokenTotals::from_row(&self.summary.row).recorded() > 0 {
            formats.push(FORMAT_TOKEN_COUNT);
        }
        let context = session_context(
            &self.summary.turns,
            &usage,
            &warnings,
            &self.summary.instructions,
        );
        SessionRow {
            session_id: self.summary.row["id"].as_str().map(str::to_owned),
            project,
            model: self.summary.row["model"].as_str().map(str::to_owned),
            effort: self.summary.row["reasoning"].as_str().map(str::to_owned),
            day: day.map(|date| date.to_string()),
            first_timestamp: self.first.map(stamp),
            last_timestamp: self.last.map(stamp),
            response_count: self.summary.turns.len(),
            conflicting_response_ids: rollout_reader::list(
                &self.summary.row["response_conflict_ids"],
            )
            .len(),
            formats,
            usage_basis,
            usage,
            context,
            tool_output_bytes: self.summary.tool_output_bytes.clone(),
            elapsed_seconds: self.summary.row["elapsed_seconds"].as_i64(),
            partial: !warnings.is_empty(),
            warnings: warnings.into_iter().collect(),
            interval: None,
        }
    }
}

fn stamp(time: DateTime<Utc>) -> String {
    time.to_rfc3339_opts(SecondsFormat::Secs, true)
}

/// RFC3339 UTC stamp of one declared window boundary, keeping sub-second
/// precision when the caller supplied it.
fn window_stamp(time: DateTime<Utc>) -> String {
    time.to_rfc3339_opts(SecondsFormat::AutoSi, true)
}

/// Interval coverage counters of one session or of a whole scan.
#[derive(Clone, Copy, Default)]
struct IntervalCounts {
    response_events_inside: u64,
    response_events_before: u64,
    response_events_after: u64,
    response_events_undated: u64,
    response_events_conflicting: u64,
    response_events_unidentified: u64,
    cumulative_steps_inside: u64,
    cumulative_steps_before: u64,
    cumulative_steps_after: u64,
    cumulative_steps_boundary: u64,
    cumulative_steps_reset: u64,
    cumulative_steps_incomplete: u64,
    cumulative_steps_undated: u64,
    cumulative_steps_day_crossing: u64,
}

impl IntervalCounts {
    fn merge(&mut self, other: &Self) {
        self.response_events_inside += other.response_events_inside;
        self.response_events_before += other.response_events_before;
        self.response_events_after += other.response_events_after;
        self.response_events_undated += other.response_events_undated;
        self.response_events_conflicting += other.response_events_conflicting;
        self.response_events_unidentified += other.response_events_unidentified;
        self.cumulative_steps_inside += other.cumulative_steps_inside;
        self.cumulative_steps_before += other.cumulative_steps_before;
        self.cumulative_steps_after += other.cumulative_steps_after;
        self.cumulative_steps_boundary += other.cumulative_steps_boundary;
        self.cumulative_steps_reset += other.cumulative_steps_reset;
        self.cumulative_steps_incomplete += other.cumulative_steps_incomplete;
        self.cumulative_steps_undated += other.cumulative_steps_undated;
        self.cumulative_steps_day_crossing += other.cumulative_steps_day_crossing;
    }
}

/// Per-counter amount accumulation following the recorded-value convention of
/// the existing aggregates: a counter stays `None` when no recorded value
/// exists for it and an unknown amount is possible.
#[derive(Clone, Copy, Default)]
struct IntervalFields {
    sums: [Option<u64>; 5],
    recorded: [bool; 5],
    unknown: [bool; 5],
}

impl IntervalFields {
    fn push(&mut self, totals: &TokenTotals) {
        for (index, value) in totals.fields().into_iter().enumerate() {
            if let Some(value) = value {
                self.recorded[index] = true;
                self.sums[index] = Some(self.sums[index].unwrap_or(0).saturating_add(value));
            }
        }
    }

    /// One of the counters covered by `mask` has an unknown amount.
    fn mark_unknown(&mut self, mask: [bool; 5]) {
        for (index, unknown) in mask.into_iter().enumerate() {
            self.unknown[index] |= unknown;
        }
    }

    /// `evidence` says that the session recorded usable interval evidence, so
    /// a counter with no recorded amount is a known zero rather than unknown.
    fn finish(self, evidence: bool) -> TokenTotals {
        recorded_totals(std::array::from_fn(|index| {
            if self.recorded[index] {
                self.sums[index]
            } else if self.unknown[index] || !evidence {
                None
            } else {
                Some(0)
            }
        }))
    }
}

/// Counters in report order.
fn recorded_totals(fields: [Option<u64>; 5]) -> TokenTotals {
    let [
        input_tokens,
        cached_input_tokens,
        output_tokens,
        reasoning_output_tokens,
        total_tokens,
    ] = fields;
    TokenTotals {
        input_tokens,
        cached_input_tokens,
        output_tokens,
        reasoning_output_tokens,
        total_tokens,
    }
}

/// Which counters one recorded amount covers.
fn recorded_mask(totals: &TokenTotals) -> [bool; 5] {
    totals.fields().map(|value| value.is_some())
}

/// Where one recorded usage unit lies relative to the declared window.
enum Position {
    Before,
    After,
    Inside(Option<NaiveDate>),
    /// The amount is known but cannot be split at a window boundary.
    Boundary,
    /// No usable recorded timestamp exists.
    Undated,
}

fn step_position(
    previous: Option<DateTime<Utc>>,
    current: Option<DateTime<Utc>>,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> Position {
    let (Some(previous), Some(current)) = (previous, current) else {
        return Position::Undated;
    };
    if current < start {
        return Position::Before;
    }
    if previous >= end {
        return Position::After;
    }
    if previous >= start && current < end {
        let day = (previous.date_naive() == current.date_naive()).then(|| current.date_naive());
        return Position::Inside(day);
    }
    Position::Boundary
}

/// In-window usage of one bucket key inside one session.
#[derive(Clone, Copy, Default)]
struct KeyUsage {
    units: u64,
    fields: IntervalFields,
}

/// Accumulated bucket usage over sessions.
#[derive(Clone, Copy, Default)]
struct BucketUsage {
    sessions: usize,
    units: u64,
    fields: [FieldTotal; 5],
}

impl BucketUsage {
    fn absorb(&mut self, usage: &KeyUsage) {
        self.sessions += 1;
        self.units += usage.units;
        let totals = usage.fields.finish(true);
        for (slot, value) in self.fields.iter_mut().zip(totals.fields()) {
            slot.push(value);
        }
    }

    fn finish(self, key: String) -> IntervalBucket {
        let rows = self.sessions;
        IntervalBucket {
            key,
            sessions: self.sessions,
            units: self.units,
            usage: recorded_totals(std::array::from_fn(|index| self.fields[index].value(rows))),
        }
    }
}

/// One session's interval accounting.
struct IntervalOutcome {
    row: SessionInterval,
    counts: IntervalCounts,
    by_model: BTreeMap<String, KeyUsage>,
    by_effort: BTreeMap<String, KeyUsage>,
    by_day: BTreeMap<String, KeyUsage>,
    has_usage: bool,
    has_unallocated: bool,
    has_evidence: bool,
}

/// Sum of one class over the sessions that recorded it.
#[derive(Clone, Copy, Default)]
struct TotalsSum {
    fields: [FieldTotal; 5],
    rows: usize,
}

impl TotalsSum {
    fn push(&mut self, totals: &TokenTotals) {
        self.rows += 1;
        for (slot, value) in self.fields.iter_mut().zip(totals.fields()) {
            slot.push(value);
        }
    }

    fn finish(self) -> TokenTotals {
        let rows = self.rows;
        recorded_totals(std::array::from_fn(|index| self.fields[index].value(rows)))
    }
}

/// Accumulated interval accounting of one scan.
#[derive(Default)]
struct IntervalAggregate {
    counts: IntervalCounts,
    usage: TotalsSum,
    unallocated: TotalsSum,
    day_unresolved: TotalsSum,
    unallocated_events: BTreeMap<String, u64>,
    by_model: BTreeMap<String, BucketUsage>,
    by_effort: BTreeMap<String, BucketUsage>,
    by_day: BTreeMap<String, BucketUsage>,
    sessions_with_usage: usize,
    sessions_with_unallocated: usize,
    sessions_without_evidence: usize,
}

impl IntervalAggregate {
    fn absorb(&mut self, outcome: &IntervalOutcome) {
        self.counts.merge(&outcome.counts);
        if outcome.has_evidence {
            self.usage.push(&outcome.row.usage);
        }
        if outcome.has_evidence || outcome.has_unallocated {
            self.unallocated.push(&outcome.row.unallocated);
            self.day_unresolved.push(&outcome.row.day_unresolved);
        }
        for (reason, count) in &outcome.row.unallocated_events {
            *self.unallocated_events.entry(reason.clone()).or_default() += count;
        }
        for (key, usage) in &outcome.by_model {
            self.by_model.entry(key.clone()).or_default().absorb(usage);
        }
        for (key, usage) in &outcome.by_effort {
            self.by_effort.entry(key.clone()).or_default().absorb(usage);
        }
        for (key, usage) in &outcome.by_day {
            self.by_day.entry(key.clone()).or_default().absorb(usage);
        }
        self.sessions_with_usage += usize::from(outcome.has_usage);
        self.sessions_with_unallocated += usize::from(outcome.has_unallocated);
        self.sessions_without_evidence += usize::from(!outcome.has_evidence);
    }

    fn finish(self) -> IntervalUsage {
        let coverage = IntervalCoverage {
            sessions_with_usage: self.sessions_with_usage,
            sessions_with_unallocated: self.sessions_with_unallocated,
            sessions_without_evidence: self.sessions_without_evidence,
            response_events_inside: self.counts.response_events_inside,
            response_events_before: self.counts.response_events_before,
            response_events_after: self.counts.response_events_after,
            response_events_undated: self.counts.response_events_undated,
            response_events_conflicting: self.counts.response_events_conflicting,
            response_events_unidentified: self.counts.response_events_unidentified,
            cumulative_steps_inside: self.counts.cumulative_steps_inside,
            cumulative_steps_before: self.counts.cumulative_steps_before,
            cumulative_steps_after: self.counts.cumulative_steps_after,
            cumulative_steps_boundary: self.counts.cumulative_steps_boundary,
            cumulative_steps_reset: self.counts.cumulative_steps_reset,
            cumulative_steps_incomplete: self.counts.cumulative_steps_incomplete,
            cumulative_steps_undated: self.counts.cumulative_steps_undated,
            cumulative_steps_day_crossing: self.counts.cumulative_steps_day_crossing,
        };
        let finish = |buckets: BTreeMap<String, BucketUsage>| {
            buckets
                .into_iter()
                .map(|(key, usage)| usage.finish(key))
                .collect()
        };
        IntervalUsage {
            usage: self.usage.finish(),
            unallocated: self.unallocated.finish(),
            unallocated_events: self.unallocated_events,
            coverage,
            by_model: finish(self.by_model),
            by_effort: finish(self.by_effort),
            by_day: finish(self.by_day),
            day_unresolved: self.day_unresolved.finish(),
        }
    }
}

fn reason_count(reasons: &mut BTreeMap<String, u64>, reason: &str) {
    *reasons.entry(reason.to_owned()).or_default() += 1;
}

/// Attribute one in-window amount to its model, effort and day buckets.
fn attribute(
    outcome_model: &mut BTreeMap<String, KeyUsage>,
    outcome_effort: &mut BTreeMap<String, KeyUsage>,
    outcome_day: Option<&mut BTreeMap<String, KeyUsage>>,
    attribution: (&Option<String>, &Option<String>),
    day: Option<&str>,
    totals: &TokenTotals,
) {
    for (buckets, value) in [
        (outcome_model, attribution.0),
        (outcome_effort, attribution.1),
    ] {
        let key = value
            .clone()
            .unwrap_or_else(|| ATTRIBUTION_UNASSIGNED.to_owned());
        let usage = buckets.entry(key).or_default();
        usage.units += 1;
        usage.fields.push(totals);
    }
    if let (Some(buckets), Some(day)) = (outcome_day, day) {
        let usage = buckets.entry(day.to_owned()).or_default();
        usage.units += 1;
        usage.fields.push(totals);
    }
}

/// Interval accounting of one session under the declared reset, gap and
/// boundary policy. Only identifiable usage units are attributed; everything
/// else stays visibly unallocated.
fn analyze_interval(
    summary: &SessionSummary,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> IntervalOutcome {
    let mut counts = IntervalCounts::default();
    let mut inside = IntervalFields::default();
    let mut unallocated = IntervalFields::default();
    let mut day_unresolved = IntervalFields::default();
    let mut reasons: BTreeMap<String, u64> = BTreeMap::new();
    let mut by_model: BTreeMap<String, KeyUsage> = BTreeMap::new();
    let mut by_effort: BTreeMap<String, KeyUsage> = BTreeMap::new();
    let mut by_day: BTreeMap<String, KeyUsage> = BTreeMap::new();
    let mut models: BTreeSet<String> = BTreeSet::new();
    let mut efforts: BTreeSet<String> = BTreeSet::new();
    let single = (
        summary.row["model"].as_str().map(str::to_owned),
        summary.row["reasoning"].as_str().map(str::to_owned),
    );
    let basis = if !summary.turns.is_empty() {
        BASIS_RESPONSE_DELTA
    } else if !summary.cumulative.is_empty() {
        BASIS_CUMULATIVE_INCREMENT
    } else {
        BASIS_UNAVAILABLE
    };
    let mut inside_units = 0u64;
    if basis == BASIS_RESPONSE_DELTA {
        for turn in &summary.turns {
            let recorded = TokenTotals::from_usage(&turn.usage);
            let conflicting = turn
                .response_id
                .as_ref()
                .is_some_and(|id| summary.conflicts.contains(id));
            if conflicting {
                counts.response_events_conflicting += 1;
                reason_count(&mut reasons, "conflicting_response_id");
                let mask = recorded_mask(&recorded);
                unallocated.mark_unknown(mask);
                // The identity's amount is unknown, so the interval cannot
                // claim a zero for the counters it recorded.
                inside.mark_unknown(mask);
                continue;
            }
            match turn.timestamp {
                None => {
                    counts.response_events_undated += 1;
                    reason_count(&mut reasons, "missing_timestamp");
                    unallocated.push(&recorded);
                    inside.mark_unknown(recorded_mask(&recorded));
                }
                Some(instant) if instant < start => counts.response_events_before += 1,
                Some(instant) if instant >= end => counts.response_events_after += 1,
                Some(instant) => {
                    counts.response_events_inside += 1;
                    inside_units += 1;
                    inside.push(&recorded);
                    let day = instant.date_naive().to_string();
                    attribute(
                        &mut by_model,
                        &mut by_effort,
                        Some(&mut by_day),
                        (&turn.model, &turn.effort),
                        Some(&day),
                        &recorded,
                    );
                    models.insert(
                        turn.model
                            .clone()
                            .unwrap_or_else(|| ATTRIBUTION_UNASSIGNED.to_owned()),
                    );
                    efforts.insert(
                        turn.effort
                            .clone()
                            .unwrap_or_else(|| ATTRIBUTION_UNASSIGNED.to_owned()),
                    );
                }
            }
        }
    } else if basis == BASIS_CUMULATIVE_INCREMENT {
        let mut previous: Option<&rollout_reader::UsageSnapshot> = None;
        for snapshot in &summary.cumulative {
            if let Some(previous) = previous {
                let mut deltas = [None; 5];
                let mut unknown = [false; 5];
                let mut reset = false;
                let mut incomplete = false;
                for (index, key) in rollout_reader::TOKEN_FIELDS.iter().enumerate() {
                    match (
                        previous.usage.get(*key).copied().flatten(),
                        snapshot.usage.get(*key).copied().flatten(),
                    ) {
                        (Some(before), Some(after)) if after >= before => {
                            deltas[index] = Some(after - before);
                        }
                        (Some(_), Some(_)) => {
                            reset = true;
                            unknown[index] = true;
                        }
                        _ => {
                            incomplete = true;
                            unknown[index] = true;
                        }
                    }
                }
                let recorded = recorded_totals(deltas);
                let position = step_position(previous.timestamp, snapshot.timestamp, start, end);
                if !matches!(position, Position::Before | Position::After) {
                    if reset {
                        counts.cumulative_steps_reset += 1;
                        reason_count(&mut reasons, "counter_reset");
                    }
                    if incomplete {
                        counts.cumulative_steps_incomplete += 1;
                        reason_count(&mut reasons, "incomplete_counters");
                    }
                }
                match position {
                    Position::Before => counts.cumulative_steps_before += 1,
                    Position::After => counts.cumulative_steps_after += 1,
                    Position::Undated => {
                        counts.cumulative_steps_undated += 1;
                        reason_count(&mut reasons, "missing_timestamp");
                        unallocated.push(&recorded);
                        inside.mark_unknown(recorded_mask(&recorded));
                        unallocated.mark_unknown(unknown);
                        inside.mark_unknown(unknown);
                    }
                    Position::Boundary => {
                        counts.cumulative_steps_boundary += 1;
                        reason_count(&mut reasons, "boundary_crossing");
                        unallocated.push(&recorded);
                        inside.mark_unknown(recorded_mask(&recorded));
                        unallocated.mark_unknown(unknown);
                    }
                    Position::Inside(day) => {
                        counts.cumulative_steps_inside += 1;
                        inside_units += 1;
                        inside.push(&recorded);
                        unallocated.mark_unknown(unknown);
                        inside.mark_unknown(unknown);
                        attribute(
                            &mut by_model,
                            &mut by_effort,
                            None,
                            (&single.0, &single.1),
                            None,
                            &recorded,
                        );
                        models.insert(
                            single
                                .0
                                .clone()
                                .unwrap_or_else(|| ATTRIBUTION_UNASSIGNED.to_owned()),
                        );
                        efforts.insert(
                            single
                                .1
                                .clone()
                                .unwrap_or_else(|| ATTRIBUTION_UNASSIGNED.to_owned()),
                        );
                        match day {
                            Some(day) => {
                                let usage = by_day.entry(day.to_string()).or_default();
                                usage.units += 1;
                                usage.fields.push(&recorded);
                            }
                            None => {
                                counts.cumulative_steps_day_crossing += 1;
                                day_unresolved.push(&recorded);
                            }
                        }
                    }
                }
            }
            previous = Some(snapshot);
        }
    }
    for snapshot in &summary.unidentified {
        let in_window = snapshot
            .timestamp
            .is_none_or(|instant| instant >= start && instant < end);
        if !in_window {
            continue;
        }
        counts.response_events_unidentified += 1;
        reason_count(&mut reasons, "missing_response_identity");
        let recorded = TokenTotals::from_usage(&snapshot.usage);
        unallocated.push(&recorded);
        inside.mark_unknown(recorded_mask(&recorded));
    }
    let evidence = basis != BASIS_UNAVAILABLE;
    let has_usage = inside_units > 0;
    let has_unallocated = reasons.values().any(|count| *count > 0);
    let mut model_list: Vec<String> = models.into_iter().collect();
    let mut effort_list: Vec<String> = efforts.into_iter().collect();
    if model_list.is_empty() {
        model_list.push(ATTRIBUTION_UNASSIGNED.to_owned());
    }
    if effort_list.is_empty() {
        effort_list.push(ATTRIBUTION_UNASSIGNED.to_owned());
    }
    let row = SessionInterval {
        basis,
        usage: inside.finish(evidence),
        unallocated: unallocated.finish(evidence),
        unallocated_events: reasons,
        models: model_list,
        efforts: effort_list,
        day_unresolved: day_unresolved.finish(evidence),
    };
    IntervalOutcome {
        row,
        counts,
        by_model,
        by_effort,
        by_day,
        has_usage,
        has_unallocated,
        has_evidence: evidence,
    }
}

#[derive(Clone, Copy)]
struct FieldTotal {
    sum: Option<u64>,
    recorded: usize,
}

impl Default for FieldTotal {
    fn default() -> Self {
        Self {
            sum: Some(0),
            recorded: 0,
        }
    }
}

impl FieldTotal {
    fn push(&mut self, value: Option<u64>) {
        if let Some(value) = value {
            self.recorded += 1;
            self.sum = self.sum.and_then(|total| total.checked_add(value));
        }
    }

    fn value(self, rows: usize) -> Option<u64> {
        if self.recorded == 0 && rows > 0 {
            None
        } else {
            self.sum
        }
    }

    fn overflowed(self) -> bool {
        self.recorded > 0 && self.sum.is_none()
    }
}

#[derive(Default)]
struct Accumulator {
    sessions: usize,
    responses: usize,
    basis: BTreeMap<String, usize>,
    input: FieldTotal,
    cached: FieldTotal,
    output: FieldTotal,
    reasoning: FieldTotal,
    total: FieldTotal,
    missing_usage: usize,
    with_cache: usize,
    without_cache: usize,
    summed_turn_input: FieldTotal,
    final_turn_input: FieldTotal,
    with_turn_metrics: usize,
    without_turn_metrics: usize,
    base_bytes: FieldTotal,
    developer_bytes: FieldTotal,
    with_instruction_bytes: usize,
    partial: bool,
}

impl Accumulator {
    fn push(&mut self, row: &SessionRow) {
        self.sessions += 1;
        self.responses += row.response_count;
        *self
            .basis
            .entry(row.usage_basis.unwrap_or(BASIS_UNAVAILABLE).to_owned())
            .or_default() += 1;
        self.input.push(row.usage.input_tokens);
        self.cached.push(row.usage.cached_input_tokens);
        self.output.push(row.usage.output_tokens);
        self.reasoning.push(row.usage.reasoning_output_tokens);
        self.total.push(row.usage.total_tokens);
        if row.usage_basis.is_none() {
            self.missing_usage += 1;
        }
        if row.context.cached_input_ratio.is_some() {
            self.with_cache += 1;
        } else {
            self.without_cache += 1;
        }
        match (
            row.context.summed_turn_input_tokens,
            row.context.final_turn_input_tokens,
        ) {
            (Some(summed), Some(final_input)) => {
                self.summed_turn_input.push(Some(summed));
                self.final_turn_input.push(Some(final_input));
                self.with_turn_metrics += 1;
            }
            _ => self.without_turn_metrics += 1,
        }
        self.base_bytes
            .push(Some(row.context.instruction_base_bytes));
        self.developer_bytes
            .push(Some(row.context.instruction_developer_bytes));
        if row.context.instruction_base_bytes > 0 || row.context.instruction_developer_bytes > 0 {
            self.with_instruction_bytes += 1;
        }
        self.partial |= row.partial;
    }

    fn overflowed(&self) -> bool {
        [
            self.input,
            self.cached,
            self.output,
            self.reasoning,
            self.total,
            self.summed_turn_input,
            self.final_turn_input,
        ]
        .iter()
        .any(|field| field.overflowed())
    }

    fn finish(self, key: &str) -> Bucket {
        let rows = self.sessions;
        let overflowed = self.overflowed();
        let ratio = |cached: Option<u64>, input: Option<u64>| match (cached, input) {
            (Some(cached), Some(input)) if input > 0 => Some(cached as f64 / input as f64),
            _ => None,
        };
        let cached = self.cached.value(rows);
        let input = self.input.value(rows);
        let summed_turn_input = self.summed_turn_input.value(rows);
        let final_turn_input = self.final_turn_input.value(rows);
        Bucket {
            key: key.to_owned(),
            sessions: rows,
            responses: self.responses,
            usage_basis: self.basis,
            usage: TokenTotals {
                input_tokens: input,
                cached_input_tokens: cached,
                output_tokens: self.output.value(rows),
                reasoning_output_tokens: self.reasoning.value(rows),
                total_tokens: self.total.value(rows),
            },
            missing_usage_sessions: self.missing_usage,
            context: ContextAggregate {
                cached_input_ratio: ratio(cached, input),
                summed_turn_input_tokens: summed_turn_input,
                final_turn_input_tokens: final_turn_input,
                repayment_multiplier: ratio(summed_turn_input, final_turn_input),
                sessions_with_cache_efficiency: self.with_cache,
                sessions_without_cache_efficiency: self.without_cache,
                sessions_with_turn_metrics: self.with_turn_metrics,
                sessions_without_turn_metrics: self.without_turn_metrics,
                instruction_base_bytes: self.base_bytes.sum.unwrap_or(0),
                instruction_developer_bytes: self.developer_bytes.sum.unwrap_or(0),
                sessions_with_instruction_bytes: self.with_instruction_bytes,
            },
            partial: self.partial || overflowed,
        }
    }
}
