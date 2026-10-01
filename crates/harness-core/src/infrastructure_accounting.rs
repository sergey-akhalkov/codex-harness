//! Infrastructure-adjusted accounting for one already observed attempt.
//!
//! This is not a monitor, a second journal, or a prediction of an unloaded
//! host. It consumes captured admission, activity and request facts and
//! returns raw totals plus a bounded adjustment. Missing, foreign or
//! unbracketed evidence stays unknown. A queued label alone is not blocking.

use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

/// Attribution rule the report and policy pin before either arm.
pub const RULE_VERSION: &str = "infrastructure-attribution.v1";
/// Smallest new measurement lineage that can consume this instrumentation.
/// Pre-telemetry attempts are not a comparable adjusted baseline.
pub const MEASUREMENT_LINEAGE: &str = "infrastructure-attribution.v1";
/// The only cause eligible for subtraction.
pub const ELIGIBLE_CAUSE: &str = "unrelated-external-blocking";

const TOKEN: &str = "infrastructure-attribution.v1";

/// What a predeclared claim measures. Chosen before results.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MetricView {
    WorkEfficiency,
    Operational,
}

/// A treatment that must not be removed from its own measurement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mechanism {
    None,
    Admission,
    Scheduling,
    Waiting,
    Cache,
}

impl Mechanism {
    /// Admission, scheduling and waiting are the queue effect under test.
    /// Cache cost is not an external wait; it stays in the adjusted view
    /// because this owner never classifies it as eligible noise.
    pub fn owns_queue(self) -> bool {
        matches!(self, Self::Admission | Self::Scheduling | Self::Waiting)
    }
}

/// The infrastructure clause bound into the predeclared uncertainty text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Binding {
    pub view: MetricView,
    pub mechanism: Mechanism,
}

/// Canonical clause. Callers may prefix it with human uncertainty text.
pub fn binding_clause(view: MetricView, mechanism: Mechanism) -> String {
    let view = match view {
        MetricView::WorkEfficiency => "work-efficiency",
        MetricView::Operational => "operational",
    };
    let mechanism = match mechanism {
        Mechanism::None => "none",
        Mechanism::Admission => "admission",
        Mechanism::Scheduling => "scheduling",
        Mechanism::Waiting => "waiting",
        Mechanism::Cache => "cache",
    };
    format!(
        "{TOKEN} view={view} causes={ELIGIBLE_CAUSE} coverage=comparable-bounds material=inconclusive-unless-bound mechanism={mechanism} lineage={MEASUREMENT_LINEAGE}"
    )
}

/// `Ok(None)` when the clause is absent. A present but incomplete clause is
/// an error: it must not be treated as the older raw-only policy.
pub fn parse_binding(uncertainty: &str) -> Result<Option<Binding>, &'static str> {
    let Some(start) = uncertainty.find(TOKEN) else {
        return Ok(None);
    };
    let clause = &uncertainty[start..];
    let mut view = None;
    let mut mechanism = None;
    let mut causes = None;
    let mut coverage = None;
    let mut material = None;
    let mut lineage = None;
    for token in clause.split_whitespace() {
        if token == TOKEN {
            continue;
        }
        let Some((key, value)) = token.split_once('=') else {
            break;
        };
        match key {
            "view" => {
                view = Some(match value {
                    "work-efficiency" => MetricView::WorkEfficiency,
                    "operational" => MetricView::Operational,
                    _ => return Err("infrastructure view is not work-efficiency or operational"),
                });
            }
            "causes" => causes = Some(value),
            "coverage" => coverage = Some(value),
            "material" => material = Some(value),
            "mechanism" => {
                mechanism = Some(match value {
                    "none" => Mechanism::None,
                    "admission" => Mechanism::Admission,
                    "scheduling" => Mechanism::Scheduling,
                    "waiting" => Mechanism::Waiting,
                    "cache" => Mechanism::Cache,
                    _ => return Err("infrastructure mechanism is not a supported token"),
                });
            }
            "lineage" => lineage = Some(value),
            _ => return Err("infrastructure binding has an unknown field"),
        }
    }
    if causes != Some(ELIGIBLE_CAUSE) {
        return Err("infrastructure binding must name only unrelated-external-blocking");
    }
    if coverage != Some("comparable-bounds") || material != Some("inconclusive-unless-bound") {
        return Err("infrastructure binding must keep comparable bounds and inconclusive gaps");
    }
    if lineage != Some(MEASUREMENT_LINEAGE) {
        return Err("infrastructure binding names a different measurement lineage");
    }
    match (view, mechanism) {
        (Some(view), Some(mechanism)) => Ok(Some(Binding { view, mechanism })),
        _ => Err("infrastructure binding is missing view or mechanism"),
    }
}

