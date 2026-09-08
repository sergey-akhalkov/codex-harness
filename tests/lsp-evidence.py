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
from collections.abc import Mapping
from typing import Protocol, TypeGuard, runtime_checkable


@runtime_checkable
class Decoder(Protocol):
    def decode(self, source: str) -> object: ...


def decoder_api(value: object) -> Decoder:
    assert isinstance(value, Decoder)
    return value


def is_mapping(value: object) -> TypeGuard[Mapping[object, object]]:
    return isinstance(value, Mapping)


def is_list(value: object) -> TypeGuard[list[object]]:
    return isinstance(value, list)


def object_map(value: object) -> Mapping[object, object]:
    assert is_mapping(value), "Expected a JSON object"
    return value


def object_list(value: object) -> list[object]:
    assert is_list(value), "Expected a JSON array"
    return value


def text(value: object) -> str:
    assert isinstance(value, str), "Expected JSON text"
    return value


def at(value: object, *keys: str) -> object:
    for key in keys:
        value = object_map(value)[key]
    return value


decoder = decoder_api(json.JSONDecoder())


def read_json(path: Path) -> object:
    return decoder.decode(path.read_text(encoding="utf-8-sig"))


def validate(entry: Mapping[object, object]) -> dict[object, object]:
    probe = Path(text(entry["probe"])).resolve(strict=True)
    previous = probe / "report.json"
    original = object_map(read_json(previous)) if previous.is_file() else None
    events_path = probe / "private-agent-events.json"
    if events_path.is_file():
        events = [object_map(event) for event in object_list(read_json(events_path))]
    else:
        events = [object_map(decoder.decode(line)) for line in
            (probe / "private-agent-events.jsonl").read_text(encoding="utf-8-sig").splitlines()]
    finals = "\n".join(text(at(event, "params", "item", "text")) for event in events
        if event.get("method") == "item/completed" and at(event, "params", "item", "type") == "agentMessage"
        and object_map(at(event, "params", "item")).get("phase") in ("final", "final_answer"))
    diagnostics = probe / "scoped-diagnostics.json"
    if diagnostics.is_file():
        retained = read_json(diagnostics)
        reports = [object_map(report) for report in retained] if is_list(retained) else [object_map(retained)]
    else:
        reports = [object_map(read_json(path)) for path in
            (probe / "codex home/harness/runtime/lsp").rglob("report-*.json")]
    rows = [{"workspace": report["workspace"], **object_map(result)} for report in reports
        for result in object_list(report["results"])]
    language = text(entry["language"])
    backend = {"javascript": "typescript", "delphi": "pascal"}.get(language, language)
    errors = [row for row in rows if row["status"] == "diagnostics" and row["diagnostics"] and row["backend"] == backend]
    assert errors, "No actual error report for the selected language"
    diagnostic_items = [object_map(item) for row in errors for item in object_list(row["diagnostics"])]
    received = False
    for item in diagnostic_items:
        code = str(item.get("code", ""))
        lines = str(item.get("message", "")).splitlines()
        message = lines[0].strip() if lines else ""
        if (code and code in finals) or (message and message in finals):
            received = True
            break
    assert received and "clean" in finals, "Agent final did not report the actual error and clearance"
    clean: list[dict[object, object]] = []
    for row in rows:
        if row["status"] != "clean" or row["diagnostics"] or row["backend"] != backend:
            continue
        source = (Path(text(row["workspace"])) / text(row["file"])).resolve(strict=True)
        assert source.is_relative_to(probe), "Evidence attempted to leave its disposable workspace"
        if hashlib.sha256(source.read_bytes()).hexdigest() == row["revision"]:
            clean.append(row)
    assert clean, "No authoritative empty set matches the actual corrected source"
    assert any(event.get("method") == "hook/completed" and at(event, "params", "run", "eventName") == "postToolUse"
        and at(event, "params", "run", "status") == "completed" for event in events), "No completed automatic native hook"
    assert sum(event.get("method") == "item/completed" and at(event, "params", "item", "type") == "fileChange"
        and at(event, "params", "item", "status") == "completed" for event in events) >= 2, "Two real native edits missing"
    return {**entry, "passed": True, "native_test_report_present": original is not None,
        "evidence": "native edits + completed automatic hooks + actual full diagnostic reports + agent final + current source SHA256",
        "files": sorted({text(row["file"]) for row in errors}), "codes": sorted({str(item["code"]) for item in diagnostic_items if "code" in item}),
        "clearance": "current-authoritative-empty", "native_version": original.get("nativeVersion") if original else "See retained native events/trust transcript"}


class Arguments(argparse.Namespace):
    manifest: str = ""
    report: str = ""


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    _ = parser.add_argument("--manifest", required=True)
    _ = parser.add_argument("--report", required=True)
    arguments = parser.parse_args(namespace=Arguments())
    results: list[dict[object, object]] = []
    for raw_entry in object_list(read_json(Path(arguments.manifest))):
        entry = object_map(raw_entry)
        try:
            results.append(validate(entry))
        except Exception as error:
            results.append({**entry, "passed": False, "reason": f"{type(error).__name__}: {error}"})
    _ = Path(arguments.report).write_text(json.dumps(results, ensure_ascii=False, indent=2), encoding="utf-8")
    print(json.dumps({"passed": sum(bool(row["passed"]) for row in results), "total": len(results), "report": arguments.report}))
    return 0 if all(row["passed"] for row in results) else 1


if __name__ == "__main__":
    raise SystemExit(main())
