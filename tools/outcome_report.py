"""Small, deterministic accounting for private native outcome attempts.

Timestamps are UTC epoch seconds. Whole-result wall time is the enclosing span,
including verification, gaps and rework; child spans are never added to it.
Unknown counters stay null. No token-to-billing or quota conversion is made.
"""
from __future__ import annotations

import copy
import math
from typing import Any

MATCH_FIELDS = (
    "case_revision", "source_state", "input_identity", "runtime", "model",
    "effort", "provider", "config_identity", "tool_identity", "hook_revision",
    "allowed_effects", "cache_policy", "preparation_policy", "budget",
    "oracle_identity", "instructions_identity", "other_skills", "stop_conditions",
    "criterion",
)


def timestamp(value: Any) -> float | None:
    if isinstance(value, (int, float)) and not isinstance(value, bool) and math.isfinite(value):
        return float(value)
    return None


def wall_span(spans: list[dict[str, Any]]) -> float | None:
    """Return elapsed wall span, not the sum (nor union) of worker durations."""
    if not spans:
        return None
    starts = [timestamp(s.get("started_at")) for s in spans]
    ends = [timestamp(s.get("ended_at")) for s in spans]
    if any(s is None or e is None or e < s for s, e in zip(starts, ends)):
        return None
    return max(e for e in ends if e is not None) - min(s for s in starts if s is not None)


def finish_attempt(record: dict[str, Any]) -> dict[str, Any]:
    """Derive correctness only from executed acceptance and completed native work.

Call again after rework with all native_runs/checks retained. Earlier failed
checks remain in the record; required checks in the final round decide outcome.
"""
    row = copy.deepcopy(record)
    native = row.get("native_runs", [])
    checks = row.get("checks", [])
    derived = {"incomplete_timing", "no_opposite_arm", "unresolved_retry_chain", "retry_identity_mismatch"}
    reasons = [r for r in row.get("excluded_reasons", []) if r not in derived and not r.startswith("outcome_")]
    current = max((c.get("round", 0) for c in checks), default=0)
    required = [c for c in checks if c.get("round", 0) == current and c.get("required", True)]
    expected_ids = set(row.get("required_check_ids", [])) or {c.get("id") for c in checks if c.get("required", True)}
    complete_checks = bool(required) and expected_ids <= {c.get("id") for c in required} and all(
        c.get("executed") is True and type(c.get("exit_code")) is int and c.get("evidence") and
        timestamp(c.get("ended_at")) is not None for c in required)
    correct = complete_checks and all(c.get("passed") is True for c in required)
    last_native = native[-1] if native else {}
    for run in native:
        reasons.extend(run.get("evidence_errors", []))
    row["correct"] = bool(correct and last_native.get("status") == "completed")
    if row["correct"]:
        row["status"] = "accepted"
    elif any(c.get("executed") and c.get("passed") is False for c in required):
        row["status"] = "failed"
    else:
        row["status"] = row.get("status") if row.get("status") in ("failed", "blocked", "timeout") else "incomplete"
    execution_start = row.get("execution_started_at", row.get("started_at"))
    spans = [{"started_at": execution_start, "ended_at": row.get("ended_at")},
             *native, *checks, *row.get("children", []), *row.get("interventions", [])]
    row["elapsed_seconds"] = wall_span(spans)
    preparation = row.get("preparation")
    if preparation and "execution_started_at" not in row:
        row["elapsed_seconds"] = 0.0 if not native and not checks else None
    row["preparation_seconds"] = wall_span([preparation]) if preparation else 0.0
    row["observed_wall_seconds"] = wall_span([{"started_at": row.get("started_at"), "ended_at": row.get("ended_at")}])
    # Pair preparation happens before either model starts. Report its own span,
    # without charging this arm for the other arm's preparation or execution.
    row["verified_seconds"] = (row["elapsed_seconds"] + row["preparation_seconds"]
        if row["elapsed_seconds"] is not None and row["preparation_seconds"] is not None else None)
    if row["elapsed_seconds"] is None:
        reasons.append("incomplete_timing")
    signals = [n.get("first_useful_signal") for n in native]
    signals += [{"at": c.get("ended_at"), "evidence": c.get("evidence"), "kind": "acceptance_result"}
                for c in checks if c.get("executed") is True and isinstance(c.get("exit_code"), int)]
    valid = [s for s in signals if isinstance(s, dict) and timestamp(s.get("at")) is not None
             and s.get("evidence") and timestamp(row.get("started_at")) is not None
             and s["at"] >= row["started_at"] and timestamp(row.get("ended_at")) is not None
             and s["at"] <= row["ended_at"]]
    signal = min(valid, key=lambda s: s["at"]) if valid else None
    row["first_useful_signal"] = signal
    row["first_useful_seconds"] = (signal["at"] - execution_start + row["preparation_seconds"]
        if signal is not None and row["preparation_seconds"] is not None else None)
    if row["status"] != "accepted":
        reasons.append("outcome_" + str(row["status"]))
    row["excluded_reasons"] = sorted(set(reasons))
    return row