/// Inputs a new comparison must share. Reviewed candidate code can be reused;
/// pre-telemetry attempts cannot.
pub fn lineage_contract() -> &'static str {
    "infrastructure-attribution.v1 requires a new measurement of both arms under the predeclared infrastructure binding, host QPC activity, native queue evidence, and the existing independent acceptance and preparation identities. Reviewed candidate revisions may be reused. Pre-telemetry attempts are not a measured zero and are not a comparable adjusted baseline. config_identity comparability stays an independent gate."
}

#[derive(Clone, Copy, Debug)]
struct Interval {
    start: u64,
    end: u64,
}

fn normalize(mut intervals: Vec<Interval>) -> Vec<Interval> {
    intervals.retain(|interval| interval.end > interval.start);
    intervals.sort_by_key(|interval| (interval.start, interval.end));
    let mut merged: Vec<Interval> = Vec::new();
    for interval in intervals {
        if let Some(last) = merged.last_mut()
            && interval.start <= last.end
        {
            last.end = last.end.max(interval.end);
            continue;
        }
        merged.push(interval);
    }
    merged
}

fn measure(intervals: &[Interval]) -> u64 {
    intervals
        .iter()
        .map(|interval| interval.end.saturating_sub(interval.start))
        .fold(0, u64::saturating_add)
}

fn intersect(left: &[Interval], right: &[Interval]) -> Vec<Interval> {
    let mut out = Vec::new();
    let mut right_index = 0;
    for interval in left {
        while right_index < right.len() && right[right_index].end <= interval.start {
            right_index += 1;
        }
        let mut cursor = right_index;
        while cursor < right.len() && right[cursor].start < interval.end {
            let start = interval.start.max(right[cursor].start);
            let end = interval.end.min(right[cursor].end);
            if end > start {
                out.push(Interval { start, end });
            }
            cursor += 1;
        }
    }
    normalize(out)
}

fn subtract(source: &[Interval], remove: &[Interval]) -> Vec<Interval> {
    let mut out = Vec::new();
    for interval in source {
        let mut pieces = vec![*interval];
        for cut in remove {
            let mut next = Vec::new();
            for piece in pieces {
                if cut.end <= piece.start || cut.start >= piece.end {
                    next.push(piece);
                    continue;
                }
                if cut.start > piece.start {
                    next.push(Interval {
                        start: piece.start,
                        end: cut.start.min(piece.end),
                    });
                }
                if cut.end < piece.end {
                    next.push(Interval {
                        start: cut.end.max(piece.start),
                        end: piece.end,
                    });
                }
            }
            pieces = next;
        }
        out.extend(pieces);
    }
    normalize(out)
}

fn clip(intervals: &[Interval], window: Interval) -> Vec<Interval> {
    intersect(intervals, &[window])
}

fn u64_field(value: &Value, key: &str) -> Option<u64> {
    value.get(key).and_then(Value::as_u64)
}

fn text_field<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
}

fn bool_field(value: &Value, key: &str) -> Option<bool> {
    value.get(key).and_then(Value::as_bool)
}

fn seconds(ns: u64) -> f64 {
    ns as f64 / 1_000_000_000.0
}

fn interval_of(value: &Value) -> Option<Interval> {
    let start = u64_field(value, "start_ns")?;
    let end = u64_field(value, "end_ns")?;
    (end > start).then_some(Interval { start, end })
}

struct Tokens {
    input: Option<u64>,
    cached: Option<u64>,
    output: Option<u64>,
    reasoning: Option<u64>,
    total: Option<u64>,
}

impl Tokens {
    fn from_value(value: &Value) -> Self {
        Self {
            input: u64_field(value, "input_tokens"),
            cached: u64_field(value, "cached_input_tokens"),
            output: u64_field(value, "output_tokens"),
            reasoning: u64_field(value, "reasoning_output_tokens"),
            total: u64_field(value, "total_tokens"),
        }
    }

    fn consistent(&self) -> bool {
        match (self.cached, self.input) {
            (Some(cached), Some(input)) if cached > input => return false,
            _ => {}
        }
        match (self.reasoning, self.output) {
            (Some(reasoning), Some(output)) if reasoning > output => return false,
            _ => {}
        }
        true
    }

