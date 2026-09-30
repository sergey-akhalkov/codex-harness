//! Report data model shared by the analyzer, the renderers and the tests.
use harness_core::rollout_reader::Usage;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Recorded cumulative thread snapshot, either older `event_msg` counts or the
/// newer per-response thread capture.
pub const BASIS_THREAD_CUMULATIVE: &str = "thread_cumulative";
/// Sum of recorded per-response counters when no cumulative snapshot exists.
pub const BASIS_RESPONSE_SUM: &str = "response_sum";
/// Reported basis value for sessions without any recorded usage.
pub const BASIS_UNAVAILABLE: &str = "unavailable";

/// Older `event_msg` token-count format.
pub const FORMAT_TOKEN_COUNT: &str = "event_msg_token_count";
/// Newer `token_usage_record` format.
pub const FORMAT_USAGE_RECORD: &str = "token_usage_record";

/// Version of the declared accounting contract and of the interval usage
/// output. Bumped when the meaning of the accounting fields changes.
pub const ACCOUNTING_VERSION: u32 = 1;
/// Version of the operational incremental counters.
pub const INCREMENTAL_VERSION: u32 = 1;
/// Accounting mode: lifetime totals of sessions active inside a window.
pub const MODE_ACTIVITY: &str = "activity";
/// Accounting mode: usage attributable to a declared UTC interval.
pub const MODE_INTERVAL: &str = "interval";

/// Day grouping of the activity-shaped buckets: the day a session started,
/// never daily spending.
pub const DAY_BASIS_SESSION_START: &str = "session_start_lifetime";
/// Interval basis built from recorded per-response usage deltas.
pub const BASIS_RESPONSE_DELTA: &str = "response_delta";
/// Interval basis built from increments between recorded cumulative counters.
pub const BASIS_CUMULATIVE_INCREMENT: &str = "cumulative_increment";
/// Attribution bucket for usage units without recorded model or effort.
pub const ATTRIBUTION_UNASSIGNED: &str = "unassigned";

/// Declared accounting semantics of one scan.
#[derive(Clone, Debug, Serialize)]
pub struct Accounting {
    /// Version of this declaration and of the interval usage contract.
    pub version: u32,
    /// `activity` or `interval`.
    pub mode: String,
    /// Activity window of the scan.
    pub window_days: Option<u32>,
    /// Day grouping of the activity-shaped buckets in this report. They are
    /// session lifetime aggregates, not spending per day.
    pub day_basis: &'static str,
    /// Declared interval of an interval scan.
    pub interval: Option<IntervalWindow>,
}

impl Accounting {
    /// Activity declaration for a scan bounded by recorded session activity.
    pub fn activity(window_days: Option<u32>) -> Self {
        Self {
            version: ACCOUNTING_VERSION,
            mode: MODE_ACTIVITY.to_owned(),
            window_days,
            day_basis: DAY_BASIS_SESSION_START,
            interval: None,
        }
    }
}

/// Declared half-open UTC interval of one interval scan.
#[derive(Clone, Debug, Serialize)]
pub struct IntervalWindow {
    /// Inclusive UTC start, RFC3339.
    pub start: String,
    /// Exclusive UTC end, RFC3339.
    pub end: String,
    /// Recorded values the window filters: `recorded_event_timestamp`.
    pub basis: &'static str,
    /// Where the instants come from.
    pub timestamp_source: &'static str,
    /// Boundary convention.
    pub boundary: &'static str,
    pub policy: IntervalPolicy,
}

/// Explicit reset, gap and boundary rules of interval accounting.
#[derive(Clone, Debug, Serialize)]
pub struct IntervalPolicy {
    pub duplication: &'static str,
    pub cumulative: &'static str,
    pub reset: &'static str,
    pub boundary_crossing: &'static str,
    pub missing_timestamp: &'static str,
    pub attribution: &'static str,
}

