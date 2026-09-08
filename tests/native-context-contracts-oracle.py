"""Read-only independent oracle for owned native context probe receipts.

Prints observations, never treats config/account eligibility as runtime activation.
Does not call a model, alter receipts, or rely on the assistant's success claim.
"""
from __future__ import annotations

import argparse
import json
import re
import sqlite3
from datetime import datetime
from pathlib import Path
from typing import Any


def read_json(path: Path) -> dict[str, Any]:
    return json.loads(path.read_text(encoding="utf-8-sig")) if path.exists() else {}


def json_lines(path: Path) -> list[dict[str, Any]]:
    # A completed evidence file must parse in full. Partial lines are not success.
    return [json.loads(line) for line in path.read_text(encoding="utf-8-sig").splitlines() if line]


def sampling_evidence(root: Path) -> list[dict[str, Any]]:
    """Read native sampling diagnostics, including ephemeral auxiliary threads.

    Root rollout model fields alone missed TUI's automatic Luna title request.
    An absent diagnostic log remains unknown, never Astra-only proof.
    """
    database = root / "home/logs_2.sqlite"
    if not database.exists():
        return []
    result: list[dict[str, Any]] = []
    connection = sqlite3.connect(database.as_uri() + "?mode=ro", uri=True)
    try:
        for ident, seconds, nanos, message in connection.execute(
                "select id,ts,ts_nanos,feedback_log_body from logs where target='feedback_tags' order by id"):
            model = re.search(r'model="([^"]+)"', message)
            thread = re.search(r"thread\.id=([a-f0-9-]+)", message)
            if model and "features=[" in message:
                result.append({"log_id": ident, "unix_time": seconds + nanos / 1e9,
                               "model": model[1], "thread_id": thread[1] if thread else None,
                               "context_management": "ContextManagement" in message.split("features=[", 1)[1]})
    finally:
        connection.close()
    return result