    fn to_json(&self) -> Value {
        json!({
            "input_tokens": self.input,
            "cached_input_tokens": self.cached,
            "output_tokens": self.output,
            "reasoning_output_tokens": self.reasoning,
            "total_tokens": self.total,
        })
    }
}

fn add_option(total: &mut Option<u64>, value: Option<u64>) {
    *total = match (*total, value) {
        (Some(left), Some(right)) => left.checked_add(right),
        _ => None,
    };
}

fn sub_option(raw: Option<u64>, excluded: Option<u64>) -> Option<u64> {
    match (raw, excluded) {
        (Some(raw), Some(excluded)) => raw.checked_sub(excluded),
        (Some(raw), None) => Some(raw),
        _ => None,
    }
}

/// Derive the adjustment for one attempt. `observed_ns` is the raw elapsed
/// duration already retained by the report; it is not replaced.
pub fn adjust(capture: &Value, observed_ns: Option<u64>) -> Value {
    let mut gaps = Vec::new();
    let detail_overflow = capture
        .get("detail_overflow")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let activity_overflow = capture
        .get("activity_overflow")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if detail_overflow {
        gaps.push("raw_detail_overflow".to_owned());
    }
    if activity_overflow {
        gaps.push("compact_activity_overflow".to_owned());
    }
    let window = capture.get("window").and_then(interval_of);
    let Some(window) = window else {
        gaps.push("attempt_window_unknown".to_owned());
        return incomplete(observed_ns, gaps, detail_overflow, activity_overflow);
    };
    let admissions = capture
        .get("admissions")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let activity = capture
        .get("activity")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let requests = capture
        .get("requests")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    if admissions.is_empty() {
        gaps.push("admission_telemetry_absent".to_owned());
    }

    let mut external = Vec::new();
    let mut unresolved = Vec::new();
    let mut queue_exposure = Vec::new();
    let mut bracket_possible = Vec::new();
    let mut failed = Vec::new();
    let mut saw_measured_zero = false;
    let mut saw_queue = false;
    let mut failed_before_start = false;
    let mut polling_operations = 0_u64;

    for admission in &admissions {
        let class = text_field(admission, "class").unwrap_or("unknown");
        let id = text_field(admission, "id").unwrap_or("unidentified");
        let domain_match = bool_field(admission, "domain_match").unwrap_or(false);
        let started = bool_field(admission, "started");
        let terminal = text_field(admission, "terminal").unwrap_or("");
        if class == "inherited" {
            continue;
        }
        if class == "measured_zero" && domain_match {
            saw_measured_zero = true;
            continue;
        }
        saw_queue = true;
        if class == "failed" {
            failed.push(id.to_owned());
            if started == Some(false) && matches!(terminal, "timeout" | "cancelled" | "failure") {
                failed_before_start = true;
            }
            continue;
        }
        let Some(raw_interval) = interval_of(admission) else {
            gaps.push(format!("admission_{id}_boundary_incomplete"));
            unresolved.push(window);
            continue;
        };
        let recorded = clip(&[raw_interval], window);
        if recorded.is_empty() {
            gaps.push(format!("admission_{id}_clipped_away"));
            if !domain_match {
                unresolved.push(window);
            }
            continue;
        }
        if class == "self_contention" && domain_match {
            queue_exposure.extend(recorded);
            continue;
        }
        if !domain_match {
            unresolved.push(window);
            gaps.push(format!("admission_{id}_wrong_domain"));
            continue;
        }
        if class != "unrelated_wait" {
            unresolved.extend(recorded);
            gaps.push(format!("admission_{id}_not_external"));
            continue;
        }
        let (Some(end_late), Some(tick)) = (
            u64_field(admission, "endpoint_end_ns"),
            u64_field(admission, "tick_ns"),
        ) else {
            unresolved.extend(recorded.clone());
            gaps.push(format!("admission_{id}_endpoint_unknown"));
            continue;
        };
        if u64_field(admission, "endpoint_start_ns").is_none() {
            unresolved.extend(recorded.clone());
            gaps.push(format!("admission_{id}_endpoint_unknown"));
            continue;
        }
        queue_exposure.extend(recorded.clone());
        let mut usable = Vec::new();
        for interval in &recorded {
            let shrink = end_late.saturating_add(tick);
            let certain_start = interval.start.saturating_add(tick);
            let certain_end = interval.end.saturating_sub(shrink);
            if certain_end <= certain_start
                || interval.end.saturating_sub(interval.start) <= shrink.saturating_add(tick)
            {
                unresolved.push(*interval);
                gaps.push(format!("admission_{id}_bracket_consumes_interval"));
                continue;
            }
            usable.push(Interval {
                start: certain_start,
                end: certain_end,
            });
            if certain_start > interval.start {
                bracket_possible.push(Interval {
                    start: interval.start,
                    end: certain_start,
                });
            }
            if interval.end > certain_end {
                bracket_possible.push(Interval {
                    start: certain_end,
                    end: interval.end,
                });
            }
        }
        external.extend(usable);
    }

    let external = normalize(external);
    let mut blocked = Vec::new();
    let mut useful = Vec::new();
    let mut receipt_possible = Vec::new();
    let mut unplaced_useful = false;
    let mut sourced_commands: Vec<(Option<String>, Option<String>)> = Vec::new();
    for item in &activity {
        let kind = text_field(item, "kind").unwrap_or("other");
        let tool = text_field(item, "tool_call_id");
        let command = text_field(item, "command_id");
        let placement = text_field(item, "placement").unwrap_or("unplaced");
        let matches_admission = admissions.iter().any(|admission| {
            text_field(admission, "class") == Some("unrelated_wait")
                && tool.is_some()
                && tool == text_field(admission, "tool_call_id")
                && command.is_some()
                && command == text_field(admission, "command_id")
        });
        if matches_admission {
            polling_operations = polling_operations.saturating_add(1);
        }
        let placed = interval_of(item).map(|interval| clip(&[interval], window));
        let Some(placed) = placed else {
            if kind != "command" || !matches_admission {
                unplaced_useful = true;
                gaps.push("unplaced_activity".to_owned());
            } else {
                gaps.push("blocked_interval_unobserved".to_owned());
            }
            continue;
        };
        // Receipt time is an upper bound on notification delivery, not the
        // work instant. It cannot prove blocked coverage and cannot exclude
        // earlier useful overlap. A source or exact placement is a supported
        // bound; anything else stays unplaced.
        if placement == "receipt" {
            if matches_admission {
                gaps.push("blocked_receipt_not_coverage_bound".to_owned());
            } else {
                for interval in placed {
                    receipt_possible.push(Interval {
                        start: window.start,
                        end: interval.end,
                    });
                }
                gaps.push("receipt_bound_not_work_instant".to_owned());
            }
            continue;
        }
        if placement != "exact" && placement != "source" {
            unplaced_useful = true;
            gaps.push("unplaced_activity".to_owned());
            continue;
        }
        if matches_admission {
            let tick = admissions
                .iter()
                .find(|admission| {
                    tool == text_field(admission, "tool_call_id")
                        && command == text_field(admission, "command_id")
                })
                .and_then(|admission| u64_field(admission, "tick_ns"))
                .unwrap_or(1);
            if placement == "exact" || placement == "source" {
                sourced_commands.push((tool.map(str::to_owned), command.map(str::to_owned)));
            }
            for interval in placed {
                if interval.end.saturating_sub(interval.start) <= tick.saturating_mul(2) {
                    continue;
                }
                blocked.push(Interval {
                    start: interval.start.saturating_add(tick),
                    end: interval.end.saturating_sub(tick),
                });
            }
        } else {
            useful.extend(placed);
        }
    }
    if activity_overflow {
        unplaced_useful = true;
    }
    for request in &requests {
        let correlated = correlated_wait(request, &admissions);
        if correlated && interval_of(request).is_some() {
            continue;
        }
        if correlated {
            gaps.push("wait_request_interval_unobserved".to_owned());
            continue;
        }
        if interval_of(request).is_none() {
            // A structural single-tool launch has no request interval of its
            // own. That does not make the named command interval model work,
            // and it does not authorize excluding the request's tokens.
            if names_sourced_command(request, &sourced_commands) {
                gaps.push("request_interval_not_observed".to_owned());
            } else {
                unplaced_useful = true;
                gaps.push("unplaced_model_request".to_owned());
            }
        } else if let Some(interval) = interval_of(request) {
            useful.extend(clip(&[interval], window));
        }
    }

    let blocked = normalize(blocked);
    let useful = normalize(useful);
    let covered = intersect(&external, &blocked);
    let uncovered = subtract(&external, &blocked);
    if !uncovered.is_empty() {
        gaps.push("queue_not_independently_blocked".to_owned());
    }
    unresolved.extend(uncovered);
    let mut deductible_region = if unplaced_useful {
        unresolved.extend(covered.clone());
        gaps.push("possible_useful_overlap_unplaced".to_owned());
        Vec::new()
    } else {
        subtract(&covered, &useful)
    };
    let receipt_possible = normalize(receipt_possible);
    if !receipt_possible.is_empty() {
        let hidden = intersect(&deductible_region, &receipt_possible);
        if !hidden.is_empty() {
            unresolved.extend(hidden);
        }
        deductible_region = subtract(&deductible_region, &receipt_possible);
    }
    // A measured endpoint bracket is not a discarded point. The shrunk
    // interval is the certain deduction; the bracket remains a possible
    // deduction where the task was independently blocked, counted once.
    let bracket_hold = subtract(
        &subtract(&intersect(&normalize(bracket_possible), &blocked), &useful),
        &receipt_possible,
    );
    let bracket_hold = subtract(&bracket_hold, &deductible_region);
    if !bracket_hold.is_empty() {
        unresolved.extend(bracket_hold);
        gaps.push("endpoint_bracket_unresolved".to_owned());
    }
    let deductible = measure(&deductible_region);
    let unresolved_ns = measure(&normalize(unresolved));
    let observed = observed_ns.unwrap_or(0);
    let deductible = deductible.min(observed);
    let unresolved_ns = unresolved_ns.min(observed.saturating_sub(deductible));
    let adjusted = observed.saturating_sub(deductible);
    let adjusted_low = adjusted.saturating_sub(unresolved_ns);
    let adjusted_high = adjusted;
    if observed_ns.is_none() {
        gaps.push("observed_elapsed_unknown".to_owned());
    }
    // Absent telemetry is not a measured zero. The unknown queue can be any
    // portion of the attempt, so the adjusted range stays open.
    if admissions.is_empty()
        && let Some(observed) = observed_ns
    {
        return absent_range(observed, gaps, detail_overflow, activity_overflow);
    }
    let proven_zero = saw_measured_zero && !saw_queue && unresolved_ns == 0 && gaps.is_empty();

    let (raw_usage, excluded_usage, adjusted_usage, excluded_ids, usage_incomplete) =
        usage_adjustment(&requests, &admissions, &deductible_region, &blocked);

    gaps.sort();
    gaps.dedup();
    let coverage = if gaps.is_empty() && observed_ns.is_some() {
        "measured"
    } else if deductible > 0 || proven_zero {
        "partial"
    } else {
        "unresolved"
    };
    json!({
        "lineage": MEASUREMENT_LINEAGE,
        "rule_version": RULE_VERSION,
        "eligible_cause": ELIGIBLE_CAUSE,
        "observed_ns": observed_ns,
        "observed_seconds": observed_ns.map(seconds),
        "deductible_ns": deductible,
        "deductible_seconds": seconds(deductible),
        "adjusted_ns": observed_ns.map(|_| adjusted),
        "adjusted_seconds": observed_ns.map(|_| seconds(adjusted)),
        "adjusted_low_ns": observed_ns.map(|_| adjusted_low),
        "adjusted_high_ns": observed_ns.map(|_| adjusted_high),
        "adjusted_low_seconds": observed_ns.map(|_| seconds(adjusted_low)),
        "adjusted_high_seconds": observed_ns.map(|_| seconds(adjusted_high)),
        "unresolved_ns": unresolved_ns,
        "unresolved_seconds": seconds(unresolved_ns),
        "queue_exposure_ns": measure(&normalize(queue_exposure)),
        "passive_wait_ns": deductible,
        "polling_operations": polling_operations,
        "coverage": coverage,
        "gaps": gaps,
        "failed_admissions": failed,
        "failed_before_start": failed_before_start,
        "proven_zero_queue": proven_zero && gaps.is_empty(),
        "absent_telemetry": admissions.is_empty(),
        "detail_overflow": detail_overflow,
        "activity_overflow": activity_overflow,
        "excluded_requests": excluded_ids,
        "usage": {
            "raw": raw_usage.to_json(),
            "excluded": excluded_usage.to_json(),
            "adjusted": adjusted_usage.to_json(),
            "incomplete": usage_incomplete,
        },
        "limitation": "adjusted elapsed time is a normalization of observed work, not a predicted completion time on an unloaded host",
    })
}

