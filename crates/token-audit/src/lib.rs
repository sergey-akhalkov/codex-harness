//! Measured token-usage analysis over local Codex rollout sessions.
//!
//! Reports read rollout JSONL through `harness_core::rollout_reader`, the one
//! tolerant event reader shared with `codex-harness delegation-usage`, and
//! aggregate recorded values per session, project, model, reasoning effort and
//! day. Every reported quantity is a recorded rollout value: token counters,
//! byte sizes or coverage counters. Project identities are hashed by default,
//! and conversation content is never retained or reported.
mod baseline;
mod findings;
mod identity;
mod model;
mod render;
mod report;

pub use baseline::{
    BASELINE_SCHEMA_VERSION, BaselineDiff, BaselineSnapshot,
    default_directory as default_baseline_directory, diff as baseline_diff,
    resolve as resolve_baseline, save as save_baseline, snapshot as baseline_snapshot,
};
pub use findings::{Finding, FindingEvidence, FindingsReport, ValidationPlan, analyze};
pub use identity::write_private_sources;
pub use model::{
    Bucket, ContextAggregate, CoverageReport, Format, Report, Scan, SessionContext, SessionRow,
    TokenTotals,
};
pub use render::{render_findings_json, render_findings_text, render_json, render_text};
pub use report::{ScanOptions, default_sessions_root, discover, now, scan};

/// Schema version of the JSON report and of the private-source record.
pub const SCHEMA_VERSION: u32 = 1;

/// Explicit markers for turn-level values that recorded rollouts do not carry.
pub const UNAVAILABLE_NO_TURN_RECORDS: &str = "no_turn_records";
pub const UNAVAILABLE_MISSING_TURN_IDENTITY: &str = "missing_turn_identity";
pub const UNAVAILABLE_INCOMPLETE_RESPONSE_RECORDS: &str = "incomplete_response_records";
pub const UNAVAILABLE_INCOMPLETE_TURN_INPUT: &str = "incomplete_turn_input";
pub const UNAVAILABLE_ZERO_FINAL_INPUT: &str = "final_turn_input_zero";
pub const UNAVAILABLE_COUNTER_OVERFLOW: &str = "counter_overflow";

/// Explicit markers for cache efficiency that recorded counters cannot express.
pub const UNAVAILABLE_MISSING_CACHE_FIELDS: &str = "missing_input_or_cached_tokens";
pub const UNAVAILABLE_ZERO_INPUT_TOKENS: &str = "zero_input_tokens";

/// Coverage warning codes produced by the analyzer itself.
pub const WARN_MISSING_SESSION_USAGE: &str = "missing_session_usage";
pub const WARN_SESSION_TIMESTAMP_MISSING: &str = "session_timestamp_missing";
pub const WARN_COUNTER_TOTAL_OVERFLOW: &str = "counter_total_overflow";
pub const WARN_MIXED_USAGE_BASIS: &str = "mixed_usage_basis";

/// Stated limits of every produced report.
pub const LIMITATION: &str = "Recorded rollout values only: token counters, instruction bytes and coverage. No currency, quota or transcript content, and no synthesized turn metrics.";
