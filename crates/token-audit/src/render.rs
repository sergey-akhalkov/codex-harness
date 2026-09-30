//! JSON and text renderers for the report.
//!
//! The JSON renderers emit the complete machine contract. The text renderers
//! are the bounded interactive presentation: they keep totals, coverage and
//! warnings complete, rank records by recorded total tokens, state how many
//! records the presentation omitted and name the retained complete JSON.
use crate::baseline::{
    BaselineDiff, BucketMovement, CoverageMovement, SessionMovement, SnapshotCoverage,
};
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

impl BaselineDiff {
    /// Sessions or groups presented per section in the bounded text diff.
    pub const TEXT_ROWS: usize = 20;

    /// Byte bound of the bounded text diff. Row sections are trimmed so the
    /// complete presentation stays inside it; the header, totals, coverage,
    /// locator and limitation stay complete.
    pub const TEXT_BYTES: usize = 32_768;

    /// Bounded line-oriented comparison for interactive reading. `detail`
    /// names the retained complete comparison that `token-audit detail` reads
    /// without rescanning sessions. Sections are ranked by absolute recorded
    /// token movement, largest first, with the record identity as the
    /// deterministic tie-break; every presented section states how many
    /// movements it omitted.
    pub fn render_text(&self, detail: &Detail) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "token-audit baseline diff  generated={}  baseline={}\n",
            self.generated_at, self.baseline
        ));
        out.push_str(&format!(
            "snapshot status={} schema={} current_schema={} version_changed={} compatible={} comparable={}\n",
            self.snapshot_status,
            self.snapshot_schema_version
                .map_or_else(|| "unknown".to_owned(), |version| version.to_string()),
            self.current_schema_version,
            self.version_changed,
            self.compatible,
            self.comparable
        ));
        if let Some(reason) = &self.snapshot_reason {
            out.push_str(&format!("snapshot note: {reason}\n"));
        }
        for reason in &self.comparability {
            out.push_str(&format!("not comparable: {reason}\n"));
        }
        out.push_str(&coverage_movement_text(&self.coverage));
        out.push_str(&bucket_movement_text(&self.totals));
        let mut ranked: Vec<&SessionMovement> = self.sessions.iter().collect();
        ranked.sort_by(|left, right| {
            right
                .significance()
                .cmp(&left.significance())
                .then_with(|| left.session_id.cmp(&right.session_id))
        });
        let rows: Vec<String> = ranked
            .iter()
            .map(|movement| session_movement_text(movement))
            .collect();
        push_ranked_movements(&mut out, "sessions", &rows);
        for (label, movements) in [
            ("project", &self.by_project),
            ("model", &self.by_model),
            ("effort", &self.by_effort),
            ("day", &self.by_day),
        ] {
            let mut ranked: Vec<&BucketMovement> = movements.iter().collect();
            ranked.sort_by(|left, right| {
                right
                    .significance()
                    .cmp(&left.significance())
                    .then_with(|| left.key.cmp(&right.key))
            });
            let rows: Vec<String> = ranked
                .iter()
                .map(|movement| bucket_movement_text(movement))
                .collect();
            push_ranked_movements(&mut out, label, &rows);
        }
        out.push_str(&format!("{}\n", baseline_detail_note(detail)));
        out.push_str(&format!("limitation {}\n", self.limitation));
        out
    }
}

/// Bytes reserved below the row budget for the locator, the limitation and
/// the section headers of the remaining sections.
const TEXT_TAIL_RESERVE: usize = 512;

/// Appends one bounded, ranked movement section: a header with the exact
/// presented and omitted counts, then the highest-significance rows that fit
/// the remaining byte bound. The selection is deterministic because both the
/// order and the fit test depend only on the recorded data.
fn push_ranked_movements(out: &mut String, label: &str, rows: &[String]) {
    let total = rows.len();
    if total == 0 {
        out.push_str(&format!(
            "{label} ranked by absolute recorded token movement, none recorded\n"
        ));
        return;
    }
    let budget = BaselineDiff::TEXT_BYTES.saturating_sub(TEXT_TAIL_RESERVE);
    let mut shown = total.min(BaselineDiff::TEXT_ROWS);
    while shown > 0 {
        let header = ranked_movement_header(label, shown, total);
        let body: usize = rows.iter().take(shown).map(String::len).sum();
        if out.len() + header.len() + body <= budget {
            break;
        }
        shown -= 1;
    }
    out.push_str(&ranked_movement_header(label, shown, total));
    for row in rows.iter().take(shown) {
        out.push_str(row);
    }
}