fn incomplete(
    observed_ns: Option<u64>,
    gaps: Vec<String>,
    detail_overflow: bool,
    activity_overflow: bool,
) -> Value {
    json!({
        "lineage": MEASUREMENT_LINEAGE,
        "rule_version": RULE_VERSION,
        "eligible_cause": ELIGIBLE_CAUSE,
        "observed_ns": observed_ns,
        "observed_seconds": observed_ns.map(seconds),
        "deductible_ns": 0,
        "deductible_seconds": 0.0,
        "adjusted_ns": Value::Null,
        "adjusted_seconds": Value::Null,
        "adjusted_low_ns": Value::Null,
        "adjusted_high_ns": Value::Null,
        "adjusted_low_seconds": Value::Null,
        "adjusted_high_seconds": Value::Null,
        "unresolved_ns": observed_ns,
        "unresolved_seconds": observed_ns.map(seconds),
        "queue_exposure_ns": 0,
        "passive_wait_ns": 0,
        "polling_operations": 0,
        "coverage": "unresolved",
        "gaps": gaps,
        "failed_admissions": [],
        "failed_before_start": false,
        "proven_zero_queue": false,
        "absent_telemetry": true,
        "detail_overflow": detail_overflow,
        "activity_overflow": activity_overflow,
        "excluded_requests": [],
        "usage": {
            "raw": Tokens { input: None, cached: None, output: None, reasoning: None, total: None }.to_json(),
            "excluded": Tokens { input: None, cached: None, output: None, reasoning: None, total: None }.to_json(),
            "adjusted": Tokens { input: None, cached: None, output: None, reasoning: None, total: None }.to_json(),
            "incomplete": true,
        },
        "limitation": "adjusted elapsed time is a normalization of observed work, not a predicted completion time on an unloaded host",
    })
}

