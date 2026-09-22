//! JSON and text renderers for the report.
//!
//! The JSON renderers emit the complete machine contract. The text renderers
//! are the bounded interactive presentation: they keep totals, coverage and
//! warnings complete, rank records by recorded total tokens, state how many
//! records the presentation omitted and name the retained complete JSON.
use crate::findings::FindingsReport;
use crate::model::{Bucket, CoverageReport, Report, SessionContext, SessionRow, TokenTotals};
use crate::retention::{Detail, Kind};
use std::collections::BTreeMap;

/// Ranked records shown per group in the interactive presentation.
pub const PRESENTATION_LIMIT: usize = 12;

/// Pretty JSON report, newline terminated.
pub fn render_json(report: &Report) -> String {
    serde_json::to_string_pretty(report).map_or_else(
        |error| format!("{{\"error\":\"{error}\"}}\n"),
        |rendered| format!("{rendered}\n"),
    )
}

/// Bounded line-oriented report for interactive reading. `detail` names the
/// retained complete same-scan JSON that `token-audit detail` reads.
pub fn render_text(report: &Report, detail: &Detail) -> String {
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
    let sessions = ranked_sessions(&report.sessions);
    out.push_str(&ranking_text(
        "sessions",
        "recorded total tokens",
        sessions.len().min(PRESENTATION_LIMIT),
        sessions.len(),
    ));
    for row in sessions.iter().take(PRESENTATION_LIMIT) {
        out.push_str(&session_text(row));
    }
    for (label, buckets) in [
        ("project", &report.by_project),
        ("model", &report.by_model),
        ("effort", &report.by_effort),
        ("day", &report.by_day),
    ] {
        let ranked = ranked_buckets(buckets);
        out.push_str(&ranking_text(
            &format!("{label}s"),
            "recorded total tokens",
            ranked.len().min(PRESENTATION_LIMIT),
            ranked.len(),
        ));
        for bucket in ranked.iter().take(PRESENTATION_LIMIT) {
            out.push_str(&format!(
                "{label} {}\n",
                bucket_text(bucket, Some(&bucket.key))
            ));
        }
    }
    out.push_str(&format!("{}\n", detail.note(Kind::Report)));
    out.push_str(&format!("limitation {}\n", report.limitation));
    out
}

/// Pretty JSON findings report, newline terminated.
pub fn render_findings_json(report: &FindingsReport) -> String {
    serde_json::to_string_pretty(report).map_or_else(
        |error| format!("{{\"error\":\"{error}\"}}\n"),
        |rendered| format!("{rendered}\n"),
    )
}

/// Bounded line-oriented findings text for interactive reading. `detail` names
/// the retained complete same-scan findings JSON for record reads.
pub fn render_findings_text(report: &FindingsReport, detail: &Detail) -> String {
    let mut out = String::new();
    let window = report
        .window_days
        .map_or_else(|| "all".to_owned(), |days| format!("last {days} days"));
    out.push_str(&format!(
        "token-audit findings  generated={}  window={window}  basis={}\n",
        report.generated_at,
        if report.measured_only {
            "measured only"
        } else {
            "all bases"
        }
    ));
    out.push_str(&format!("sessions-root {}\n", report.sessions_root));
    if report.findings.is_empty() {
        out.push_str("findings none\n");
    }
    out.push_str(&ranking_text(
        "findings",
        "measured mass",
        report.findings.len().min(PRESENTATION_LIMIT),
        report.findings.len(),
    ));
    for finding in report.findings.iter().take(PRESENTATION_LIMIT) {
        out.push_str(&format!(
            "{}  basis={}  mass_tokens={}  owner={}\n",
            finding.id, finding.basis, finding.mass_tokens, finding.owner
        ));
        out.push_str(&format!(
            "  evidence sessions={} projects={}\n",
            finding.evidence.session_ids.len(),
            finding.evidence.projects.len()
        ));
        out.push_str(&format!(
            "  validation {} | {} | {}\n",
            finding.validation.method, finding.validation.metric, finding.validation.command
        ));
    }
    if !report.hidden_by_basis.is_empty() {
        let hidden: Vec<String> = report
            .hidden_by_basis
            .iter()
            .map(|(basis, count)| format!("{basis}={count}"))
            .collect();
        out.push_str(&format!("hidden {}\n", hidden.join(" ")));
    }
    out.push_str(&format!("{}\n", detail.note(Kind::Findings)));
    out.push_str(&format!("limitation {}\n", report.limitation));
    out
}

/// Sessions ranked by recorded total tokens, then by identity. Sessions
/// without a recorded total stay last and are never counted as zero.
fn ranked_sessions(sessions: &[SessionRow]) -> Vec<&SessionRow> {
    let mut ranked: Vec<&SessionRow> = sessions.iter().collect();
    ranked.sort_by(|left, right| {
        right
            .usage
            .total_tokens
            .cmp(&left.usage.total_tokens)
            .then_with(|| left.session_id.cmp(&right.session_id))
    });
    ranked
}

/// Buckets ranked by recorded total tokens, then by key.
fn ranked_buckets(buckets: &[Bucket]) -> Vec<&Bucket> {
    let mut ranked: Vec<&Bucket> = buckets.iter().collect();
    ranked.sort_by(|left, right| {
        right
            .usage
            .total_tokens
            .cmp(&left.usage.total_tokens)
            .then_with(|| left.key.cmp(&right.key))
    });
    ranked
}

/// One ranked-group header stating the ranking basis and presented count.
fn ranking_text(label: &str, basis: &str, shown: usize, total: usize) -> String {
    if total == 0 {
        return format!("{label} ranked by {basis}, none recorded\n");
    }
    if total > shown {
        format!(
            "{label} ranked by {basis}, showing {shown} of {total}; {} omitted from this presentation\n",
            total - shown
        )
    } else {
        format!("{label} ranked by {basis}, all {total} presented\n")
    }
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
