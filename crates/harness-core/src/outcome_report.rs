//! Deterministic accounting for private native outcome attempts. No execution,
//! billing conversion, or claim of demonstrated benefit is made by this module.
//!
//! The report separates four concepts that a benefit decision must not
//! conflate: input validity (a row is only used after [`finish_attempt`]
//! validation), evidence completeness (executed acceptance, verified model
//! metadata and declared evaluation semantics), comparability (identical
//! material identities on both arms) and a computed effect (a recorded
//! difference, never an adoption). Attempts may declare an experiment and a
//! pair/block identity; comparisons from one declared unit are one sample, not
//! one sample per baseline-candidate edge. Retries stay inside their task
//! chain, worker usage is attributed once, and time is an enclosing span, so
//! overlapping or inherited summaries are not double counted. Usage remains
//! per run: bytes and token totals never become provider cost, billing or
//! subscription allowance.
//!
//! A declared `experiment_id`/`pair_id` scopes comparability: an edge that
//! crosses two declared units is listed but never counted as evidence for
//! either, so repeated complete pairs of one task stay one experimental unit
//! each instead of collapsing into a cross product. Complete paired attempts
//! are the unit for run-to-run variation; requests, rounds, tool calls, tool
//! operations and repeated readings within one task are dependent observations.
//!
//! A subtractive candidate additionally records its removed burden and each
//! arm's actual consumption of that burden ([`finish_attempt`] normalizes both
//! from the attempt input). The unit analysis then distinguishes an exercised
//! removal (the baseline consumed the burden, the candidate is observed not
//! to) from an unexercised, unknown or not-applied one, and refuses to turn
//! zero invocations, a missing consumption record or a deleted required check
//! into an accounted saving.
//!
//! A declared adoption scope may require additional independent corroboration
//! units from retained prior real tasks. [`corroboration_section`] consumes the
//! driver's run-local receipt into the report's `corroboration` section:
//! selection status, declared additional units, selected identity/replay
//! references and exact exclusions, bound by [`corroboration_digest`]. The
//! section is evidence about the selection only; it never fabricates a unit,
//! replaces a missing selection with a summary or claims measured benefit for a
//! selected task.
//!
//! A retained baseline result may declare a `reuse` block when it is earlier
//! evidence reused for a new comparison instead of executing the old variant
//! again. The record is normalized and verified (retained execution identity,
//! age, trace, coverage, uncertainty, predeclared selection, cache/load
//! conditions and model-metric applicability); a record that fails
//! verification becomes an exact `reuse-refused` reason that keeps the row out
//! of every comparable edge, so an unverifiable reuse never silently becomes a
//! fresh run or a usable baseline. A verified reuse is marked on its unit
//! (`baseline_reused`, `baseline_executed_now: false`) with the retained age,
//! coverage, uncertainty and original cost, and the accounting counts it
//! separately from fresh executions.
use crate::improvement_experiment::{
    CorroborationSelection, CorroborationStatus, EXPERIMENT_SCHEMA, ExclusionReason,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    io,
};

pub const MATCH_FIELDS: &[&str] = &[
    "case_revision",
    "source_state",
    "input_identity",
    "runtime",
    "model",
    "effort",
    "provider",
    "config_identity",
    "tool_identity",
    "hook_revision",
    "allowed_effects",
    "cache_policy",
    "preparation_policy",
    "budget",
    "oracle_identity",
    "instructions_identity",
    "other_skills",
    "stop_conditions",
    "criterion",
];

fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "outcome accounting input is invalid",
    )
}

fn array<'a>(row: &'a Value, field: &str) -> io::Result<&'a [Value]> {
    match row.get(field) {
        None => Ok(&[]),
        Some(Value::Array(values)) => Ok(values),
        _ => Err(invalid()),
    }
}

fn truth(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(v) => *v,
        Value::Number(v) => v.as_f64().is_some_and(|v| v != 0.0),
        Value::String(v) => !v.is_empty(),
        Value::Array(v) => !v.is_empty(),
        Value::Object(v) => !v.is_empty(),
    }
}

fn number(value: &Value) -> Option<f64> {
    value.as_f64().filter(|v| v.is_finite())
}

/// A non-empty trimmed string field, or `None` when absent or blank.
fn text<'a>(row: &'a Value, field: &str) -> Option<&'a str> {
    row.get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

/// Identity values must be present and non-placeholder to compare two
/// attempts. An absent, null, blank, `unknown` or empty container value is
/// stale identity evidence: it cannot authorize a comparable pair even when
/// the other arm records the same placeholder.
fn identity_known(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::String(value) => {
            let value = value.trim();
            !value.is_empty() && !value.eq_ignore_ascii_case("unknown")
        }
        Value::Array(values) => !values.is_empty(),
        Value::Object(values) => !values.is_empty(),
        _ => true,
    }
}

/// The recorded value that marks model metrics as inapplicable. A method
/// without model execution records no measured zero here, and this value
/// never satisfies or violates a model-based threshold.
pub const MODEL_METRICS_INAPPLICABLE: &str = "inapplicable";

/// Bound on one recorded reuse fact.
const MAX_REUSE_FIELD_BYTES: usize = 1024;

/// The declared fields of one retained-baseline reuse record. Anything else
/// is refused rather than normalized, so an undeclared claim cannot enter the
/// evidence.
const REUSE_FIELDS: &[&str] = &[
    "of",
    "executed_at",
    "age_seconds",
    "trace",
    "coverage",
    "uncertainty",
    "selection",
    "conditions",
    "qualification",
];

/// The comparison identities that exist only for a method that executes a
/// model. A model-free method records its model metrics as inapplicable;
/// these values are neither required for its pair comparability nor able to
/// establish a mismatch, because no model call is part of the measured work.
const MODEL_EXECUTION_MATCH_FIELDS: &[&str] = &["model", "effort", "provider"];

/// Whether an attempt declares a method without model execution. Its model
/// metrics are recorded as inapplicable, never as a measured zero, and no
/// model work (a call, request, round, tool operation or token usage) may be
/// measured on the attempt or on any of its native runs. A record that claims
/// inapplicable model metrics while also recording measured model work is not
/// model-free, and every model-method gate keeps applying to it.
pub fn model_free_attempt(row: &Value) -> bool {
    if row.get("model_metrics").and_then(Value::as_str) != Some(MODEL_METRICS_INAPPLICABLE) {
        return false;
    }
    if row
        .get("model_calls")
        .and_then(Value::as_u64)
        .is_some_and(|calls| calls != 0)
    {
        return false;
    }
    !model_work_measured(row)
}

fn model_work_measured(row: &Value) -> bool {
    let counters = ["requests", "rounds", "tool_calls", "tool_operations"];
    if counters
        .iter()
        .any(|key| row.get(*key).and_then(Value::as_u64).is_some())
    {
        return true;
    }
    if let Some(usage) = row.get("usage").and_then(Value::as_object) {
        let measured = usage.get("status").and_then(Value::as_str) == Some("per_run")
            || usage.get("runs").is_some_and(truth)
            || usage.get("workers").is_some_and(truth)
            || usage.get("total_tokens").and_then(Value::as_u64).is_some();
        if measured {
            return true;
        }
    }
    array(row, "native_runs")
        .map(|runs| {
            runs.iter().any(|run| {
                counters
                    .iter()
                    .any(|key| run.get(*key).and_then(Value::as_u64).is_some())
                    || run.get("usage").is_some_and(truth)
                    || run
                        .get("model_calls")
                        .and_then(Value::as_u64)
                        .is_some_and(|calls| calls != 0)
            })
        })
        .unwrap_or(false)
}

/// The operation's own measured work for a method without model execution: a
/// completed native execution whose exit status and duration were recorded
/// with a retained evidence reference, over a declared input identity. This
/// is what a model-free unit establishes completeness and comparable-pair
/// coverage with, instead of a model identity and model-round counters.
fn operation_work_recorded(row: &Value) -> bool {
    number(&row["elapsed_seconds"]).is_some()
        && identity_known(&row["matched"]["input_identity"])
        && array(row, "native_runs")
            .map(|runs| {
                runs.iter().any(|run| {
                    run["status"] == "completed"
                        && (run["exit_code"].is_i64() || run["exit_code"].is_u64())
                        && number(&run["elapsed_seconds"]).is_some()
                        && text(run, "evidence").is_some()
                })
            })
            .unwrap_or(false)
}

/// A child entry that references another attempt in the same report declares
/// inherited work: its time and usage are accounted once by the referenced
/// attempt instead of being counted again here.
fn inherited_child(child: &Value) -> Option<&str> {
    ["attempt_id", "inherited_from"]
        .iter()
        .find_map(|key| child.get(*key).and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

/// The arm's declared treatment. An absent `treatment` block or an absent
/// `kind` is the additive default; `subtraction` and `simplification` declare
/// a subtractive candidate. Any other recorded kind is refused rather than
/// guessed.
fn treatment_facts(record: &Value) -> io::Result<Value> {
    let Some(value) = record.get("treatment") else {
        return Ok(json!({"kind": "additive", "removed": null}));
    };
    if value.is_null() {
        return Ok(json!({"kind": "additive", "removed": null}));
    }
    let object = value.as_object().ok_or_else(invalid)?;
    let kind = match object.get("kind").and_then(Value::as_str) {
        None | Some("addition" | "additive") => "additive",
        Some("subtraction" | "simplification") => "subtractive",
        Some(_) => return Err(invalid()),
    };
    let removed = object
        .get("removed")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    Ok(json!({"kind": kind, "removed": removed}))
}

/// One arm's recorded consumption of a subtractive treatment's removed
/// burden. Invocation counts are a separate measure: a skill or tool can
/// incur catalogue, instruction or initialization cost without being invoked,
/// so an absent record or zero invocations never establishes that nothing was
/// consumed. `consumed` and `absent` are observations only with retained
/// evidence; an unrecognized status is refused.
fn consumption_facts(record: &Value) -> io::Result<Value> {
    let Some(value) = record.get("consumption") else {
        return Ok(Value::Null);
    };
    if value.is_null() {
        return Ok(Value::Null);
    }
    let object = value.as_object().ok_or_else(invalid)?;
    let text = |key: &str| {
        object
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
    };
    let capability = text("capability").ok_or_else(invalid)?;
    let status = match text("status") {
        Some("consumed") => "consumed",
        Some("absent") => "absent",
        Some("unknown") => "unknown",
        _ => return Err(invalid()),
    };
    Ok(json!({
        "capability": capability,
        "status": status,
        "evidenced": text("evidence").is_some(),
    }))
}

/// The required checks of an attempt's final round that executed with
/// evidence. A candidate recording fewer of them than the baseline removed
/// acceptance coverage; its own verdict cannot establish retained behavior.
fn executed_check_ids(row: &Value) -> io::Result<BTreeSet<String>> {
    let checks = array(row, "checks")?;
    let current = checks
        .iter()
        .map(round)
        .collect::<io::Result<Vec<_>>>()?
        .into_iter()
        .max()
        .unwrap_or(0);
    Ok(checks
        .iter()
        .filter(|check| {
            round(check).ok() == Some(current)
                && required(check)
                && truth(&check["executed"])
                && truth(&check["evidence"])
        })
        .filter_map(|check| check["id"].as_str().map(str::to_owned))
        .collect())
}

/// The declared experimental unit of an attempt: the pair/block identity when
/// declared, otherwise the case as the conservative enclosing block. All
/// baseline-candidate edges from one unit are one sample, not independent
/// repetitions.
fn unit_of(row: &Value) -> String {
    let experiment = text(row, "experiment_id").unwrap_or("");
    let case = row["case_id"].as_str().unwrap_or("");
    match text(row, "pair_id") {
        Some(pair) => format!("{experiment}/{case}/{pair}"),
        None => format!("{experiment}/{case}"),
    }
}

/// Enclosing elapsed span, including gaps; neither sum nor union of durations.
pub fn wall_span(spans: &[Value]) -> Option<f64> {
    let mut start = f64::INFINITY;
    let mut end = f64::NEG_INFINITY;
    for span in spans {
        let a = number(&span["started_at"])?;
        let b = number(&span["ended_at"])?;
        if b < a {
            return None;
        }
        start = start.min(a);
        end = end.max(b);
    }
    let elapsed = end - start;
    elapsed.is_finite().then_some(elapsed)
}

/// One retained observed counter over an attempt's native runs: `null` when no
/// run recorded it, otherwise the sum. A missing counter is never assumed.
fn counter_total(runs: &[Value], key: &str) -> Value {
    if runs.is_empty() {
        return Value::Null;
    }
    let mut total = 0_u64;
    for run in runs {
        match run
            .get(key)
            .and_then(Value::as_u64)
            .and_then(|value| total.checked_add(value))
        {
            Some(value) => total = value,
            None => return Value::Null,
        }
    }
    json!(total)
}

fn strings(values: &[Value]) -> io::Result<BTreeSet<String>> {
    values
        .iter()
        .map(|v| v.as_str().map(str::to_owned).ok_or_else(invalid))
        .collect()
}

fn round(check: &Value) -> io::Result<i64> {
    check
        .get("round")
        .map_or(Ok(0), |v| v.as_i64().ok_or_else(invalid))
}

fn required(check: &Value) -> bool {
    check.get("required").is_none_or(truth)
}

fn execution_start(row: &Value) -> &Value {
    row.get("execution_started_at")
        .unwrap_or(&row["started_at"])
}

/// Normalize the optional `reuse` record of a retained baseline attempt.
///
/// A baseline result may be retained evidence from an earlier execution that
/// is reused instead of running the old variant again. Reuse is never
/// inferred: the record must name the retained execution identity, when it
/// was executed, its age, the retained trace reference, the coverage and
/// uncertainty recorded with it, the predeclared selection, the relevant
/// cache/load conditions and — for a method that executes a model — the
/// verified runtime qualification and context isolation. A record that fails
/// any requirement yields exact refusal reasons: the attempt stays retained
/// for review, never enters a comparable edge, and the refusal is visible
/// with the row. A method without model execution keeps its model metrics
/// inapplicable; `qualification` is normalized to `inapplicable` for it, and
/// an unrelated model qualification is refused rather than consumed.
fn reuse_evidence(record: &Value) -> io::Result<(Option<Value>, Vec<String>)> {
    let Some(value) = record.get("reuse") else {
        return Ok((None, Vec::new()));
    };
    if value.is_null() {
        return Ok((None, Vec::new()));
    }
    let Some(object) = value.as_object() else {
        return Ok((
            None,
            vec![
                "the recorded reuse evidence is not an object; the retained baseline cannot be verified"
                    .to_owned(),
            ],
        ));
    };
    if record.get("arm").and_then(Value::as_str) == Some("candidate") {
        return Ok((
            None,
            vec![
                "only a retained baseline may be reused; a reused candidate would replace the measured treatment and cannot enter the comparison"
                    .to_owned(),
            ],
        ));
    }
    let mut refusals: Vec<String> = Vec::new();
    for key in object.keys() {
        if !REUSE_FIELDS.contains(&key.as_str()) {
            refusals.push(format!(
                "the recorded reuse evidence has an unknown field '{key}'; reuse consumes only the declared retention facts"
            ));
        }
    }
    let bounded = |key: &str, bound: usize| -> Option<String> {
        object
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty() && value.len() <= bound)
            .map(str::to_owned)
    };
    let of = bounded("of", MAX_REUSE_FIELD_BYTES);
    if of.is_none() {
        refusals.push(
            "the reused baseline names no retained execution identity; reuse requires the exact retained attempt reference"
                .to_owned(),
        );
    }
    let executed_at = object.get("executed_at").and_then(number);
    if executed_at.is_none() {
        refusals.push(
            "the reused baseline records no original execution time; reuse cannot verify that the retained execution predates this comparison"
                .to_owned(),
        );
    }
    let age_seconds = object
        .get("age_seconds")
        .and_then(number)
        .filter(|v| *v >= 0.0);
    if age_seconds.is_none() {
        refusals.push(
            "the reused baseline records no retention age; age must be exposed rather than silently assumed"
                .to_owned(),
        );
    }
    let trace = bounded("trace", MAX_REUSE_FIELD_BYTES);
    if trace.is_none() {
        refusals.push(
            "the retained baseline trace is missing; the retained execution cannot be verified and is not reused"
                .to_owned(),
        );
    }
    let retained_coverage = bounded("coverage", MAX_REUSE_FIELD_BYTES);
    if retained_coverage.is_none() {
        refusals.push(
            "the retained baseline coverage is not recorded; an unverified coverage cannot be reused"
                .to_owned(),
        );
    }
    let uncertainty = bounded("uncertainty", MAX_REUSE_FIELD_BYTES);
    if uncertainty.is_none() {
        refusals.push(
            "the retained baseline uncertainty is not recorded; unresolved uncertainty must stay visible with the reused evidence"
                .to_owned(),
        );
    }
    let selection = object.get("selection").and_then(Value::as_object);
    if let Some(selection) = selection {
        for key in selection.keys() {
            if !matches!(key.as_str(), "at" | "basis") {
                refusals.push(format!(
                    "the reused baseline selection has an unknown field '{key}'; only the recorded selection time and basis are consumed"
                ));
            }
        }
    }
    let selected_at = selection.and_then(|s| s.get("at")).and_then(number);
    let selection_basis = selection
        .and_then(|s| s.get("basis"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty() && value.len() <= MAX_REUSE_FIELD_BYTES)
        .map(str::to_owned);
    if selected_at.is_none() || selection_basis.is_none() {
        refusals.push(
            "the reused baseline records no predeclared selection; a baseline selected after the candidate result cannot satisfy the frozen comparison policy"
                .to_owned(),
        );
    }
    let conditions = object.get("conditions").and_then(Value::as_object);
    if let Some(conditions) = conditions {
        for key in conditions.keys() {
            if !matches!(key.as_str(), "cache" | "load") {
                refusals.push(format!(
                    "the reused baseline records an unknown condition '{key}'; reuse consumes only the declared cache/load conditions"
                ));
            }
        }
    }
    let condition = |key: &str| -> Option<String> {
        conditions
            .and_then(|c| c.get(key))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty() && value.len() <= 128)
            .map(str::to_owned)
    };
    let cache = condition("cache");
    let load = condition("load");
    if cache.is_none() || load.is_none() {
        refusals.push(
            "the retained baseline's relevant cache/load conditions are not recorded; unchanged conditions cannot be verified"
                .to_owned(),
        );
    }
    let model_free = model_free_attempt(record);
    let raw_qualification = object.get("qualification");
    let qualification = match raw_qualification {
        None | Some(Value::Null) => None,
        Some(value) => value
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty() && value.len() <= 128)
            .map(str::to_owned),
    };
    let qualification = match (model_free, raw_qualification, qualification) {
        (true, None | Some(Value::Null), _) => Some(MODEL_METRICS_INAPPLICABLE.to_owned()),
        (_, _, value) => value,
    };
    match (model_free, qualification.as_deref()) {
        (true, Some(MODEL_METRICS_INAPPLICABLE)) | (false, Some("verified")) => {}
        (true, _) => refusals.push(
            "a method without model execution keeps its model metrics inapplicable; an unrelated model qualification is not consumed and can never establish a model saving"
                .to_owned(),
        ),
        (false, _) => refusals.push(
            "model-dependent reuse requires the retained runtime qualification and context isolation; the reused baseline records none"
                .to_owned(),
        ),
    }
    if !refusals.is_empty() {
        return Ok((None, refusals));
    }
    Ok((
        Some(json!({
            "of": of,
            "executed_at": executed_at,
            "age_seconds": age_seconds,
            "trace": trace,
            "coverage": retained_coverage,
            "uncertainty": uncertainty,
            "selected_at": selected_at,
            "selection_basis": selection_basis,
            "conditions": {"cache": cache, "load": load},
            "qualification": qualification,
            "model_metrics": if model_free { MODEL_METRICS_INAPPLICABLE } else { "measured" },
        })),
        Vec::new(),
    ))
}