fn contained(interval: Interval, region: &[Interval]) -> bool {
    subtract(&[interval], region).is_empty()
}

fn subset_holds(excluded: &Tokens, raw: &Tokens) -> bool {
    let within = |part: Option<u64>, whole: Option<u64>| match (part, whole) {
        (Some(part), Some(whole)) => part <= whole,
        (None, _) => true,
        (Some(_), None) => false,
    };
    within(excluded.input, raw.input)
        && within(excluded.cached, raw.cached)
        && within(excluded.output, raw.output)
        && within(excluded.reasoning, raw.reasoning)
        && within(excluded.total, raw.total)
}

/// The one unrelated admission this request actually names. Any nonempty id
/// is not correlation, and two admissions with the same id are ambiguous.
fn correlated_admission<'a>(request: &Value, admissions: &'a [Value]) -> Option<&'a Value> {
    let tool = text_field(request, "tool_call_id");
    let command = text_field(request, "command_id");
    if tool.is_none() && command.is_none() {
        return None;
    }
    let mut matched = admissions.iter().filter(|admission| {
        text_field(admission, "class") == Some("unrelated_wait")
            && bool_field(admission, "domain_match") == Some(true)
            && match (tool, command) {
                (Some(tool), Some(command)) => {
                    text_field(admission, "tool_call_id") == Some(tool)
                        && text_field(admission, "command_id") == Some(command)
                }
                (Some(tool), None) => text_field(admission, "tool_call_id") == Some(tool),
                (None, Some(command)) => text_field(admission, "command_id") == Some(command),
                (None, None) => false,
            }
    });
    let first = matched.next()?;
    matched.next().is_none().then_some(first)
}

