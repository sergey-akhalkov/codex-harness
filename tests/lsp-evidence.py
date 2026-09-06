"""Revalidate retained, scoped native probe evidence without another model run.

Input JSON: [{"language": "toml", "probe": "absolute disposable probe path"}].
Reads only the specified probes and their synthetic workspace files. A previous
test exit status is retained separately from the assertions on actual evidence.
"""
from __future__ import annotations
import argparse
import hashlib
import json
from pathlib import Path


def validate(entry):
    probe = Path(entry["probe"]).resolve(strict=True)
    previous = probe / "report.json"
    original = json.loads(previous.read_text(encoding="utf-8-sig")) if previous.is_file() else None
    events_path = probe / "private-agent-events.json"
    if events_path.is_file():
        events = json.loads(events_path.read_text(encoding="utf-8-sig"))
    else:
        events = [json.loads(line) for line in (probe / "private-agent-events.jsonl").read_text(encoding="utf-8-sig").splitlines()]
    finals = "\n".join(event["params"]["item"]["text"] for event in events
        if event.get("method") == "item/completed" and event["params"]["item"]["type"] == "agentMessage"
        and event["params"]["item"].get("phase") in ("final", "final_answer"))
    diagnostics = probe / "scoped-diagnostics.json"
    if diagnostics.is_file():
        reports = json.loads(diagnostics.read_text(encoding="utf-8-sig"))
        reports = reports if isinstance(reports, list) else [reports]
    else:
        reports = [json.loads(path.read_text(encoding="utf-8-sig")) for path in
            (probe / "codex home/harness/runtime/lsp").rglob("report-*.json")]
    rows = [{"workspace": report["workspace"], **result} for report in reports for result in report["results"]]
    backend = {"javascript": "typescript", "delphi": "pascal"}.get(entry["language"], entry["language"])
    errors = [row for row in rows if row["status"] == "diagnostics" and row["diagnostics"] and row["backend"] == backend]
    assert errors, "No actual error report for the selected language"
    received = any((str(item.get("code", "")) and str(item["code"]) in finals) or
        str(item.get("message", "")).splitlines()[0].strip() in finals for row in errors for item in row["diagnostics"])
    assert received and "clean" in finals, "Agent final did not report the actual error and clearance"
    clean = []
    for row in rows:
        if row["status"] != "clean" or row["diagnostics"] or row["backend"] != backend:
            continue
        source = (Path(row["workspace"]) / row["file"]).resolve(strict=True)
        assert source.is_relative_to(probe), "Evidence attempted to leave its disposable workspace"
        if hashlib.sha256(source.read_bytes()).hexdigest() == row["revision"]:
            clean.append(row)
    assert clean, "No authoritative empty set matches the actual corrected source"
    assert any(event.get("method") == "hook/completed" and event["params"]["run"]["eventName"] == "postToolUse"
        and event["params"]["run"]["status"] == "completed" for event in events), "No completed automatic native hook"
    assert sum(event.get("method") == "item/completed" and event["params"]["item"]["type"] == "fileChange"
        and event["params"]["item"]["status"] == "completed" for event in events) >= 2, "Two real native edits missing"
    return {**entry, "passed": True, "native_test_report_present": original is not None,
        "evidence": "native edits + completed automatic hooks + actual full diagnostic reports + agent final + current source SHA256",
        "files": sorted({row["file"] for row in errors}), "codes": sorted({str(item["code"]) for row in errors for item in row["diagnostics"] if "code" in item}),
        "clearance": "current-authoritative-empty", "native_version": original.get("nativeVersion") if original else "See retained native events/trust transcript"}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", required=True)
    parser.add_argument("--report", required=True)
    arguments = parser.parse_args()
    results = []
    for entry in json.loads(Path(arguments.manifest).read_text(encoding="utf-8-sig")):
        try:
            results.append(validate(entry))
        except Exception as error:
            results.append({**entry, "passed": False, "reason": f"{type(error).__name__}: {error}"})
    Path(arguments.report).write_text(json.dumps(results, ensure_ascii=False, indent=2), encoding="utf-8")
    print(json.dumps({"passed": sum(row["passed"] for row in results), "total": len(results), "report": arguments.report}))
    return 0 if all(row["passed"] for row in results) else 1


if __name__ == "__main__":
    raise SystemExit(main())