/// Retains all input fields, failed checks and native runs. Only executed final
/// round acceptance plus completed native work can establish correctness; a
/// recorded cancellation that acceptance never resolved keeps its status.
pub fn finish_attempt(record: &Value) -> io::Result<Value> {
    if !record.is_object() {
        return Err(invalid());
    }
    let mut row = record.clone();
    let native = array(record, "native_runs")?;
    let checks = array(record, "checks")?;
    let derived = [
        "incomplete_timing",
        "no_opposite_arm",
        "unresolved_retry_chain",
        "retry_identity_mismatch",
    ];
    let mut reasons = strings(array(record, "excluded_reasons")?)?;
    reasons.retain(|r| !derived.contains(&r.as_str()) && !r.starts_with("outcome_"));
    let current = checks
        .iter()
        .map(round)
        .collect::<io::Result<Vec<_>>>()?
        .into_iter()
        .max()
        .unwrap_or(0);
    let final_checks: Vec<_> = checks
        .iter()
        .filter(|c| round(c).ok() == Some(current) && required(c))
        .collect();
    let mut expected = strings(array(record, "required_check_ids")?)?;
    if expected.is_empty() {
        for check in checks.iter().filter(|c| required(c)) {
            expected.insert(check["id"].as_str().ok_or_else(invalid)?.to_owned());
        }
    }
    let actual: BTreeSet<_> = final_checks
        .iter()
        .filter_map(|c| c["id"].as_str())
        .collect();
    let complete = !final_checks.is_empty()
        && expected.iter().all(|id| actual.contains(id.as_str()))
        && final_checks.iter().all(|c| {
            c["executed"] == true
                && (c["exit_code"].is_i64() || c["exit_code"].is_u64())
                && truth(&c["evidence"])
                && number(&c["ended_at"]).is_some()
        });
    let correct = complete
        && final_checks.iter().all(|c| c["passed"] == true)
        && native.last().is_some_and(|n| n["status"] == "completed");
    for run in native {
        reasons.extend(strings(array(run, "evidence_errors")?)?);
    }
    row["correct"] = correct.into();
    let status = if correct {
        "accepted"
    } else if final_checks
        .iter()
        .any(|c| truth(&c["executed"]) && c["passed"] == false)
    {
        "failed"
    } else {
        record["status"]
            .as_str()
            .filter(|status| matches!(*status, "failed" | "blocked" | "timeout" | "cancelled"))
            .unwrap_or("incomplete")
    };
    row["status"] = status.into();
    let start = execution_start(record);
    let mut spans = vec![json!({"started_at": start, "ended_at": record["ended_at"]})];
    spans.extend_from_slice(native);
    spans.extend_from_slice(checks);
    let children: Vec<Value> = array(record, "children")?
        .iter()
        .filter(|child| inherited_child(child).is_none())
        .cloned()
        .collect();
    spans.extend_from_slice(&children);
    spans.extend_from_slice(array(record, "interventions")?);
    let mut elapsed = wall_span(&spans);
    let preparation = &record["preparation"];
    if truth(preparation) && record.get("execution_started_at").is_none() {
        elapsed = (native.is_empty() && checks.is_empty()).then_some(0.0);
    }
    let prep = if truth(preparation) {
        wall_span(std::slice::from_ref(preparation))
    } else {
        Some(0.0)
    };
    row["elapsed_seconds"] = json!(elapsed);
    row["preparation_seconds"] = json!(prep);
    row["observed_wall_seconds"] = json!(wall_span(&[
        json!({"started_at": record["started_at"], "ended_at": record["ended_at"]})
    ]));
    row["verified_seconds"] = json!(
        elapsed
            .zip(prep)
            .map(|(a, b)| a + b)
            .filter(|v| v.is_finite())
    );
    if elapsed.is_none() {
        reasons.insert("incomplete_timing".into());
    }
    let mut signals: Vec<_> = native
        .iter()
        .filter_map(|n| n.get("first_useful_signal").cloned())
        .collect();
    signals.extend(checks.iter().filter(|c| c["executed"] == true && (c["exit_code"].is_i64() || c["exit_code"].is_u64()))
        .map(|c| json!({"at": c["ended_at"], "evidence": c["evidence"], "kind": "acceptance_result"})));
    let signal = signals
        .into_iter()
        .filter(|s| {
            if !s.is_object() || !truth(&s["evidence"]) {
                return false;
            }
            match (
                number(&s["at"]),
                number(&record["started_at"]),
                number(&record["ended_at"]),
            ) {
                (Some(at), Some(a), Some(b)) => at >= a && at <= b,
                _ => false,
            }
        })
        .min_by(|a, b| {
            number(&a["at"])
                .unwrap()
                .total_cmp(&number(&b["at"]).unwrap())
        });
    row["first_useful_seconds"] = json!(
        signal
            .as_ref()
            .and_then(|s| number(&s["at"]))
            .zip(number(start))
            .zip(prep)
            .map(|((at, start), prep)| at - start + prep)
            .filter(|v| v.is_finite())
    );
    row["first_useful_signal"] = signal.unwrap_or(Value::Null);
    if status != "accepted" {
        reasons.insert(format!("outcome_{status}"));
    }
    // Model requests, sequential interaction rounds, outer tool calls and the
    // operations those calls perform stay distinct counters. A batched outer
    // call can perform several operations, so a lower call count alone is
    // never a reduced-work measurement. Each counter is summed only over runs
    // that recorded it; a missing counter stays unknown, never zero.
    row["requests"] = counter_total(native, "requests");
    row["rounds"] = counter_total(native, "rounds");
    row["tool_calls"] = counter_total(native, "tool_calls");
    row["tool_operations"] = counter_total(native, "tool_operations");
    row["unit"] = json!(unit_of(record));
    // A retained baseline may declare that its result is earlier evidence
    // reused for this comparison instead of a fresh execution of the old
    // variant. The normalized reuse record is carried with the row; a record
    // that fails verification becomes a retained refusal that keeps the row
    // out of every comparable edge, so a fabricated or unverifiable reuse is
    // never silently treated as either a fresh run or a usable baseline.
    let (reuse, reuse_refusals) = reuse_evidence(record)?;
    if let Some(evidence) = reuse {
        row["reuse_evidence"] = evidence;
    }
    if !reuse_refusals.is_empty() {
        for refusal in &reuse_refusals {
            reasons.insert(format!("reuse-refused: {refusal}"));
        }
        row["reuse_refused"] = json!(reuse_refusals);
    }
    row["excluded_reasons"] = json!(reasons);
    let treatment = treatment_facts(record)?;
    row["treatment_kind"] = treatment["kind"].clone();
    row["removed_burden"] = treatment["removed"].clone();
    row["consumption_evidence"] = consumption_facts(record)?;
    // The retained adjusted view from an earlier summarization is compared
    // below against a fresh reduction of the retained native trace. Replay is
    // deterministic: the same capture, observed elapsed time and rule version
    // must reproduce the same deductions and decision inputs.
    let retained_infrastructure = row.get("infrastructure").cloned();
    crate::infrastructure_accounting::attach(&mut row);
    if let Some(attribution) = attribution_record(&row, retained_infrastructure.as_ref()) {
        row["attribution"] = attribution;
    }
    Ok(row)
}

/// Re-derive the infrastructure adjustment of one attempt from its retained
/// native trace. The reduction is deterministic and performs no model call:
/// the same retained capture and observed elapsed time reproduce the same
/// adjusted view, exclusions and unresolved classifications. `None` when the
/// row retains no native capture to replay.
pub fn replay_attempt(row: &Value) -> Option<Value> {
    let capture = row.get("infrastructure_capture")?;
    let mut shell = json!({
        "elapsed_seconds": row.get("elapsed_seconds").cloned().unwrap_or(Value::Null),
        "infrastructure_capture": capture.clone(),
    });
    crate::infrastructure_accounting::attach(&mut shell);
    shell.get("infrastructure").cloned()
}

/// The retained attribution record of one attempt: the rule identity that
/// produced the adjusted view, the replay status against the retained native
/// trace, the raw/adjusted/excluded reconciliation totals, and the rule or
/// reason of every exclusion and unresolved classification. `None` when the
/// attempt retains no infrastructure evidence at all.
fn attribution_record(row: &Value, retained: Option<&Value>) -> Option<Value> {
    let infrastructure = row.get("infrastructure")?;
    if !infrastructure.is_object() {
        return None;
    }
    let text = |value: &Value, key: &str| {
        value
            .get(key)
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_default()
    };
    let rule = text(infrastructure, "rule_version");
    let lineage = text(infrastructure, "lineage");
    let cause = text(infrastructure, "eligible_cause");
    let capture = row.get("infrastructure_capture");
    let (replay, replay_detail) = match capture {
        Some(capture) if capture.is_object() => match retained {
            // The first reduction of a retained trace is itself the
            // deterministic replay of that trace.
            None => ("reproduced".to_owned(), Value::Null),
            Some(retained) if retained == infrastructure => ("reproduced".to_owned(), Value::Null),
            Some(retained) => {
                let mut differing = BTreeSet::new();
                if let (Some(retained), Some(current)) =
                    (retained.as_object(), infrastructure.as_object())
                {
                    for key in retained.keys().chain(current.keys()) {
                        if retained.get(key) != current.get(key) {
                            differing.insert(key.clone());
                        }
                    }
                }
                (
                    "mismatch".to_owned(),
                    json!(format!(
                        "the retained adjusted view differs from a fresh reduction of the retained native trace in: {}",
                        differing.into_iter().collect::<Vec<_>>().join(", ")
                    )),
                )
            }
        },
        Some(_) => (
            "unavailable".to_owned(),
            json!("the retained capture is malformed and cannot be reduced"),
        ),
        None => (
            "unavailable".to_owned(),
            json!(
                "no retained native capture; the adjusted view is retained but cannot be replayed"
            ),
        ),
    };
    let duplicates: Vec<String> = duplicate_request_ids(row).into_iter().collect();
    let reconciliation = attribution_reconciliation(infrastructure, &duplicates);
    let exclusions = attribution_exclusions(infrastructure, row, &duplicates);
    Some(json!({
        "rule_version": (!rule.is_empty()).then_some(rule),
        "lineage": (!lineage.is_empty()).then_some(lineage),
        "cause": (!cause.is_empty()).then_some(cause),
        "replay": replay,
        "replay_detail": replay_detail,
        "coverage": infrastructure.get("coverage").cloned().unwrap_or_else(|| json!("unresolved")),
        "duplicates": duplicates,
        "reconciliation": reconciliation,
        "exclusions": exclusions,
        "basis": "deterministic reduction of the retained native trace under the frozen attribution rule; the raw totals stay beside the adjusted view; every exclusion and unresolved classification keeps its rule and evidence or reason, and missing telemetry stays unknown",
    }))
}

/// Request identities that occur more than once in the retained capture. A
/// repeated identity is one request, not two savings, and the degraded
/// accounting coverage is retained beside the reconciled totals.
fn duplicate_request_ids(row: &Value) -> BTreeSet<String> {
    let mut seen: BTreeMap<&str, usize> = BTreeMap::new();
    let Some(requests) = row
        .get("infrastructure_capture")
        .and_then(|capture| capture.get("requests"))
        .and_then(Value::as_array)
    else {
        return BTreeSet::new();
    };
    for request in requests {
        if let Some(id) = request
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
        {
            *seen.entry(id).or_default() += 1;
        }
    }
    seen.into_iter()
        .filter(|(_, count)| *count > 1)
        .map(|(id, _)| id.to_owned())
        .collect()
}

/// Raw/adjusted/excluded reconciliation totals for the two measured
/// quantities eligible for subtraction. Each identity is checked exactly; an
/// inconsistent total is an accounting gap, never a saving, and an absent
/// component stays unknown.
fn attribution_reconciliation(infrastructure: &Value, duplicates: &[String]) -> Value {
    let u64_field = |value: &Value, key: &str| value.get(key).and_then(Value::as_u64);
    let observed = u64_field(infrastructure, "observed_ns");
    let adjusted_high = u64_field(infrastructure, "adjusted_ns");
    let adjusted_low = u64_field(infrastructure, "adjusted_low_ns");
    let deducted = u64_field(infrastructure, "deductible_ns");
    let unresolved = u64_field(infrastructure, "unresolved_ns");
    let elapsed_status = match (observed, adjusted_high, adjusted_low, deducted, unresolved) {
        (Some(observed), Some(high), Some(low), Some(deducted), Some(unresolved)) => {
            let exact_high = high.checked_add(deducted) == Some(observed);
            let exact_low = low
                .checked_add(deducted)
                .and_then(|value| value.checked_add(unresolved))
                == Some(observed);
            if exact_high && exact_low {
                "consistent"
            } else {
                "gap"
            }
        }
        _ => "unknown",
    };
    let usage = infrastructure.get("usage");
    let side = |name: &str, field: &str| {
        usage
            .and_then(|usage| usage.get(name))
            .and_then(|tokens| u64_field(tokens, field))
    };
    let raw_total = side("raw", "total_tokens");
    let excluded_total = side("excluded", "total_tokens");
    let adjusted_total = side("adjusted", "total_tokens");
    let usage_incomplete = usage
        .and_then(|usage| usage.get("incomplete"))
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let tokens_status = match (raw_total, excluded_total, adjusted_total) {
        (Some(raw), Some(excluded), Some(adjusted)) => {
            if adjusted.checked_add(excluded) == Some(raw) {
                if usage_incomplete || !duplicates.is_empty() {
                    "degraded"
                } else {
                    "consistent"
                }
            } else {
                "gap"
            }
        }
        _ => "unknown",
    };
    let status = if elapsed_status == "gap" || tokens_status == "gap" {
        "gap"
    } else if tokens_status == "degraded" {
        "degraded"
    } else if elapsed_status == "consistent" {
        "consistent"
    } else {
        "unknown"
    };
    json!({
        "status": status,
        "elapsed": {
            "status": elapsed_status,
            "observed_seconds": infrastructure.get("observed_seconds").cloned().unwrap_or(Value::Null),
            "adjusted_low_seconds": infrastructure.get("adjusted_low_seconds").cloned().unwrap_or(Value::Null),
            "adjusted_high_seconds": infrastructure.get("adjusted_high_seconds").cloned().unwrap_or(Value::Null),
            "excluded_seconds": infrastructure.get("deductible_seconds").cloned().unwrap_or(Value::Null),
            "unresolved_seconds": infrastructure.get("unresolved_seconds").cloned().unwrap_or(Value::Null),
            "identity": "observed_seconds = adjusted_high_seconds + excluded_seconds and observed_seconds = adjusted_low_seconds + excluded_seconds + unresolved_seconds; unresolved usage stays included and no time deduction implies a token, energy or currency deduction",
        },
        "tokens": {
            "status": tokens_status,
            "raw_total_tokens": raw_total,
            "excluded_total_tokens": excluded_total,
            "adjusted_total_tokens": adjusted_total,
            "identity": "raw_total_tokens = adjusted_total_tokens + excluded_total_tokens; duplicate or incomplete request usage degrades coverage instead of creating savings",
        },
        "duplicates": duplicates,
        "basis": "the two identities are checked exactly on the retained totals; absent components stay unknown, never zero",
    })
}