fn correlated_wait(request: &Value, admissions: &[Value]) -> bool {
    bool_field(request, "wait_only") == Some(true)
        && correlated_admission(request, admissions).is_some()
}

/// A structural single-tool request that names one source or exact command is
/// the launch of that command, not work inside it. Missing request bounds do
/// not become the admission interval and do not exclude tokens.
fn names_sourced_command(request: &Value, sourced: &[(Option<String>, Option<String>)]) -> bool {
    if bool_field(request, "structural_single_tool") != Some(true)
        || bool_field(request, "wait_only") == Some(true)
    {
        return false;
    }
    let tool = text_field(request, "tool_call_id");
    let command = text_field(request, "command_id");
    if tool.is_none() && command.is_none() {
        return false;
    }
    let mut matched =
        sourced
            .iter()
            .filter(|(stored_tool, stored_command)| match (tool, command) {
                (Some(tool), Some(command)) => {
                    stored_tool.as_deref() == Some(tool)
                        && stored_command.as_deref() == Some(command)
                }
                (Some(tool), None) => stored_tool.as_deref() == Some(tool),
                (None, Some(command)) => stored_command.as_deref() == Some(command),
                (None, None) => false,
            });
    matched.next().is_some() && matched.next().is_none()
}

fn usage_adjustment(
    requests: &[Value],
    admissions: &[Value],
    deductible: &[Interval],
    blocked: &[Interval],
) -> (Tokens, Tokens, Tokens, Vec<String>, bool) {
    let mut raw = Tokens {
        input: Some(0),
        cached: Some(0),
        output: Some(0),
        reasoning: Some(0),
        total: Some(0),
    };
    let mut excluded = Tokens {
        input: Some(0),
        cached: Some(0),
        output: Some(0),
        reasoning: Some(0),
        total: Some(0),
    };
    let mut excluded_ids = Vec::new();
    let mut incomplete = false;
    if requests.is_empty() {
        return (
            Tokens {
                input: None,
                cached: None,
                output: None,
                reasoning: None,
                total: None,
            },
            Tokens {
                input: None,
                cached: None,
                output: None,
                reasoning: None,
                total: None,
            },
            Tokens {
                input: None,
                cached: None,
                output: None,
                reasoning: None,
                total: None,
            },
            excluded_ids,
            false,
        );
    }
    let mut seen: BTreeMap<&str, usize> = BTreeMap::new();
    for request in requests {
        if let Some(id) = text_field(request, "id") {
            *seen.entry(id).or_default() += 1;
        }
    }
    let mut counted: BTreeSet<&str> = BTreeSet::new();
    for request in requests {
        let tokens = Tokens::from_value(request);
        let id = text_field(request, "id");
        let duplicate = id.is_some_and(|id| seen.get(id).copied().unwrap_or(0) > 1);
        if duplicate {
            incomplete = true;
        }
        if let Some(id) = id
            && !counted.insert(id)
        {
            // A repeated request id is one request, not two savings.
            continue;
        }
        if !tokens.consistent()
            || tokens.input.is_none()
            || tokens.output.is_none()
            || tokens.total.is_none()
        {
            incomplete = true;
            add_option(&mut raw.input, tokens.input);
            add_option(&mut raw.cached, tokens.cached);
            add_option(&mut raw.output, tokens.output);
            add_option(&mut raw.reasoning, tokens.reasoning);
            add_option(&mut raw.total, tokens.total);
            continue;
        }
        add_option(&mut raw.input, tokens.input);
        add_option(&mut raw.cached, tokens.cached);
        add_option(&mut raw.output, tokens.output);
        add_option(&mut raw.reasoning, tokens.reasoning);
        add_option(&mut raw.total, tokens.total);
        if duplicate || !correlated_wait(request, admissions) {
            if bool_field(request, "wait_only") != Some(false)
                && bool_field(request, "structural_single_tool") != Some(false)
            {
                incomplete = true;
            }
            continue;
        }
        let placed = interval_of(request);
        let contained_blocked = placed.is_some_and(|interval| contained(interval, blocked));
        let contained_deductible = placed.is_some_and(|interval| contained(interval, deductible));
        // Exclude only a whole observed request whose own interval sits inside
        // the deductible region. An admission interval is not a request interval.
        if contained_deductible {
            add_option(&mut excluded.input, tokens.input);
            add_option(&mut excluded.cached, tokens.cached);
            add_option(&mut excluded.output, tokens.output);
            add_option(&mut excluded.reasoning, tokens.reasoning);
            add_option(&mut excluded.total, tokens.total);
            if let Some(id) = id {
                excluded_ids.push(id.to_owned());
            }
        } else if contained_blocked
            || placed.is_none()
            || bool_field(request, "wait_only") != Some(false)
        {
            incomplete = true;
        }
    }
    if !subset_holds(&excluded, &raw) {
        incomplete = true;
        excluded = Tokens {
            input: Some(0),
            cached: Some(0),
            output: Some(0),
            reasoning: Some(0),
            total: Some(0),
        };
        excluded_ids.clear();
    }
    let adjusted = Tokens {
        input: sub_option(raw.input, excluded.input),
        cached: sub_option(raw.cached, excluded.cached),
        output: sub_option(raw.output, excluded.output),
        reasoning: sub_option(raw.reasoning, excluded.reasoning),
        total: sub_option(raw.total, excluded.total),
    };
    (raw, excluded, adjusted, excluded_ids, incomplete)
}

