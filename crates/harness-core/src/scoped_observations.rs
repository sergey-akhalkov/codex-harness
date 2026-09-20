//! Scoped account observations for budget-honest pacing.
//!
//! Three sources are accepted and nothing else:
//!
//! * the installed native Codex/GPT limit snapshot - the provider-issued
//!   `rate_limits` record the CLI writes into its own session files,
//! * actual provider refusals reported by the lead or an executor, and
//! * bounded dashboard snapshots supplied by the user.
//!
//! Reading is local and passive: this module issues no provider request, never
//! derives a remaining allowance from local request counts or token usage, and
//! never invents a percentage. Missing, unparsable or stale telemetry stays
//! unknown, and [`account_view`] reports it as unknown instead of zero or
//! unlimited. A dashboard snapshot is an opaque bounded input: only the fields
//! the user typed are recorded, and the raw dashboard is never fetched or
//! parsed.
use crate::board_cli::json_ok;
use serde_json::Value;
use std::{
    fs, io,
    io::{BufReader, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

/// Default freshness bound of a recorded observation, in seconds.
pub const DEFAULT_MAX_AGE_SECONDS: u64 = 900;
pub const MIN_MAX_AGE_SECONDS: u64 = 30;
pub const MAX_MAX_AGE_SECONDS: u64 = 86_400;
pub const MAX_SCOPE: usize = 40;
pub const MAX_WINDOW_MINUTES: u32 = 100_800;
pub const MAX_REFUSALS: u32 = 64;

/// Limit identity of the native Codex/GPT account in the CLI's own limit
/// record. A record with another identity is not a lead-account observation.
pub const CODEX_LIMIT_ID: &str = "codex";

const OBSERVATION_PREFIX: &str = "pacing-observation v1";
const UNKNOWN: &str = "unknown";
/// Bounded discovery and tail reading of the native session records.
const MAX_SESSION_FILES: usize = 8;
const MAX_SESSION_SCAN: usize = 4096;
const MAX_SESSION_TAIL: u64 = 256 * 1024;

/// Where an observation comes from. There is no fourth source: no probe call,
/// no request-count estimate and no dashboard scrape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ObservationSource {
    NativeLimit,
    ProviderRefusal,
    DashboardSnapshot,
}

impl ObservationSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NativeLimit => "native-limit",
            Self::ProviderRefusal => "provider-refusal",
            Self::DashboardSnapshot => "dashboard-snapshot",
        }
    }

    fn parse(value: &str) -> io::Result<Self> {
        match value {
            "native-limit" => Ok(Self::NativeLimit),
            "provider-refusal" => Ok(Self::ProviderRefusal),
            "dashboard-snapshot" => Ok(Self::DashboardSnapshot),
            _ => Err(invalid(format!("unknown observation source {value}"))),
        }
    }
}

/// One bounded observation of an account scope. `used_percent` is `None`
/// whenever the value is unknown; a refusal reports no percentage at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountObservation {
    pub scope: String,
    pub source: ObservationSource,
    pub used_percent: Option<u8>,
    pub resets_at: Option<u64>,
    pub window_minutes: Option<u32>,
    pub refusals: u32,
    pub observed_at: u64,
    pub max_age_seconds: u64,
}

