//! Scoped account observations for budget-honest pacing.
//!
//! Three sources are accepted and nothing else: the installed native
//! Codex/GPT limit snapshot the CLI itself records, actual provider refusals
//! reported by the lead or an executor, and bounded dashboard snapshots the
//! user supplies. Unknown stays unknown - no probe call, no local request-count
//! remainder, no invented percentage. The lead records an observation as a
//! native `pacing-observation v1` board comment (see
//! `.agents/skills/board-workflow`); this module reads those comments back.
//!
//! Missing, unparsable or stale telemetry stays unknown, and [`account_view`]
//! reports it as unknown instead of zero or unlimited. A dashboard snapshot is
//! an opaque bounded input: only the fields the user typed are recorded, and
//! nothing is fetched or scraped.

use std::io;

/// Record marker of the documented observation comment.
const OBSERVATION_PREFIX: &str = "pacing-observation v1";
const UNKNOWN: &str = "unknown";
/// Bounds that keep recorded observations bounded and honest.
const MIN_MAX_AGE_SECONDS: u64 = 30;
const MAX_MAX_AGE_SECONDS: u64 = 86_400;
const MAX_SCOPE: usize = 40;
const MAX_WINDOW_MINUTES: u32 = 100_800;
const MAX_REFUSALS: u32 = 64;

/// Where an observation comes from. There is no fourth source: no probe call,
/// no request-count estimate and no dashboard scrape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObservationSource {
    NativeLimit,
    ProviderRefusal,
    DashboardSnapshot,
}

impl ObservationSource {
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
    fn bounded(self) -> io::Result<Self> {
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
    fn is_fresh(&self, now: u64) -> bool {
        now >= self.observed_at && now - self.observed_at <= self.max_age_seconds
    }
}

/// The bounded view of one account scope at a moment in time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountView {
    pub scope: String,
    pub used_percent: Option<u8>,
    pub resets_at: Option<u64>,
    pub refusals: u32,
    /// Fresh observations the scope had to drop because they expired.
    pub stale_ignored: usize,
    /// Fresh observations that contributed.
    fresh: usize,
}

impl AccountView {
    /// True when nothing describes this scope: no fresh and no stale input.
    pub fn is_unknown(&self) -> bool {
        self.fresh == 0 && self.stale_ignored == 0
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
    AccountView {
        scope: scope.to_owned(),
        used_percent: reading.and_then(|item| item.used_percent),
        resets_at: reading.and_then(|item| item.resets_at),
        refusals,
        stale_ignored,
        fresh: fresh.len(),
    }
}

/// Parses recorded observations, skipping anything that is not a well-formed
/// bounded observation. Unknown fields stay unknown.
pub fn parse_observation_comments(comments: &[String]) -> Vec<AccountObservation> {
    comments
        .iter()
        .filter_map(|comment| parse_observation_comment(comment))
        .collect()
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

    const NOW: u64 = 1_789_852_000;

    /// One documented `pacing-observation v1` record as the board-workflow
    /// skill defines it.
    fn observation(
        scope: &str,
        source: &str,
        used: &str,
        resets_at: &str,
        refusals: u32,
        observed_at: u64,
        max_age: u64,
    ) -> String {
        format!(
            "pacing-observation v1 scope={scope} source={source} used={used} resets_at={resets_at} window_minutes=10080 refusals={refusals} observed_at={observed_at} max_age={max_age}"
        )
    }

    #[test]
    fn documented_observations_parse_with_unknowns_preserved() {
        let comments = vec![
            observation("xai", "provider-refusal", "unknown", "unknown", 2, NOW, 900),
            observation(
                "zai",
                "dashboard-snapshot",
                "62",
                "unknown",
                0,
                NOW + 100,
                900,
            ),
            observation("gpt", "native-limit", "93", "1789853000", 0, NOW + 200, 900),
        ];
        let parsed = parse_observation_comments(&comments);
        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed[0].source, ObservationSource::ProviderRefusal);
        assert_eq!(parsed[0].used_percent, None);
        assert_eq!(parsed[0].refusals, 2);
        assert_eq!(parsed[0].resets_at, None);
        assert_eq!(parsed[1].used_percent, Some(62));
        assert_eq!(parsed[2].used_percent, Some(93));
        assert_eq!(parsed[2].resets_at, Some(1_789_853_000));
        assert_eq!(parsed[2].window_minutes, Some(10_080));
    }