fn absent_range(
    observed: u64,
    mut gaps: Vec<String>,
    detail_overflow: bool,
    activity_overflow: bool,
) -> Value {
    gaps.push("admission_telemetry_absent".to_owned());
    gaps.sort();
    gaps.dedup();
    json!({
        "lineage": MEASUREMENT_LINEAGE,
        "rule_version": RULE_VERSION,
        "eligible_cause": ELIGIBLE_CAUSE,
        "observed_ns": observed,
        "observed_seconds": seconds(observed),
        "deductible_ns": 0,
        "deductible_seconds": 0.0,
        "adjusted_ns": observed,
        "adjusted_seconds": seconds(observed),
        "adjusted_low_ns": 0,
        "adjusted_high_ns": observed,
        "adjusted_low_seconds": 0.0,
        "adjusted_high_seconds": seconds(observed),
        "unresolved_ns": observed,
        "unresolved_seconds": seconds(observed),
        "queue_exposure_ns": 0,
        "passive_wait_ns": 0,
        "polling_operations": 0,
        "coverage": "unresolved",
        "gaps": gaps,
        "failed_admissions": [],
        "failed_before_start": false,
        "proven_zero_queue": false,
        "absent_telemetry": true,
        "detail_overflow": detail_overflow,
        "activity_overflow": activity_overflow,
        "excluded_requests": [],
        "usage": {
            "raw": Tokens { input: None, cached: None, output: None, reasoning: None, total: None }.to_json(),
            "excluded": Tokens { input: None, cached: None, output: None, reasoning: None, total: None }.to_json(),
            "adjusted": Tokens { input: None, cached: None, output: None, reasoning: None, total: None }.to_json(),
            "incomplete": true,
        },
        "limitation": "adjusted elapsed time is a normalization of observed work, not a predicted completion time on an unloaded host",
    })
}