/// One reason entry per exclusion and unresolved classification carried by the
/// retained adjusted view. The reason, evidence reference and rule are kept so
/// that no adjusted metric's subtraction can be read without its cause.
fn attribution_exclusions(
    infrastructure: &Value,
    row: &Value,
    duplicates: &[String],
) -> Vec<Value> {
    let mut out = Vec::new();
    let rule = infrastructure
        .get("rule_version")
        .cloned()
        .unwrap_or(Value::Null);
    let cause = infrastructure
        .get("eligible_cause")
        .cloned()
        .unwrap_or(Value::Null);
    let capture_retained = row
        .get("infrastructure_capture")
        .is_some_and(Value::is_object);
    let evidence = if capture_retained {
        "retained native queue capture on this attempt (admissions, activity, requests)"
    } else {
        "retained adjusted view on this attempt; the native capture is not retained"
    };
    if infrastructure
        .get("deductible_ns")
        .and_then(Value::as_u64)
        .unwrap_or(0)
        > 0
    {
        out.push(json!({
            "kind": "deducted",
            "metric": "elapsed",
            "amount_seconds": infrastructure.get("deductible_seconds").cloned().unwrap_or(Value::Null),
            "rule": rule,
            "cause": cause,
            "reason": "verified unrelated external queue blocking, clipped to the attempt, unioned once and reduced by overlapping useful work",
            "evidence": evidence,
        }));
    }
    let excluded_requests: Vec<Value> = infrastructure
        .get("excluded_requests")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if !excluded_requests.is_empty() {
        let ids = excluded_requests
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(", ");
        out.push(json!({
            "kind": "deducted",
            "metric": "tokens",
            "amount_tokens": infrastructure
                .get("usage")
                .and_then(|usage| usage.get("excluded"))
                .and_then(|tokens| tokens.get("total_tokens"))
                .cloned()
                .unwrap_or(Value::Null),
            "requests": excluded_requests,
            "rule": rule,
            "cause": cause,
            "reason": "whole observed wait-only model request(s), independently correlated with the verified blocking episode and contained in it",
            "evidence": format!("retained native request evidence: {ids}"),
        }));
    }
    for duplicate in duplicates {
        out.push(json!({
            "kind": "unresolved",
            "metric": "tokens",
            "rule": rule,
            "reason": format!("duplicate request identity {duplicate} is one request, not two savings; the duplicated usage degrades accounting coverage"),
            "evidence": format!("retained native request evidence: {duplicate}"),
        }));
    }
    if infrastructure
        .get("usage")
        .and_then(|usage| usage.get("incomplete"))
        .and_then(Value::as_bool)
        == Some(true)
    {
        out.push(json!({
            "kind": "unresolved",
            "metric": "tokens",
            "rule": rule,
            "reason": "accounted usage is incomplete: mixed, duplicated or unplaceable request usage stays included and is not fractionally reconstructed",
            "evidence": evidence,
        }));
    }
    for gap in infrastructure
        .get("gaps")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        let metric = if gap.contains("request") || gap.contains("model") {
            "tokens"
        } else {
            "elapsed"
        };
        out.push(json!({
            "kind": "unresolved",
            "metric": metric,
            "rule": rule,
            "reason": gap,
            "evidence": format!("retained native evidence: {gap}"),
        }));
    }
    out
}

pub fn comparison_reasons(left: &Value, right: &Value) -> io::Result<Vec<String>> {
    let mut reasons = BTreeSet::new();
    if !(identity_known(&left["case_id"]) && identity_known(&right["case_id"])) {
        reasons.insert("unknown:case_id".into());
    } else if left["case_id"] != right["case_id"] {
        reasons.insert("different_case".into());
    }
    if !((left["arm"] == "baseline" && right["arm"] == "candidate")
        || (right["arm"] == "baseline" && left["arm"] == "candidate"))
    {
        reasons.insert("not_opposite_arms".into());
    }
    // A method without model execution is a first-class measured unit: its
    // pair comparability rests on its declared identities and its own
    // measured operation work, never on a model identity, effort or provider.
    // Two different declared methods never form one comparable pair.
    let left_model_free = model_free_attempt(left);
    let right_model_free = model_free_attempt(right);
    if left_model_free != right_model_free {
        reasons.insert("different_method".into());
    } else if left_model_free
        && let (Some(left_method), Some(right_method)) =
            (text(left, "method"), text(right, "method"))
        && left_method != right_method
    {
        reasons.insert("different_method".into());
    }
    // A declared experiment/pair identity scopes the comparison: an edge that
    // crosses two declared units is not evidence for either of them, while
    // attempts that declare no finer identity share their experiment/case unit
    // and keep the repeated-selection handling.
    if unit_of(left) != unit_of(right) {
        reasons.insert("different_declared_unit".into());
    }
    let empty = serde_json::Map::new();
    let a = match left.get("matched") {
        None => &empty,
        Some(v) => v.as_object().ok_or_else(invalid)?,
    };
    let b = match right.get("matched") {
        None => &empty,
        Some(v) => v.as_object().ok_or_else(invalid)?,
    };
    let keys: BTreeSet<_> = MATCH_FIELDS
        .iter()
        .copied()
        .chain(a.keys().map(String::as_str))
        .chain(b.keys().map(String::as_str))
        .collect();
    // A reused baseline binds retained evidence to the current comparison:
    // its retained identity and relevant conditions must still match exactly.
    // A mismatch or an unverified identity refuses the reuse with the exact
    // changed or missing fact instead of letting stale evidence look like the
    // same measurement.
    let reused = if left.get("reuse_evidence").is_some() {
        Some((left, right))
    } else if right.get("reuse_evidence").is_some() {
        Some((right, left))
    } else {
        None
    };
    for key in keys {
        if left_model_free && right_model_free && MODEL_EXECUTION_MATCH_FIELDS.contains(&key) {
            // The model-execution identity is inapplicable to this method: it
            // is not required to be known, and a recorded value is compared
            // only when both arms declare one.
            if let (Some(x), Some(y)) = (a.get(key), b.get(key))
                && identity_known(x)
                && identity_known(y)
                && x != y
            {
                reasons.insert(format!("mismatch:{key}"));
                if reused.is_some() {
                    reasons.insert(format!(
                        "reuse-refused: the retained baseline was measured under a different {key}; stale retained evidence cannot be reused and a fresh control is required"
                    ));
                }
            }
            continue;
        }
        match (a.get(key), b.get(key)) {
            (Some(x), Some(y)) if identity_known(x) && identity_known(y) => {
                if x != y {
                    reasons.insert(format!("mismatch:{key}"));
                    if reused.is_some() {
                        reasons.insert(format!(
                            "reuse-refused: the retained baseline was measured under a different {key}; stale retained evidence cannot be reused and a fresh control is required"
                        ));
                    }
                }
            }
            _ => {
                reasons.insert(format!("unknown:{key}"));
                if reused.is_some() {
                    reasons.insert(format!(
                        "reuse-refused: the comparison does not verify {key} between the retained baseline and the current arm; unverified comparability cannot be reused"
                    ));
                }
            }
        }
    }
    if let Some((retained, current)) = reused {
        if current["arm"] != json!("candidate") {
            reasons.insert(
                "reuse-refused: a retained baseline may only be reused against the candidate arm; a retained candidate would replace the measured treatment"
                    .to_owned(),
            );
        }
        if retained["attempt_id"] == current["attempt_id"] {
            reasons.insert(
                "reuse-refused: the reuse names the candidate's own attempt as its retained baseline; a reused baseline must reference an earlier retained execution"
                    .to_owned(),
            );
        }
        let recorded = retained.get("reuse_evidence");
        let conditions = current.get("conditions").and_then(Value::as_object);
        for key in ["cache", "load"] {
            let expected = recorded
                .and_then(|value| value.get("conditions"))
                .and_then(|value| value.get(key))
                .and_then(Value::as_str);
            let actual = conditions
                .and_then(|value| value.get(key))
                .and_then(Value::as_str);
            match (expected, actual) {
                (Some(expected), Some(actual)) if expected == actual => {}
                (Some(expected), Some(actual)) => {
                    reasons.insert(format!(
                        "reuse-refused: the retained baseline was measured under a different {key} condition ({expected:?} vs {actual:?}); changed conditions require a fresh control"
                    ));
                }
                (Some(_), None) => {
                    reasons.insert(format!(
                        "reuse-refused: the current comparison does not record its {key} condition; the retained baseline's conditions cannot be verified"
                    ));
                }
                (None, _) => {}
            }
        }
    }
    for row in [left, right] {
        reasons.extend(strings(array(row, "excluded_reasons")?)?);
        if model_free_attempt(row) {
            // A model-free method completes its evidence with the
            // operation's own measured work instead of model metadata.
            if !operation_work_recorded(row) {
                reasons.insert("operation_work_unverified".into());
            }
        } else if !truth(&row["discovery_verified"]) {
            reasons.insert("discovery_unverified".into());
        }
    }
    Ok(reasons.into_iter().collect())
}

/// Normalized evaluation declaration recorded on an attempt before outcomes
/// are observed. An absent declaration stays absent: it is a limitation, not
/// an inferred plan. A present declaration must be an object.
#[derive(Debug, Default)]
struct Declaration {
    task_mix: Option<String>,
    objective: Option<String>,
    /// The pre-agreed comparison basis; a maintenance-only result is adopted
    /// only when the recorded declaration carries it.
    basis: Option<String>,
    effect_percent: Option<f64>,
    nuisance: bool,
    stopping: Option<String>,
    uncertainty: Option<String>,
    horizon_tasks: Option<f64>,
    costs: Option<(f64, f64, f64)>,
}

/// What one declared unit records about a subtractive treatment: the removed
/// burden and whether both arms establish that the burden was actually
/// consumed before removal, separately from invocation counts.
struct SubtractiveFacts {
    applicability: &'static str,
    removed: Option<String>,
    retained_checks: bool,
    limitations: Vec<String>,
}

/// One (experiment, case) group's complete-pair variation evidence and the
/// dependent within-run events that are deliberately not replications.
#[derive(Default)]
struct VariationGroup {
    complete_pairs: u64,
    untimed_pairs: u64,
    elapsed_effects: Vec<f64>,
    attempts: u64,
    native_runs: u64,
    requests: u64,
    rounds: u64,
    tool_calls: u64,
    tool_operations: u64,
    unknown_requests: u64,
    unknown_rounds: u64,
    unknown_tool_calls: u64,
    unknown_tool_operations: u64,
}

/// Consume the normalized per-arm treatment and consumption records into the
/// unit's applicability. `None` means the unit is not subtractive; only `Some`
/// with `applicability == "exercised"` and retained checks may support a
/// saving.
fn subtractive_facts(
    baseline: Option<&Value>,
    candidate: Option<&Value>,
) -> io::Result<Option<SubtractiveFacts>> {
    let arms = [("baseline", baseline), ("candidate", candidate)];
    let declared: Vec<bool> = arms
        .iter()
        .filter_map(|(_, row)| row.map(|row| row["treatment_kind"] == json!("subtractive")))
        .collect();
    if !declared.iter().any(|value| *value) {
        return Ok(None);
    }
    let mut limitations = Vec::new();
    if declared.len() != 2 || declared.iter().any(|value| !*value) {
        limitations.push("the subtractive treatment is not declared on both arms".to_owned());
    }
    let names: BTreeSet<String> = arms
        .iter()
        .filter_map(|(_, row)| {
            row.and_then(|row| row["removed_burden"].as_str())
                .map(str::to_owned)
        })
        .collect();
    let removed = if names.len() == 1 {
        names.into_iter().next()
    } else {
        if names.len() > 1 {
            limitations.push(
                "the subtractive treatment names different removed burdens across the arms"
                    .to_owned(),
            );
        }
        None
    };
    let consumed = |row: Option<&Value>| -> Option<bool> {
        let row = row?;
        let record = &row["consumption_evidence"];
        if record.is_null() || record["evidenced"] != json!(true) {
            return None;
        }
        if removed
            .as_deref()
            .is_some_and(|name| record["capability"].as_str() != Some(name))
        {
            return None;
        }
        match record["status"].as_str() {
            Some("consumed") => Some(true),
            Some("absent") => Some(false),
            _ => None,
        }
    };
    let applicability = match (consumed(baseline), consumed(candidate)) {
        (Some(true), Some(false)) => "exercised",
        (Some(false), Some(false)) => {
            limitations.push(format!(
                "the workload did not exercise the removed burden {}; removing it cannot establish a useful saving, and unexercised usefulness remains unresolved",
                removed.as_deref().unwrap_or("(unnamed)")
            ));
            "not_exercised"
        }
        (Some(true), Some(true)) => {
            limitations.push(format!(
                "the candidate arm still records consumption of the removed burden {}; the intended context treatment is not established, and a smaller source tree or zero invocations cannot support a saving",
                removed.as_deref().unwrap_or("(unnamed)")
            ));
            "unknown"
        }
        _ => {
            limitations.push(format!(
                "actual consumption of the removed burden {} is not recorded with retained evidence on one or both arms; zero invocations or a smaller source tree is not consumption evidence",
                removed.as_deref().unwrap_or("(unnamed)")
            ));
            "unknown"
        }
    };
    let baseline_checks = executed_check_ids(baseline.unwrap_or(&Value::Null))?;
    let candidate_checks = executed_check_ids(candidate.unwrap_or(&Value::Null))?;
    let retained_checks =
        !baseline_checks.is_empty() && baseline_checks.is_subset(&candidate_checks);
    if !retained_checks {
        limitations.push(
            "the candidate arm records fewer required checks than the baseline; removing the check that would expose a regression cannot support adoption"
                .to_owned(),
        );
    }
    Ok(Some(SubtractiveFacts {
        applicability,
        removed,
        retained_checks,
        limitations,
    }))
}

fn parse_declaration(value: Option<&Value>) -> io::Result<Option<Declaration>> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    let object = value.as_object().ok_or_else(invalid)?;
    let entry = |key: &str| -> Option<String> {
        object
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    };
    let costs = object
        .get("costs")
        .and_then(Value::as_object)
        .and_then(|costs| {
            [
                "implementation_seconds",
                "evaluation_seconds",
                "maintenance_seconds_per_task",
            ]
            .iter()
            .map(|key| costs.get(*key).and_then(number))
            .collect::<Option<Vec<f64>>>()
        })
        .map(|values| (values[0], values[1], values[2]));
    Ok(Some(Declaration {
        task_mix: entry("task_mix"),
        objective: entry("objective"),
        basis: entry("basis"),
        effect_percent: object
            .get("effect_percent")
            .and_then(number)
            .filter(|value| *value >= 0.0),
        nuisance: object.get("nuisance").is_some_and(truth),
        stopping: entry("stopping"),
        uncertainty: entry("uncertainty"),
        horizon_tasks: object
            .get("horizon_tasks")
            .and_then(number)
            .filter(|value| *value > 0.0),
        costs,
    }))
}