/// Attributable interval usage of one session.
#[derive(Clone, Debug, Serialize)]
pub struct SessionInterval {
    /// `response_delta`, `cumulative_increment` or `unavailable`.
    pub basis: &'static str,
    /// Recorded usage attributable to the interval.
    pub usage: TokenTotals,
    /// Recorded amounts whose interval allocation stays unknown.
    pub unallocated: TokenTotals,
    /// Unknown allocation counts per reason code.
    pub unallocated_events: BTreeMap<String, u64>,
    /// Attributed models of in-window usage; `unassigned` when none.
    pub models: Vec<String>,
    /// Attributed efforts of in-window usage; `unassigned` when none.
    pub efforts: Vec<String>,
    /// Attributable usage that day buckets could not resolve.
    pub day_unresolved: TokenTotals,
}

/// Aggregate interval usage of one scan.
#[derive(Clone, Debug, Serialize)]
pub struct IntervalUsage {
    pub usage: TokenTotals,
    pub unallocated: TokenTotals,
    pub unallocated_events: BTreeMap<String, u64>,
    pub coverage: IntervalCoverage,
    pub by_model: Vec<IntervalBucket>,
    pub by_effort: Vec<IntervalBucket>,
    /// Attributable usage per recorded event day (UTC), never session start.
    pub by_day: Vec<IntervalBucket>,
    /// Attributable usage the day buckets could not resolve, so
    /// `by_day` plus this remainder equals `usage`.
    pub day_unresolved: TokenTotals,
}

/// Attributable interval usage of one bucket key.
#[derive(Clone, Debug, Serialize)]
pub struct IntervalBucket {
    pub key: String,
    /// Sessions contributing attributable usage to this key.
    pub sessions: usize,
    /// In-window usage units of this key: responses or cumulative steps.
    pub units: u64,
    pub usage: TokenTotals,
}

/// Interval coverage counters of one scan.
#[derive(Clone, Debug, Default, Serialize)]
pub struct IntervalCoverage {
    pub sessions_with_usage: usize,
    pub sessions_with_unallocated: usize,
    pub sessions_without_evidence: usize,
    pub response_events_inside: u64,
    pub response_events_before: u64,
    pub response_events_after: u64,
    pub response_events_undated: u64,
    pub response_events_conflicting: u64,
    pub response_events_unidentified: u64,
    pub cumulative_steps_inside: u64,
    pub cumulative_steps_before: u64,
    pub cumulative_steps_after: u64,
    pub cumulative_steps_boundary: u64,
    pub cumulative_steps_reset: u64,
    pub cumulative_steps_incomplete: u64,
    pub cumulative_steps_undated: u64,
    /// In-window cumulative steps that cross a UTC day boundary, whose amount
    /// is attributable to the interval but not resolvable to one day.
    pub cumulative_steps_day_crossing: u64,
}

/// Versioned operational counters of one analyzer scan.
///
/// These counters are mechanical I/O and timing observations. They are
/// excluded from full-scan equivalence because they describe how the scan was
/// performed, not what it found.
#[derive(Clone, Debug, Serialize)]
pub struct IncrementalStats {
    pub version: u32,
    pub enabled: bool,
    pub files_discovered: usize,
    pub files_reused: usize,
    pub files_full_parsed: usize,
    pub bytes_read: u64,
    pub bytes_discovered: u64,
    pub events_parsed: u64,
    /// Full parses per invalidation reason.
    pub invalidations: BTreeMap<String, u64>,
    pub checkpoints_written: usize,
    pub checkpoint_write_failures: usize,
    pub checkpoints_pruned: usize,
    /// Measured wall time of the scan, in milliseconds.
    pub scan_millis: u64,
}

impl IncrementalStats {
    /// An analyzer scan that parsed every discovered file.
    pub fn disabled() -> Self {
        Self {
            version: INCREMENTAL_VERSION,
            enabled: false,
            files_discovered: 0,
            files_reused: 0,
            files_full_parsed: 0,
            bytes_read: 0,
            bytes_discovered: 0,
            events_parsed: 0,
            invalidations: BTreeMap::new(),
            checkpoints_written: 0,
            checkpoint_write_failures: 0,
            checkpoints_pruned: 0,
            scan_millis: 0,
        }
    }
}

