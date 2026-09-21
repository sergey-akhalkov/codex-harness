//! JSON and text renderers for the report.
use crate::model::{Bucket, CoverageReport, Report, SessionContext, SessionRow, TokenTotals};
use std::collections::BTreeMap;

/// Pretty JSON report, newline terminated.
pub fn render_json(report: &Report) -> String {
    serde_json::to_string_pretty(report).map_or_else(
        |error| format!("{{\"error\":\"{error}\"}}\n"),
        |rendered| format!("{rendered}\n"),
    )
}

/// Line-oriented text report for interactive reading.
pub fn render_text(report: &Report) -> String {
    let mut out = String::new();
    let window = report
        .window_days
        .map_or_else(|| "all".to_owned(), |days| format!("last {days} days"));
    out.push_str(&format!(
        "token-audit report  generated={}  window={window}  files={}  sessions={}\n",
        report.generated_at,
        report.files_discovered,
        report.sessions.len()
    ));
    out.push_str(&format!("sessions-root {}\n", report.sessions_root));
    out.push_str(&format!("coverage {}\n", coverage_text(&report.coverage)));
    out.push_str(&format!("totals {}\n", bucket_text(&report.totals, None)));
    out.push_str(&format!(
        "warnings {}\n",
        counts_text(&report.coverage.warning_counts)
    ));
    for row in &report.sessions {
        out.push_str(&session_text(row));
    }
    for (label, buckets) in [
        ("project", &report.by_project),
        ("model", &report.by_model),
        ("effort", &report.by_effort),
        ("day", &report.by_day),
    ] {
        for bucket in buckets {
            out.push_str(&format!(
                "{label} {}\n",
                bucket_text(bucket, Some(&bucket.key))
            ));
        }
    }
    out
}

fn coverage_text(coverage: &CoverageReport) -> String {
    format!(
        "formats={} lines={} events={} recognized={} unrecognized={} corrupt={} oversized={} excluded_by_window={} without_usage={} without_project={} without_model={} without_effort={} without_day={} without_turn_metrics={} partial={}",
        counts_text(&coverage.formats),
        coverage.lines,
        coverage.events,
        coverage.recognized_events,
        coverage.unrecognized_events,
        coverage.corrupt_lines,
        coverage.oversized_lines,
        coverage.sessions_excluded_by_window,
        coverage.sessions_without_usage,
        coverage.sessions_without_project,
        coverage.sessions_without_model,
        coverage.sessions_without_effort,
        coverage.sessions_without_day,
        coverage.sessions_without_turn_metrics,
        coverage.partial
    )
}

fn session_text(row: &SessionRow) -> String {
    let mut out = format!(
        "session {} project={} model={} effort={} day={} responses={} conflicts={} {}\n",
        row.session_id.as_deref().unwrap_or("unknown"),
        row.project.as_deref().unwrap_or("unknown"),
        row.model.as_deref().unwrap_or("unknown"),
        row.effort.as_deref().unwrap_or("unknown"),
        row.day.as_deref().unwrap_or("unknown"),
        row.response_count,
        row.conflicting_response_ids,
        token_text(&row.usage)
    );
    out.push_str(&format!(
        "  basis={} cache={} repayment={} turn_input={} final_turn_input={} instruction_bytes={}/{} elapsed={} warnings={}\n",
        row.usage_basis.unwrap_or("unavailable"),
        cache_text(&row.context),
        repayment_text(&row.context),
        number(row.context.summed_turn_input_tokens),
        number(row.context.final_turn_input_tokens),
        row.context.instruction_base_bytes,
        row.context.instruction_developer_bytes,
        row.elapsed_seconds
            .map_or_else(|| "unknown".to_owned(), |value| value.to_string()),
        if row.warnings.is_empty() {
            "-".to_owned()
        } else {
            row.warnings.join(",")
        }
    ));
    out
}

fn bucket_text(bucket: &Bucket, key: Option<&str>) -> String {
    let key = key.map_or_else(String::new, |key| format!("{key} "));
    format!(
        "{key}sessions={} responses={} {} basis={} cache={} ({}/{}) repayment={} turn_metrics={}/{} instruction_bytes={}/{} of {} sessions missing_usage={} partial={}",
        bucket.sessions,
        bucket.responses,
        token_text(&bucket.usage),
        counts_text(&bucket.usage_basis),
        ratio(bucket.context.cached_input_ratio),
        bucket.context.sessions_with_cache_efficiency,
        bucket.context.sessions_without_cache_efficiency,
        ratio(bucket.context.repayment_multiplier),
        bucket.context.sessions_with_turn_metrics,
        bucket.context.sessions_without_turn_metrics,
        bucket.context.instruction_base_bytes,
        bucket.context.instruction_developer_bytes,
        bucket.context.sessions_with_instruction_bytes,
        bucket.missing_usage_sessions,
        bucket.partial
    )
}

fn cache_text(context: &SessionContext) -> String {
    match (
        context.cached_input_ratio,
        context.cache_efficiency_unavailable.as_deref(),
    ) {
        (Some(value), _) => format!("{value:.3}"),
        (None, Some(reason)) => format!("unavailable({reason})"),
        (None, None) => "unavailable".to_owned(),
    }
}

fn repayment_text(context: &SessionContext) -> String {
    match (
        context.repayment_multiplier,
        context.turn_metrics_unavailable.as_deref(),
    ) {
        (Some(value), _) => format!("{value:.3}"),
        (None, Some(reason)) => format!("unavailable({reason})"),
        (None, None) => "unavailable".to_owned(),
    }
}

fn token_text(totals: &TokenTotals) -> String {
    format!(
        "input={} cached={} output={} reasoning={} total={}",
        number(totals.input_tokens),
        number(totals.cached_input_tokens),
        number(totals.output_tokens),
        number(totals.reasoning_output_tokens),
        number(totals.total_tokens)
    )
}

fn number(value: Option<u64>) -> String {
    value.map_or_else(|| "unknown".to_owned(), |value| value.to_string())
}

fn ratio(value: Option<f64>) -> String {
    value.map_or_else(|| "unavailable".to_owned(), |value| format!("{value:.3}"))
}

fn counts_text<K: ToString + Ord>(counts: &BTreeMap<K, usize>) -> String {
    if counts.is_empty() {
        return "none".to_owned();
    }
    counts
        .iter()
        .map(|(key, count)| format!("{}={count}", key.to_string()))
        .collect::<Vec<_>>()
        .join(" ")
}