/// Failed attempts remain available for success-rate comparisons. Missing usage
/// stays null; parent/child token totals are not summed here. Attempts may
/// declare experiment/pair identities; every comparable baseline-candidate
/// edge is listed, but edges from one declared unit are one sample, not
/// independent repetitions. Each unit also carries its predeclared
/// classification, evidence completeness and computed effect; the report
/// itself still makes no adoption or savings claim.
pub fn summarize_attempts(attempts: &[Value]) -> io::Result<Value> {
    if attempts.len() > 1024 {
        return Err(invalid());
    }
    let mut rows = attempts
        .iter()
        .map(finish_attempt)
        .collect::<io::Result<Vec<_>>>()?;
    let mut by_id = BTreeMap::new();
    for (index, row) in rows.iter().enumerate() {
        let id = row["attempt_id"]
            .as_str()
            .filter(|v| !v.is_empty())
            .ok_or_else(invalid)?;
        if by_id.insert(id.to_owned(), index).is_some() {
            return Err(invalid());
        }
    }
    for index in 0..rows.len() {
        let mut chain = vec![index];
        let mut seen = BTreeSet::from([index]);
        let mut reasons = strings(array(&rows[index], "excluded_reasons")?)?;
        let mut complete = true;
        while truth(&rows[*chain.last().unwrap()]["retry_of"]) {
            let id = rows[*chain.last().unwrap()]["retry_of"]
                .as_str()
                .ok_or_else(invalid)?;
            let parent = match by_id.get(id).copied() {
                Some(p) if seen.insert(p) => p,
                _ => {
                    reasons.insert("unresolved_retry_chain".into());
                    complete = false;
                    break;
                }
            };
            if rows[parent]["case_id"] != rows[index]["case_id"]
                || rows[parent]["arm"] != rows[index]["arm"]
                || rows[parent]["unit"] != rows[index]["unit"]
            {
                reasons.insert("retry_identity_mismatch".into());
                complete = false;
                break;
            }
            chain.push(parent);
        }
        let accounted: Vec<_> = chain.iter().map(|&i| {
            let start = number(execution_start(&rows[i]));
            json!({"started_at": start, "ended_at": start.zip(number(&rows[i]["elapsed_seconds"])).map(|(a,b)| a+b).filter(|v| v.is_finite())})
        }).collect();
        let elapsed = complete.then(|| wall_span(&accounted)).flatten();
        let prep = number(&rows[*chain.last().unwrap()]["preparation_seconds"]);
        rows[index]["result_attempt_ids"] = json!(
            chain
                .iter()
                .rev()
                .map(|&i| rows[i]["attempt_id"].clone())
                .collect::<Vec<_>>()
        );
        rows[index]["total_result_seconds"] = json!(elapsed);
        rows[index]["total_verified_seconds"] = json!(
            elapsed
                .zip(prep)
                .map(|(a, b)| a + b)
                .filter(|v| v.is_finite())
        );
        // Retried work stays accounted: the chain total includes every
        // recorded attempt's observed counters, and an unknown counter on any
        // recorded attempt keeps the whole total unknown.
        let chain_counter = |key: &str| -> Value {
            let mut total = 0_u64;
            for &i in &chain {
                match rows[i][key]
                    .as_u64()
                    .and_then(|value| total.checked_add(value))
                {
                    Some(value) => total = value,
                    None => return Value::Null,
                }
            }
            json!(total)
        };
        let total_tool_operations = chain_counter("tool_operations");
        let total_rounds = chain_counter("rounds");
        let total_tool_calls = chain_counter("tool_calls");
        let total_requests = chain_counter("requests");
        rows[index]["total_tool_operations"] = total_tool_operations;
        rows[index]["total_rounds"] = total_rounds;
        rows[index]["total_tool_calls"] = total_tool_calls;
        rows[index]["total_requests"] = total_requests;
        rows[index]["task_root"] = if complete {
            rows[*chain.last().unwrap()]["attempt_id"].clone()
        } else {
            Value::Null
        };
        let mut usage = Vec::new();
        let mut unknown_runs = 0usize;
        for run in array(&rows[index], "native_runs")? {
            if truth(&run["usage"]) {
                usage.push(run["usage"].clone());
            } else {
                unknown_runs += 1;
            }
        }
        let mut workers = Vec::new();
        let mut inherited_children = 0usize;
        for (child_index, child) in array(&rows[index], "children")?.iter().enumerate() {
            if let Some(referenced) = inherited_child(child) {
                if !by_id.contains_key(referenced) {
                    return Err(invalid());
                }
                inherited_children += 1;
                continue;
            }
            if truth(&child["usage"]) {
                workers.push(json!({
                    "role": "worker",
                    "child": child_index,
                    "usage": child["usage"],
                }));
            }
        }
        if !truth(&rows[index]["usage"]) {
            let mut summary = if usage.is_empty() && workers.is_empty() {
                json!({"status":"unknown", "total_tokens":null})
            } else {
                json!({"status":"per_run", "runs":usage})
            };
            if !workers.is_empty() {
                summary["workers"] = json!(workers);
            }
            if unknown_runs > 0 && summary["status"] == "per_run" {
                summary["unknown_runs"] = json!(unknown_runs);
            }
            if inherited_children > 0 {
                summary["inherited_children"] = json!(inherited_children);
            }
            rows[index]["usage"] = summary;
        }
        rows[index]["excluded_reasons"] = json!(reasons);
    }
    let mut pairs = Vec::new();
    let mut pair_bytes = 0usize;
    for left in &rows {
        for right in &rows {
            if left["arm"] == "baseline"
                && right["arm"] == "candidate"
                && left["case_id"] == right["case_id"]
            {
                let reasons = comparison_reasons(left, right)?;
                let pair = json!({
                    "baseline": left["attempt_id"],
                    "candidate": right["attempt_id"],
                    "unit": left["unit"],
                    "comparable": reasons.is_empty(),
                    "excluded_reasons": reasons,
                });
                pair_bytes = pair_bytes
                    .checked_add(serde_json::to_vec(&pair)?.len())
                    .ok_or_else(invalid)?;
                if pair_bytes > 16 * 1024 * 1024 {
                    return Err(invalid());
                }
                pairs.push(pair);
            }
        }
    }
    for row in &mut rows {
        if !pairs
            .iter()
            .any(|p| row["attempt_id"] == p["baseline"] || row["attempt_id"] == p["candidate"])
        {
            let mut reasons = strings(array(row, "excluded_reasons")?)?;
            reasons.insert("no_opposite_arm".into());
            row["excluded_reasons"] = json!(reasons);
        }
    }

    // Declared units: all edges from one unit are one sample. A unit is
    // one-to-one only when each arm contributes exactly one result attempt
    // (a retry chain counts as one result).
    let referenced: BTreeSet<String> = rows
        .iter()
        .filter_map(|row| row["retry_of"].as_str().map(str::to_owned))
        .collect();
    let tip_ids: BTreeSet<String> = rows
        .iter()
        .filter_map(|row| row["attempt_id"].as_str())
        .filter(|id| !referenced.contains(*id))
        .map(str::to_owned)
        .collect();
    let mut unit_arms: BTreeMap<String, (BTreeSet<String>, BTreeSet<String>)> = BTreeMap::new();
    for pair in &pairs {
        if !truth(&pair["comparable"]) {
            continue;
        }
        let unit = pair["unit"].as_str().ok_or_else(invalid)?.to_owned();
        let mut endpoints = Vec::new();
        for side in ["baseline", "candidate"] {
            let id = pair[side].as_str().ok_or_else(invalid)?;
            let index = *by_id.get(id).ok_or_else(invalid)?;
            endpoints.push(
                rows[index]["task_root"]
                    .as_str()
                    .ok_or_else(invalid)?
                    .to_owned(),
            );
        }
        let entry = unit_arms.entry(unit).or_default();
        entry.0.insert(endpoints.remove(0));
        entry.1.insert(endpoints.remove(0));
    }
    // A unit is one-to-one only when each arm contributes exactly one task
    // result: a retry chain counts as one result, while several accepted
    // candidates behind one baseline are repeated selection, not independent
    // repetitions. The arm's result is the chain tip of its declared task.
    let arm_results = |roots: &BTreeSet<String>| -> io::Result<Vec<&Value>> {
        let mut results: Vec<&Value> = Vec::new();
        if let Some(root) = roots.iter().next()
            && roots.len() == 1
        {
            results.extend(rows.iter().filter(|row| {
                row["task_root"].as_str() == Some(root.as_str())
                    && row["attempt_id"]
                        .as_str()
                        .is_some_and(|id| tip_ids.contains(id))
            }));
        }
        Ok(results)
    };
    let unit_results: BTreeMap<String, (usize, usize)> = unit_arms
        .iter()
        .map(|(unit, (baseline, candidate))| {
            Ok((
                unit.clone(),
                (arm_results(baseline)?.len(), arm_results(candidate)?.len()),
            ))
        })
        .collect::<io::Result<_>>()?;
    for pair in &mut pairs {
        let independent = truth(&pair["comparable"])
            && pair["unit"]
                .as_str()
                .and_then(|unit| unit_results.get(unit))
                .is_some_and(|(baseline, candidate)| *baseline == 1 && *candidate == 1);
        pair["independent"] = independent.into();
    }
    let mut units = Vec::new();
    for unit in unit_arms.keys() {
        let edges: Vec<&Value> = pairs
            .iter()
            .filter(|pair| truth(&pair["comparable"]) && pair["unit"] == json!(unit))
            .collect();
        let mut baseline_ids = BTreeSet::new();
        let mut candidate_ids = BTreeSet::new();
        let mut participants: Vec<&Value> = Vec::new();
        for edge in &edges {
            for side in ["baseline", "candidate"] {
                let id = edge[side].as_str().ok_or_else(invalid)?;
                let row = &rows[*by_id.get(id).ok_or_else(invalid)?];
                if side == "baseline" {
                    baseline_ids.insert(id.to_owned());
                } else {
                    candidate_ids.insert(id.to_owned());
                }
                if !participants
                    .iter()
                    .any(|participant| participant["attempt_id"] == row["attempt_id"])
                {
                    participants.push(row);
                }
            }
        }
        let (baseline_roots, candidate_roots) = unit_arms.get(unit).ok_or_else(invalid)?;
        let baseline_results = arm_results(baseline_roots)?;
        let candidate_results = arm_results(candidate_roots)?;
        let one_to_one = baseline_results.len() == 1 && candidate_results.len() == 1;
        let baseline_result = baseline_results.first().copied();
        let candidate_result = candidate_results.first().copied();
        // A unit whose method executes no model call proves its completeness
        // through the operation's own measured work (duration, exit and the
        // declared inputs); model identity, model metadata and model-round
        // counters do not apply to it, and its metrics stay inapplicable
        // rather than becoming a measured zero.
        let model_free = baseline_result.is_some_and(model_free_attempt)
            && candidate_result.is_some_and(model_free_attempt);
        let operation_work = model_free
            && baseline_result.is_some_and(operation_work_recorded)
            && candidate_result.is_some_and(operation_work_recorded);
        let model_free_method = if model_free {
            match (
                baseline_result.and_then(|row| text(row, "method")),
                candidate_result.and_then(|row| text(row, "method")),
            ) {
                (Some(baseline), Some(candidate)) if baseline == candidate => {
                    Some(baseline.to_owned())
                }
                _ => None,
            }
        } else {
            None
        };
        // The declaration is compared across every attempt of the declared
        // task, including rework attempts that are not themselves comparable
        // edge endpoints.
        for row in &rows {
            let Some(root) = row["task_root"].as_str() else {
                continue;
            };
            if !baseline_roots.contains(root) && !candidate_roots.contains(root) {
                continue;
            }
            if !participants
                .iter()
                .any(|participant| participant["attempt_id"] == row["attempt_id"])
            {
                participants.push(row);
            }
        }
        let mut limitations = Vec::new();
        let subtractive = subtractive_facts(baseline_result, candidate_result)?;
        if let Some(facts) = &subtractive {
            limitations.extend(facts.limitations.iter().cloned());
        }
        let declarations: Vec<Value> = participants
            .iter()
            .map(|row| row.get("declaration").cloned().unwrap_or(Value::Null))
            .collect();
        let declaration_value = declarations.first().cloned().unwrap_or(Value::Null);
        if declarations.iter().any(|value| *value != declaration_value) {
            limitations.push("declaration differs across the unit's attempts".to_owned());
        }
        let declaration = parse_declaration(Some(&declaration_value))?;
        let objective = declaration
            .as_ref()
            .and_then(|value| value.objective.as_deref());
        let required_declared = declaration.as_ref().is_some_and(|value| {
            value.task_mix.is_some()
                && value.nuisance
                && value.stopping.is_some()
                && value.uncertainty.is_some()
                && (!matches!(objective, Some("time" | "resource"))
                    || value.effect_percent.is_some())
        });
        let claim_class = match objective {
            Some("quality") => "deterministic_quality",
            Some("time" | "resource") => "stochastic_savings",
            Some("subscription") => "unmeasured_subscription",
            _ => "undeclared",
        };
        match &declaration {
            None => {
                limitations.push(
                    "no predeclared task mix, objective, effect or stopping policy".to_owned(),
                );
            }
            Some(declared) => {
                if declared.task_mix.is_none() {
                    limitations.push("task mix not declared".to_owned());
                }
                if !declared.nuisance {
                    limitations.push("nuisance controls not declared".to_owned());
                }
                if declared.stopping.is_none() {
                    limitations.push("stopping policy not declared".to_owned());
                }
                if declared.uncertainty.is_none() {
                    limitations.push("uncertainty policy not declared".to_owned());
                }
                if matches!(objective, Some("time" | "resource"))
                    && declared.effect_percent.is_none()
                {
                    limitations.push("declared meaningful effect missing".to_owned());
                }
            }
        }
        if objective == Some("subscription") {
            limitations.push(
                "subscription allowance is not measurable from token or byte totals".to_owned(),
            );
        }
        if let Some(other) = objective
            && !matches!(other, "quality" | "time" | "resource" | "subscription")
        {
            limitations.push(
                "declared objective is not quality, time, resource or subscription".to_owned(),
            );
        }
        let mut effect =
            json!({"baseline_seconds": null, "candidate_seconds": null, "delta_seconds": null});
        let mut positive_effect = Value::Null;
        let mut evidence_complete = false;
        let mut net_saving = Value::Null;
        let baseline_accepted = baseline_result.is_some_and(|row| row["status"] == "accepted");
        let candidate_accepted = candidate_result.is_some_and(|row| row["status"] == "accepted");
        if !one_to_one {
            limitations.push(
                "repeated attempts within the declared unit: aggregate before inference".to_owned(),
            );
        } else {
            let baseline_seconds =
                baseline_result.and_then(|row| number(&row["total_result_seconds"]));
            let candidate_seconds =
                candidate_result.and_then(|row| number(&row["total_result_seconds"]));
            let delta = candidate_seconds.zip(baseline_seconds).map(|(c, b)| c - b);
            effect = json!({"baseline_seconds": baseline_seconds, "candidate_seconds": candidate_seconds, "delta_seconds": delta});
            let verified = if model_free {
                operation_work
            } else {
                baseline_result.is_some_and(|row| {
                    truth(&row["discovery_verified"])
                        && truth(&row["observed_model_metadata_verified"])
                }) && candidate_result.is_some_and(|row| {
                    truth(&row["discovery_verified"])
                        && truth(&row["observed_model_metadata_verified"])
                })
            };
            if !verified {
                limitations.push(
                    if model_free {
                        "the operation's own measured work (duration, exit and declared inputs) is not recorded on both arms"
                    } else {
                        "model metadata not verified on both arms"
                    }
                    .to_owned(),
                );
            }
            match objective {
                Some("quality") => {
                    positive_effect = json!(candidate_accepted && !baseline_accepted);
                    if baseline_accepted && candidate_accepted {
                        limitations.push(
                            "both arms accepted: no quality difference is recorded".to_owned(),
                        );
                    } else if !candidate_accepted {
                        limitations
                            .push("acceptance evidence incomplete on one or both arms".to_owned());
                    }
                    evidence_complete = verified
                        && required_declared
                        && candidate_accepted
                        && baseline_result.is_some();
                }
                Some("time" | "resource") => {
                    let reduction_percent = delta
                        .zip(baseline_seconds)
                        .filter(|(_, baseline)| *baseline > 0.0)
                        .map(|(delta, baseline)| -delta / baseline * 100.0);
                    let met = declaration
                        .as_ref()
                        .and_then(|value| value.effect_percent)
                        .is_none_or(|minimum| {
                            reduction_percent.is_some_and(|value| value >= minimum)
                        });
                    positive_effect =
                        json!(reduction_percent.is_some_and(|value| value > 0.0) && met);
                    match reduction_percent {
                        None => limitations
                            .push("no comparable elapsed result recorded for both arms".to_owned()),
                        Some(value) if value <= 0.0 => limitations.push(
                            "no positive recorded effect for the declared objective".to_owned(),
                        ),
                        Some(_) if !met => limitations.push(
                            "recorded effect is below the declared meaningful minimum".to_owned(),
                        ),
                        Some(_) => {}
                    }
                    if let Some(declared) = declaration.as_ref()
                        && let (
                            Some(reduction),
                            Some(horizon),
                            Some((implementation, evaluation, maintenance)),
                        ) = (
                            delta.map(|delta| -delta),
                            declared.horizon_tasks,
                            declared.costs,
                        )
                    {
                        let total = reduction * horizon
                            - (implementation + evaluation + maintenance * horizon);
                        let verdict = if total > 0.0 {
                            "net_saving"
                        } else {
                            "does_not_repay"
                        };
                        if verdict == "does_not_repay" {
                            limitations.push(
                                "declared per-task saving does not repay evaluation over the declared horizon"
                                    .to_owned(),
                            );
                        }
                        net_saving =
                            json!({"verdict": verdict, "seconds": total, "horizon_tasks": horizon});
                    }
                    evidence_complete = verified
                        && required_declared
                        && baseline_accepted
                        && candidate_accepted
                        && delta.is_some();
                }
                _ => {}
            }
            // A subtractive saving exists only when the burden was actually
            // consumed before removal and the candidate is observed not to
            // consume it, with the baseline's required checks retained. An
            // unexercised, unknown or not-applied removal cannot show a
            // positive effect or complete evidence, whatever the elapsed
            // difference.
            if let Some(facts) = &subtractive
                && (facts.applicability != "exercised" || !facts.retained_checks)
            {
                positive_effect = json!(false);
                evidence_complete = false;
            }
        }
        let declared = declaration.as_ref().map(|value| {
            json!({
                "task_mix": value.task_mix,
                "objective": value.objective,
                "basis": value.basis,
                "effect_percent": value.effect_percent,
                "nuisance": value.nuisance,
                "stopping": value.stopping,
                "uncertainty": value.uncertainty,
                "horizon_tasks": value.horizon_tasks,
                "costs": value.costs.map(|(implementation, evaluation, maintenance)| json!({
                    "implementation_seconds": implementation,
                    "evaluation_seconds": evaluation,
                    "maintenance_seconds_per_task": maintenance,
                })),
            })
        });
        // A retained baseline is not a fresh execution: the unit records the
        // retained identity, age, coverage, uncertainty, conditions and
        // original cost beside the effect, so the reuse stays visible instead
        // of being presented as a new run of the old variant.
        let reuse = baseline_result.and_then(|row| row.get("reuse_evidence"));
        if let Some(evidence) = reuse {
            let at = number(&evidence["executed_at"])
                .map(|value| format!("{value:.1}"))
                .unwrap_or_else(|| "an unrecorded time".to_owned());
            let age = number(&evidence["age_seconds"])
                .map(|value| format!("{value:.1} s"))
                .unwrap_or_else(|| "an unrecorded age".to_owned());
            limitations.push(format!(
                "the baseline result is retained evidence executed at {at} and reused for this comparison ({age} old); its original cost is accounted once and no fresh baseline execution is claimed"
            ));
        }
        let mut unit_value = json!({
            "unit": unit,
            "experiment_id": participants.first().and_then(|row| text(row, "experiment_id")).unwrap_or(""),
            "case_id": participants.first().map_or("", |row| row["case_id"].as_str().unwrap_or("")),
            "pair_id": participants.first().and_then(|row| text(row, "pair_id")),
            "baseline_attempts": baseline_ids,
            "candidate_attempts": candidate_ids,
            "baseline_result": baseline_result.map(|row| row["attempt_id"].clone()),
            "candidate_result": candidate_result.map(|row| row["attempt_id"].clone()),
            "comparable_pairs": edges.len(),
            "one_to_one": one_to_one,
            "claim_class": claim_class,
            "declared": declared,
            "evidence_complete": evidence_complete,
            "positive_effect": positive_effect,
            "effect": effect,
            "net_saving": net_saving,
            "limitations": limitations,
            "infrastructure_effect": crate::infrastructure_accounting::unit_effect(
                baseline_result,
                candidate_result,
            ),
        });
        if let Some(facts) = &subtractive {
            unit_value["subtractive"] = json!(true);
            unit_value["removed_burden"] = json!(facts.removed);
            unit_value["applicability"] = json!(facts.applicability);
            unit_value["retained_checks"] = json!(facts.retained_checks);
        }
        if model_free {
            // The method distinction is part of the unit record: the
            // evaluator consumes exactly this declaration, and the recorded
            // metrics stay inapplicable rather than becoming a measured zero.
            unit_value["method"] = json!(model_free_method);
            unit_value["model_metrics"] = json!(MODEL_METRICS_INAPPLICABLE);
            unit_value["operation_work"] = json!(operation_work);
        }
        if let Some(evidence) = reuse {
            unit_value["baseline_reused"] = json!(true);
            unit_value["baseline_executed_now"] = json!(false);
            unit_value["reuse"] = json!({
                "of": evidence["of"],
                "executed_at": evidence["executed_at"],
                "selected_at": evidence["selected_at"],
                "selection_basis": evidence["selection_basis"],
                "age_seconds": evidence["age_seconds"],
                "trace": evidence["trace"],
                "coverage": evidence["coverage"],
                "uncertainty": evidence["uncertainty"],
                "conditions": evidence["conditions"],
                "original_seconds": baseline_result.and_then(|row| number(&row["total_result_seconds"])),
                "model_metrics": evidence["model_metrics"],
                "qualification": evidence["qualification"],
            });
        }
        units.push(unit_value);
    }

    // Empirical variation across complete paired attempts, kept separate from
    // the per-unit measurement/attribution bounds above. Requests, rounds,
    // tool operations and repeated readings within one task are dependent
    // observations: they are counted here only to show that they are not
    // treated as independent replications. A group with fewer than two
    // complete pairs leaves run-to-run variation unmeasured, never zero.
    let mut variation_groups: BTreeMap<(String, String), VariationGroup> = BTreeMap::new();
    for unit in &units {
        let key = (
            unit["experiment_id"].as_str().unwrap_or("").to_owned(),
            unit["case_id"].as_str().unwrap_or("").to_owned(),
        );
        let group = variation_groups.entry(key).or_default();
        if unit["one_to_one"] == json!(true) {
            group.complete_pairs += 1;
            match (
                number(&unit["effect"]["baseline_seconds"]),
                number(&unit["effect"]["candidate_seconds"]),
            ) {
                (Some(baseline), Some(candidate)) if baseline > 0.0 => {
                    group
                        .elapsed_effects
                        .push((baseline - candidate) / baseline * 100.0);
                }
                _ => group.untimed_pairs += 1,
            }
        }
    }
    for row in &rows {
        let key = (
            text(row, "experiment_id").unwrap_or("").to_owned(),
            row["case_id"].as_str().unwrap_or("").to_owned(),
        );
        let group = variation_groups.entry(key).or_default();
        group.attempts += 1;
        group.native_runs += array(row, "native_runs")?.len() as u64;
        // A counter no run recorded contributes to the unknown tally instead
        // of entering the dependent-event sum as an implicit zero.
        for (key, sum, unknown) in [
            ("requests", &mut group.requests, &mut group.unknown_requests),
            ("rounds", &mut group.rounds, &mut group.unknown_rounds),
            (
                "tool_calls",
                &mut group.tool_calls,
                &mut group.unknown_tool_calls,
            ),
            (
                "tool_operations",
                &mut group.tool_operations,
                &mut group.unknown_tool_operations,
            ),
        ] {
            match row[key].as_u64() {
                Some(value) => *sum += value,
                None => *unknown += 1,
            }
        }
    }
    let variation: Vec<Value> = variation_groups
        .into_iter()
        .map(|((experiment_id, case_id), group)| {
            let range = (group.complete_pairs >= 2
                && group.untimed_pairs == 0
                && !group.elapsed_effects.is_empty())
            .then(|| {
                [
                    group
                        .elapsed_effects
                        .iter()
                        .copied()
                        .fold(f64::INFINITY, f64::min),
                    group
                        .elapsed_effects
                        .iter()
                        .copied()
                        .fold(f64::NEG_INFINITY, f64::max),
                ]
            });
            json!({
                "experiment_id": experiment_id,
                "case_id": case_id,
                "complete_pairs": group.complete_pairs,
                "observed_elapsed_effect_percent": range,
                "run_variation": if group.complete_pairs >= 2 { "observed-pairs" } else { "unmeasured" },
                "within_run_events": {
                    "attempts": group.attempts,
                    "native_runs": group.native_runs,
                    "requests": group.requests,
                    "rounds": group.rounds,
                    "tool_calls": group.tool_calls,
                    "tool_operations": group.tool_operations,
                    "unknown_counters": {
                        "requests": group.unknown_requests,
                        "rounds": group.unknown_rounds,
                        "tool_calls": group.unknown_tool_calls,
                        "tool_operations": group.unknown_tool_operations,
                    },
                },
                "basis": "complete one-to-one paired attempts are the experimental unit; requests, rounds, tool calls, tool operations and repeated readings within one task are dependent observations counted here as events, not replications; a counter no run recorded stays unknown and is counted under unknown_counters instead of becoming zero; a missing variance is unmeasured, never zero; this record assigns no confidence level and establishes no equivalence",
            })
        })
        .collect();

    // Complete task accounting: a task is one retry chain tip, its cost is
    // every attributable attempt (including failed attempts, workers and
    // rework), and each attempt contributes exactly once. Unknown costs stay
    // unknown; zero accepted tasks leave the per-task ratio undefined.
    let mut attributed = 0.0;
    let mut unknown_time = 0usize;
    let mut measured_usage = 0usize;
    for row in &rows {
        // The chain total (`total_verified_seconds`) repeats across every
        // attempt of one retry chain; complete accounting uses each attempt's
        // own verified seconds so retries and workers are neither lost nor
        // double counted.
        match number(&row["verified_seconds"]) {
            Some(seconds) => attributed += seconds,
            None => unknown_time += 1,
        }
        if row["usage"]["status"] == "per_run" {
            measured_usage += 1;
        }
    }
    let attempts_count = rows.len();
    let tasks: Vec<&Value> = rows
        .iter()
        .filter(|row| {
            row["attempt_id"]
                .as_str()
                .is_some_and(|id| tip_ids.contains(id))
        })
        .collect();
    let accepted_tasks = tasks
        .iter()
        .filter(|row| row["status"] == "accepted")
        .count();
    let acceptance_rate = if tasks.is_empty() {
        Value::Null
    } else {
        json!(accepted_tasks as f64 / tasks.len() as f64)
    };
    let attributed_seconds = if unknown_time == 0 {
        json!({"status": "complete", "seconds": attributed, "unknown_attempts": 0})
    } else {
        json!({"status": "partial", "seconds": null, "known_seconds": attributed, "unknown_attempts": unknown_time})
    };
    let cost_per_accepted_task = if accepted_tasks == 0 {
        json!({"status": "undefined", "seconds": null, "reason": "no accepted task"})
    } else if unknown_time > 0 {
        json!({"status": "incomplete", "seconds": null, "reason": "some attempt costs are unknown"})
    } else {
        json!({"status": "complete", "seconds": attributed / accepted_tasks as f64})
    };
    let mut case_mix: BTreeMap<(String, String), (usize, usize, usize)> = BTreeMap::new();
    for row in &rows {
        let key = (
            text(row, "experiment_id").unwrap_or("").to_owned(),
            row["case_id"].as_str().unwrap_or("").to_owned(),
        );
        let entry = case_mix.entry(key).or_default();
        entry.0 += 1;
        let is_task = row["attempt_id"]
            .as_str()
            .is_some_and(|id| tip_ids.contains(id));
        if is_task {
            entry.1 += 1;
            if row["status"] == "accepted" {
                entry.2 += 1;
            }
        }
    }
    let case_mix: Vec<Value> = case_mix
        .into_iter()
        .map(|((experiment_id, case_id), (attempts, tasks, accepted))| {
            json!({
                "experiment_id": experiment_id,
                "case_id": case_id,
                "attempts": attempts,
                "tasks": tasks,
                "accepted_tasks": accepted,
            })
        })
        .collect();
    // Retained reuse stays visible in the accounting: a reused baseline
    // attests to an earlier execution whose original cost is counted once,
    // while a refused reuse is retained for review and never supplies
    // comparable evidence.
    let reused_baseline_attempts = rows
        .iter()
        .filter(|row| row.get("reuse_evidence").is_some())
        .count();
    let reuse_refused_attempts = rows
        .iter()
        .filter(|row| row.get("reuse_refused").is_some())
        .count();
    let accounting = json!({
        "attempts": attempts_count,
        "tasks": tasks.len(),
        "accepted_tasks": accepted_tasks,
        "acceptance_rate": acceptance_rate,
        "reused_baseline_attempts": reused_baseline_attempts,
        "reuse_refused_attempts": reuse_refused_attempts,
        "attributed_seconds": attributed_seconds,
        "cost_per_accepted_task": cost_per_accepted_task,
        "coverage": {
            "attempts": attempts_count,
            "attempts_with_known_time": attempts_count - unknown_time,
            "attempts_with_unknown_time": unknown_time,
            "usage_measured_attempts": measured_usage,
            "usage_unknown_attempts": attempts_count - measured_usage,
        },
        "case_mix": case_mix,
        "basis": "Each attempt contributes its own enclosing time once; retries, workers, interventions and failed attempts are included; overlapping spans are not summed. A reused baseline contributes the original retained execution's cost exactly once and is never presented as a fresh run. Usage stays per run and is never a billing or subscription measurement.",
    });
    Ok(json!({
        "schema_version": 2,
        "attempts": rows,
        "comparisons": pairs,
        "units": units,
        "variation": variation,
        "independent_units": unit_arms.len(),
        "accounting": accounting,
        "benefit_status": "not_evaluated",
        "limitation": "Local case evidence; tokens are not billing or subscription-quota savings. Units and claim classes are eligibility evidence for a benefit decision, not demonstrated benefit.",
    }))
}