impl AccountObservation {
    /// The bounds that keep observations bounded and honest. A native read
    /// without a percentage is rejected: an unusable read stays unknown at the
    /// caller, not a fabricated reading.
    pub fn bounded(self) -> io::Result<Self> {
        let scope = require_token("scope", &self.scope, MAX_SCOPE)?;
        if self.observed_at == 0 {
            return Err(invalid("observed_at is required"));
        }
        if !(MIN_MAX_AGE_SECONDS..=MAX_MAX_AGE_SECONDS).contains(&self.max_age_seconds) {
            return Err(invalid(format!(
                "max_age must be between {MIN_MAX_AGE_SECONDS} and {MAX_MAX_AGE_SECONDS} seconds"
            )));
        }
        if self.refusals > MAX_REFUSALS {
            return Err(invalid(format!("refusals must be at most {MAX_REFUSALS}")));
        }
        if let Some(percent) = self.used_percent
            && percent > 100
        {
            return Err(invalid("used_percent must be at most 100"));
        }
        if let Some(window) = self.window_minutes
            && (window == 0 || window > MAX_WINDOW_MINUTES)
        {
            return Err(invalid(format!(
                "window_minutes must be between 1 and {MAX_WINDOW_MINUTES}"
            )));
        }
        match self.source {
            ObservationSource::NativeLimit => {
                if self.used_percent.is_none() {
                    return Err(invalid("a native limit read requires used_percent"));
                }
            }
            ObservationSource::ProviderRefusal => {
                if self.refusals == 0 {
                    return Err(invalid("a provider refusal requires a refusal count"));
                }
                if self.used_percent.is_some() {
                    return Err(invalid(
                        "a provider refusal reports no percentage; keep used_percent unknown",
                    ));
                }
            }
            ObservationSource::DashboardSnapshot => {
                if self.used_percent.is_none()
                    && self.resets_at.is_none()
                    && self.window_minutes.is_none()
                {
                    return Err(invalid(
                        "a dashboard snapshot must declare at least one reading",
                    ));
                }
            }
        }
        Ok(Self { scope, ..self })
    }

    /// Freshness is evaluated at the call site so a stale observation is
    /// reported as unknown rather than silently reused.
    pub fn is_fresh(&self, now: u64) -> bool {
        now >= self.observed_at && now - self.observed_at <= self.max_age_seconds
    }

    pub fn expires_at(&self) -> u64 {
        self.observed_at + self.max_age_seconds
    }

    /// One-line board record. `used`, `resets_at` and `window_minutes` carry
    /// the literal token `unknown` when nothing is known.
    pub fn to_comment(&self) -> String {
        format!(
            "{OBSERVATION_PREFIX} scope={} source={} used={} resets_at={} window_minutes={} refusals={} observed_at={} max_age={}",
            self.scope,
            self.source.as_str(),
            number_or_unknown(self.used_percent.map(u64::from)),
            number_or_unknown(self.resets_at),
            number_or_unknown(self.window_minutes.map(u64::from)),
            self.refusals,
            self.observed_at,
            self.max_age_seconds
        )
    }
}

/// The bounded, deduplicated view of one account scope at a moment in time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountView {
    pub scope: String,
    pub used_percent: Option<u8>,
    pub resets_at: Option<u64>,
    pub window_minutes: Option<u32>,
    pub refusals: u32,
    /// Sources that supplied fresh data, sorted and deduplicated.
    pub sources: Vec<ObservationSource>,
    /// Fresh observations the scope had to drop because they expired.
    pub stale_ignored: usize,
    /// When the newest contributing observation stops being fresh.
    pub expires_at: Option<u64>,
}

impl AccountView {
    /// True when nothing fresh describes this scope.
    pub fn is_unknown(&self) -> bool {
        self.sources.is_empty() && self.stale_ignored == 0
    }

    /// A short, inspectable basis for a pacing reason.
    pub fn describe(&self) -> String {
        if self.is_unknown() {
            return format!("{}: telemetry unknown", self.scope);
        }
        let used = match self.used_percent {
            Some(percent) => format!("{percent}% used"),
            None => "used unknown".to_owned(),
        };
        let resets = match self.resets_at {
            Some(at) => format!(" resets_at={at}"),
            None => String::new(),
        };
        let refusals = if self.refusals > 0 {
            format!(" refusals={}", self.refusals)
        } else {
            String::new()
        };
        let stale = if self.stale_ignored > 0 {
            format!(" stale_ignored={}", self.stale_ignored)
        } else {
            String::new()
        };
        format!("{}: {used}{resets}{refusals}{stale}", self.scope)
    }
}

