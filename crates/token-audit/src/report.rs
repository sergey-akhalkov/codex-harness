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
        BASIS_RESPONSE_SUM, BASIS_THREAD_CUMULATIVE, BASIS_UNAVAILABLE, Bucket, ContextAggregate,
        CoverageReport, FORMAT_TOKEN_COUNT, FORMAT_USAGE_RECORD, Report, Scan, SessionContext,
        SessionRow, TokenTotals,
    },
};
use chrono::{DateTime, Duration, NaiveDate, SecondsFormat, Utc};
use harness_core::rollout_reader::{self, Coverage, InstructionBytes, SessionSummary, TurnUsage};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs, io,
    path::{Path, PathBuf},
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
    let files = discover(&options.sessions_root)?;
    let files_discovered = files.len();
    let cutoff = options
        .days
        .map(|days| options.generated_at - Duration::days(i64::from(days)));
    let mut coverage = CoverageReport::default();
    let mut sessions = Vec::new();
    let mut projects = BTreeMap::new();
    let mut inputs = Vec::new();
    for path in files {
        let summary = rollout_reader::read(&path);
        coverage.files_scanned += 1;
        add_file_coverage(&mut coverage, &summary.coverage);
        inputs.push(path.clone());
        let first = rollout_reader::timestamp(&summary.row["first_timestamp"]);
        let last = rollout_reader::timestamp(&summary.row["last_timestamp"]);
        if cutoff.is_some_and(|cutoff| last.is_some_and(|last| last < cutoff)) {
            coverage.sessions_excluded_by_window += 1;
            continue;
        }
        let row = FileScan {
            path: &path,
            summary: &summary,
            first,
            last,
        }
        .session_row(&mut projects);
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
            elapsed_seconds: self.summary.row["elapsed_seconds"].as_i64(),
            partial: !warnings.is_empty(),
            warnings: warnings.into_iter().collect(),
        }
    }
}

fn stamp(time: DateTime<Utc>) -> String {
    time.to_rfc3339_opts(SecondsFormat::Secs, true)
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