/// Schema of the `corroboration` section a summary report carries when a
/// declared adoption scope requires additional independent retained units.
pub const CORROBORATION_SCHEMA: u32 = 1;

/// Input bound on corroboration references carried by one report section,
/// mirroring the attempt bound of [`summarize_attempts`].
const MAX_CORROBORATION_REFERENCES: usize = 1024;

/// The state of the declared corroboration selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CorroborationState {
    /// Enough independent applicable replayable retained units were selected.
    Ready,
    /// Fewer units than declared: the broader claim remains unsupported.
    Inconclusive,
    /// Selection was not performed; the exact reason is recorded.
    Unavailable,
}

impl CorroborationState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Inconclusive => "inconclusive",
            Self::Unavailable => "unavailable",
        }
    }
}

/// One selected corroboration unit: identity and replay references only. A
/// fresh executor reimplements the retained task without an earlier answer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CorroborationUnitReference {
    pub owner: String,
    pub case_id: String,
    pub experiment: String,
    pub mechanism: String,
    pub conditions: String,
    /// Frozen root commit identity of the replayed snapshot.
    pub revision: String,
    /// Content digest over the frozen tree entries.
    pub tree_sha256: String,
}

/// One candidate left out of the corroboration selection, by identity and
/// exact reason.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CorroborationExclusion {
    pub owner: String,
    pub case_id: String,
    pub reason: ExclusionReason,
}

/// The report's corroboration section: the declared additional corroboration
/// requirement, the selection outcome over retained prior real tasks and the
/// identity/replay references of the selected units.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CorroborationSection {
    pub schema: u32,
    pub status: CorroborationState,
    pub ready: bool,
    /// Additional independent units the declared scope requires beyond the
    /// run's own declared plan unit.
    pub required_units: u32,
    pub units: Vec<CorroborationUnitReference>,
    pub excluded: Vec<CorroborationExclusion>,
    /// The exact selection or unavailability reason; `None` when ready.
    pub reason: Option<String>,
    /// Digest binding the section's content ([`corroboration_digest`]).
    pub digest: String,
}

