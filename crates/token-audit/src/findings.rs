//! Ranked optimization findings over a measured report.
//!
//! Findings are data, not advice text: every number carries a basis, a
//! measured token mass at stake, evidence locators into recorded sessions, the
//! owning record that carries remediation, and a validation plan. Detectors
//! stay small and independent; remediation prose deliberately lives in the
//! owning records, never here.
use crate::model::{Report, SessionRow};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

pub const BASIS_MEASURED: &str = "measured";
pub const BASIS_INFERRED: &str = "inferred";

/// A finding is eligible for planning, never an instruction.
pub const FINDINGS_LIMITATION: &str = "Findings carry recorded evidence with a basis label and an owning record; they contain no remediation prose, no costs, no quota percentages and no auto-applied fixes.";

/// Evidence locator of a finding: identities only, never transcript content.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FindingEvidence {
    pub session_ids: Vec<String>,
    /// Hashed project identities contributing to the finding.
    pub projects: Vec<String>,
    /// Recorded counters backing the mass, in a stable order.
    pub detail: BTreeMap<String, u64>,
}

/// How the owning record's improvement would be re-checked.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ValidationPlan {
    pub method: String,
    pub metric: String,
    pub command: String,
}

/// One ranked optimization finding.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Finding {
    pub id: String,
    pub basis: &'static str,
    pub mass_tokens: u64,
    pub evidence: FindingEvidence,
    /// Existing record or skill that owns remediation.
    pub owner: &'static str,
    pub validation: ValidationPlan,
}

/// Complete findings report over one scan.
#[derive(Clone, Debug, Serialize)]
pub struct FindingsReport {
    pub schema_version: u32,
    pub command: &'static str,
    pub generated_at: String,
    pub sessions_root: String,
    pub window_days: Option<u32>,
    pub measured_only: bool,
    pub findings: Vec<Finding>,
    /// Findings hidden by the current basis filter, counted per basis.
    pub hidden_by_basis: BTreeMap<String, usize>,
    pub limitation: &'static str,
}

/// Detection thresholds; every value stays explicit and testable.
const REPAYMENT_MULTIPLIER_THRESHOLD: f64 = 10.0;
const LOW_WORTH_MIN_INPUT_TOKENS: u64 = 200_000;
const LOW_WORTH_MAX_OUTPUT_TOKENS: u64 = 1_000;
const OUTLIER_FACTOR: f64 = 10.0;
const OUTLIER_MIN_SESSIONS: usize = 5;
const TOOL_MASS_MIN_BYTES: u64 = 1024 * 1024;
const HIGH_EFFORTS: [&str; 2] = ["max", "xhigh"];

const OWNER_RUNTIME: &str = harness_core::report_owners::TOKEN_WORKFLOW;
const OWNER_DELEGATION: &str = harness_core::report_owners::AGENT_DELEGATION;
const OWNER_SUBSCRIPTIONS: &str = harness_core::report_owners::SUBSCRIPTION_MODELS;
const OWNER_TOOLING: &str = harness_core::report_owners::TOKEN_EFFICIENT_WORKFLOW;

/// Runs every detector, then ranks by measured mass and applies the basis
/// filter. Zero-mass findings never surface.
pub fn analyze(report: &Report, measured_only: bool) -> FindingsReport {
    let mut detected = Vec::new();
    detected.extend(context_repayment(report));
    detected.extend(low_worth_sessions(report));
    detected.extend(session_outliers(report));
    detected.extend(tool_output_mass(report));
    detected.extend(effort_mix(report));
    detected.retain(|finding| finding.mass_tokens > 0);
    detected.sort_by(|left, right| {
        right
            .mass_tokens
            .cmp(&left.mass_tokens)
            .then_with(|| left.id.cmp(&right.id))
    });
    let mut hidden_by_basis: BTreeMap<String, usize> = BTreeMap::new();
    if measured_only {
        let (kept, hidden): (Vec<_>, Vec<_>) = detected
            .into_iter()
            .partition(|finding| finding.basis == BASIS_MEASURED);
        for finding in hidden {
            *hidden_by_basis.entry(finding.basis.to_owned()).or_default() += 1;
        }
        detected = kept;
    }
    FindingsReport {
        schema_version: crate::SCHEMA_VERSION,
        command: "findings",
        generated_at: report.generated_at.clone(),
        sessions_root: report.sessions_root.clone(),
        window_days: report.window_days,
        measured_only,
        findings: detected,
        hidden_by_basis,
        limitation: FINDINGS_LIMITATION,
    }
}

fn evidence(sessions: &[&SessionRow]) -> FindingEvidence {
    let mut detail: BTreeMap<String, u64> = BTreeMap::new();
    let mut projects: BTreeSet<String> = BTreeSet::new();
    let mut session_ids = Vec::new();
    for row in sessions {
        session_ids.push(row.session_id.clone().unwrap_or_else(|| "unknown".into()));
        if let Some(project) = &row.project {
            projects.insert(project.clone());
        }
        detail.insert(
            format!("session_{}_input_tokens", session_ids.last().unwrap()),
            row.usage.input_tokens.unwrap_or(0),
        );
    }
    FindingEvidence {
        session_ids,
        projects: projects.into_iter().collect(),
        detail,
    }
}

fn plan(method: &str, metric: &str) -> ValidationPlan {
    ValidationPlan {
        method: method.to_owned(),
        metric: metric.to_owned(),
        command: "token-audit report --format json".to_owned(),
    }
}

