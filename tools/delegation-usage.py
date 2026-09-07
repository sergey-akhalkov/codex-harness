"""Redacted native Codex JSONL accounting; Python standard library only.

Usage: python tools/delegation-usage.py ROLLOUT [ROLLOUT ...] --output report.json
Only supplied files are read; descendants must be supplied explicitly. The last
cumulative token_count total is used, never last_token_usage or summed snapshots.
Null means unknown. Totals sum known values, with partial flags for omissions;
they are observed tokens, not subscription quota or billing estimates.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import re
import sys


TOKEN_FIELDS = (
    "input_tokens", "cached_input_tokens", "output_tokens",
    "reasoning_output_tokens", "total_tokens",
)
MODEL_PROVIDERS = {
    "gpt-6-astra": "OpenAI", "openai/gpt-6-astra": "OpenAI",
    "xai/grok-4.6": "xai", "grok-4.6": "xai",
}
EFFORTS = {"none", "minimal", "low", "medium", "high", "xhigh", "max"}


def _identifier(value):
    # Allow only bounded metadata identifiers, never arbitrary message strings.
    if isinstance(value, str) and re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_./:-]{0,127}", value):
        return value
    return None


def _object(value):
    return value if isinstance(value, dict) else {}


def _read_rollout(path):
    ids, parents, models, efforts = set(), set(), set(), set()
    usage: dict[str, int | None] = dict.fromkeys(TOKEN_FIELDS)
    warnings = set()
    previous_usage = None
    saw_meta = False
    try:
        with path.open("rb") as stream:
            for line in stream:
                if not line.strip():
                    continue
                try:
                    event = json.loads(line)
                except (ValueError, UnicodeError, RecursionError):
                    warnings.add("corrupt_jsonl")
                    continue
                if not isinstance(event, dict):
                    warnings.add("invalid_event")
                    continue
                kind = event.get("type")
                if not isinstance(kind, str):
                    warnings.add("invalid_event_type")
                    continue
                if kind not in {"session_meta", "turn_context", "event_msg"}:
                    continue
                payload = event.get("payload")
                if not isinstance(payload, dict):
                    warnings.add("invalid_payload")
                    continue
                if kind == "session_meta":
                    saw_meta = True
                    for key in ("id", "session_id"):
                        if payload.get(key) is not None:
                            value = _identifier(payload[key])
                            if value:
                                ids.add(value)
                            else:
                                warnings.add("invalid_thread_id")
                    candidates = [payload.get("parent_thread_id")]
                    for source_key in ("source", "thread_source"):
                        source = _object(payload.get(source_key))
                        spawn = _object(_object(source.get("subagent")).get("thread_spawn"))
                        candidates.append(spawn.get("parent_thread_id"))
                    for candidate in candidates:
                        if candidate is not None:
                            parent = _identifier(candidate)
                            if parent:
                                parents.add(parent)
                            else:
                                warnings.add("invalid_parent_id")
                elif kind == "turn_context":
                    model = _identifier(payload.get("model"))
                    if model:
                        models.add(model)
                    else:
                        warnings.add("missing_model_context")
                    effort = payload.get("effort", payload.get("reasoning_effort"))
                    if isinstance(effort, str) and effort in EFFORTS:
                        efforts.add(effort)
                    else:
                        warnings.add("missing_reasoning_context")
                elif payload.get("type") == "token_count":
                    info = _object(payload.get("info"))
                    if "total_token_usage" not in info:
                        # Native rate-limit-only events have info=null.
                        continue
                    raw = _object(info["total_token_usage"])
                    usage = {
                        key: raw[key] if type(raw.get(key)) is int and raw[key] >= 0 else None
                        for key in TOKEN_FIELDS
                    }
                    if any(value is None for value in usage.values()):
                        warnings.add("invalid_or_missing_token_fields")
                    if previous_usage and any(
                        current is not None and prior is not None and current < prior
                        for current, prior in zip(usage.values(), previous_usage.values())
                    ):
                        warnings.add("cumulative_usage_decreased")
                    previous_usage = usage
    except OSError:
        # Do not serialize exception text: it can contain private input paths.
        warnings.add("unreadable_input")

    thread_id = next(iter(ids)) if len(ids) == 1 else None
    if not saw_meta or not ids:
        warnings.add("missing_thread_id")
    elif len(ids) > 1:
        warnings.add("conflicting_thread_ids")
    if thread_id is None:
        # Uncorrelated counters cannot safely participate in deduplicated totals.
        usage = dict.fromkeys(TOKEN_FIELDS)
    if len(parents) > 1:
        warnings.add("conflicting_parent_ids")
    model = next(iter(models)) if len(models) == 1 else None
    provider = MODEL_PROVIDERS.get(model) if model is not None else None
    if len(models) > 1:
        warnings.add("mixed_model_attribution")
    elif provider is None:
        warnings.add("unsupported_or_missing_model")
    if len(efforts) > 1:
        warnings.add("mixed_reasoning")
    if not efforts:
        warnings.add("missing_reasoning")
    if any(value is None for value in usage.values()):
        warnings.add("missing_usage")
    # An unlabelled turn could have contributed to the cumulative counter.
    if "missing_model_context" in warnings:
        provider = None
    row = {
        "id": thread_id,
        "parent_id": next(iter(parents)) if len(parents) == 1 else None,
        "model": model,
        "reasoning": next(iter(efforts)) if len(efforts) == 1 else None,
        **usage,
        "missing_usage": any(value is None for value in usage.values()),
        "provider": provider,
        "partial": bool(warnings),
    }
    # Retain only accounting metadata for duplicate comparison, never messages.
    fingerprint = (row, sorted(models), sorted(efforts), sorted(parents), sorted(warnings))
    return row, fingerprint, warnings


def _totals(rows, partial=False):
    result = {}
    for key in TOKEN_FIELDS:
        values = [row[key] for row in rows if row[key] is not None]
        result[key] = sum(values) if values or not rows else None
    result.update(
        thread_count=len(rows),
        missing_usage=sum(row["missing_usage"] for row in rows),
        partial=partial or any(row["partial"] for row in rows),
    )
    return result


def summarize_rollouts(paths: list[Path]) -> dict[str, object]:
    """Return allowlisted metadata and observed totals for explicitly given files.

    Repeated paths and equal accounting copies of an ID count once. Conflicting
    copies of an ID invalidate that thread's counters regardless of input order.
    Mixed/unsupported models retain thread counters in overall totals, but are
    excluded from provider totals. Transport model_provider is not model identity.
    Warnings contain only codes, input ordinals and sanitized thread IDs.
    """
    seen_paths, by_id, fingerprints = set(), {}, {}
    threads, warnings = [], []
    for ordinal, path in enumerate(paths, 1):
        path = Path(path)
        try:
            resolved = path.resolve()
        except (OSError, ValueError, RuntimeError):
            warnings.append({"code": "invalid_input_path", "input": ordinal})
            continue
        if resolved in seen_paths:
            continue
        seen_paths.add(resolved)
        row, fingerprint, problems = _read_rollout(resolved)
        for code in sorted(problems):
            warnings.append({"code": code, "input": ordinal, "thread_id": row["id"]})
        thread_id = row["id"]
        if thread_id is not None and thread_id in by_id:
            if fingerprints[thread_id] != fingerprint:
                old = by_id[thread_id]
                old.update(dict.fromkeys(TOKEN_FIELDS))
                old.update(missing_usage=True, partial=True)
                for key in ("parent_id", "model", "reasoning", "provider"):
                    if old[key] != row[key]:
                        old[key] = None
                warnings.append({"code": "conflicting_duplicate_id", "input": ordinal,
                                 "thread_id": thread_id})
            continue
        threads.append(row)
        if thread_id is not None:
            by_id[thread_id] = row
            # A copy is needed because conflict handling mutates the public row.
            fingerprints[thread_id] = (row.copy(), *fingerprint[1:])
    if not paths:
        warnings.append({"code": "no_inputs"})
    partial = bool(warnings)
    return {
        "threads": threads,
        "by_provider": {
            provider: _totals([row for row in threads if row["provider"] == provider], partial)
            for provider in ("OpenAI", "xai")
        },
        "totals": _totals(threads, partial),
        "unattributed": _totals([row for row in threads if row["provider"] is None], partial),
        "warnings": warnings,
        "partial": partial,
    }


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("paths", nargs="+", type=Path)
    parser.add_argument("--output", type=Path, help="JSON destination; stdout when omitted")
    args = parser.parse_args(argv)
    try:
        if args.output:
            output = args.output.resolve()
            if any(output == path.resolve() or (
                output.exists() and path.exists() and output.samefile(path)
            ) for path in args.paths):
                print("Output must not overwrite an input rollout.", file=sys.stderr)
                return 2
        report = summarize_rollouts(args.paths)
        rendered = json.dumps(report, indent=2, ensure_ascii=True) + "\n"
        if args.output:
            args.output.write_text(rendered, encoding="utf-8")
        else:
            print(rendered, end="")
    except (OSError, ValueError, RuntimeError):
        print("Unable to read inputs or write the report.", file=sys.stderr)
        return 2
    # Partial telemetry is represented in JSON, not a CLI execution failure.
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
