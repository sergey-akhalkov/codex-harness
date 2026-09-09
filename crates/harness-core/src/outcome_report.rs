//! Deterministic accounting for private native outcome attempts. No execution,
//! billing conversion, or claim of demonstrated benefit is made by this module.
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
    spans.extend_from_slice(array(record, "children")?);
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
    row["excluded_reasons"] = json!(reasons);
    Ok(row)
}

pub fn comparison_reasons(left: &Value, right: &Value) -> io::Result<Vec<String>> {
    let mut reasons = BTreeSet::new();
    if left["case_id"] != right["case_id"] {
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
            (Some(x), Some(y)) if !x.is_null() && !y.is_null() => {
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

/// Failed attempts remain available for success-rate comparisons. Missing usage
/// stays null; parent/child token totals are not summed here.
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
        let usage: Vec<_> = array(&rows[index], "native_runs")?
            .iter()
            .map(|n| &n["usage"])
            .filter(|v| truth(v))
            .cloned()
            .collect();
        if !truth(&rows[index]["usage"]) {
            rows[index]["usage"] = if usage.is_empty() {
                json!({"status":"unknown", "total_tokens":null})
            } else {
                json!({"status":"per_run", "runs":usage})
            };
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
                let pair = json!({"baseline": left["attempt_id"], "candidate":right["attempt_id"], "comparable":reasons.is_empty(), "excluded_reasons": reasons});
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
    Ok(
        json!({"schema_version":1,"attempts":rows,"comparisons":pairs,"benefit_status":"not_evaluated",
        "limitation":"Local case evidence; tokens are not billing or subscription-quota savings."}),
    )
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
}