/// Combines observations of one scope at `now`. Only fresh observations count;
/// the newest fresh percentage wins, and refusals from distinct episodes add
/// up. A scope with no usable reading stays unknown.
pub fn account_view(observations: &[AccountObservation], scope: &str, now: u64) -> AccountView {
    let mut fresh: Vec<&AccountObservation> = Vec::new();
    let mut stale_ignored = 0usize;
    for observation in observations.iter().filter(|item| item.scope == scope) {
        if observation.is_fresh(now) {
            fresh.push(observation);
        } else {
            stale_ignored += 1;
        }
    }
    let reading = fresh
        .iter()
        .filter(|item| item.used_percent.is_some())
        .max_by_key(|item| item.observed_at);
    let refusals = fresh
        .iter()
        .map(|item| item.refusals)
        .sum::<u32>()
        .min(MAX_REFUSALS);
    let mut sources: Vec<ObservationSource> = fresh.iter().map(|item| item.source).collect();
    sources.sort();
    sources.dedup();
    AccountView {
        scope: scope.to_owned(),
        used_percent: reading.and_then(|item| item.used_percent),
        resets_at: reading.and_then(|item| item.resets_at),
        window_minutes: reading.and_then(|item| item.window_minutes),
        refusals,
        expires_at: fresh.iter().map(|item| item.expires_at()).max(),
        sources,
        stale_ignored,
    }
}

/// Reads the newest native Codex/GPT limit snapshot under `codex_home`.
///
/// The read is bounded: at most [`MAX_SESSION_FILES`] newest session files are
/// opened, only their last [`MAX_SESSION_TAIL`] bytes are parsed, and only a
/// provider-issued `rate_limits` record with the Codex limit identity is
/// accepted. `Ok(None)` means the installed contract exposes no usable
/// snapshot within `max_age_seconds` - a missing or stale reading stays
/// unknown. Local token counts in the same records are never read as a
/// remaining allowance.
pub fn native_limit_read(
    codex_home: &Path,
    scope: &str,
    now: u64,
    max_age_seconds: u64,
) -> io::Result<Option<AccountObservation>> {
    if !(MIN_MAX_AGE_SECONDS..=MAX_MAX_AGE_SECONDS).contains(&max_age_seconds) {
        return Err(invalid(format!(
            "max_age must be between {MIN_MAX_AGE_SECONDS} and {MAX_MAX_AGE_SECONDS} seconds"
        )));
    }
    let sessions = codex_home.join("sessions");
    if !sessions.is_dir() {
        return Ok(None);
    }
    for path in recent_session_files(&sessions)? {
        let fallback = file_seconds(&path)?;
        for line in tail_lines(&path, MAX_SESSION_TAIL)? {
            let Ok(record) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            let Some(observation) = native_observation(&record, scope, fallback, max_age_seconds)
            else {
                continue;
            };
            let Ok(observation) = observation.bounded() else {
                continue;
            };
            if observation.is_fresh(now) {
                return Ok(Some(observation));
            }
        }
    }
    Ok(None)
}

/// A dashboard snapshot as the user typed it: opaque, bounded and free to omit
/// fields. Omitted fields stay unknown; the snapshot is never fetched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DashboardSnapshotDraft {
    pub scope: String,
    pub used_percent: Option<u8>,
    pub resets_at: Option<u64>,
    pub window_minutes: Option<u32>,
    pub observed_at: u64,
    pub max_age_seconds: u64,
}

pub fn dashboard_snapshot(draft: DashboardSnapshotDraft) -> io::Result<AccountObservation> {
    AccountObservation {
        scope: draft.scope,
        source: ObservationSource::DashboardSnapshot,
        used_percent: draft.used_percent,
        resets_at: draft.resets_at,
        window_minutes: draft.window_minutes,
        refusals: 0,
        observed_at: draft.observed_at,
        max_age_seconds: draft.max_age_seconds,
    }
    .bounded()
}

/// An actual provider refusal seen by an executor. It reports no percentage:
/// the refusal is the observation.
pub fn provider_refusal(
    scope: &str,
    refusals: u32,
    observed_at: u64,
    max_age_seconds: u64,
) -> io::Result<AccountObservation> {
    AccountObservation {
        scope: scope.to_owned(),
        source: ObservationSource::ProviderRefusal,
        used_percent: None,
        resets_at: None,
        window_minutes: None,
        refusals,
        observed_at,
        max_age_seconds,
    }
    .bounded()
}

