"""Redacted native Codex JSONL accounting; Python standard library only.

Usage:
  python tools/delegation-usage.py ROLLOUT [ROLLOUT ...] --output report.json
  python tools/delegation-usage.py ROLLOUT [ROLLOUT ...] --format markdown --output report.md

Only supplied files are read; descendants must be supplied explicitly. Thread
totals use the last cumulative token_count snapshot, never last_token_usage or
summed snapshots. Distinct model responses are counted once by response_id when
present; those per-response usage objects are request deltas, not extra totals
to add on top of the thread snapshot. Reasoning output is included in output,
not additional to it. Null means unknown. Totals of known thread snapshots are
observed tokens, not subscription quota or billing estimates.
"""

# Thread identity is session_meta.id. session_meta.session_id may name the shared
# parent or session context; that mismatch alone is not a second thread and must
# not drop the child's counters. A later session_meta that restates the parent id
# is inherited or forked history, not a reason to merge the child into the parent.

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import re
import sys
from datetime import datetime, timezone


TOKEN_FIELDS = (
    "input_tokens", "cached_input_tokens", "output_tokens",
    "reasoning_output_tokens", "total_tokens",
)
MODEL_PROVIDERS = {
    "gpt-6-astra": "OpenAI", "openai/gpt-6-astra": "OpenAI",
    "xai/grok-4.6": "xai", "grok-4.6": "xai",
}
EFFORTS = {"none", "minimal", "low", "medium", "high", "xhigh", "max"}
HOOK_MARKERS = (
    "<hook_prompt",
    "<subagent_notification",
    "<codex_internal_context",
)
CONTINUATION_MARKERS = (
    "<turn_aborted",
    "the user interrupted",
)
CONTINUATION_TRIGGER_PREFIXES = (
    "<turn_aborted",
)
CONTINUATION_TRIGGER_PHRASES = (
    "resume after tool-host restart",
    "resume user goal",
    "your turn ended",
)


def _identifier(value):
    # Allow only bounded metadata identifiers, never arbitrary message strings.
    if isinstance(value, str) and re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_./:-]{0,127}", value):
        return value
    return None


def _object(value):
    return value if isinstance(value, dict) else {}


def _nonneg_int(value):
    return value if type(value) is int and value >= 0 else None


def _usage_from(raw):
    raw = _object(raw)
    return {
        key: _nonneg_int(raw.get(key)) if key in raw else None
        for key in TOKEN_FIELDS
    }


def _message_text(payload):
    content = payload.get("content")
    if isinstance(content, str):
        return content
    parts = []
    if isinstance(content, list):
        for item in content:
            if isinstance(item, dict):
                for key in ("text", "input_text"):
                    value = item.get(key)
                    if isinstance(value, str):
                        parts.append(value)
    return "".join(parts)


def _starts_with_marker(text, markers):
    stripped = text.lstrip()
    lowered = stripped[:96].lower()
    return any(stripped.startswith(marker) or lowered.startswith(marker.lower()) for marker in markers)


def _is_continuation_trigger(text):
    stripped = text.lstrip()
    lowered = stripped[:160].lower()
    if any(stripped.startswith(prefix) or lowered.startswith(prefix) for prefix in CONTINUATION_TRIGGER_PREFIXES):
        return True
    return any(phrase in lowered for phrase in CONTINUATION_TRIGGER_PHRASES)


def _parse_timestamp(value):
    if not isinstance(value, str) or not value:
        return None
    try:
        parsed = datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError:
        return None
    if parsed.tzinfo is None:
        parsed = parsed.replace(tzinfo=timezone.utc)
    return parsed.astimezone(timezone.utc)


def _elapsed_seconds(timestamps):
    parsed = [stamp for stamp in (_parse_timestamp(value) for value in timestamps) if stamp is not None]
    if len(parsed) < 2:
        return None
    return max(0, int((max(parsed) - min(parsed)).total_seconds()))


