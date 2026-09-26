//! Shared process-result mechanics for the bounded native observers.
//!
//! `structured.rs`, `regression.rs` and `outcome_run.rs` each own a distinct
//! observation contract, evidence shape and receipt, but they apply the same
//! two rules to a stopped job: the status vocabulary and the output-limit
//! test. Each owning crate includes this file directly, so the observers keep
//! one source of these rules without adding a new shared library API.

use harness_core::process::StopReason;
use std::fs;
use std::path::Path;

/// One stopped process's status name. A reached output limit takes priority
/// over the stop reason; the remaining names are the `StopReason` variants
/// that every observer receipt exposes.
pub(crate) fn stop_status(reason: StopReason, limit_hit: bool) -> &'static str {
    if limit_hit {
        return "output-limit";
    }
    match reason {
        StopReason::Exited => "exited",
        StopReason::Timeout => "timeout",
        StopReason::Cancelled => "cancelled",
        StopReason::MemoryLimit => "memory-limit",
    }
}

/// Whether any observed file exceeded the bounded output limit. A file that
/// does not exist is not over its limit.
pub(crate) fn limit_exceeded(
    limit: u64,
    paths: impl IntoIterator<Item = impl AsRef<Path>>,
) -> bool {
    paths
        .into_iter()
        .any(|path| fs::metadata(path).is_ok_and(|metadata| metadata.len() > limit))
}