    #[test]
    fn unusable_or_inconsistent_observations_stay_unknown() {
        let cases = [
            // A refusal reports the refusal, never a percentage.
            observation("xai", "provider-refusal", "80", "unknown", 1, NOW, 900),
            observation("xai", "provider-refusal", "unknown", "unknown", 0, NOW, 900),
            // A native read without a percentage is unusable.
            observation("gpt", "native-limit", "unknown", "unknown", 0, NOW, 900),
            // A dashboard snapshot must declare at least one reading.
            "pacing-observation v1 scope=gpt source=dashboard-snapshot used=unknown resets_at=unknown window_minutes=unknown refusals=0 observed_at=1789852000 max_age=900".to_owned(),
            // Unknown source, missing fields and out-of-range bounds.
            observation("gpt", "scraped-page", "10", "unknown", 0, NOW, 900),
            "pacing-observation v1 source=dashboard-snapshot used=10 observed_at=1789852000 max_age=900".to_owned(),
            observation("gpt", "dashboard-snapshot", "10", "unknown", 0, 0, 900),
            observation("gpt", "dashboard-snapshot", "10", "unknown", 0, NOW, 5),
            "pacing-observation v1 scope=gpt source=dashboard-snapshot used=10 resets_at=unknown window_minutes=0 refusals=0 observed_at=1789852000 max_age=900".to_owned(),
            "pacing-observation v1 scope=gpt source=dashboard-snapshot used=10 resets_at=unknown window_minutes=10080 refusals=many observed_at=1789852000 max_age=900".to_owned(),
        ];
        for case in cases {
            assert!(
                parse_observation_comments(std::slice::from_ref(&case)).is_empty(),
                "{case}"
            );
        }
        // A percentage above 100 is clamped, exactly as the native snapshot
        // intake always did; the recorded value is never invented upward.
        let clamped = parse_observation_comments(&[observation(
            "gpt",
            "dashboard-snapshot",
            "104",
            "unknown",
            0,
            NOW,
            900,
        )]);
        assert_eq!(clamped[0].used_percent, Some(100));
    }

    #[test]
    fn view_keeps_unknown_unknown_and_reports_stale_input() {
        let scope = "gpt";
        let refusal = parse_observation_comments(&[observation(
            scope,
            "provider-refusal",
            "unknown",
            "unknown",
            1,
            NOW - 10,
            900,
        )]);
        let view = account_view(&refusal, scope, NOW);
        assert_eq!(view.used_percent, None);
        assert_eq!(view.refusals, 1);
        assert!(!view.is_unknown());
        assert!(view.describe().contains("used unknown"));

        let empty = account_view(&[], scope, NOW);
        assert!(empty.is_unknown());
        assert_eq!(empty.used_percent, None);
        assert_eq!(empty.refusals, 0);
        assert!(empty.describe().contains("telemetry unknown"));

        let stale = account_view(&refusal, scope, NOW + 901);
        assert_eq!(stale.used_percent, None);
        assert_eq!(stale.refusals, 0);
        assert_eq!(stale.stale_ignored, 1);
        assert!(!stale.is_unknown());
        assert!(stale.describe().contains("stale_ignored=1"));

        // Exactly at the freshness bound the observation still counts.
        assert_eq!(account_view(&refusal, scope, NOW + 890).refusals, 1);
    }

    #[test]
    fn view_prefers_the_newest_percentage_and_keeps_scopes_apart() {
        let comments = vec![
            observation(
                "zai",
                "dashboard-snapshot",
                "20",
                &(NOW + 600).to_string(),
                0,
                NOW - 600,
                900,
            ),
            observation(
                "zai",
                "dashboard-snapshot",
                "88",
                &(NOW + 900).to_string(),
                0,
                NOW - 5,
                900,
            ),
            observation(
                "xai",
                "provider-refusal",
                "unknown",
                "unknown",
                1,
                NOW - 1,
                900,
            ),
        ];
        let parsed = parse_observation_comments(&comments);
        let view = account_view(&parsed, "zai", NOW);
        assert_eq!(view.used_percent, Some(88));
        assert_eq!(view.resets_at, Some(NOW + 900));
        assert_eq!(view.refusals, 0);

        let other = account_view(&parsed, "xai", NOW);
        assert_eq!(other.used_percent, None);
        assert_eq!(other.refusals, 1);
    }
}