def comparison_reasons(left: dict[str, Any], right: dict[str, Any]) -> list[str]:
    reasons: list[str] = []
    if left.get("case_id") != right.get("case_id"):
        reasons.append("different_case")
    if {left.get("arm"), right.get("arm")} != {"baseline", "candidate"}:
        reasons.append("not_opposite_arms")
    a, b = left.get("matched", {}), right.get("matched", {})
    # Every caller-declared material field participates, not just a whitelist.
    for key in sorted(set(MATCH_FIELDS) | set(a) | set(b)):
        if key not in a or key not in b or a[key] is None or b[key] is None:
            reasons.append("unknown:" + key)
        elif a[key] != b[key]:
            reasons.append("mismatch:" + key)
    for row in (left, right):
        reasons.extend(row.get("excluded_reasons", []))
        if not row.get("discovery_verified"):
            reasons.append("discovery_unverified")
    return sorted(set(reasons))


def summarize_attempts(attempts: list[dict[str, Any]]) -> dict[str, Any]:
    """Retain every attempt, with explicit pair exclusions and retry accounting.

Failed outcomes remain available for success-rate comparisons. They cannot
support a speed-only claim. This formatter never declares demonstrated benefit.
"""
    rows = [finish_attempt(a) for a in attempts]
    by_id = {r["attempt_id"]: r for r in rows}
    if len(by_id) != len(rows):
        raise ValueError("Duplicate attempt_id would lose attempt accounting")
    for row in rows:
        chain, seen, current = [row], {row["attempt_id"]}, row
        retry_complete = True
        while current.get("retry_of"):
            parent = by_id.get(current["retry_of"])
            if parent is None or parent["attempt_id"] in seen:
                row["excluded_reasons"].append("unresolved_retry_chain")
                retry_complete = False
                break
            if parent.get("case_id") != row.get("case_id") or parent.get("arm") != row.get("arm"):
                row["excluded_reasons"].append("retry_identity_mismatch")
                retry_complete = False
                break
            chain.append(parent)
            seen.add(parent["attempt_id"])
            current = parent
        row["result_attempt_ids"] = [c["attempt_id"] for c in reversed(chain)]
        # A prior verification/child end can extend beyond its parent's own end.
        accounted = [{"started_at": c.get("execution_started_at", c.get("started_at")), "ended_at":
                      c.get("execution_started_at", c["started_at"]) + c["elapsed_seconds"] if c.get("elapsed_seconds") is not None else None}
                     for c in chain]
        row["total_result_seconds"] = wall_span(accounted) if retry_complete else None
        initial_preparation = chain[-1]["preparation_seconds"]
        row["total_verified_seconds"] = (row["total_result_seconds"] + initial_preparation
            if row["total_result_seconds"] is not None and initial_preparation is not None else None)
        # Preserve per-attempt usage. Reuse the native parser for unique threads;
        # summing parent/child totals here could double count the same rollout.
        run_usage = [n["usage"] for n in row.get("native_runs", []) if n.get("usage")]
        row["usage"] = row.get("usage") or ({"status": "per_run", "runs": run_usage} if run_usage else
                                             {"status": "unknown", "total_tokens": None})
    pairs = []
    for left in rows:
        for right in rows:
            if left.get("arm") == "baseline" and right.get("arm") == "candidate" and left.get("case_id") == right.get("case_id"):
                reasons = comparison_reasons(left, right)
                pairs.append({"baseline": left["attempt_id"], "candidate": right["attempt_id"],
                              "comparable": not reasons, "excluded_reasons": reasons})
    for row in rows:
        partners = [p for p in pairs if row["attempt_id"] in (p["baseline"], p["candidate"])]
        if not partners:
            row["excluded_reasons"].append("no_opposite_arm")
        row["excluded_reasons"] = sorted(set(row["excluded_reasons"]))
    return {"schema_version": 1, "attempts": rows, "comparisons": pairs,
            "benefit_status": "not_evaluated", "limitation": "Local case evidence; tokens are not billing or subscription-quota savings."}


def concise_report(report: dict[str, Any]) -> str:
    lines = ["Case | Arm | Attempt | Outcome | Preparation seconds | Native through checks wall seconds | Total verified seconds | Comparison exclusions",
             "--- | --- | --- | --- | ---: | ---: | ---: | ---"]
    for row in report["attempts"]:
        seconds = row.get("total_result_seconds")
        reasons = set(row["excluded_reasons"])
        partners = [p for p in report["comparisons"] if row["attempt_id"] in (p["baseline"], p["candidate"])]
        if partners and not any(p["comparable"] for p in partners):
            reasons.update(r for p in partners for r in p["excluded_reasons"])
        durations = [row.get("preparation_seconds"), seconds, row.get("total_verified_seconds")]
        values = [row["case_id"], row["arm"], row["attempt_id"], row["status"],
                  *("unknown" if value is None else f"{value:.3f}" for value in durations), ", ".join(sorted(reasons)) or "none"]
        lines.append(" | ".join(str(v).replace("|", "\\|").replace("\n", " ") for v in values))
    return "\n".join(lines) + "\n\n" + report["limitation"] + "\n"