def _project_label(cwd, roots):
    candidates = []
    if isinstance(cwd, str) and cwd.strip():
        candidates.append(cwd)
    if isinstance(roots, list):
        candidates.extend(root for root in roots if isinstance(root, str) and root.strip())
    labels = []
    seen = set()
    for candidate in candidates:
        name = Path(candidate).name or "workspace"
        if name not in seen:
            seen.add(name)
            labels.append(name)
    if not labels:
        return None
    if len(labels) == 1:
        return labels[0]
    return "mixed"


GENERIC_PROJECTS = {
    "codex-harness", "pmac-emulator", "fixture", "neutral", "direct", "levels",
    "mixed", "project", "projects", "unknown", "workspace with spaces",
}


def _public_project(label):
    if label in GENERIC_PROJECTS or label is None:
        return label or "unknown"
    if " with spaces" in label:
        return "workspace with spaces"
    digest = hashlib.sha256(label.encode("utf-8")).hexdigest()[:8]
    return f"workspace-{digest}"


def _sha256_file(path):
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _count_actual_continuations(triggers, turn_stamps, ordinary_user_stamps):
    parsed_triggers = sorted(stamp for stamp in (_parse_timestamp(value) for value in triggers) if stamp is not None)
    parsed_turns = sorted(stamp for stamp in (_parse_timestamp(value) for value in turn_stamps) if stamp is not None)
    parsed_users = sorted(stamp for stamp in (_parse_timestamp(value) for value in ordinary_user_stamps) if stamp is not None)
    claimed = set()
    counted = 0
    for trigger in parsed_triggers:
        next_turn = next((turn for turn in parsed_turns if turn > trigger), None)
        if next_turn is None:
            continue
        next_user = next((user for user in parsed_users if user > trigger), None)
        if next_user is not None and next_user < next_turn:
            continue
        if next_turn in claimed:
            continue
        claimed.add(next_turn)
        counted += 1
    return counted