/// Records one observation as a board comment. The comment is the durable,
/// inspectable record; no model call is involved.
pub fn record_observation(
    bd: &Path,
    project: &Path,
    item_id: &str,
    observation: &AccountObservation,
) -> io::Result<()> {
    json_ok(
        bd,
        project,
        &["comment", item_id, "--json", &observation.to_comment()],
    )?;
    Ok(())
}

/// Parses recorded observations, skipping anything that is not a well-formed
/// bounded observation. Unknown fields stay unknown.
pub fn parse_observation_comments(comments: &[String]) -> Vec<AccountObservation> {
    comments
        .iter()
        .filter_map(|comment| parse_observation_comment(comment))
        .collect()
}

/// Reads recorded observations from the comments of one board item.
pub fn list_observations(
    bd: &Path,
    project: &Path,
    item_id: &str,
) -> io::Result<Vec<AccountObservation>> {
    let value = json_ok(bd, project, &["comments", item_id, "--json"])?;
    let comments: Vec<String> = comment_values(&value)
        .iter()
        .filter_map(comment_text)
        .map(str::to_owned)
        .collect();
    Ok(parse_observation_comments(&comments))
}

fn parse_observation_comment(comment: &str) -> Option<AccountObservation> {
    let rest = comment.strip_prefix(OBSERVATION_PREFIX)?.trim();
    let mut scope = None;
    let mut source = None;
    let mut used = None;
    let mut resets_at = None;
    let mut window_minutes = None;
    let mut refusals = 0u32;
    let mut observed_at = None;
    let mut max_age = None;
    for part in rest.split_whitespace() {
        let Some((key, value)) = part.split_once('=') else {
            continue;
        };
        match key {
            "scope" => scope = Some(value.to_owned()),
            "source" => source = ObservationSource::parse(value).ok(),
            "used" => used = Some(value),
            "resets_at" => resets_at = Some(value),
            "window_minutes" => window_minutes = Some(value),
            "refusals" => refusals = value.parse().ok()?,
            "observed_at" => observed_at = value.parse().ok(),
            "max_age" => max_age = value.parse().ok(),
            _ => {}
        }
    }
    AccountObservation {
        scope: scope?,
        source: source?,
        used_percent: unknown_or_number(used?).map(|value| value.min(100) as u8),
        resets_at: unknown_or_number(resets_at?),
        window_minutes: unknown_or_number(window_minutes?).map(|value| value as u32),
        refusals,
        observed_at: observed_at?,
        max_age_seconds: max_age?,
    }
    .bounded()
    .ok()
}

fn unknown_or_number(value: &str) -> Option<u64> {
    if value == UNKNOWN {
        None
    } else {
        value.parse().ok()
    }
}

fn number_or_unknown(value: Option<u64>) -> String {
    match value {
        Some(number) => number.to_string(),
        None => UNKNOWN.to_owned(),
    }
}

/// Extracts the provider-issued limit record. Token counts, request counts and
/// any other local bookkeeping are ignored: they are not a remaining
/// allowance.
fn native_observation(
    record: &Value,
    scope: &str,
    fallback: u64,
    max_age_seconds: u64,
) -> Option<AccountObservation> {
    let limits = record.get("payload")?.get("rate_limits")?;
    if limits.get("limit_id").and_then(Value::as_str)? != CODEX_LIMIT_ID {
        return None;
    }
    let primary = limits.get("primary")?;
    let percent = primary.get("used_percent")?.as_f64()?;
    if !percent.is_finite() {
        return None;
    }
    let resets_at = primary.get("resets_at").and_then(Value::as_u64);
    let window_minutes = primary
        .get("window_minutes")
        .and_then(Value::as_u64)
        .map(|value| value as u32);
    let observed_at = record
        .get("timestamp")
        .and_then(Value::as_str)
        .and_then(parse_rfc3339_utc)
        .unwrap_or(fallback);
    Some(AccountObservation {
        scope: scope.to_owned(),
        source: ObservationSource::NativeLimit,
        used_percent: Some(percent.round().clamp(0.0, 100.0) as u8),
        resets_at: resets_at.filter(|value| *value > 0),
        window_minutes: window_minutes.filter(|value| *value > 0),
        refusals: 0,
        observed_at,
        max_age_seconds,
    })
}