/// Attach the derived adjustment beside the raw row. Raw elapsed is unchanged.
pub fn attach(row: &mut Value) {
    let Some(capture) = row.get("infrastructure_capture").cloned() else {
        return;
    };
    if !capture.is_object() {
        row["infrastructure"] = json!({
            "coverage": "unresolved",
            "gaps": ["capture_malformed"],
            "proven_zero_queue": false,
            "absent_telemetry": true,
            "limitation": "adjusted elapsed time is a normalization of observed work, not a predicted completion time on an unloaded host",
        });
        return;
    }
    let observed_ns = row
        .get("elapsed_seconds")
        .and_then(Value::as_f64)
        .filter(|seconds| seconds.is_finite() && *seconds >= 0.0)
        .and_then(|seconds| {
            let ns = seconds * 1_000_000_000.0;
            (ns <= u64::MAX as f64).then_some(ns.round() as u64)
        });
    row["infrastructure"] = adjust(&capture, observed_ns);
}

/// Bounds the policy uses for one arm. Missing evidence is not a zero, and a
/// collapsed point with an open boundary is not a supported estimate.
pub fn arm_bounds(row: &Value) -> Option<(f64, f64)> {
    let infra = row.get("infrastructure")?;
    let low = infra.get("adjusted_low_seconds").and_then(Value::as_f64)?;
    let high = infra.get("adjusted_high_seconds").and_then(Value::as_f64)?;
    if !(low.is_finite() && high.is_finite() && high >= low) {
        return None;
    }
    let coverage = infra
        .get("coverage")
        .and_then(Value::as_str)
        .unwrap_or("unresolved");
    let unresolved = infra
        .get("unresolved_seconds")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    let point = (high - low) <= f64::EPSILON;
    if point && coverage != "measured" && unresolved <= f64::EPSILON {
        return None;
    }
    Some((low, high))
}

pub fn failed_before_start(row: &Value) -> bool {
    row.get("infrastructure")
        .and_then(|value| value.get("failed_before_start"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

/// Insert the unit-level adjusted range. Raw `effect` stays the operational view.
pub fn unit_effect(baseline: Option<&Value>, candidate: Option<&Value>) -> Value {
    let bounds = |row: Option<&Value>| row.and_then(arm_bounds);
    let (baseline_low, baseline_high) = bounds(baseline).unzip();
    let (candidate_low, candidate_high) = bounds(candidate).unzip();
    json!({
        "baseline_low_seconds": baseline_low,
        "baseline_high_seconds": baseline_high,
        "candidate_low_seconds": candidate_low,
        "candidate_high_seconds": candidate_high,
        "lineage": MEASUREMENT_LINEAGE,
        "contract": lineage_contract(),
    })
}

pub fn reduction_range(
    baseline_low: f64,
    baseline_high: f64,
    candidate_low: f64,
    candidate_high: f64,
) -> Option<(f64, f64)> {
    if baseline_low <= 0.0 || baseline_high <= 0.0 {
        return None;
    }
    let min = (baseline_low - candidate_high) / baseline_low * 100.0;
    let max = (baseline_high - candidate_low) / baseline_high * 100.0;
    (min.is_finite() && max.is_finite()).then_some((min.min(max), min.max(max)))
}