fn ranked_movement_header(label: &str, shown: usize, total: usize) -> String {
    if shown < total {
        format!(
            "{label} ranked by absolute recorded token movement, showing {shown} of {total}; {} omitted from this presentation\n",
            total - shown
        )
    } else {
        format!("{label} ranked by absolute recorded token movement, all {total} presented\n")
    }
}

fn coverage_movement_text(coverage: &CoverageMovement) -> String {
    let mut out = String::new();
    match &coverage.baseline {
        Some(baseline) => out.push_str(&format!(
            "coverage baseline {}\n",
            coverage_snapshot_text(baseline)
        )),
        None => out.push_str("coverage baseline not recorded\n"),
    }
    out.push_str(&format!(
        "coverage current {}\n",
        coverage_snapshot_text(&coverage.current)
    ));
    if coverage.degraded {
        out.push_str(&format!(
            "coverage degraded: {}; a lower recorded subtotal is not a saving\n",
            coverage.reasons.join("; ")
        ));
    }
    out
}

fn coverage_snapshot_text(coverage: &SnapshotCoverage) -> String {
    format!(
        "sessions={} without_usage={} corrupt_lines={} oversized_lines={} unrecognized_events={} partial={} usage_basis={} warnings={}",
        coverage.sessions,
        coverage.sessions_without_usage,
        coverage.corrupt_lines,
        coverage.oversized_lines,
        coverage.unrecognized_events,
        coverage.partial,
        counts_text(&coverage.usage_basis),
        counts_text(&coverage.warnings)
    )
}

fn session_movement_text(movement: &SessionMovement) -> String {
    format!(
        "session {} status={} total {} -> {} delta {} basis {} -> {} partial {} -> {} warnings {} -> {}\n",
        movement.session_id,
        movement.status,
        number(movement.baseline_total_tokens),
        number(movement.current_total_tokens),
        optional_delta(movement.delta_total_tokens),
        movement
            .baseline_usage_basis
            .as_deref()
            .unwrap_or("unavailable"),
        movement
            .current_usage_basis
            .as_deref()
            .unwrap_or("unavailable"),
        optional_bool(movement.baseline_partial),
        movement.current_partial,
        warnings_text(&movement.baseline_warnings),
        warnings_text(&movement.current_warnings)
    )
}

fn bucket_movement_text(movement: &BucketMovement) -> String {
    format!(
        "{} sessions {} -> {} total {} -> {} delta {} basis {} -> {} missing_usage {} -> {} partial {} -> {}\n",
        movement.key,
        optional_count(movement.baseline_sessions),
        movement.current_sessions,
        number(movement.baseline_total_tokens),
        number(movement.current_total_tokens),
        optional_delta(movement.delta_total_tokens),
        counts_text(&movement.baseline_usage_basis),
        counts_text(&movement.current_usage_basis),
        optional_count(movement.baseline_missing_usage_sessions),
        movement.current_missing_usage_sessions,
        optional_bool(movement.baseline_partial),
        movement.current_partial
    )
}

fn warnings_text(warnings: &[String]) -> String {
    if warnings.is_empty() {
        "-".to_owned()
    } else {
        warnings.join(",")
    }
}

fn optional_count(value: Option<usize>) -> String {
    value.map_or_else(|| "unknown".to_owned(), |value| value.to_string())
}

fn optional_delta(value: Option<i64>) -> String {
    value.map_or_else(|| "unknown".to_owned(), |value| format!("{value:+}"))
}

fn optional_bool(value: Option<bool>) -> String {
    value.map_or_else(|| "unknown".to_owned(), |value| value.to_string())
}

fn baseline_detail_note(detail: &Detail) -> String {
    match detail {
        Detail::Retained { path, warning } => {
            let mut note = format!(
                "retained {}  (complete comparison; read one movement: token-audit detail --diff {} --session ID, --group project:KEY, --sessions or --groups day, with optional --offset/--limit)",
                path.display(),
                path.display()
            );
            if let Some(warning) = warning {
                note.push_str(&format!("\nretention warning: {warning}"));
            }
            note
        }
        Detail::Unavailable(reason) => format!("retained unavailable: {reason}"),
    }
}