/// Newest session records first, bounded by [`MAX_SESSION_SCAN`] visited
/// entries and [`MAX_SESSION_FILES`] results.
fn recent_session_files(sessions: &Path) -> io::Result<Vec<PathBuf>> {
    let mut stack = vec![sessions.to_path_buf()];
    let mut visited = 0usize;
    let mut files: Vec<(u64, PathBuf)> = Vec::new();
    while let Some(directory) = stack.pop() {
        // Session directories are dated, so descending names visit the newest
        // records first and the visit bound cannot hide them.
        let mut entries: Vec<fs::DirEntry> = fs::read_dir(&directory)?.collect::<Result<_, _>>()?;
        entries.sort_by_key(|entry| std::cmp::Reverse(entry.file_name().to_owned()));
        let mut directories = Vec::new();
        for entry in entries {
            visited += 1;
            if visited > MAX_SESSION_SCAN {
                break;
            }
            let path = entry.path();
            let metadata = entry.metadata()?;
            if metadata.is_dir() {
                directories.push(path);
            } else if metadata.is_file()
                && path
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("jsonl"))
            {
                let modified = metadata
                    .modified()
                    .ok()
                    .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                    .map(|duration| duration.as_secs())
                    .unwrap_or(0);
                files.push((modified, path));
            }
        }
        for directory in directories.into_iter().rev() {
            stack.push(directory);
        }
        if visited > MAX_SESSION_SCAN {
            break;
        }
    }
    files.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| right.1.cmp(&left.1)));
    files.truncate(MAX_SESSION_FILES);
    Ok(files.into_iter().map(|(_, path)| path).collect())
}

/// Reads the last `limit` bytes of a file as text. A partial first line is
/// dropped because the byte offset can split a record.
fn tail_lines(path: &Path, limit: u64) -> io::Result<Vec<String>> {
    let file = fs::File::open(path)?;
    let length = file.metadata()?.len();
    let start = length.saturating_sub(limit);
    let mut reader = BufReader::new(file);
    reader.seek(SeekFrom::Start(start))?;
    let mut bytes = Vec::new();
    reader.take(limit).read_to_end(&mut bytes)?;
    let text = String::from_utf8_lossy(&bytes);
    let mut lines: Vec<String> = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(str::to_owned)
        .collect();
    if start > 0 && !lines.is_empty() {
        lines.remove(0);
    }
    lines.reverse();
    Ok(lines)
}

fn file_seconds(path: &Path) -> io::Result<u64> {
    Ok(fs::metadata(path)?
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs())
        .unwrap_or(0))
}

/// Parses the fixed `YYYY-MM-DDTHH:MM:SS[.fff]Z` form the native CLI records.
fn parse_rfc3339_utc(text: &str) -> Option<u64> {
    let bytes = text.as_bytes();
    if bytes.len() < 20 || !text.ends_with('Z') {
        return None;
    }
    let year: i64 = text.get(0..4)?.parse().ok()?;
    let month: i64 = text.get(5..7)?.parse().ok()?;
    let day: i64 = text.get(8..10)?.parse().ok()?;
    let hour: u64 = text.get(11..13)?.parse().ok()?;
    let minute: u64 = text.get(14..16)?.parse().ok()?;
    let second: u64 = text.get(17..19)?.parse().ok()?;
    if text.as_bytes().get(4) != Some(&b'-')
        || text.as_bytes().get(7) != Some(&b'-')
        || text.as_bytes().get(10) != Some(&b'T')
        || text.as_bytes().get(13) != Some(&b':')
        || text.as_bytes().get(16) != Some(&b':')
        || !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return None;
    }
    let days = days_from_civil(year, month, day);
    let seconds = days * 86_400 + (hour * 3600 + minute * 60 + second) as i64;
    u64::try_from(seconds).ok()
}

/// Days since 1970-01-01 for a proleptic Gregorian date (civil calendar
/// algorithm), so no date library is needed for one record field.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let month_shift = if month > 2 { month - 3 } else { month + 9 };
    let day_of_year = (153 * month_shift + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

fn comment_values(value: &Value) -> &[Value] {
    match value {
        Value::Array(rows) => rows,
        Value::Object(map) => map
            .get("comments")
            .or_else(|| map.get("items"))
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[]),
        _ => &[],
    }
}