fn context_repayment(report: &Report) -> Vec<Finding> {
    let mut findings = Vec::new();
    for row in &report.sessions {
        let Some(multiplier) = row.context.repayment_multiplier else {
            continue;
        };
        if multiplier < REPAYMENT_MULTIPLIER_THRESHOLD {
            continue;
        }
        let mass = row.context.summed_turn_input_tokens.unwrap_or(0);
        let mut finding = Finding {
            id: format!(
                "context-repayment:{}",
                row.session_id.as_deref().unwrap_or("unknown")
            ),
            basis: BASIS_MEASURED,
            mass_tokens: mass,
            evidence: evidence(&[row]),
            owner: OWNER_RUNTIME,
            validation: plan(
                "adopt a context-discipline change in the owning record, then re-audit",
                "session repayment multiplier and summed turn input tokens",
            ),
        };
        finding.evidence.detail.insert(
            "repayment_multiplier_percent".into(),
            (multiplier * 100.0) as u64,
        );
        findings.push(finding);
    }
    findings
}

fn low_worth_sessions(report: &Report) -> Vec<Finding> {
    report
        .sessions
        .iter()
        .filter(|row| {
            row.usage.input_tokens.unwrap_or(0) >= LOW_WORTH_MIN_INPUT_TOKENS
                && row.usage.output_tokens.unwrap_or(u64::MAX) <= LOW_WORTH_MAX_OUTPUT_TOKENS
        })
        .map(|row| Finding {
            id: format!(
                "low-worth-session:{}",
                row.session_id.as_deref().unwrap_or("unknown")
            ),
            basis: BASIS_MEASURED,
            mass_tokens: row.usage.input_tokens.unwrap_or(0),
            evidence: evidence(&[row]),
            owner: OWNER_RUNTIME,
            validation: plan(
                "split or retire the low-yield pattern in the owning record, then re-audit",
                "input tokens of sessions under the output floor",
            ),
        })
        .collect()
}

fn session_outliers(report: &Report) -> Vec<Finding> {
    let mut totals: Vec<u64> = report
        .sessions
        .iter()
        .filter_map(|row| row.usage.total_tokens)
        .collect();
    if totals.len() < OUTLIER_MIN_SESSIONS {
        return Vec::new();
    }
    totals.sort_unstable();
    let median = totals[totals.len() / 2];
    if median == 0 {
        return Vec::new();
    }
    report
        .sessions
        .iter()
        .filter(|row| {
            row.usage
                .total_tokens
                .is_some_and(|total| (total as f64) > OUTLIER_FACTOR * median as f64)
        })
        .map(|row| {
            let mut finding = Finding {
                id: format!(
                    "session-outlier:{}",
                    row.session_id.as_deref().unwrap_or("unknown")
                ),
                basis: BASIS_MEASURED,
                mass_tokens: row.usage.total_tokens.unwrap_or(0),
                evidence: evidence(&[row]),
                owner: OWNER_DELEGATION,
                validation: plan(
                    "review whether the outlier workload belongs in a delegated lane",
                    "session total tokens against the scan median",
                ),
            };
            finding
                .evidence
                .detail
                .insert("median_total_tokens".into(), median);
            finding
        })
        .collect()
}

fn tool_output_mass(report: &Report) -> Vec<Finding> {
    let mut per_tool: BTreeMap<&str, (u64, Vec<&SessionRow>)> = BTreeMap::new();
    for row in &report.sessions {
        for (tool, bytes) in &row.tool_output_bytes {
            let entry = per_tool.entry(tool).or_default();
            entry.0 += *bytes;
            entry.1.push(row);
        }
    }
    per_tool
        .into_iter()
        .filter(|(_, (bytes, _))| *bytes >= TOOL_MASS_MIN_BYTES)
        .map(|(tool, (bytes, rows))| {
            let mut finding = Finding {
                id: format!("tool-output-mass:{tool}"),
                // Byte volume is recorded; any token figure derived from it is
                // an estimate, so the basis says so and the default filter
                // hides this finding until explicitly requested.
                basis: BASIS_INFERRED,
                mass_tokens: bytes / 4,
                evidence: evidence(&rows),
                owner: OWNER_TOOLING,
                validation: plan(
                    "measure bytes/4 as a declared estimate; compress or bound the tool's output, then re-audit",
                    "recorded output bytes per tool name",
                ),
            };
            finding.evidence.detail.insert("output_bytes".into(), bytes);
            finding
        })
        .collect()
}

fn effort_mix(report: &Report) -> Vec<Finding> {
    let mut sessions = Vec::new();
    for bucket in &report.by_effort {
        if HIGH_EFFORTS.contains(&bucket.key.as_str()) {
            for row in &report.sessions {
                if row.effort.as_deref() == Some(bucket.key.as_str()) {
                    sessions.push(row);
                }
            }
        }
    }
    if sessions.is_empty() {
        return Vec::new();
    }
    let mass = sessions
        .iter()
        .map(|row| row.usage.input_tokens.unwrap_or(0))
        .sum();
    vec![Finding {
        id: "effort-mix:high-effort-sessions".to_owned(),
        basis: BASIS_MEASURED,
        mass_tokens: mass,
        evidence: evidence(&sessions),
        owner: OWNER_SUBSCRIPTIONS,
        validation: plan(
            "route eligible work to a lower effort or delegated profile, then re-audit",
            "input tokens of sessions recorded at high effort",
        ),
    }]
}