def _read_rollout(path):
    thread_ids, parents, models, efforts = [], set(), set(), set()
    usage: dict[str, int | None] = dict.fromkeys(TOKEN_FIELDS)
    warnings = set()
    previous_usage = None
    saw_meta = False
    cwd_values, turn_ids, root_turn_ids = set(), set(), set()
    response_usages = {}
    response_conflicts = set()
    timestamps = []
    spawned_children, compaction_ids = set(), set()
    task_started = task_complete = turn_aborted = 0
    hook_messages = hook_chars = continuation_notices = 0
    user_messages = commentary_messages = final_messages = 0
    spawn_calls = compacted_windows = 0
    turn_durations = []
    cli_versions = set()
    continuation_triggers = []
    model_turn_stamps = []
    session_ids, forked_from_ids = set(), set()
    ordinary_user_stamps = []
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
                if kind not in {"session_meta", "turn_context", "event_msg", "token_usage_record", "response_item", "compacted"}:
                    continue
                payload = event.get("payload")
                if not isinstance(payload, dict):
                    warnings.add("invalid_payload")
                    continue
                if kind == "session_meta":
                    saw_meta = True
                    raw_id = payload.get("id")
                    if raw_id is not None:
                        value = _identifier(raw_id)
                        if value:
                            if value not in thread_ids:
                                thread_ids.append(value)
                        else:
                            warnings.add("invalid_thread_id")
                    raw_session = payload.get("session_id")
                    if raw_session is not None:
                        session = _identifier(raw_session)
                        if session:
                            session_ids.add(session)
                        else:
                            warnings.add("invalid_thread_id")
                    version = _identifier(payload.get("cli_version"))
                    if version:
                        cli_versions.add(version)
                    forked = _identifier(payload.get("forked_from_id"))
                    if forked:
                        forked_from_ids.add(forked)
                        warnings.add("forked_or_compacted_history")
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
                    cwd_values.add(payload.get("cwd"))
                    roots = payload.get("workspace_roots")
                    if isinstance(roots, list):
                        for root in roots:
                            cwd_values.add(root)
                    turn = _identifier(payload.get("turn_id"))
                    if turn:
                        turn_ids.add(turn)
                    root_turn = _identifier(payload.get("root_turn_id"))
                    if root_turn:
                        root_turn_ids.add(root_turn)
                elif payload.get("type") == "token_count":
                    info = _object(payload.get("info"))
                    if "total_token_usage" not in info:
                        # Native rate-limit-only events have info=null.
                        continue
                    usage = _usage_from(info["total_token_usage"])
                    if any(value is None for value in usage.values()):
                        warnings.add("invalid_or_missing_token_fields")
                    if previous_usage and any(
                        current is not None and prior is not None and current < prior
                        for current, prior in zip(usage.values(), previous_usage.values())
                    ):
                        warnings.add("cumulative_usage_decreased")
                    previous_usage = usage
                    cached, inp = usage.get("cached_input_tokens"), usage.get("input_tokens")
                    if isinstance(cached, int) and isinstance(inp, int) and cached > inp:
                        warnings.add("cached_input_exceeds_input")
                elif kind == "event_msg" and payload.get("type") == "task_started":
                    task_started += 1
                    if isinstance(event.get("timestamp"), str):
                        model_turn_stamps.append(event.get("timestamp"))
                elif kind == "event_msg" and payload.get("type") == "task_complete":
                    task_complete += 1
                    duration = _nonneg_int(payload.get("duration_ms"))
                    if duration is not None:
                        turn_durations.append(duration)
                elif kind == "event_msg" and payload.get("type") == "turn_aborted":
                    turn_aborted += 1
                elif kind == "token_usage_record":
                    response_id = _identifier(payload.get("response_id"))
                    record_usage = _usage_from(payload.get("usage"))
                    if response_id:
                        previous = response_usages.get(response_id)
                        if previous is None:
                            response_usages[response_id] = record_usage
                            if isinstance(event.get("timestamp"), str):
                                model_turn_stamps.append(event.get("timestamp"))
                        elif previous != record_usage:
                            response_usages[response_id] = dict.fromkeys(TOKEN_FIELDS)
                            response_conflicts.add(response_id)
                            warnings.add("conflicting_response_id")
                    else:
                        warnings.add("missing_response_id")
                    if any(value is None for value in record_usage.values()):
                        warnings.add("invalid_or_missing_token_fields")
                    cached, inp = record_usage.get("cached_input_tokens"), record_usage.get("input_tokens")
                    if isinstance(cached, int) and isinstance(inp, int) and cached > inp:
                        warnings.add("cached_input_exceeds_input")
                    if isinstance(record_usage.get("output_tokens"), int) and isinstance(record_usage.get("reasoning_output_tokens"), int):
                        if record_usage["reasoning_output_tokens"] > record_usage["output_tokens"]:
                            warnings.add("reasoning_not_included_in_output")
                elif kind == "response_item":
                    item_type = payload.get("type")
                    if item_type == "message":
                        text = _message_text(payload)
                        role = payload.get("role")
                        if role == "user":
                            if _starts_with_marker(text, HOOK_MARKERS):
                                hook_messages += 1
                                hook_chars += len(text)
                            elif _is_continuation_trigger(text) or _starts_with_marker(text, CONTINUATION_MARKERS):
                                continuation_notices += 1
                                if isinstance(event.get("timestamp"), str):
                                    continuation_triggers.append(event.get("timestamp"))
                            else:
                                user_messages += 1
                                if isinstance(event.get("timestamp"), str):
                                    ordinary_user_stamps.append(event.get("timestamp"))
                        elif payload.get("phase") == "commentary":
                            commentary_messages += 1
                        elif payload.get("phase") == "final_answer":
                            final_messages += 1
                    elif item_type == "function_call":
                        name = payload.get("name")
                        if isinstance(name, str) and name == "spawn_agent":
                            spawn_calls += 1
                elif kind == "compacted":
                    compacted_windows += 1
                    compaction_id = _identifier(payload.get("compaction_response_id"))
                    if compaction_id:
                        compaction_ids.add(compaction_id)
                stamp = event.get("timestamp")
                if isinstance(stamp, str):
                    timestamps.append(stamp)
                if kind == "event_msg" and payload.get("type") == "item_completed":
                    for child in payload.get("item", {}).get("receiver_thread_ids", []) if isinstance(payload.get("item"), dict) else []:
                        child_id = _identifier(child)
                        if child_id:
                            spawned_children.add(child_id)
    except OSError:
        # Do not serialize exception text: it can contain private input paths.
        warnings.add("unreadable_input")

    inherited = session_ids | parents | forked_from_ids
    primary = thread_ids[0] if thread_ids else None
    extra_ids = [value for value in thread_ids[1:] if value != primary]
    if extra_ids and (parents or forked_from_ids) and all(value in inherited for value in extra_ids):
        warnings.add("forked_or_compacted_history")
        thread_id = primary
    elif extra_ids:
        warnings.add("conflicting_thread_ids")
        thread_id = None
    else:
        thread_id = primary
    if not saw_meta or not thread_ids:
        warnings.add("missing_thread_id")
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
    if thread_id is None:
        response_usages = {}
        spawned_children = set()
    projects = {
        _project_label(value, None) if isinstance(value, str) else None
        for value in cwd_values
        if isinstance(value, str) and value.strip()
    }
    projects.discard(None)
    project = next(iter(projects)) if len(projects) == 1 else ("mixed" if projects else None)
    if len(projects) > 1:
        warnings.add("mixed_project")
    response_count = len(response_usages)
    if compacted_windows:
        warnings.add("forked_or_compacted_history")
    if turn_aborted:
        warnings.add("interrupted_turn")
    actual_continuations = _count_actual_continuations(
        continuation_triggers, model_turn_stamps, ordinary_user_stamps,
    )
    if spawned_children:
        # Presence of spawn records is not proof that child rollouts were supplied.
        pass
    row = {
        "id": thread_id,
        "parent_id": next(iter(parents)) if len(parents) == 1 else None,
        "model": model,
        "reasoning": next(iter(efforts)) if len(efforts) == 1 else None,
        **usage,
        "missing_usage": any(value is None for value in usage.values()),
        "provider": provider,
        "partial": bool(warnings),
        "project": project,
        "cli_version": next(iter(cli_versions)) if len(cli_versions) == 1 else None,
        "elapsed_seconds": _elapsed_seconds(timestamps),
        "response_count": response_count,
        "response_ids": sorted(response_usages),
        "response_usages": {key: dict(value) for key, value in response_usages.items()},
        "response_conflict_ids": sorted(response_conflicts),
        "spawned_child_ids": sorted(spawned_children),
        "spawn_calls": spawn_calls,
        "hook_messages": hook_messages,
        "hook_chars": hook_chars,
        "continuation_notices": continuation_notices,
        "actual_continuations": actual_continuations,
        "task_started": task_started,
        "task_complete": task_complete,
        "turn_aborted": turn_aborted,
        "commentary_messages": commentary_messages,
        "final_messages": final_messages,
        "user_messages": user_messages,
        "compacted_windows": compacted_windows,
        "turn_duration_ms_sum": sum(turn_durations) if turn_durations else None,
        "first_timestamp": min((stamp for stamp in timestamps if _parse_timestamp(stamp)), default=None, key=_parse_timestamp) if timestamps else None,
        "last_timestamp": max((stamp for stamp in timestamps if _parse_timestamp(stamp)), default=None, key=_parse_timestamp) if timestamps else None,
    }
    # Retain only accounting metadata for duplicate comparison, never messages.
    fingerprint = (
        {key: row[key] for key in ("id", "parent_id", "model", "reasoning", *TOKEN_FIELDS, "provider")},
        sorted(models), sorted(efforts), sorted(parents), sorted(warnings),
        tuple(sorted((key, tuple(sorted(value.items()))) for key, value in response_usages.items())),
    )
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
                token_conflict = any(old.get(key) != row.get(key) for key in TOKEN_FIELDS)
                identity_conflict = any(old.get(key) != row.get(key) for key in ("parent_id", "model", "reasoning", "provider"))
                if token_conflict or identity_conflict:
                    old.update(dict.fromkeys(TOKEN_FIELDS))
                    old["missing_usage"] = True
                old["partial"] = True
                for key in ("parent_id", "model", "reasoning", "provider"):
                    if old[key] != row[key]:
                        old[key] = None
                old_usages = old.get("response_usages") or {}
                new_usages = row.get("response_usages") or {}
                merged = dict(old_usages)
                conflicts = set(old.get("response_conflict_ids", [])) | set(row.get("response_conflict_ids", []))
                for response_id, record_usage in new_usages.items():
                    previous = merged.get(response_id)
                    if previous is None:
                        merged[response_id] = dict(record_usage)
                    elif previous != record_usage:
                        merged[response_id] = dict.fromkeys(TOKEN_FIELDS)
                        conflicts.add(response_id)
                old["response_usages"] = merged
                old["response_conflict_ids"] = sorted(conflicts)
                old["response_ids"] = sorted(merged)
                old["response_count"] = len(merged)
                warnings.append({"code": "conflicting_duplicate_id", "input": ordinal,
                                 "thread_id": thread_id})
            continue
        threads.append(row)
        if thread_id is not None:
            by_id[thread_id] = row
            # A copy is needed because conflict handling mutates the public row.
            fingerprints[thread_id] = fingerprint
    if not paths:
        warnings.append({"code": "no_inputs"})
    missing_children = _missing_children(threads)
    for item in missing_children:
        warnings.append({"code": "missing_child", "thread_id": item.get("parent_id")})
    if _overlapping_elapsed(threads):
        warnings.append({"code": "concurrent_or_overlapping_elapsed"})
    partial = bool(warnings)
    for row in threads:
        row["partial"] = bool(row["partial"] or any(
            item.get("parent_id") == row.get("id") for item in missing_children
        ))
    return {
        "threads": threads,
        "by_provider": {
            provider: _totals([row for row in threads if row["provider"] == provider], partial)
            for provider in ("OpenAI", "xai")
        },
        "totals": _totals(threads, partial),
        "unattributed": _totals([row for row in threads if row["provider"] is None], partial),
        "responses": _response_totals(threads, partial),
        "missing_children": missing_children,
        "overlapping_elapsed": _overlapping_elapsed(threads),
        "limitation": "Observed tokens from supplied rollouts; not weekly quota, billing, or an exact saving.",
        "warnings": warnings,
        "partial": partial,
    }


