//! Local baseline snapshots and the measured diff loop.
//!
//! A baseline is one small JSON aggregate per audit under
//! `CODEX_HOME/harness/token-audit/baselines/`, with a `latest` pointer. It
//! stores aggregates and hashed identities only: no transcript content, no
//! raw workspace paths, no token estimates.
use crate::model::{Bucket, Report, Scan, SessionRow, TokenTotals};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
};

pub const BASELINE_SCHEMA_VERSION: u32 = 1;

/// One session digest: identities and recorded counters only.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionDigest {
    pub session_id: Option<String>,
    pub project: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub day: Option<String>,
    pub usage: TokenTotals,
}

/// Aggregate snapshot of one completed scan.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BaselineSnapshot {
    pub schema_version: u32,
    pub created_at: String,
    pub sessions_root: String,
    pub window_days: Option<u32>,
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
    pub baseline_total_tokens: Option<u64>,
    pub current_total_tokens: Option<u64>,
    pub delta_total_tokens: Option<i64>,
}

/// Movement of one aggregate bucket.
#[derive(Clone, Debug, Serialize)]
pub struct BucketMovement {
    pub key: String,
    pub baseline_sessions: usize,
    pub current_sessions: usize,
    pub baseline_total_tokens: Option<u64>,
    pub current_total_tokens: Option<u64>,
    pub delta_total_tokens: Option<i64>,
}

/// Complete diff of the current scan against one baseline.
#[derive(Clone, Debug, Serialize)]
pub struct BaselineDiff {
    pub schema_version: u32,
    pub command: &'static str,
    pub generated_at: String,
    pub baseline: String,
    pub current_root: String,
    pub compatible: bool,
    /// Named reason when the snapshot cannot be compared.
    pub incompatibility: Option<String>,
    pub sessions: Vec<SessionMovement>,
    pub by_project: Vec<BucketMovement>,
    pub by_model: Vec<BucketMovement>,
    pub by_day: Vec<BucketMovement>,
    pub totals: BucketMovement,
    pub limitation: &'static str,
}

pub const BASELINE_LIMITATION: &str = "Baselines store aggregates and hashed identities only; diff reports recorded token movement without costs, quotas or transcript content.";

fn digest(row: &SessionRow) -> SessionDigest {
    SessionDigest {
        session_id: row.session_id.clone(),
        project: row.project.clone(),
        model: row.model.clone(),
        effort: row.effort.clone(),
        day: row.day.clone(),
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
        sessions: report.sessions.iter().map(digest).collect(),
        by_project: report.by_project.clone(),
        by_model: report.by_model.clone(),
        by_effort: report.by_effort.clone(),
        by_day: report.by_day.clone(),
        totals: report.totals.clone(),
    }
}

/// Default baseline directory under the resolved CODEX_HOME.
pub fn default_directory() -> Option<PathBuf> {
    std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
        .map(|root| root.join("harness").join("token-audit").join("baselines"))
}

/// Writes one snapshot with its `latest` pointer and returns its file name.
pub fn save(directory: &Path, scan: &Scan) -> io::Result<String> {
    fs::create_dir_all(directory)?;
    let name = format!(
        "baseline-{}.json",
        scan.report
            .generated_at
            .replace([':', '-', 'T', 'Z', '.'], "")
    );
    let encoded = serde_json::to_vec_pretty(&snapshot(scan)).map_err(io::Error::other)?;
    let temporary = directory.join(format!("{name}.tmp"));
    fs::write(&temporary, encoded)?;
    fs::rename(&temporary, directory.join(&name))?;
    let pointer = directory.join("latest");
    let temporary_pointer = directory.join("latest.tmp");
    fs::write(&temporary_pointer, name.as_bytes())?;
    fs::rename(&temporary_pointer, pointer)?;
    Ok(name)
}

/// Resolves the requested baseline name, or the `latest` pointer.
pub fn resolve(directory: &Path, requested: Option<&str>) -> io::Result<PathBuf> {
    match requested {
        Some(name) if name != "latest" => {
            let path = directory.join(format!("{name}.json"));
            path.is_file().then_some(path).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("baseline {name} not found in {}", directory.display()),
                )
            })
        }
        _ => {
            let pointer = directory.join("latest");
            let name = fs::read_to_string(&pointer)?.trim().to_owned();
            let path = directory.join(name);
            path.is_file().then_some(path).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    "the latest baseline pointer names no existing snapshot",
                )
            })
        }
    }
}

fn bucket_totals(buckets: &[Bucket]) -> BTreeMap<String, (usize, Option<u64>)> {
    buckets
        .iter()
        .map(|bucket| {
            (
                bucket.key.clone(),
                (bucket.sessions, bucket.usage.total_tokens),
            )
        })
        .collect()
}

fn report_totals(report: &Report) -> BTreeMap<String, (usize, Option<u64>)> {
    bucket_totals(&report.by_project)
}