/// Output format shared by the commands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Json,
    Text,
}

impl Format {
    /// Parses the `--format` value.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "json" => Some(Self::Json),
            "text" => Some(Self::Text),
            _ => None,
        }
    }

    /// Canonical name of the format.
    pub fn name(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Text => "text",
        }
    }
}

/// Recorded token counters of one session, response set or aggregate.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenTotals {
    pub input_tokens: Option<u64>,
    pub cached_input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub reasoning_output_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
}

impl TokenTotals {
    /// Counters recorded in a shared-reader usage map.
    pub fn from_usage(usage: &Usage) -> Self {
        Self {
            input_tokens: field(usage, "input_tokens"),
            cached_input_tokens: field(usage, "cached_input_tokens"),
            output_tokens: field(usage, "output_tokens"),
            reasoning_output_tokens: field(usage, "reasoning_output_tokens"),
            total_tokens: field(usage, "total_tokens"),
        }
    }

    /// Counters recorded as plain fields of a reader row.
    pub fn from_row(row: &serde_json::Value) -> Self {
        Self {
            input_tokens: row["input_tokens"].as_u64(),
            cached_input_tokens: row["cached_input_tokens"].as_u64(),
            output_tokens: row["output_tokens"].as_u64(),
            reasoning_output_tokens: row["reasoning_output_tokens"].as_u64(),
            total_tokens: row["total_tokens"].as_u64(),
        }
    }

    /// Counters recorded in this set.
    pub fn recorded(&self) -> usize {
        self.fields().iter().filter(|value| value.is_some()).count()
    }

    /// Counters in report order.
    pub fn fields(&self) -> [Option<u64>; 5] {
        [
            self.input_tokens,
            self.cached_input_tokens,
            self.output_tokens,
            self.reasoning_output_tokens,
            self.total_tokens,
        ]
    }
}

fn field(usage: &Usage, key: &str) -> Option<u64> {
    usage.get(key).copied().flatten()
}

/// Context economics of one session.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct SessionContext {
    /// Cached input tokens over recorded input tokens, both recorded values.
    pub cached_input_ratio: Option<f64>,
    pub cache_efficiency_unavailable: Option<String>,
    /// Summed per-turn input tokens of the session's turn-identified responses.
    pub summed_turn_input_tokens: Option<u64>,
    /// Input tokens of the final turn-identified response.
    pub final_turn_input_tokens: Option<u64>,
    /// Summed per-turn input divided by the final turn's input.
    pub repayment_multiplier: Option<f64>,
    pub turn_metrics_unavailable: Option<String>,
    /// Recorded base instruction bytes of the session.
    pub instruction_base_bytes: u64,
    /// Recorded per-turn developer instruction bytes of the session.
    pub instruction_developer_bytes: u64,
}

/// Context economics of an aggregate bucket, summed over its sessions.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ContextAggregate {
    pub cached_input_ratio: Option<f64>,
    pub summed_turn_input_tokens: Option<u64>,
    pub final_turn_input_tokens: Option<u64>,
    pub repayment_multiplier: Option<f64>,
    pub sessions_with_cache_efficiency: usize,
    pub sessions_without_cache_efficiency: usize,
    pub sessions_with_turn_metrics: usize,
    pub sessions_without_turn_metrics: usize,
    pub instruction_base_bytes: u64,
    pub instruction_developer_bytes: u64,
    pub sessions_with_instruction_bytes: usize,
}