def _interval(row):
    elapsed = row.get("elapsed_seconds")
    if elapsed is None:
        return None
    return elapsed


def _missing_children(threads):
    known = {row["id"] for row in threads if row.get("id")}
    missing = []
    for row in threads:
        for child in row.get("spawned_child_ids") or []:
            if child not in known:
                missing.append({"parent_id": row.get("id"), "child_id": child})
        expected = row.get("spawn_calls") or 0
        if expected and not row.get("spawned_child_ids") and row.get("id"):
            missing.append({"parent_id": row.get("id"), "child_id": None})
    return missing


def _overlapping_elapsed(threads):
    intervals = []
    for row in threads:
        start = _parse_timestamp(row.get("first_timestamp"))
        end = _parse_timestamp(row.get("last_timestamp"))
        if start is None or end is None or end < start:
            continue
        intervals.append((start, end))
    for index, (left_start, left_end) in enumerate(intervals):
        for right_start, right_end in intervals[index + 1:]:
            if left_start <= right_end and right_start <= left_end:
                return True
    return False


def _response_totals(threads, partial=False):
    seen = {}
    conflicting = {identity for row in threads for identity in row.get("response_conflict_ids", [])}
    for row in threads:
        for response_id, usage in (row.get("response_usages") or {}).items():
            previous = seen.get(response_id)
            if previous is None:
                seen[response_id] = dict(usage)
            elif previous != usage:
                seen[response_id] = dict.fromkeys(TOKEN_FIELDS)
                conflicting.add(response_id)
    rows = [{"missing_usage": any(value is None for value in usage.values()), "partial": any(value is None for value in usage.values()), **usage} for usage in seen.values()]
    totals = _totals(rows, partial or bool(conflicting))
    totals["response_count"] = len(seen)
    totals["conflicting_response_ids"] = len(conflicting)
    return totals