def observe(root: Path) -> dict[str, Any]:
    report = read_json(root / "report.json")
    expected = read_json(root / "oracle.json")
    process = read_json(root / "process-result.json")
    config = read_json(root / "effective-config.json").get("config", {})
    hooks = json_lines(root / "hook-events.jsonl") if (root / "hook-events.jsonl").exists() else []
    sampling = sampling_evidence(root)
    paths = sorted((root / "home/sessions").rglob("*.jsonl"))
    rows = [row for path in paths for row in json_lines(path)]
    messages = [r for r in rows if r.get("type") == "response_item" and
                r.get("payload", {}).get("type") == "message" and
                r["payload"].get("role") == "assistant" and
                r["payload"].get("phase") == "final_answer"]
    finals = [{"timestamp": r["timestamp"], "text": "".join(c.get("text", "") for c in r["payload"].get("content", []))} for r in messages]
    events = [r for r in rows if r.get("type") == "event_msg"]
    complete = [r for r in events if r["payload"].get("type") == "task_complete"]
    models = sorted({r["payload"]["model"] for r in rows if r.get("type") == "turn_context"})
    sources = [r for r in hooks if r["input"].get("hook_event_name") == "SessionStart"]
    delivered: list[dict[str, Any]] = []
    for h in hooks:
        marker = h.get("marker")
        if marker:
            matched = [i for i, f in enumerate(finals) if marker in f["text"] and f["timestamp"] >= h["at"]]
            context_times = [r["timestamp"] for r in rows if r.get("type") == "response_item" and
                             r["payload"].get("type") == "message" and r["payload"].get("role") == "developer" and
                             marker in json.dumps(r["payload"].get("content", []))]
            preceding_compacts = [c for c in hooks if c["input"].get("hook_event_name") == "PostCompact" and c["at"] < h["at"]]
            trigger = preceding_compacts[-1]["input"].get("trigger") if preceding_compacts and h["input"].get("source") == "compact" else None
            delivered.append({"event": h["input"]["hook_event_name"], "source": h["input"].get("source"),
                              "trigger": trigger, "marker": marker, "final_indices": matched,
                              "developer_context_times": context_times, "at": h["at"]})
    calls = [r for r in rows if r.get("type") == "response_item" and r["payload"].get("type") in ("function_call", "custom_tool_call")]
    compact = [r for r in rows if r.get("type") == "compacted" or
               (r.get("type") == "event_msg" and r["payload"].get("type") == "context_compacted")]
    skill_results: list[dict[str, Any]] = []
    for field, answer in (("revision1", "created_answer"), ("revision2", "updated_answer")):
        if expected.get(field):
            skill_results.append({"revision": field, "correct_final_indices": [i for i, f in enumerate(finals)
                                  if expected[field] in f["text"] and re.search(rf"\b{expected[answer]}\b", f["text"])]})
    trust = read_json(root / "hook-trust.json")
    trusted_hooks = [h for d in trust.get("data", []) for h in d.get("hooks", [])]
    texts = "\n".join(p.read_text(encoding="utf-8-sig", errors="replace") for p in (root / "home/log").glob("*.log"))
    runtime_lines = [line[:1500] for line in texts.splitlines() if re.search(r"context.management|experimental.mode|context.window|compact", line, re.I)]
    winner = expected.get("eligible_winner")
    candidate_matches = [i for i, f in enumerate(finals) if winner and
                         re.search(rf"^(?:Eligible )?candidate(?: id)?:\s*[*`]*{re.escape(winner)}\b", f["text"], re.I | re.M) and
                         expected.get("forbidden", "MISSING") not in f["text"]]
    output_pass = (bool(candidate_matches) if report.get("scenario") == "pilot" else
                   len(skill_results) == 2 and all(s["correct_final_indices"] for s in skill_results))
    skill_reads = [{"at": r["timestamp"], "name": r["payload"].get("name")}
                   for r in calls if "contract-adjustment" in json.dumps(r["payload"]) and
                   "SKILL.md" in json.dumps(r["payload"])]
    skill_call_ids = {r["payload"].get("call_id") for r in calls if
                      "contract-adjustment" in json.dumps(r["payload"]) and "SKILL.md" in json.dumps(r["payload"])}
    skill_results_read = [{"at": r["timestamp"], "revision": field} for r in rows if
        r.get("type") == "response_item" and r["payload"].get("type") in ("function_call_output", "custom_tool_call_output") and
        r["payload"].get("call_id") in skill_call_ids for field in ("revision1", "revision2") if
        expected.get(field) and expected[field] in json.dumps(r["payload"].get("output"))]
    automatic_continuations = []
    for pre in hooks:
        if pre["input"].get("hook_event_name") != "PreCompact" or pre["input"].get("trigger") != "auto":
            continue
        turn = pre["input"].get("turn_id")
        starts = [r for r in rows if r.get("type") == "event_msg" and
                  r["payload"].get("type") == "task_started" and r["payload"].get("turn_id") == turn]
        ends = [r for r in complete if r["payload"].get("turn_id") == turn]
        posts = [h for h in hooks if h["input"].get("hook_event_name") == "PostCompact" and
                 h["input"].get("turn_id") == turn and h["input"].get("trigger") == "auto" and h["at"] > pre["at"]]
        if not starts or not ends or not posts:
            continue
        start, end, post = starts[0]["timestamp"], ends[-1]["timestamp"], posts[0]["at"]
        deliveries = [d for d in delivered if d["event"] == "SessionStart" and d["trigger"] == "auto" and post < d["at"] < end]
        after = [s for s in sampling if datetime.fromisoformat(post).timestamp() < s["unix_time"] < datetime.fromisoformat(end).timestamp()]
        next_sample = after[0] if after else None
        pids = [s["detail"]["pid"] for s in report.get("stages", []) if s["name"] == "cli-started" and s["at"] < start and
                any(e["name"] == "cli-exit" and e["detail"]["pid"] == s["detail"]["pid"] and e["at"] > end for e in report["stages"])]
        automatic_continuations.append({
            "turn_id": turn, "start": start, "end": end, "native_process_ids": pids,
            "native_compaction_between_hooks": any(pre["at"] < c["timestamp"] < post for c in compact),
            "no_intervening_turn": not any(r.get("type") == "event_msg" and r["payload"].get("type") == "task_started" and
                                           start < r["timestamp"] < end for r in rows),
            "next_sampling": next_sample,
            "context_before_next_sampling": bool(next_sample) and any(
                datetime.fromisoformat(post).timestamp() < datetime.fromisoformat(at).timestamp() < next_sample["unix_time"]
                for d in deliveries for at in d["developer_context_times"]),
            "acknowledged_final_indices": [i for d in deliveries for i in d["final_indices"] if finals[i]["timestamp"] < end],
        })
    tool_errors = [{"at": r["timestamp"], "type": r["payload"].get("type"),
                    "signature": "CreateProcessAsUserW failed: -1073283067"}
                   for r in rows if r.get("type") == "response_item" and
                   r["payload"].get("type") in ("function_call_output", "custom_tool_call_output") and
                   "CreateProcessAsUserW failed: -1073283067" in json.dumps(r["payload"])]
    return {
        "root": str(root), "attempt_status": report.get("status"), "failure": report.get("error"),
        "natural_worker_exit": process.get("Status") == "exited" and process.get("ExitCode") == 0,
        "cli_exits": [s["detail"] for s in report.get("stages", []) if s["name"] == "cli-exit"],
        "parser": [s["detail"] for s in report.get("stages", []) if s["name"] == "parser"],
        "effective_setting": config.get("features", {}).get("context_management"),
        "eligibility": [s["detail"] for s in report.get("stages", []) if s["name"] == "eligibility"],
        "model_route": report.get("route"), "actual_models": models,
        "native_sampling": sampling,
        "all_sampling_models": sorted({s["model"] for s in sampling}),
        "astra_only_sampling": bool(sampling) and all(s["model"] == "gpt-6-astra" for s in sampling),
        "context_management_sampling_after_compaction": [s for s in sampling if s["context_management"] and
            any(s["unix_time"] > datetime.fromisoformat(c["timestamp"]).timestamp() for c in compact)],
        "all_hooks_trusted": bool(trusted_hooks) and all(h.get("trustStatus") == "trusted" for h in trusted_hooks),
        "session_ids": sorted({r["payload"]["id"] for r in rows if r.get("type") == "session_meta"}),
        "rollout_paths": [str(p) for p in paths], "completed_turns": len(complete),
        "finals": finals, "skill_oracle": skill_results,
        "skill_reads": skill_reads,
        "skill_read_results": skill_results_read,
        "automatic_continuations": automatic_continuations,
        "session_start_sources": [h["input"].get("source") for h in sources],
        "hook_delivery": delivered,
        "compaction_hooks": [{"at": h["at"], "event": h["input"]["hook_event_name"], "trigger": h["input"].get("trigger"), "turn": h["input"].get("turn_id")} for h in hooks if h["input"]["hook_event_name"] in ("PreCompact", "PostCompact")],
        "native_compaction_records": [{"at": r["timestamp"], "type": r["type"]} for r in compact],
        "candidate_oracle_final_indices": candidate_matches,
        "output_oracle_pass": output_pass,
        "tool_start_failures": tool_errors,
        "tool_calls": [{"at": r["timestamp"], "name": r["payload"].get("name"), "arguments": r["payload"].get("arguments", r["payload"].get("input", ""))} for r in calls],
        "runtime_log_evidence": runtime_lines,
        "experimental_runtime_activation": any(s["context_management"] for s in sampling),
        "full_acceptance": "use --check-contract plus recorded fixture/source review; output correctness alone is insufficient",
    }


