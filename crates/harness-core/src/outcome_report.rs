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
use serde_json::{Value, json};
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

/// Retains all input fields, failed checks and native runs. Only executed final
/// round acceptance plus completed native work can establish correctness.
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
            .filter(|status| matches!(*status, "failed" | "blocked" | "timeout"))
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
    row["tool_operations"] = counter_total(native, "tool_operations");
    row["rounds"] = counter_total(native, "rounds");
    row["unit"] = json!(unit_of(record));
    row["excluded_reasons"] = json!(reasons);
    crate::infrastructure_accounting::attach(&mut row);
    Ok(row)
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
    for key in keys {
        match (a.get(key), b.get(key)) {
            (Some(x), Some(y)) if identity_known(x) && identity_known(y) => {
                if x != y {
                    reasons.insert(format!("mismatch:{key}"));
                }
            }
            _ => {
                reasons.insert(format!("unknown:{key}"));
            }
        }
    }
    for row in [left, right] {
        reasons.extend(strings(array(row, "excluded_reasons")?)?);
        if !truth(&row["discovery_verified"]) {
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
    effect_percent: Option<f64>,
    nuisance: bool,
    stopping: Option<String>,
    uncertainty: Option<String>,
    horizon_tasks: Option<f64>,
    costs: Option<(f64, f64, f64)>,
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
        rows[index]["total_tool_operations"] = total_tool_operations;
        rows[index]["total_rounds"] = total_rounds;
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
            let verified = baseline_result.is_some_and(|row| {
                truth(&row["discovery_verified"]) && truth(&row["observed_model_metadata_verified"])
            }) && candidate_result.is_some_and(|row| {
                truth(&row["discovery_verified"]) && truth(&row["observed_model_metadata_verified"])
            });
            if !verified {
                limitations.push("model metadata not verified on both arms".to_owned());
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
        }
        let declared = declaration.as_ref().map(|value| {
            json!({
                "task_mix": value.task_mix,
                "objective": value.objective,
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
        units.push(json!({
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
        }));
    }

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
    let accounting = json!({
        "attempts": attempts_count,
        "tasks": tasks.len(),
        "accepted_tasks": accepted_tasks,
        "acceptance_rate": acceptance_rate,
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
        "basis": "Each attempt contributes its own enclosing time once; retries, workers, interventions and failed attempts are included; overlapping spans are not summed. Usage stays per run and is never a billing or subscription measurement.",
    });
    Ok(json!({
        "schema_version": 2,
        "attempts": rows,
        "comparisons": pairs,
        "units": units,
        "independent_units": unit_arms.len(),
        "accounting": accounting,
        "benefit_status": "not_evaluated",
        "limitation": "Local case evidence; tokens are not billing or subscription-quota savings. Units and claim classes are eligibility evidence for a benefit decision, not demonstrated benefit.",
    }))
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