def compact_markdown(report: dict[str, object]) -> str:
    totals = report.get("totals") or {}
    responses = report.get("responses") or {}
    threads = report.get("threads") or []
    sources = report.get("sources") or []
    missing = report.get("missing_children") or []
    # Cumulative snapshots can contain inherited history. Keep legacy JSON fields
    # for existing consumers, but do not promote their sum as reconciled usage.
    reconciled = responses.get("total_tokens") if not report.get("partial") and responses.get("total_tokens") == totals.get("total_tokens") else "unknown"
    response_input, response_cached = responses.get("input_tokens"), responses.get("cached_input_tokens")
    uncached = response_input - response_cached if isinstance(response_input, int) and isinstance(response_cached, int) and response_input >= response_cached else "unknown"
    lines = [
        "# Subscription usage from existing local rollouts",
        "",
        "Observed native JSONL accounting only. Token counts are not weekly quota",
        "percentages, billing, or proof that a later task caused a reset-window change.",
        "Reasoning output is included in output. Cumulative thread snapshots are not",
        "summed with per-response deltas. Raw logs, identities and input hashes stay",
        "in host-private evidence.",
        "",
        "| Category | Count / tokens | Notes |",
        "| --- | ---: | --- |",
        f"| CLI versions | {', '.join(sorted({row.get('cli_version') for row in threads if row.get('cli_version')}) or ['unknown'])} | Native session_meta |",
        f"| Threads supplied | {totals.get('thread_count', 0)} | Explicit inputs only |",
        f"| Distinct response IDs | {responses.get('response_count', 0)} | Counted once |",
        f"| Thread input | {totals.get('input_tokens')} | Last cumulative snapshot per thread |",
        f"| Cached input | {totals.get('cached_input_tokens')} | Included in input |",
        f"| Thread output | {totals.get('output_tokens')} | Includes reasoning |",
        f"| Reasoning output | {totals.get('reasoning_output_tokens')} | Not added again |",
        f"| Reconciled total | {reconciled} | Unknown when coverage is partial or the series disagree; not a quota share |",
        f"| Cumulative snapshot sum | {totals.get('total_tokens')} | Last snapshot per supplied thread; inherited history can overlap |",
        f"| Per-response delta sum | {responses.get('total_tokens')} | Deduplicated stable response IDs; partial observed series, not added to cumulative snapshots |",
        f"| Response input / cached / uncached | {response_input} / {response_cached} / {uncached} | Cached input is included in input |",
        f"| Automatic context occurrences / chars | {sum(row.get('hook_messages') or 0 for row in threads)} / {sum(row.get('hook_chars') or 0 for row in threads)} | Recognized hook/subagent/internal-context markers; inherited or repeated occurrences are not deduplicated findings |",
        f"| Ordinary turns started | {sum(row.get('task_started') or 0 for row in threads)} | Not counted as continuation |",
        f"| Commentary messages | {sum(row.get('commentary_messages') or 0 for row in threads)} | Ordinary status; not continuation |",
        f"| Continuation notices | {sum(row.get('continuation_notices') or 0 for row in threads)} | Interrupt/timeout/resume trigger text |",
        f"| Actual continuations | {sum(row.get('actual_continuations') or 0 for row in threads)} | Distinct resumptions correlated with the next later turn before an ordinary user request; heuristic, not causal proof |",
        f"| Missing children | {len(missing)} | Spawn recorded, rollout not supplied |",
        f"| Compacted windows | {sum(row.get('compacted_windows') or 0 for row in threads)} | Compaction envelopes are not extra responses; cumulative inheritance may overlap |",
        "",
        "## Model / effort / project",
        "",
        "| Role | Model | Provider | Effort | Project | Elapsed s | Responses | Partial |",
        "| --- | --- | --- | --- | --- | ---: | ---: | --- |",
    ]
    groups = {}
    for row in threads:
        key = (
            "child" if row.get("parent_id") else "root",
            row.get("model") or "unknown",
            row.get("provider") or "unknown",
            row.get("reasoning") or "unknown",
            _public_project(row.get("project")),
        )
        group = groups.setdefault(key, {"threads": 0, "elapsed": 0, "elapsed_known": False, "responses": 0, "partial": False})
        group["threads"] += 1
        if row.get("elapsed_seconds") is not None:
            group["elapsed"] += row["elapsed_seconds"]
            group["elapsed_known"] = True
        group["responses"] += row.get("response_count") or 0
        group["partial"] = group["partial"] or bool(row.get("partial"))
    for key, group in sorted(groups.items()):
        role, model, provider, effort, project = key
        elapsed = group["elapsed"] if group["elapsed_known"] else "unknown"
        lines.append(
            f"| {role} x{group['threads']} | {model} | {provider} | {effort} | {project} | {elapsed} | {group['responses']} | {'yes' if group['partial'] else 'no'} |"
        )
    if missing:
        lines.append("| missing-child | unknown | unknown | unknown | unknown | unknown | 0 | yes |")
    lines.extend([
        "",
        "## Limitations",
        "",
        "- Partial records stay explicit; missing usage is null, not zero.",
        "- Concurrent parent/child elapsed time is not attributed to one task.",
        "- Quota snapshots from different reset windows are incomparable.",
        "- Cumulative snapshot sums and deduplicated response deltas are separate series. Their disagreement is unresolved; neither is a reconciled complete total when coverage is partial.",
        "- Automatic-context counts are recognized marker occurrences, not unique findings.",
        "- Actual continuations are a timestamp heuristic: an intervening ordinary user request breaks correlation, and two triggers sharing one resumed turn count once.",
        "- Source transcripts, credentials and absolute paths are omitted.",
        f"- Private source count: {len(sources)} hashed inputs retained only in host evidence.",
        "",
    ])
    warning_codes = sorted({item.get("code") for item in report.get("warnings") or [] if item.get("code")})
    if warning_codes:
        lines.append("Warning codes: " + ", ".join(warning_codes) + ".")
        lines.append("")
    return "\n".join(lines)