fn comment_text(value: &Value) -> Option<&str> {
    value
        .get("text")
        .and_then(Value::as_str)
        .or_else(|| value.get("body").and_then(Value::as_str))
        .or_else(|| value.get("comment").and_then(Value::as_str))
        .or_else(|| value.get("content").and_then(Value::as_str))
}

fn require_token(name: &str, value: &str, max: usize) -> io::Result<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(invalid(format!("{name} is required")));
    }
    if trimmed.len() > max {
        return Err(invalid(format!("{name} exceeds {max} bytes")));
    }
    if trimmed.bytes().any(|byte| {
        !(byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b':' | b'@'))
    }) {
        return Err(invalid(format!("{name} contains unsupported characters")));
    }
    Ok(trimmed.to_owned())
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn native_record(timestamp: &str, used_percent: f64, resets_at: u64, limit_id: &str) -> String {
        format!(
            r#"{{"timestamp":"{timestamp}","type":"event_msg","payload":{{"type":"token_count","info":{{"total_token_usage":{{"total_tokens":91000}}}},"rate_limits":{{"limit_id":"{limit_id}","primary":{{"used_percent":{used_percent},"window_minutes":10080,"resets_at":{resets_at}}},"plan_type":"pro"}}}}}}"#
        )
    }

    fn session_home(name: &str, files: &[(&str, &str)]) -> std::path::PathBuf {
        let root =
            std::env::temp_dir().join(format!("scoped-observation-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let day = root.join("sessions/2026/09/19");
        fs::create_dir_all(&day).unwrap();
        for (file, body) in files {
            fs::write(day.join(file), body).unwrap();
        }
        root
    }

    #[test]
    fn native_read_uses_the_newest_record_and_ignores_other_limits() {
        let home = session_home(
            "newest",
            &[
                (
                    "rollout-a.jsonl",
                    &format!(
                        "{}\n{}\n",
                        native_record(
                            "2026-09-19T21:10:00.000Z",
                            4.0,
                            1_790_418_200,
                            CODEX_LIMIT_ID
                        ),
                        native_record(
                            "2026-09-19T21:14:00.000Z",
                            91.0,
                            1_790_418_203,
                            CODEX_LIMIT_ID
                        )
                    ),
                ),
                (
                    "rollout-b.jsonl",
                    &format!(
                        "{}\n",
                        native_record(
                            "2026-09-19T21:15:00.000Z",
                            55.0,
                            1_790_418_203,
                            "other-provider"
                        )
                    ),
                ),
            ],
        );
        let now = 1_789_853_000;
        let observation = native_limit_read(&home, "gpt", now, DEFAULT_MAX_AGE_SECONDS)
            .unwrap()
            .unwrap();
        assert_eq!(observation.source, ObservationSource::NativeLimit);
        assert_eq!(observation.scope, "gpt");
        assert_eq!(observation.used_percent, Some(91));
        assert_eq!(observation.resets_at, Some(1_790_418_203));
        assert_eq!(observation.window_minutes, Some(10_080));
        assert_eq!(observation.observed_at, 1_789_852_440);
        assert_eq!(observation.max_age_seconds, DEFAULT_MAX_AGE_SECONDS);
        assert!(observation.is_fresh(now));
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn unknown_and_stale_native_telemetry_stays_unknown() {
        let missing = session_home("missing", &[]);
        assert_eq!(
            native_limit_read(&missing, "gpt", 1_789_853_000, DEFAULT_MAX_AGE_SECONDS).unwrap(),
            None
        );
        let _ = fs::remove_dir_all(&missing);

        let stale = session_home(
            "stale",
            &[(
                "rollout.jsonl",
                &format!(
                    "{}\n",
                    native_record(
                        "2026-09-19T21:14:00.000Z",
                        42.0,
                        1_790_418_203,
                        CODEX_LIMIT_ID
                    )
                ),
            )],
        );
        let recorded = 1_789_852_440;
        let found = native_limit_read(&stale, "gpt", recorded + 60, DEFAULT_MAX_AGE_SECONDS)
            .unwrap()
            .unwrap();
        assert_eq!(found.used_percent, Some(42));
        assert_eq!(
            native_limit_read(
                &stale,
                "gpt",
                recorded + DEFAULT_MAX_AGE_SECONDS + 1,
                DEFAULT_MAX_AGE_SECONDS
            )
            .unwrap(),
            None,
            "an expired snapshot is not a reading"
        );
        assert_eq!(
            native_limit_read(&stale, "gpt", recorded + 3_600, 3_600)
                .unwrap()
                .unwrap()
                .used_percent,
            Some(42),
            "a caller that accepts an older snapshot reads the same provider value"
        );
        let _ = fs::remove_dir_all(&stale);
    }

    #[test]
    fn local_token_counts_are_never_a_remaining_allowance() {
        let home = session_home(
            "usage-only",
            &[(
                "rollout.jsonl",
                "{\"timestamp\":\"2026-09-19T21:14:00.000Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"total_token_usage\":{\"total_tokens\":910000},\"last_token_usage\":{\"total_tokens\":12000}}}}\n",
            )],
        );
        assert_eq!(
            native_limit_read(&home, "gpt", 1_789_853_000, DEFAULT_MAX_AGE_SECONDS).unwrap(),
            None
        );
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn malformed_records_do_not_produce_a_reading() {
        let home = session_home(
            "malformed",
            &[
                (
                    "rollout-a.jsonl",
                    "{\"timestamp\":\"2026-09-19T21:14:00.000Z\"\n",
                ),
                ("rollout-b.jsonl", "{}\n"),
            ],
        );
        assert_eq!(
            native_limit_read(&home, "gpt", 1_789_853_000, DEFAULT_MAX_AGE_SECONDS).unwrap(),
            None
        );
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn observation_comments_round_trip_with_unknowns_preserved() {
        let recorded = provider_refusal("xai", 2, 1_789_852_000, DEFAULT_MAX_AGE_SECONDS).unwrap();
        let snapshot = dashboard_snapshot(DashboardSnapshotDraft {
            scope: "zai".into(),
            used_percent: Some(62),
            resets_at: None,
            window_minutes: Some(300),
            observed_at: 1_789_852_100,
            max_age_seconds: DEFAULT_MAX_AGE_SECONDS,
        })
        .unwrap();
        let comments = vec![recorded.to_comment(), snapshot.to_comment()];
        let parsed = parse_observation_comments(&comments);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0], recorded);
        assert_eq!(parsed[1], snapshot);
        assert!(parsed[0].to_comment().contains("used=unknown"));
        assert!(comments[0].contains("source=provider-refusal"));
    }

    #[test]
    fn a_refusal_never_becomes_an_invented_percentage() {
        let error = provider_refusal("xai", 0, 1_789_852_000, DEFAULT_MAX_AGE_SECONDS).unwrap_err();
        assert!(error.to_string().contains("refusal count"));
        let error = AccountObservation {
            scope: "xai".into(),
            source: ObservationSource::ProviderRefusal,
            used_percent: Some(100),
            resets_at: None,
            window_minutes: None,
            refusals: 1,
            observed_at: 1_789_852_000,
            max_age_seconds: DEFAULT_MAX_AGE_SECONDS,
        }
        .bounded()
        .unwrap_err();
        assert!(error.to_string().contains("no percentage"));
        let error = AccountObservation {
            scope: "gpt".into(),
            source: ObservationSource::NativeLimit,
            used_percent: None,
            resets_at: None,
            window_minutes: None,
            refusals: 0,
            observed_at: 1_789_852_000,
            max_age_seconds: DEFAULT_MAX_AGE_SECONDS,
        }
        .bounded()
        .unwrap_err();
        assert!(error.to_string().contains("requires used_percent"));
    }

    #[test]
    fn view_keeps_unknown_unknown_and_reports_stale_input() {
        let scope = "gpt";
        let now = 1_789_852_000;
        let refusal = provider_refusal(scope, 1, now - 10, DEFAULT_MAX_AGE_SECONDS).unwrap();
        let view = account_view(std::slice::from_ref(&refusal), scope, now);
        assert_eq!(view.used_percent, None);
        assert_eq!(view.refusals, 1);
        assert_eq!(view.sources, vec![ObservationSource::ProviderRefusal]);
        assert!(!view.is_unknown());
        assert!(view.describe().contains("used unknown"));

        let empty = account_view(&[], scope, now);
        assert!(empty.is_unknown());
        assert_eq!(empty.used_percent, None);
        assert_eq!(empty.refusals, 0);
        assert!(empty.describe().contains("telemetry unknown"));

        let stale = account_view(&[refusal], scope, now + DEFAULT_MAX_AGE_SECONDS + 1);
        assert_eq!(stale.used_percent, None);
        assert_eq!(stale.refusals, 0);
        assert_eq!(stale.stale_ignored, 1);
        assert!(!stale.is_unknown());
    }

    #[test]
    fn view_prefers_the_newest_percentage_and_keeps_scopes_apart() {
        let now = 1_789_852_000;
        let older = dashboard_snapshot(DashboardSnapshotDraft {
            scope: "zai".into(),
            used_percent: Some(20),
            resets_at: Some(now + 600),
            window_minutes: Some(300),
            observed_at: now - 600,
            max_age_seconds: DEFAULT_MAX_AGE_SECONDS,
        })
        .unwrap();
        let newer = dashboard_snapshot(DashboardSnapshotDraft {
            scope: "zai".into(),
            used_percent: Some(88),
            resets_at: Some(now + 900),
            window_minutes: Some(300),
            observed_at: now - 5,
            max_age_seconds: DEFAULT_MAX_AGE_SECONDS,
        })
        .unwrap();
        let other = provider_refusal("xai", 1, now - 1, DEFAULT_MAX_AGE_SECONDS).unwrap();
        let view = account_view(&[older.clone(), newer.clone(), other.clone()], "zai", now);
        assert_eq!(view.used_percent, Some(88));
        assert_eq!(view.resets_at, Some(now + 900));
        assert_eq!(view.refusals, 0);
        assert_eq!(view.sources, vec![ObservationSource::DashboardSnapshot]);
        assert_eq!(view.expires_at, Some(newer.expires_at()));

        let other_view = account_view(&[older, newer, other], "xai", now);
        assert_eq!(other_view.used_percent, None);
        assert_eq!(other_view.refusals, 1);
    }

    #[test]
    fn timestamp_parsing_matches_known_instants() {
        assert_eq!(parse_rfc3339_utc("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(
            parse_rfc3339_utc("2026-09-19T21:14:33.288Z"),
            Some(1_789_852_473)
        );
        assert_eq!(parse_rfc3339_utc("2026-09-19T21:14:33"), None);
        assert_eq!(parse_rfc3339_utc("2026-13-19T21:14:33Z"), None);
    }

    #[test]
    #[ignore = "requires HARNESS_PACING_CODEX_HOME pointing at the installed Codex home; reads only the CLI's own limit snapshot"]
    fn live_native_limit_read_reports_the_installed_snapshot_or_unknown() {
        let home = std::path::PathBuf::from(
            std::env::var_os("HARNESS_PACING_CODEX_HOME").expect("HARNESS_PACING_CODEX_HOME"),
        );
        let now = std::time::SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        match native_limit_read(&home, "gpt", now, DEFAULT_MAX_AGE_SECONDS).unwrap() {
            Some(observation) => {
                assert!(observation.used_percent.is_some());
                assert!(observation.observed_at <= now);
                assert!(observation.is_fresh(now));
                println!("default bound: {}", observation.to_comment());
            }
            None => println!("default bound: no fresh snapshot, telemetry stays unknown"),
        }
        // The same read with an explicitly wider bound shows what the
        // installed contract exposed and how old that snapshot already is.
        match native_limit_read(&home, "gpt", now, 3_600).unwrap() {
            Some(observation) => {
                println!(
                    "older snapshot accepted (age {}s): {}",
                    now - observation.observed_at,
                    observation.to_comment()
                );
            }
            None => println!("wider bound: the installed contract exposes no snapshot"),
        }
    }
}