/// Digest binding one corroboration section's content, so a changed selection
/// state cannot be consumed as the one an earlier decision recorded.
pub fn corroboration_digest(section: &CorroborationSection) -> io::Result<String> {
    let mut content = section.clone();
    content.digest.clear();
    let bytes = serde_json::to_vec(&content).map_err(io::Error::other)?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

/// A non-blank bounded evidence string. Identity and replay references are
/// carried verbatim; a placeholder cannot stand in for a real one.
fn evidence_text(value: &str) -> io::Result<String> {
    if value.trim().is_empty() || value.len() > 1024 {
        return Err(invalid());
    }
    Ok(value.to_owned())
}

/// Normalize and validate the driver's run-local corroboration receipt
/// (`corroboration.json`) into the report's corroboration section. The receipt
/// contract is consumed exactly: schema, selection status, declared additional
/// units, unit identity/replay references and exclusions. A selected receipt
/// must carry its selection, and a unit list that contradicts its own declared
/// requirement is refused rather than normalized, so a fabricated unit cannot
/// enter the report.
pub fn corroboration_section(receipt: &Value) -> io::Result<CorroborationSection> {
    let object = receipt.as_object().ok_or_else(invalid)?;
    for key in object.keys() {
        if !matches!(
            key.as_str(),
            "schema" | "status" | "required_units" | "selection" | "reason"
        ) {
            return Err(invalid());
        }
    }
    if object.get("schema").and_then(Value::as_u64) != Some(1) {
        return Err(invalid());
    }
    let required_units = object
        .get("required_units")
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(invalid)?;
    let reason = match object.get("reason") {
        None | Some(Value::Null) => None,
        Some(value) => Some(
            value
                .as_str()
                .filter(|text| !text.trim().is_empty() && text.len() <= 4096)
                .ok_or_else(invalid)?
                .to_owned(),
        ),
    };
    let status = object
        .get("status")
        .and_then(Value::as_str)
        .ok_or_else(invalid)?;
    let (status, ready, units, excluded, reason) = match status {
        "selected" => {
            if required_units == 0 || reason.is_some() {
                return Err(invalid());
            }
            let selection: CorroborationSelection =
                serde_json::from_value(object.get("selection").cloned().ok_or_else(invalid)?)
                    .map_err(|_| invalid())?;
            if selection.schema != EXPERIMENT_SCHEMA
                || selection.required_units != required_units
                || selection.units.len() > MAX_CORROBORATION_REFERENCES
                || selection.excluded.len() > MAX_CORROBORATION_REFERENCES
            {
                return Err(invalid());
            }
            let ready = selection.is_ready();
            if (ready && selection.units.len() != required_units as usize)
                || (!ready && selection.units.len() >= required_units as usize)
            {
                return Err(invalid());
            }
            let units = selection
                .units
                .iter()
                .map(|unit| {
                    Ok(CorroborationUnitReference {
                        owner: evidence_text(&unit.owner)?,
                        case_id: evidence_text(&unit.case_id)?,
                        experiment: evidence_text(&unit.experiment)?,
                        mechanism: evidence_text(&unit.mechanism)?,
                        conditions: evidence_text(&unit.conditions)?,
                        revision: evidence_text(&unit.revision)?,
                        tree_sha256: evidence_text(&unit.tree_sha256)?,
                    })
                })
                .collect::<io::Result<Vec<_>>>()?;
            let excluded = selection
                .excluded
                .iter()
                .map(|unit| {
                    Ok(CorroborationExclusion {
                        owner: evidence_text(&unit.owner)?,
                        case_id: evidence_text(&unit.case_id)?,
                        reason: unit.reason.clone(),
                    })
                })
                .collect::<io::Result<Vec<_>>>()?;
            let (status, reason) = match &selection.status {
                CorroborationStatus::Ready => (CorroborationState::Ready, None),
                CorroborationStatus::Inconclusive(detail) => (
                    CorroborationState::Inconclusive,
                    Some(evidence_text(detail)?),
                ),
            };
            (status, ready, units, excluded, reason)
        }
        "unavailable" => {
            if object
                .get("selection")
                .is_some_and(|value| !value.is_null())
            {
                return Err(invalid());
            }
            (
                CorroborationState::Unavailable,
                false,
                Vec::new(),
                Vec::new(),
                Some(reason.ok_or_else(invalid)?),
            )
        }
        _ => return Err(invalid()),
    };
    let section = CorroborationSection {
        schema: CORROBORATION_SCHEMA,
        status,
        ready,
        required_units,
        units,
        excluded,
        reason,
        digest: String::new(),
    };
    let digest = corroboration_digest(&section)?;
    Ok(CorroborationSection { digest, ..section })
}

/// Attach the validated corroboration section to a summary report, returning a
/// new report. An existing section is replaced; the section is bound by its own
/// digest so a changed state is detectable by the decision owner.
pub fn attach_corroboration(report: &Value, receipt: &Value) -> io::Result<Value> {
    if report.get("schema_version").and_then(Value::as_u64) != Some(2) {
        return Err(invalid());
    }
    let mut report = report.as_object().cloned().ok_or_else(invalid)?;
    report.insert(
        "corroboration".to_owned(),
        serde_json::to_value(corroboration_section(receipt)?).map_err(io::Error::other)?,
    );
    Ok(Value::Object(report))
}

/// One-line display of an evidence value; embedded newlines cannot break the
/// line-oriented rendering.
fn display_text(value: &str) -> String {
    value.replace(['\n', '\r'], " ")
}

/// The exclusion reason in the words of the selector contract.
fn exclusion_text(reason: &ExclusionReason) -> String {
    match reason {
        ExclusionReason::NotApplicable => {
            "not applicable to the declared mechanism and conditions".to_owned()
        }
        ExclusionReason::AlreadyUsed => {
            "already part of the declared plan or not independent of an earlier unit".to_owned()
        }
        ExclusionReason::NotReplayable { detail } => format!(
            "the retained copy no longer verifies as pristine: {}",
            display_text(detail)
        ),
    }
}

pub fn concise_report(report: &Value) -> io::Result<String> {
    let mut lines = vec!["Case | Arm | Attempt | Outcome | Preparation seconds | Native through checks wall seconds | Total verified seconds | Comparison exclusions".to_owned(),
        "--- | --- | --- | --- | ---: | ---: | ---: | ---".to_owned()];
    for row in array(report, "attempts")? {
        let mut reasons = strings(array(row, "excluded_reasons")?)?;
        let partners: Vec<_> = array(report, "comparisons")?
            .iter()
            .filter(|p| row["attempt_id"] == p["baseline"] || row["attempt_id"] == p["candidate"])
            .collect();
        if !partners.is_empty() && !partners.iter().any(|p| truth(&p["comparable"])) {
            for pair in partners {
                reasons.extend(strings(array(pair, "excluded_reasons")?)?);
            }
        }
        let mut values: Vec<_> = ["case_id", "arm", "attempt_id", "status"]
            .into_iter()
            .map(|k| row[k].as_str().map(str::to_owned).ok_or_else(invalid))
            .collect::<io::Result<_>>()?;
        values.extend(
            [
                "preparation_seconds",
                "total_result_seconds",
                "total_verified_seconds",
            ]
            .iter()
            .map(|k| number(&row[k]).map_or_else(|| "unknown".to_owned(), |v| format!("{v:.3}"))),
        );
        values.push(if reasons.is_empty() {
            "none".into()
        } else {
            reasons.into_iter().collect::<Vec<_>>().join(", ")
        });
        lines.push(
            values
                .iter()
                .map(|s| s.replace('|', "\\|").replace('\n', " "))
                .collect::<Vec<_>>()
                .join(" | "),
        );
    }
    // Bounded presentation keeps the middle of the evidence: every attempt row
    // and every declared unit stays visible, including failed attempts and
    // exclusion reasons. No head/tail view may replace this complete table.
    lines.push(String::new());
    lines.push(
        "Unit | Class | Comparable pairs | One-to-one | Evidence complete | Positive effect | Net saving | Limitations"
            .to_owned(),
    );
    lines.push("--- | --- | ---: | --- | --- | --- | --- | ---".to_owned());
    for unit in array(report, "units")? {
        let effect = if unit["positive_effect"].is_null() {
            "unknown".to_owned()
        } else {
            unit["positive_effect"].to_string()
        };
        let net_saving = unit["net_saving"]["verdict"]
            .as_str()
            .unwrap_or("not computed")
            .to_owned();
        let limitations = strings(array(unit, "limitations")?)?;
        let values = [
            unit["unit"].as_str().ok_or_else(invalid)?.to_owned(),
            unit["claim_class"].as_str().ok_or_else(invalid)?.to_owned(),
            unit["comparable_pairs"].to_string(),
            unit["one_to_one"].to_string(),
            unit["evidence_complete"].to_string(),
            effect,
            net_saving,
            if limitations.is_empty() {
                "none".to_owned()
            } else {
                limitations.into_iter().collect::<Vec<_>>().join(", ")
            },
        ];
        lines.push(
            values
                .iter()
                .map(|s| s.replace('|', "\\|").replace('\n', " "))
                .collect::<Vec<_>>()
                .join(" | "),
        );
    }
    // Empirical variation across complete paired attempts stays visible
    // beside the measurement bounds; within-run events are listed, never
    // aggregated into replications or confidence.
    lines.push(String::new());
    for group in array(report, "variation")? {
        let range = group["observed_elapsed_effect_percent"]
            .as_array()
            .filter(|values| values.len() == 2)
            .map(|values| {
                format!(
                    "{}..{}",
                    values[0]
                        .as_f64()
                        .map_or_else(|| "unknown".to_owned(), |value| format!("{value:.3}")),
                    values[1]
                        .as_f64()
                        .map_or_else(|| "unknown".to_owned(), |value| format!("{value:.3}")),
                )
            })
            .unwrap_or_else(|| "unmeasured".to_owned());
        lines.push(format!(
            "variation experiment={} case={} complete_pairs={} run_variation={} observed_elapsed_effect_percent={} within_run_events attempts={} native_runs={} requests={} rounds={} tool_calls={} tool_operations={}",
            group["experiment_id"].as_str().unwrap_or(""),
            group["case_id"]
                .as_str()
                .unwrap_or("")
                .replace('|', "\\|")
                .replace('\n', " "),
            group["complete_pairs"],
            group["run_variation"].as_str().unwrap_or("unmeasured"),
            range,
            group["within_run_events"]["attempts"],
            group["within_run_events"]["native_runs"],
            group["within_run_events"]["requests"],
            group["within_run_events"]["rounds"],
            group["within_run_events"]["tool_calls"],
            group["within_run_events"]["tool_operations"],
        ));
    }
    // The declared corroboration state stays visible with its evidence
    // references: a ready selection names the units a broader claim rests on,
    // and an inconclusive or unavailable selection keeps its exact reason and
    // excluded candidates instead of being replaced by a summary.
    if let Some(section) = report.get("corroboration") {
        let section: CorroborationSection =
            serde_json::from_value(section.clone()).map_err(|_| invalid())?;
        lines.push(String::new());
        lines.push(format!(
            "corroboration: status={} required_units={} units={} excluded={} digest={}",
            section.status.as_str(),
            section.required_units,
            section.units.len(),
            section.excluded.len(),
            section.digest
        ));
        for unit in &section.units {
            lines.push(format!(
                "corroboration unit: owner={} case={} experiment={} mechanism={} conditions={} revision={} tree_sha256={}",
                display_text(&unit.owner),
                display_text(&unit.case_id),
                display_text(&unit.experiment),
                display_text(&unit.mechanism),
                display_text(&unit.conditions),
                display_text(&unit.revision),
                display_text(&unit.tree_sha256),
            ));
        }
        for excluded in &section.excluded {
            lines.push(format!(
                "corroboration excluded: owner={} case={} reason={}",
                display_text(&excluded.owner),
                display_text(&excluded.case_id),
                display_text(&exclusion_text(&excluded.reason)),
            ));
        }
        if let Some(reason) = &section.reason {
            lines.push(format!("corroboration reason: {}", display_text(reason)));
        }
    }
    let accounting = &report["accounting"];
    lines.push(String::new());
    lines.push(format!(
        "accounting attempts={} tasks={} accepted_tasks={} acceptance_rate={} attributed={} cost_per_accepted_task={}",
        accounting["attempts"],
        accounting["tasks"],
        accounting["accepted_tasks"],
        accounting["acceptance_rate"]
            .as_f64()
            .map_or_else(|| "undefined".to_owned(), |value| format!("{value:.3}")),
        accounting["attributed_seconds"]["status"]
            .as_str()
            .ok_or_else(invalid)?,
        accounting["cost_per_accepted_task"]["status"]
            .as_str()
            .ok_or_else(invalid)?,
    ));
    Ok(format!(
        "{}\n\n{}\n",
        lines.join("\n"),
        report["limitation"].as_str().ok_or_else(invalid)?
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attempt(id: &str, arm: &str, start: f64, end: f64) -> Value {
        let matched: BTreeMap<_, _> = MATCH_FIELDS.iter().map(|k| (*k, "fixed")).collect();
        json!({"attempt_id":id,"case_id":"focused","arm":arm,"started_at":start,"ended_at":end,
            "discovery_verified":true,"matched":matched,
            "native_runs":[{"started_at":start,"ended_at":start+2.0,"status":"completed"}],
            "checks":[{"id":"acceptance","started_at":start+2.0,"ended_at":end,"required":true,"executed":true,"passed":true,"exit_code":0,"evidence":"private/log"}],
            "children":[],"interventions":[],"retry_of":null})
    }

    #[test]
    fn preparation_gaps_overlap_and_unknown_usage_are_preserved() {
        let mut row = attempt("a", "baseline", 100.0, 110.0);
        row["started_at"] = json!(0.0);
        row["execution_started_at"] = json!(100.0);
        row["preparation"] = json!({"started_at":0,"ended_at":5});
        row["children"] =
            json!([{"started_at":101,"ended_at":108},{"started_at":102,"ended_at":109}]);
        let result = summarize_attempts(&[row.clone()]).unwrap();
        let a = &result["attempts"][0];
        assert_eq!(a["elapsed_seconds"], 10.0);
        assert_eq!(a["preparation_seconds"], 5.0);
        assert_eq!(a["total_verified_seconds"], 15.0);
        assert_eq!(a["first_useful_seconds"], 15.0);
        assert_eq!(a["observed_wall_seconds"], 110.0);
        assert!(a["usage"]["total_tokens"].is_null());
        assert_eq!(
            wall_span(&[
                json!({"started_at":0,"ended_at":2}),
                json!({"started_at":5,"ended_at":6})
            ]),
            Some(6.0)
        );
        row["children"][0]["ended_at"] = Value::Null;
        assert!(finish_attempt(&row).unwrap()["elapsed_seconds"].is_null());
        assert_eq!(wall_span(&[]), None);
        assert_eq!(wall_span(&[json!({"started_at":false,"ended_at":1})]), None);
    }

    #[test]
    fn retry_history_and_rework_cannot_drop_required_acceptance() {
        let mut first = attempt("a", "baseline", 0.0, 10.0);
        first["checks"][0]["passed"] = false.into();
        first["checks"][0]["exit_code"] = 1.into();
        let mut second = attempt("b", "baseline", 11.0, 12.0);
        second["checks"] = json!([]);
        second["retry_of"] = "a".into();
        let mut third = attempt("c", "baseline", 14.0, 25.0);
        third["retry_of"] = "b".into();
        let result = summarize_attempts(&[first.clone(), second, third]).unwrap();
        assert_eq!(result["attempts"][0]["status"], "failed");
        assert_eq!(result["attempts"][1]["status"], "incomplete");
        assert_eq!(result["attempts"][2]["status"], "accepted");
        assert_eq!(result["attempts"][2]["total_result_seconds"], 25.0);
        assert_eq!(
            result["attempts"][2]["result_attempt_ids"],
            json!(["a", "b", "c"])
        );
        let mut weaker = first["checks"][0].clone();
        weaker["round"] = 1.into();
        weaker["id"] = "weaker".into();
        weaker["passed"] = true.into();
        weaker["exit_code"] = 0.into();
        first["checks"].as_array_mut().unwrap().push(weaker.clone());
        assert_eq!(finish_attempt(&first).unwrap()["correct"], false);
        weaker["id"] = "acceptance".into();
        first["checks"].as_array_mut().unwrap().push(weaker);
        assert_eq!(finish_attempt(&first).unwrap()["correct"], true);
        assert_eq!(
            finish_attempt(&first).unwrap()["checks"]
                .as_array()
                .unwrap()
                .len(),
            3
        );
    }

    #[test]
    fn tool_operation_and_round_counters_stay_scoped_and_unknown() {
        let mut row = attempt("a", "baseline", 0.0, 10.0);
        row["native_runs"][0]["tool_operations"] = json!(3);
        row["native_runs"][0]["rounds"] = json!(1);
        let result = summarize_attempts(&[row.clone()]).unwrap();
        assert_eq!(result["attempts"][0]["tool_operations"], 3);
        assert_eq!(result["attempts"][0]["rounds"], 1);
        assert_eq!(result["attempts"][0]["total_tool_operations"], 3);
        assert_eq!(result["attempts"][0]["total_rounds"], 1);

        // An unrecorded counter is unknown, not assumed, and the counters are
        // independent of each other.
        let mut partial = attempt("b", "baseline", 0.0, 10.0);
        partial["native_runs"][0]["tool_operations"] = json!(2);
        let result = summarize_attempts(&[partial]).unwrap();
        assert_eq!(result["attempts"][0]["tool_operations"], 2);
        assert!(result["attempts"][0]["rounds"].is_null());
        assert!(result["attempts"][0]["total_rounds"].is_null());

        // Retried work stays accounted: the chain total includes every
        // recorded attempt once, and an unknown counter keeps it unknown.
        let mut retry = attempt("c", "baseline", 11.0, 20.0);
        retry["retry_of"] = "a".into();
        retry["native_runs"][0]["tool_operations"] = json!(2);
        retry["native_runs"][0]["rounds"] = json!(1);
        let result = summarize_attempts(&[row, retry]).unwrap();
        let tip = &result["attempts"][1];
        assert_eq!(tip["total_tool_operations"], 5);
        assert_eq!(tip["total_rounds"], 2);
        assert_eq!(tip["result_attempt_ids"], json!(["a", "c"]));

        let mut unobserved = attempt("d", "baseline", 0.0, 10.0);
        unobserved["native_runs"] = json!([]);
        let result = summarize_attempts(&[unobserved]).unwrap();
        assert!(result["attempts"][0]["tool_operations"].is_null());
        assert!(result["attempts"][0]["total_tool_operations"].is_null());
    }

    #[test]
    fn acceptance_corrections_are_retained_once_in_the_enclosing_span() {
        let mut row = attempt("a", "baseline", 0.0, 9.0);
        row["native_runs"][0]["ended_at"] = json!(2.0);
        row["checks"] = json!([
            {"id":"acceptance","round":0,"started_at":2.0,"ended_at":4.0,"required":true,
                "executed":true,"passed":false,"exit_code":1,"evidence":"private/first"},
            {"id":"acceptance","round":1,"started_at":5.0,"ended_at":9.0,"required":true,
                "executed":true,"passed":true,"exit_code":0,"evidence":"private/fixed"}
        ]);
        let result = summarize_attempts(&[row.clone()]).unwrap();
        let a = &result["attempts"][0];
        // The failed check, the correction gap and the re-check are one
        // enclosing attempt span: 9 seconds, not the 2 + 2 + 4 sum.
        assert_eq!(a["elapsed_seconds"], 9.0);
        assert_eq!(a["verified_seconds"], 9.0);
        // The first executed acceptance result is the first useful signal,
        // recorded before the correction passes.
        assert_eq!(a["first_useful_seconds"], 4.0);
        assert_eq!(a["status"], "accepted");
        assert_eq!(a["checks"].as_array().unwrap().len(), 2);
        // A final round that never executed cannot establish acceptance, and
        // its failure does not discard the retained span.
        row["checks"][1]["executed"] = json!(false);
        let unresolved = finish_attempt(&row).unwrap();
        assert_eq!(unresolved["status"], "incomplete");
        assert_eq!(unresolved["elapsed_seconds"], 9.0);
    }

    #[test]
    fn batched_calls_keep_call_and_operation_counts_distinct() {
        let mut baseline = attempt("a", "baseline", 0.0, 10.0);
        baseline["native_runs"][0]["requests"] = json!(4);
        baseline["native_runs"][0]["rounds"] = json!(3);
        baseline["native_runs"][0]["tool_calls"] = json!(3);
        baseline["native_runs"][0]["tool_operations"] = json!(5);
        let mut candidate = attempt("b", "candidate", 0.0, 10.0);
        // One batched outer call performed the same five operations.
        candidate["native_runs"][0]["requests"] = json!(2);
        candidate["native_runs"][0]["rounds"] = json!(2);
        candidate["native_runs"][0]["tool_calls"] = json!(1);
        candidate["native_runs"][0]["tool_operations"] = json!(5);
        let result = summarize_attempts(&[baseline.clone(), candidate]).unwrap();
        let a = &result["attempts"][0];
        assert_eq!(a["requests"], 4);
        assert_eq!(a["rounds"], 3);
        assert_eq!(a["tool_calls"], 3);
        assert_eq!(a["tool_operations"], 5);
        assert_eq!(a["total_requests"], 4);
        assert_eq!(a["total_tool_calls"], 3);
        let b = &result["attempts"][1];
        assert_eq!(b["tool_calls"], 1);
        assert_eq!(b["tool_operations"], 5);
        // Dependent events are not cost: the lower batched call count creates
        // no saving or acceptance verdict on its own.
        let unit = &result["units"][0];
        assert!(unit["net_saving"].is_null());
        assert!(unit["positive_effect"].is_null());
        assert_eq!(unit["effect"]["delta_seconds"], 0.0);
        // A counter no run recorded stays unknown and is disclosed, never
        // counted as zero.
        let mut partial = attempt("c", "baseline", 0.0, 10.0);
        partial["native_runs"][0]["tool_operations"] = json!(5);
        let result = summarize_attempts(&[partial]).unwrap();
        assert!(result["attempts"][0]["requests"].is_null());
        assert!(result["attempts"][0]["total_requests"].is_null());
        let group = &result["variation"][0];
        assert_eq!(group["within_run_events"]["requests"], 0);
        assert_eq!(
            group["within_run_events"]["unknown_counters"]["requests"],
            1
        );
        assert_eq!(group["within_run_events"]["tool_operations"], 5);
        assert_eq!(
            group["within_run_events"]["unknown_counters"]["tool_operations"],
            0
        );
        // Retried work keeps every recorded chain counter once.
        let mut retry = attempt("d", "baseline", 11.0, 20.0);
        retry["retry_of"] = "a".into();
        retry["native_runs"][0]["requests"] = json!(1);
        retry["native_runs"][0]["rounds"] = json!(1);
        retry["native_runs"][0]["tool_calls"] = json!(2);
        retry["native_runs"][0]["tool_operations"] = json!(2);
        let result = summarize_attempts(&[baseline.clone(), retry]).unwrap();
        let tip = &result["attempts"][1];
        assert_eq!(tip["total_requests"], 5);
        assert_eq!(tip["total_rounds"], 4);
        assert_eq!(tip["total_tool_calls"], 5);
        assert_eq!(tip["total_tool_operations"], 7);
    }

    #[test]
    fn cancelled_work_keeps_recorded_status_time_and_usage() {
        let mut row = attempt("a", "baseline", 0.0, 10.0);
        row["status"] = json!("cancelled");
        row["checks"] = json!([]);
        row["native_runs"][0]["usage"] = json!({"thread":"thread-a","total_tokens":17});
        let result = summarize_attempts(&[row]).unwrap();
        let a = &result["attempts"][0];
        assert_eq!(a["status"], "cancelled");
        assert_eq!(a["correct"], false);
        // Observed time and usage remain attributed to the cancelled attempt.
        assert_eq!(a["elapsed_seconds"], 10.0);
        assert_eq!(a["observed_wall_seconds"], 10.0);
        assert_eq!(a["usage"]["status"], "per_run");
        assert_eq!(a["usage"]["runs"][0]["total_tokens"], 17);
        assert!(
            a["excluded_reasons"]
                .as_array()
                .unwrap()
                .iter()
                .any(|reason| reason == "outcome_cancelled")
        );
        // The cancellation stays in the task accounting as unsuccessful work.
        assert_eq!(result["accounting"]["attempts"], 1);
        assert_eq!(result["accounting"]["accepted_tasks"], 0);
        assert_eq!(
            result["accounting"]["cost_per_accepted_task"]["status"],
            "undefined"
        );
        assert_eq!(result["accounting"]["attributed_seconds"]["seconds"], 10.0);
    }

    #[test]
    fn every_material_field_and_unknown_or_unverified_input_excludes() {
        let a = attempt("a", "baseline", 0.0, 10.0);
        let b = attempt("b", "candidate", 0.0, 10.0);
        assert!(comparison_reasons(&a, &b).unwrap().is_empty());
        for field in MATCH_FIELDS {
            let mut changed = b.clone();
            changed["matched"][field] = "changed".into();
            assert!(
                comparison_reasons(&a, &changed)
                    .unwrap()
                    .contains(&format!("mismatch:{field}"))
            );
            changed["matched"].as_object_mut().unwrap().remove(*field);
            assert!(
                comparison_reasons(&a, &changed)
                    .unwrap()
                    .contains(&format!("unknown:{field}"))
            );
        }
        let mut changed = b.clone();
        changed["matched"]["role_availability"] = "unavailable".into();
        let result = summarize_attempts(&[a.clone(), changed]).unwrap();
        assert!(
            concise_report(&result)
                .unwrap()
                .contains("unknown:role_availability")
        );
        let incremental = summarize_attempts(&[a]).unwrap()["attempts"][0].clone();
        assert_eq!(
            summarize_attempts(&[incremental, b]).unwrap()["comparisons"][0]["comparable"],
            true
        );
        assert_eq!(result["benefit_status"], "not_evaluated");
    }

    #[test]
    fn broken_retry_and_malformed_input_never_lose_attempts_or_expose_contents() {
        let a = attempt("a", "baseline", 0.0, 10.0);
        for retry in ["missing", "a"] {
            let mut changed = a.clone();
            changed["retry_of"] = retry.into();
            let report = summarize_attempts(&[changed]).unwrap();
            assert!(report["attempts"][0]["total_result_seconds"].is_null());
        }
        assert!(summarize_attempts(&[a.clone(), a.clone()]).is_err());
        let mut changed = a.clone();
        changed["checks"] = "private-contents".into();
        assert!(
            !finish_attempt(&changed)
                .unwrap_err()
                .to_string()
                .contains("private-contents")
        );
        changed = a;
        changed["checks"][0]["exit_code"] = true.into();
        assert_eq!(finish_attempt(&changed).unwrap()["correct"], false);
    }

    #[test]
    fn completion_discovery_and_usage_are_not_inferred_from_prose_or_totals() {
        let mut a = attempt("a", "baseline", 0.0, 10.0);
        let mut unsupported_claim = a.clone();
        unsupported_claim["status"] = "accepted".into();
        unsupported_claim["checks"] = json!([]);
        assert_eq!(
            finish_attempt(&unsupported_claim).unwrap()["status"],
            "incomplete"
        );
        a["native_runs"][0]["status"] = "running".into();
        assert_eq!(finish_attempt(&a).unwrap()["status"], "incomplete");
        a["native_runs"][0]["status"] = "completed".into();
        a["native_runs"][0]["first_useful_signal"] =
            json!({"at":3,"evidence":"private/tool","kind":"tool_result"});
        a["native_runs"][0]["usage"] = json!({"thread":"same-thread","total_tokens":123});
        let duplicate = a["native_runs"][0].clone();
        a["native_runs"].as_array_mut().unwrap().push(duplicate);
        let mut b = attempt("b", "candidate", 0.0, 10.0);
        b["discovery_verified"] = false.into();
        let report = summarize_attempts(&[a, b]).unwrap();
        assert_eq!(report["attempts"][0]["first_useful_seconds"], 3.0);
        let usage = &report["attempts"][0]["usage"];
        assert_eq!(usage["status"], "per_run");
        assert_eq!(usage["runs"].as_array().unwrap().len(), 2);
        assert!(usage.get("total_tokens").is_none());
        assert_eq!(report["comparisons"][0]["comparable"], false);
        assert!(
            concise_report(&report)
                .unwrap()
                .contains("discovery_unverified")
        );
    }

    #[test]
    fn retry_identity_and_extended_verification_remain_part_of_result() {
        let mut first = attempt("a", "baseline", 0.0, 10.0);
        first["children"] = json!([{"started_at":5,"ended_at":30}]);
        let mut retry = attempt("b", "baseline", 14.0, 25.0);
        retry["retry_of"] = "a".into();
        let report = summarize_attempts(&[first.clone(), retry.clone()]).unwrap();
        assert_eq!(report["attempts"][1]["total_result_seconds"], 30.0);
        first["arm"] = "candidate".into();
        let report = summarize_attempts(&[first, retry]).unwrap();
        assert!(report["attempts"][1]["total_result_seconds"].is_null());
        assert!(
            report["attempts"][1]["excluded_reasons"]
                .as_array()
                .unwrap()
                .contains(&json!("retry_identity_mismatch"))
        );
        assert!(summarize_attempts(&vec![Value::Null; 1025]).is_err());
    }

    #[test]
    fn a_model_free_attempt_is_never_a_measured_zero_or_a_shortcut() {
        let mut baseline = attempt("mf-b", "baseline", 0.0, 10.0);
        let mut candidate = attempt("mf-c", "candidate", 0.0, 8.0);
        for row in [&mut baseline, &mut candidate] {
            row["method"] = json!("real-operation");
            row["model_calls"] = json!(0);
            row["model_metrics"] = json!(MODEL_METRICS_INAPPLICABLE);
            row["native_runs"][0]["elapsed_seconds"] = json!(2.0);
            row["native_runs"][0]["exit_code"] = json!(0);
            row["native_runs"][0]["evidence"] = json!("private/operation-receipt.json");
        }
        let report = summarize_attempts(&[baseline.clone(), candidate]).unwrap();
        assert_eq!(
            report["comparisons"][0]["comparable"], true,
            "{}",
            report["comparisons"][0]
        );
        let unit = &report["units"][0];
        assert_eq!(unit["method"], "real-operation", "{unit}");
        assert_eq!(unit["model_metrics"], MODEL_METRICS_INAPPLICABLE, "{unit}");
        assert_eq!(unit["operation_work"], true, "{unit}");
        let finished = &report["attempts"][0];
        assert!(model_free_attempt(finished));
        assert_eq!(finished["requests"], Value::Null);
        assert_eq!(finished["usage"]["status"], "unknown");

        // Measured model work contradicts an inapplicable claim, and a
        // numeric metric is not an inapplicable one.
        let mut measured = baseline.clone();
        measured["native_runs"][0]["rounds"] = json!(1);
        assert!(!model_free_attempt(&measured));
        let mut called = baseline.clone();
        called["model_calls"] = json!(1);
        assert!(!model_free_attempt(&called));
        let mut numeric = baseline.clone();
        numeric["model_metrics"] = json!(0);
        assert!(!model_free_attempt(&numeric));
        // The operation's own measured work needs the recorded exit and
        // duration of the retained execution.
        let mut missing_exit = baseline;
        missing_exit["native_runs"][0]
            .as_object_mut()
            .unwrap()
            .remove("exit_code");
        assert!(!operation_work_recorded(&missing_exit));
    }

    fn declared(objective: &str) -> Value {
        json!({
            "task_mix": "focused and freshness cases",
            "objective": objective,
            "effect_percent": 5.0,
            "nuisance": ["declared order"],
            "stopping": "fixed number of attempts",
            "uncertainty": "reported at the declared unit",
        })
    }

    fn classified_pair(
        prefix: &str,
        declaration: Option<Value>,
        candidate_end: f64,
        metadata: bool,
    ) -> Vec<Value> {
        let mut baseline = attempt(&format!("{prefix}-b"), "baseline", 0.0, 10.0);
        let mut candidate = attempt(&format!("{prefix}-c"), "candidate", 0.0, candidate_end);
        candidate["ended_at"] = json!(candidate_end);
        candidate["checks"][0]["ended_at"] = json!(candidate_end);
        if let Some(declaration) = declaration {
            baseline["declaration"] = declaration.clone();
            candidate["declaration"] = declaration;
        }
        if metadata {
            baseline["observed_model_metadata_verified"] = json!(true);
            candidate["observed_model_metadata_verified"] = json!(true);
        }
        vec![baseline, candidate]
    }

    #[test]
    fn cartesian_edges_from_one_unit_are_not_independent_samples() {
        let rows = vec![
            attempt("b1", "baseline", 0.0, 10.0),
            attempt("b2", "baseline", 0.0, 10.0),
            attempt("c1", "candidate", 0.0, 10.0),
            attempt("c2", "candidate", 0.0, 10.0),
            attempt("c3", "candidate", 0.0, 10.0),
        ];
        let report = summarize_attempts(&rows).unwrap();
        assert_eq!(report["comparisons"].as_array().unwrap().len(), 6);
        assert_eq!(
            report["independent_units"], 1,
            "six edges from one case are one declared unit, not six samples"
        );
        assert!(
            report["comparisons"]
                .as_array()
                .unwrap()
                .iter()
                .all(|pair| pair["independent"] == false),
            "Cartesian edges are descriptive, not independent repetitions"
        );
        let unit = &report["units"][0];
        assert_eq!(unit["unit"], "/focused");
        assert_eq!(unit["comparable_pairs"], 6);
        assert_eq!(unit["one_to_one"], false);
        assert!(unit["limitations"].as_array().unwrap().contains(&json!(
            "repeated attempts within the declared unit: aggregate before inference"
        )));
        assert_eq!(report["accounting"]["tasks"], 5);
    }

    #[test]
    fn declared_pair_identity_is_confirmed_and_retry_identity_must_match() {
        let mut baseline = attempt("b", "baseline", 0.0, 10.0);
        baseline["experiment_id"] = json!("exp-1");
        baseline["pair_id"] = json!("pair-1");
        let mut candidate = attempt("c", "candidate", 0.0, 10.0);
        candidate["experiment_id"] = json!("exp-1");
        candidate["pair_id"] = json!("pair-1");
        let report = summarize_attempts(&[baseline.clone(), candidate.clone()]).unwrap();
        assert_eq!(report["independent_units"], 1);
        assert_eq!(report["units"][0]["unit"], "exp-1/focused/pair-1");
        assert_eq!(report["comparisons"][0]["independent"], true);

        // A retry that changes the declared unit identity is not part of the
        // chain and cannot borrow the original task's identities.
        let mut retry = attempt("c2", "candidate", 12.0, 20.0);
        retry["retry_of"] = json!("c");
        retry["experiment_id"] = json!("exp-1");
        retry["pair_id"] = json!("pair-2");
        let report = summarize_attempts(&[baseline, candidate, retry]).unwrap();
        let row = report["attempts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["attempt_id"] == "c2")
            .unwrap();
        assert!(
            row["excluded_reasons"]
                .as_array()
                .unwrap()
                .contains(&json!("retry_identity_mismatch"))
        );
        assert!(row["total_result_seconds"].is_null());
        assert_eq!(report["independent_units"], 1);
    }

    #[test]
    fn stale_or_blank_input_identities_do_not_authorize_a_comparable_pair() {
        let a = attempt("a", "baseline", 0.0, 10.0);
        let b = attempt("b", "candidate", 0.0, 10.0);
        for placeholder in ["", "   ", "unknown", "UNKNOWN"] {
            let mut left = a.clone();
            let mut right = b.clone();
            left["matched"]["input_identity"] = json!(placeholder);
            right["matched"]["input_identity"] = json!(placeholder);
            let report = summarize_attempts(&[left, right]).unwrap();
            assert_eq!(
                report["comparisons"][0]["comparable"], false,
                "{placeholder:?} is a stale placeholder on both arms"
            );
            assert!(
                report["comparisons"][0]["excluded_reasons"]
                    .as_array()
                    .unwrap()
                    .contains(&json!("unknown:input_identity"))
            );
        }
        for field in [
            "oracle_identity",
            "input_identity",
            "instructions_identity",
            "model",
            "effort",
        ] {
            let mut right = b.clone();
            right["matched"][field] = json!("changed");
            let report = summarize_attempts(&[a.clone(), right]).unwrap();
            assert!(
                report["comparisons"][0]["excluded_reasons"]
                    .as_array()
                    .unwrap()
                    .contains(&json!(format!("mismatch:{field}"))),
                "{field}"
            );
        }
        let mut left = a.clone();
        left["case_id"] = json!("");
        let mut right = b;
        right["case_id"] = json!("");
        let report = summarize_attempts(&[left, right]).unwrap();
        assert!(
            report["comparisons"][0]["excluded_reasons"]
                .as_array()
                .unwrap()
                .contains(&json!("unknown:case_id"))
        );
    }

    #[test]
    fn worker_usage_is_attributed_once_and_inherited_summaries_are_not_double_counted() {
        let mut a = attempt("a", "baseline", 0.0, 10.0);
        a["children"] = json!([
            {"started_at": 2.5, "ended_at": 4.0, "usage": {"thread": "worker-1", "total_tokens": 11}},
            {"started_at": 4.0, "ended_at": 9.0, "usage": {"thread": "worker-2", "total_tokens": 13}},
        ]);
        let b = attempt("b", "candidate", 0.0, 10.0);
        let report = summarize_attempts(&[a.clone(), b.clone()]).unwrap();
        let usage = &report["attempts"][0]["usage"];
        assert_eq!(usage["status"], "per_run");
        assert_eq!(usage["workers"].as_array().unwrap().len(), 2);
        assert!(usage["total_tokens"].is_null(), "no invented total");
        assert_eq!(
            report["attempts"][0]["elapsed_seconds"], 10.0,
            "overlapping worker spans remain one enclosing wall span"
        );

        let mut parent = attempt("a", "baseline", 0.0, 10.0);
        parent["children"] = json!([
            {"attempt_id": "b", "started_at": 0.0, "ended_at": 10.0, "usage": {"thread": "thread-b", "total_tokens": 27}},
        ]);
        let report = summarize_attempts(&[parent.clone(), b.clone()]).unwrap();
        let usage = &report["attempts"][0]["usage"];
        assert_eq!(usage["status"], "unknown");
        assert_eq!(usage["inherited_children"], 1);
        assert_eq!(
            report["attempts"][0]["elapsed_seconds"], 10.0,
            "the inherited span is counted by the referenced attempt"
        );
        assert_eq!(report["accounting"]["attempts"], 2);

        parent["children"] =
            json!([{"attempt_id": "missing", "started_at": 0.0, "ended_at": 10.0}]);
        assert!(
            summarize_attempts(&[parent, b]).is_err(),
            "a dangling inherited reference is invalid, not silently dropped"
        );
    }

    #[test]
    fn task_accounting_is_complete_and_zero_acceptance_is_undefined() {
        let a = attempt("a", "baseline", 0.0, 10.0);
        let b = attempt("b", "candidate", 0.0, 10.0);
        let mut failed = attempt("c", "candidate", 20.0, 30.0);
        failed["checks"][0]["passed"] = false.into();
        failed["checks"][0]["exit_code"] = 1.into();
        let report = summarize_attempts(&[a, b.clone(), failed]).unwrap();
        let accounting = &report["accounting"];
        assert_eq!(accounting["attempts"], 3);
        assert_eq!(accounting["tasks"], 3);
        assert_eq!(accounting["accepted_tasks"], 2);
        assert_eq!(accounting["acceptance_rate"], json!(2.0 / 3.0));
        assert_eq!(accounting["attributed_seconds"]["status"], "complete");
        assert_eq!(accounting["attributed_seconds"]["seconds"], 30.0);
        assert_eq!(accounting["cost_per_accepted_task"]["status"], "complete");
        assert_eq!(accounting["cost_per_accepted_task"]["seconds"], 15.0);
        assert_eq!(accounting["coverage"]["usage_unknown_attempts"], 3);
        assert_eq!(accounting["case_mix"][0]["attempts"], 3);

        // A retry chain contributes each attempt's own time once; its chain
        // total is never added again.
        let mut first = attempt("p", "baseline", 0.0, 10.0);
        first["checks"][0]["passed"] = false.into();
        first["checks"][0]["exit_code"] = 1.into();
        let mut second = attempt("q", "baseline", 11.0, 20.0);
        second["retry_of"] = json!("p");
        let report = summarize_attempts(&[first, second]).unwrap();
        assert_eq!(report["accounting"]["tasks"], 1);
        assert_eq!(report["accounting"]["attributed_seconds"]["seconds"], 19.0);
        assert_eq!(report["accounting"]["accepted_tasks"], 1);

        // Every attempted task fails: the ratio is undefined, not optimal.
        let mut x = attempt("x", "baseline", 0.0, 10.0);
        x["checks"][0]["passed"] = false.into();
        x["checks"][0]["exit_code"] = 1.into();
        let report = summarize_attempts(&[x]).unwrap();
        assert_eq!(report["accounting"]["accepted_tasks"], 0);
        assert_eq!(report["accounting"]["acceptance_rate"], 0.0);
        assert_eq!(
            report["accounting"]["cost_per_accepted_task"]["status"],
            "undefined"
        );
        assert!(report["accounting"]["cost_per_accepted_task"]["seconds"].is_null());

        // An unknown cost keeps the ratio incomplete instead of understating it.
        let mut unknown = attempt("u", "baseline", 0.0, 10.0);
        unknown["children"] = json!([{"started_at": 1.0, "ended_at": null}]);
        let report = summarize_attempts(&[unknown, b]).unwrap();
        assert_eq!(
            report["accounting"]["attributed_seconds"]["status"],
            "partial"
        );
        assert!(report["accounting"]["attributed_seconds"]["seconds"].is_null());
        assert_eq!(
            report["accounting"]["cost_per_accepted_task"]["status"],
            "incomplete"
        );
    }

    #[test]
    fn claim_classes_separate_quality_stochastic_and_unmeasured_subscriptions() {
        let report =
            summarize_attempts(&classified_pair("q", Some(declared("quality")), 10.0, true))
                .unwrap();
        let unit = &report["units"][0];
        assert_eq!(unit["claim_class"], "deterministic_quality");
        assert_eq!(unit["evidence_complete"], true);
        assert_eq!(unit["positive_effect"], false);
        assert!(unit["limitations"].as_array().unwrap().contains(&json!(
            "both arms accepted: no quality difference is recorded"
        )));

        let mut declaration = declared("time");
        declaration["effect_percent"] = json!(5.0);
        declaration["horizon_tasks"] = json!(10.0);
        declaration["costs"] = json!({
            "implementation_seconds": 1.0,
            "evaluation_seconds": 1.0,
            "maintenance_seconds_per_task": 0.0,
        });
        let report =
            summarize_attempts(&classified_pair("t", Some(declaration.clone()), 8.0, true))
                .unwrap();
        let unit = &report["units"][0];
        assert_eq!(unit["claim_class"], "stochastic_savings");
        assert_eq!(unit["evidence_complete"], true);
        assert_eq!(unit["positive_effect"], true);
        assert_eq!(unit["effect"]["delta_seconds"], -2.0);
        assert_eq!(unit["net_saving"]["verdict"], "net_saving");
        assert_eq!(unit["net_saving"]["seconds"], 18.0);

        // Small savings do not repay evaluation over the declared horizon.
        declaration["horizon_tasks"] = json!(1.0);
        let report =
            summarize_attempts(&classified_pair("t2", Some(declaration), 9.5, true)).unwrap();
        let unit = &report["units"][0];
        assert_eq!(unit["positive_effect"], true);
        assert_eq!(unit["net_saving"]["verdict"], "does_not_repay");
        assert!(unit["limitations"].as_array().unwrap().contains(&json!(
            "declared per-task saving does not repay evaluation over the declared horizon"
        )));

        // A subscription claim is classified separately and never eligible.
        let report = summarize_attempts(&classified_pair(
            "s",
            Some(declared("subscription")),
            8.0,
            true,
        ))
        .unwrap();
        let unit = &report["units"][0];
        assert_eq!(unit["claim_class"], "unmeasured_subscription");
        assert_eq!(unit["evidence_complete"], false);
        assert!(unit["positive_effect"].is_null());
        assert!(unit["limitations"].as_array().unwrap().contains(&json!(
            "subscription allowance is not measurable from token or byte totals"
        )));

        // No predeclared plan: classified as undeclared, never eligible.
        let report = summarize_attempts(&classified_pair("u", None, 8.0, true)).unwrap();
        assert_eq!(report["units"][0]["claim_class"], "undeclared");
        assert_eq!(report["units"][0]["evidence_complete"], false);

        // Absent model-visible metadata leaves the evidence incomplete.
        let mut declaration = declared("time");
        declaration["effect_percent"] = json!(1.0);
        let report =
            summarize_attempts(&classified_pair("m", Some(declaration), 8.0, false)).unwrap();
        let unit = &report["units"][0];
        assert_eq!(unit["evidence_complete"], false);
        assert!(
            unit["limitations"]
                .as_array()
                .unwrap()
                .contains(&json!("model metadata not verified on both arms"))
        );
        assert_eq!(report["benefit_status"], "not_evaluated");
    }

    #[test]
    fn owning_boundaries_reject_the_specific_invalid_transitions() {
        // Validated snapshot: a claimed accepted status without executed final
        // acceptance is re-validated to incomplete instead of being trusted.
        let mut claimed = attempt("claimed", "baseline", 0.0, 10.0);
        claimed["status"] = json!("accepted");
        claimed["checks"] = json!([{
            "id": "acceptance",
            "required": true,
            "executed": false,
            "passed": true,
            "exit_code": 0
        }]);
        assert_eq!(finish_attempt(&claimed).unwrap()["status"], "incomplete");

        // Committed observation: failed attempts and their retry history stay
        // in the report; a later success does not erase them.
        let mut first = attempt("a", "baseline", 0.0, 10.0);
        first["checks"][0]["passed"] = false.into();
        first["checks"][0]["exit_code"] = 1.into();
        let mut retry = attempt("b", "baseline", 11.0, 20.0);
        retry["retry_of"] = json!("a");
        let report = summarize_attempts(&[first, retry]).unwrap();
        assert_eq!(report["attempts"].as_array().unwrap().len(), 2);
        assert_eq!(
            report["attempts"][1]["result_attempt_ids"],
            json!(["a", "b"])
        );
        assert_eq!(
            report["accounting"]["tasks"], 1,
            "one retried task, not two accepted tasks"
        );
        assert_eq!(report["accounting"]["accepted_tasks"], 1);

        // Prepared presentation: the retained table keeps the middle failing
        // attempt visible and does not upgrade the benefit status.
        let mut middle = attempt("m", "candidate", 20.0, 30.0);
        middle["checks"][0]["passed"] = false.into();
        middle["checks"][0]["exit_code"] = 1.into();
        let report = summarize_attempts(&[
            attempt("left", "baseline", 0.0, 10.0),
            attempt("right", "candidate", 0.0, 10.0),
            middle,
        ])
        .unwrap();
        let text = concise_report(&report).unwrap();
        assert!(text.contains("candidate | m | failed"), "{text}");
        assert_eq!(report["benefit_status"], "not_evaluated");

        // Verified runtime: a running native run keeps the attempt unaccepted,
        // and absent model-visible metadata leaves the unit incomplete.
        let mut running = attempt("r", "baseline", 0.0, 10.0);
        running["native_runs"][0]["status"] = json!("running");
        assert_eq!(finish_attempt(&running).unwrap()["status"], "incomplete");
        let report = summarize_attempts(&classified_pair("v", None, 8.0, false)).unwrap();
        assert_eq!(report["units"][0]["evidence_complete"], false);
    }

    #[test]
    fn no_migration_and_no_blind_pruning_selections_are_recorded_with_counterexamples() {
        use crate::benefit_gate;
        // Reviewed selections (design decision 12): the audited upstream
        // harness is a developer preview with no demonstrated local benefit,
        // so no runtime migration is selected; blind head/tail result pruning
        // can remove useful middle content and invalidate cache reuse, so no
        // blind pruning default is selected. Both decisions are durable here
        // and in the change's design decision 12.
        let rejected = benefit_gate::parse_gate_comments(&[
            "benefit-gate v1 item=optional-runtime-migration outcome=reject quality=unmeasurable matched=1 tolerance_percent=10.0 baseline_seconds=100.0 candidate_seconds=100.0 regression_percent=0.0 baseline=current candidate=migrated accounting=check+coordination+rework detail=developer preview compatibility risk; no local demonstrated benefit".to_owned(),
            "benefit-gate v1 item=blind-head-tail-pruning outcome=reject quality=unmeasurable matched=2 tolerance_percent=10.0 baseline_seconds=100.0 candidate_seconds=95.0 regression_percent=-5.0 baseline=retained candidate=pruned accounting=check+coordination+rework detail=middle diagnostics and cache reuse are lost".to_owned(),
        ]);
        assert!(!benefit_gate::default_allowed(
            &rejected,
            "optional-runtime-migration"
        ));
        assert!(!benefit_gate::default_allowed(
            &rejected,
            "blind-head-tail-pruning"
        ));

        // Counterexample 1: even a fully formed adoption record with no
        // positive effect cannot install the migration default.
        let attempted = benefit_gate::parse_gate_comments(&[
            "benefit-gate v1 item=optional-runtime-migration outcome=adopt quality=unchanged matched=2 tolerance_percent=10.0 baseline_seconds=100.0 candidate_seconds=100.0 regression_percent=0.0 baseline=current candidate=migrated accounting=check+coordination+rework detail=consistent arithmetic only".to_owned(),
        ]);
        let assessment = benefit_gate::assess(&attempted, "optional-runtime-migration").unwrap();
        assert_eq!(assessment.verdict, benefit_gate::Verdict::Unsupported);
        assert_eq!(
            assessment.limitations,
            vec![benefit_gate::Limitation::NoPositiveEffect]
        );

        // Counterexample 2: a blind head/tail view would drop the middle
        // failing diagnostic that the retained presentation keeps.
        let mut middle = attempt("m", "candidate", 20.0, 30.0);
        middle["checks"][0]["passed"] = false.into();
        middle["checks"][0]["exit_code"] = 1.into();
        let report = summarize_attempts(&[
            attempt("left", "baseline", 0.0, 10.0),
            attempt("right", "candidate", 0.0, 10.0),
            middle,
        ])
        .unwrap();
        let text = concise_report(&report).unwrap();
        assert!(text.contains("candidate | m | failed"), "{text}");
        let blind: Vec<&str> = text
            .lines()
            .take(2)
            .chain(text.lines().rev().take(2))
            .collect();
        assert!(
            !blind.join("\n").contains("candidate | m | failed"),
            "a blind head/tail view loses the middle diagnostic"
        );
    }
}