def private_source_records(paths: list[Path], threads: list[dict]) -> list[dict[str, object]]:
    records = []
    by_ordinal = {}
    for row in threads:
        by_ordinal.setdefault(row.get("id"), []).append(row)
    for ordinal, path in enumerate(paths, 1):
        path = Path(path)
        try:
            resolved = path.resolve()
            digest = _sha256_file(resolved)
            size = resolved.stat().st_size
        except (OSError, ValueError, RuntimeError):
            continue
        records.append({
            "input": ordinal,
            "sha256": digest,
            "bytes": size,
            "suffix": resolved.suffix.lower(),
        })
    return records


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("paths", nargs="*", type=Path)
    parser.add_argument("--output", type=Path, help="Report destination; stdout when omitted")
    parser.add_argument("--format", choices=("json", "markdown"), default="json")
    parser.add_argument("--private-sources", type=Path, help="Host-only JSON of input hashes; never write under Git")
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
        if args.private_sources:
            private = args.private_sources.resolve()
            if any(private == path.resolve() or (
                private.exists() and path.exists() and private.samefile(path)
            ) for path in args.paths):
                print("Private source file must not overwrite an input rollout.", file=sys.stderr)
                return 2
            sources = private_source_records(args.paths, report["threads"])
            report["sources"] = [{"input": item["input"], "bytes": item["bytes"]} for item in sources]
            private.write_text(json.dumps({"sources": sources}, indent=2, ensure_ascii=True) + "\n", encoding="utf-8")
        if args.format == "markdown":
            rendered = compact_markdown(report)
        else:
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