def contract_failures(observation: dict[str, Any], kind: str) -> list[str]:
    """Acceptance checks are stricter than output correctness or runner exit."""
    failures: list[str] = []
    if not observation["natural_worker_exit"]:
        failures.append("worker did not exit naturally with zero")
    if not observation["astra_only_sampling"]:
        failures.append("native sampling is missing or contains a non-Astra model")
    if not observation["output_oracle_pass"]:
        failures.append("independent task output is wrong or absent")
    if not observation["cli_exits"] or not all(x["natural"] and x["exit"] == 0 for x in observation["cli_exits"]):
        failures.append("native TUI natural termination is unproved")
    if not any(c["native_process_ids"] and c["native_compaction_between_hooks"] and c["no_intervening_turn"] and
               c["context_before_next_sampling"] and set(c["acknowledged_final_indices"]) & set(observation["candidate_oracle_final_indices"])
               for c in observation["automatic_continuations"]):
        failures.append("automatic continuation in one native process/turn with immediate context delivery and correct invariant is unproved")
    route = observation["model_route"] or {}
    if route.get("base_url") != "http://127.0.0.1:10100/v1" or route.get("auth_mode") != "chatgpt" or route.get("provider") != "openai":
        failures.append("subscription route identity mismatch")
    if kind == "pilot":
        if not any(p.get("exit") == 0 and p.get("setting") == "true" for p in observation["parser"]):
            failures.append("experimental setting did not parse")
        if not (observation["effective_setting"] or {}).get("experimental_mode"):
            failures.append("experimental setting is not effective")
        if not any(e.get("account", {}).get("type") == "chatgpt" and e.get("account", {}).get("planType") == "pro" for e in observation["eligibility"]):
            failures.append("recorded subscription prerequisite not established")
        if not observation["context_management_sampling_after_compaction"]:
            failures.append("experimental native runtime sampling after compaction is unproved")
    required = [("compact", "auto")] if kind == "pilot" else [("startup", None), ("compact", "manual"), ("compact", "auto"), ("resume", None)]
    for source, trigger in required:
        matches = [d for d in observation["hook_delivery"] if d["event"] == "SessionStart" and
                   d["source"] == source and d["trigger"] == trigger and d["developer_context_times"] and d["final_indices"]]
        if not matches:
            failures.append(f"SessionStart {source}/{trigger}: native context and independent output acknowledgement missing")
        if kind == "skills" and source in ("compact", "resume") and matches:
            if not any(any(d["at"] < r["at"] < observation["finals"][i]["timestamp"] and r["revision"] == "revision2" for r in observation["skill_read_results"])
                       for d in matches for i in d["final_indices"]):
                failures.append(f"current skill read after {source}/{trigger} missing")
    if kind == "skills":
        if {r["revision"] for r in observation["skill_read_results"]} != {"revision1", "revision2"}:
            failures.append("native tool results do not contain both skill revisions")
        if len(observation["session_ids"]) != 1:
            failures.append("same native session identity is unproved")
        for event in ("UserPromptSubmit", "PostToolUse"):
            if not any(d["event"] == event and d["developer_context_times"] and d["final_indices"] for d in observation["hook_delivery"]):
                failures.append(f"{event} delivery acknowledgement missing")
    return failures


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", type=Path)
    parser.add_argument("--check-output", action="store_true", help="Exit 1 when expected task output is absent; passing is not full runtime acceptance.")
    parser.add_argument("--check-contract", choices=("skills", "pilot"), help="Check native lifecycle, runtime, output and Astra-only sampling evidence.")
    args = parser.parse_args()
    observation = observe(args.root.resolve())
    if args.check_contract:
        observation["contract_failures"] = contract_failures(observation, args.check_contract)
    print(json.dumps(observation, ensure_ascii=False, indent=2))
    if args.check_output and not observation["output_oracle_pass"]:
        raise SystemExit(1)
    if args.check_contract and observation["contract_failures"]:
        raise SystemExit(1)