/// One session row of the report.
#[derive(Clone, Debug, Serialize)]
pub struct SessionRow {
    pub session_id: Option<String>,
    /// Hashed project identity; never a raw local path.
    pub project: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub day: Option<String>,
    pub first_timestamp: Option<String>,
    pub last_timestamp: Option<String>,
    pub response_count: usize,
    pub conflicting_response_ids: usize,
    /// Recorded usage formats that contributed to this session.
    pub formats: Vec<&'static str>,
    /// Where the recorded totals came from.
    pub usage_basis: Option<&'static str>,
    pub usage: TokenTotals,
    pub context: SessionContext,
    /// Recorded tool output bytes per tool name of this session.
    pub tool_output_bytes: BTreeMap<String, u64>,
    pub elapsed_seconds: Option<i64>,
    pub partial: bool,
    pub warnings: Vec<String>,
    /// Interval attribution of this session; present in interval scans only.
    pub interval: Option<SessionInterval>,
}

/// Aggregate over sessions sharing one key.
///
/// Each counter is the sum of the values recorded for that counter, so a
/// counter stays `null` when no session in the bucket recorded it. Sessions
/// whose recorded totals come from different bases are summed together, which
/// `usage_basis` makes visible per bucket.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Bucket {
    pub key: String,
    pub sessions: usize,
    pub responses: usize,
    /// Sessions per recorded usage basis; `unavailable` counts sessions with none.
    pub usage_basis: BTreeMap<String, usize>,
    pub usage: TokenTotals,
    pub missing_usage_sessions: usize,
    pub context: ContextAggregate,
    pub partial: bool,
}

/// Explicit coverage accounting of one scan.
#[derive(Clone, Debug, Default, Serialize)]
pub struct CoverageReport {
    pub files_scanned: usize,
    pub sessions: usize,
    pub sessions_excluded_by_window: usize,
    /// Sessions contributing each recorded usage format.
    pub formats: BTreeMap<&'static str, usize>,
    pub lines: u64,
    pub events: u64,
    pub recognized_events: u64,
    pub unrecognized_events: u64,
    pub corrupt_lines: u64,
    pub oversized_lines: u64,
    pub usage_basis: BTreeMap<String, usize>,
    pub sessions_without_project: usize,
    pub sessions_without_model: usize,
    pub sessions_without_effort: usize,
    pub sessions_without_day: usize,
    pub sessions_without_usage: usize,
    pub sessions_without_turn_metrics: usize,
    pub mixed_usage_basis: bool,
    /// Sessions carrying each warning code, plus analyzer-level warnings.
    pub warning_counts: BTreeMap<String, usize>,
    pub partial: bool,
}

/// Complete analyzer report.
///
/// `day` buckets by session start: the recorded first timestamp when present,
/// otherwise the date carried by the rollout file name. `window_days` bounds
/// the scan to sessions with recorded activity inside the window; sessions
/// without recorded timestamps are always included and warned about.
#[derive(Clone, Debug, Serialize)]
pub struct Report {
    pub schema_version: u32,
    pub command: &'static str,
    pub generated_at: String,
    /// Hashed identity of the scanned sessions root.
    pub sessions_root: String,
    pub window_days: Option<u32>,
    pub files_discovered: usize,
    pub sessions: Vec<SessionRow>,
    pub by_project: Vec<Bucket>,
    pub by_model: Vec<Bucket>,
    pub by_effort: Vec<Bucket>,
    pub by_day: Vec<Bucket>,
    pub totals: Bucket,
    pub coverage: CoverageReport,
    /// Declared accounting semantics of this scan.
    pub accounting: Accounting,
    /// Attributable interval usage; present in interval scans only.
    pub interval: Option<IntervalUsage>,
    /// Operational counters of the scan's reader use.
    pub incremental: IncrementalStats,
    pub limitation: &'static str,
}

/// One completed scan: the report plus the local identities that only
/// `--private-sources` may record.
#[derive(Debug)]
pub struct Scan {
    pub report: Report,
    /// Hashed project identity to raw workspace directory name.
    pub projects: BTreeMap<String, String>,
    /// Session files read by this scan, in scan order.
    pub inputs: Vec<std::path::PathBuf>,
}