/// Loads and compares one baseline against a fresh scan.
pub fn diff(baseline_path: &Path, name: &str, scan: &Scan) -> io::Result<BaselineDiff> {
    let raw = fs::read(baseline_path)?;
    let value: serde_json::Value = serde_json::from_slice(&raw).map_err(io::Error::other)?;
    let schema = value["schema_version"].as_u64().unwrap_or(0) as u32;
    let compatible = schema == BASELINE_SCHEMA_VERSION;
    let parsed: Option<BaselineSnapshot> = if compatible {
        serde_json::from_value(value).map_err(io::Error::other).ok()
    } else {
        None
    };
    let report = &scan.report;
    let baseline_sessions: BTreeMap<&str, &SessionDigest> = parsed
        .as_ref()
        .map(|snapshot| {
            snapshot
                .sessions
                .iter()
                .filter_map(|digest| digest.session_id.as_deref().map(|id| (id, digest)))
                .collect()
        })
        .unwrap_or_default();
    let current_sessions: BTreeMap<&str, &SessionRow> = report
        .sessions
        .iter()
        .filter_map(|row| row.session_id.as_deref().map(|id| (id, row)))
        .collect();
    let mut session_moves = Vec::new();
    if let Some(snapshot) = parsed.as_ref() {
        for (id, digest) in &baseline_sessions {
            session_moves.push(SessionMovement {
                session_id: (*id).to_owned(),
                status: if current_sessions.contains_key(id) {
                    "same"
                } else {
                    "gone"
                },
                baseline_total_tokens: digest.usage.total_tokens,
                current_total_tokens: current_sessions
                    .get(*id)
                    .and_then(|row| row.usage.total_tokens),
                delta_total_tokens: delta(
                    digest.usage.total_tokens,
                    current_sessions
                        .get(*id)
                        .and_then(|row| row.usage.total_tokens),
                ),
            });
        }
        for (id, row) in &current_sessions {
            if !baseline_sessions.contains_key(id) {
                session_moves.push(SessionMovement {
                    session_id: (*id).to_owned(),
                    status: "new",
                    baseline_total_tokens: None,
                    current_total_tokens: row.usage.total_tokens,
                    delta_total_tokens: delta(None, row.usage.total_tokens),
                });
            }
        }
        let _ = snapshot;
    }
    session_moves.sort_by(|left, right| left.session_id.cmp(&right.session_id));
    let baseline = parsed.as_ref();
    let by_project = baseline
        .map(|snapshot| {
            bucket_moves(
                &bucket_totals(&snapshot.by_project),
                &report_totals_with(report, |r| &r.by_project),
            )
        })
        .unwrap_or_default();
    let by_model = baseline
        .map(|snapshot| {
            bucket_moves(
                &bucket_totals(&snapshot.by_model),
                &report_totals_with(report, |r| &r.by_model),
            )
        })
        .unwrap_or_default();
    let by_day = baseline
        .map(|snapshot| {
            bucket_moves(
                &bucket_totals(&snapshot.by_day),
                &report_totals_with(report, |r| &r.by_day),
            )
        })
        .unwrap_or_default();
    let totals = BucketMovement {
        key: "totals".to_owned(),
        baseline_sessions: baseline
            .map(|snapshot| snapshot.totals.sessions)
            .unwrap_or(0),
        current_sessions: report.totals.sessions,
        baseline_total_tokens: baseline.and_then(|snapshot| snapshot.totals.usage.total_tokens),
        current_total_tokens: report.totals.usage.total_tokens,
        delta_total_tokens: delta(
            baseline.and_then(|snapshot| snapshot.totals.usage.total_tokens),
            report.totals.usage.total_tokens,
        ),
    };
    let _ = &report_totals;
    Ok(BaselineDiff {
        schema_version: BASELINE_SCHEMA_VERSION,
        command: "baseline diff",
        generated_at: report.generated_at.clone(),
        baseline: name.to_owned(),
        current_root: report.sessions_root.clone(),
        compatible,
        incompatibility: (!compatible)
            .then(|| format!("snapshot schema {schema} is not {BASELINE_SCHEMA_VERSION}")),
        sessions: session_moves,
        by_project,
        by_model,
        by_day,
        totals,
        limitation: BASELINE_LIMITATION,
    })
}

fn report_totals_with<'a>(
    report: &'a Report,
    select: impl Fn(&'a Report) -> &'a [Bucket],
) -> BTreeMap<String, (usize, Option<u64>)> {
    bucket_totals(select(report))
}

fn delta(baseline: Option<u64>, current: Option<u64>) -> Option<i64> {
    baseline.zip(current).and_then(|(baseline, current)| {
        current
            .checked_sub(baseline)
            .map(|value| value as i64)
            .or_else(|| baseline.checked_sub(current).map(|value| -(value as i64)))
    })
}

fn bucket_moves(
    baseline: &BTreeMap<String, (usize, Option<u64>)>,
    current: &BTreeMap<String, (usize, Option<u64>)>,
) -> Vec<BucketMovement> {
    let mut keys: Vec<&String> = baseline.keys().chain(current.keys()).collect();
    keys.sort();
    keys.dedup();
    keys.into_iter()
        .map(|key| {
            let (baseline_sessions, baseline_tokens) =
                baseline.get(key).copied().unwrap_or((0, None));
            let (current_sessions, current_tokens) = current.get(key).copied().unwrap_or((0, None));
            BucketMovement {
                key: (*key).clone(),
                baseline_sessions,
                current_sessions,
                baseline_total_tokens: baseline_tokens,
                current_total_tokens: current_tokens,
                delta_total_tokens: delta(baseline_tokens, current_tokens),
            }
        })
        .collect()
}
