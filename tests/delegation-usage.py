"""Deterministic, offline tests: python -B tests/delegation-usage.py."""

import importlib.util
import itertools
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


SCRIPT = Path(__file__).resolve().parents[1] / "tools" / "delegation-usage.py"
spec = importlib.util.spec_from_file_location("delegation_usage", SCRIPT)
assert spec is not None and spec.loader is not None
usage = importlib.util.module_from_spec(spec)
spec.loader.exec_module(usage)


def meta(thread="parent", parent=None):
    payload = {"id": thread, "session_id": thread, "model_provider": "openai"}
    if parent:
        payload["source"] = {"subagent": {"thread_spawn": {"parent_thread_id": parent}}}
    return {"type": "session_meta", "payload": payload}


def context(model="gpt-6-astra", effort="high"):
    return {"type": "turn_context", "payload": {"model": model, "effort": effort}}


def tokens(amount=10, **overrides):
    totals = {
        "input_tokens": amount, "cached_input_tokens": amount // 2,
        "output_tokens": amount // 2, "reasoning_output_tokens": amount // 5,
        "total_tokens": amount + amount // 2,
    }
    totals.update(overrides)
    return {"type": "event_msg", "payload": {"type": "token_count", "info": {
        "total_token_usage": totals,
        "last_token_usage": {key: 999999 for key in totals},
    }}}


class UsageTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)

    def rollout(self, name, *events):
        path = self.root / name
        path.write_text("".join(json.dumps(event) + "\n" for event in events), encoding="utf-8")
        return path

    def codes(self, report):
        return {warning["code"] for warning in report["warnings"]}

    def test_last_cumulative_snapshot_once_and_duplicate_paths_and_ids(self):
        a = self.rollout("a.jsonl", meta(), context(), tokens(), tokens(20), tokens(20))
        b = self.rollout("copy.jsonl", meta(), context(), tokens(20))
        report = usage.summarize_rollouts([a, a, a.parent / "." / a.name, b])
        self.assertEqual(len(report["threads"]), 1)
        self.assertEqual(report["totals"]["total_tokens"], 30)
        self.assertEqual(report["by_provider"]["OpenAI"]["cached_input_tokens"], 10)
        self.assertEqual(report["totals"]["reasoning_output_tokens"], 4)
        self.assertFalse(report["partial"])

    def test_parent_child_grandchild_multiple_providers(self):
        parent = self.rollout("p", meta(), context(), tokens(20))
        child = self.rollout("c", meta("child", "parent"), context("xai/grok-4.6", "xhigh"), tokens(40))
        grandchild = self.rollout("g", meta("grandchild", "child"), context(effort="max"), tokens(10))
        report = usage.summarize_rollouts([parent, child, grandchild])
        self.assertEqual([row["parent_id"] for row in report["threads"]], [None, "parent", "child"])
        self.assertEqual(report["by_provider"]["OpenAI"]["total_tokens"], 45)
        self.assertEqual(report["by_provider"]["xai"]["total_tokens"], 60)
        self.assertEqual(report["totals"]["total_tokens"], 105)
        self.assertFalse(report["partial"])

    def test_top_level_parent_metadata(self):
        event = meta("child")
        event["payload"]["parent_thread_id"] = "parent"
        path = self.rollout("c", event, context(), tokens())
        self.assertEqual(usage.summarize_rollouts([path])["threads"][0]["parent_id"], "parent")

    def test_missing_usage_is_not_zero(self):
        missing = self.rollout("missing", meta("missing"), context())
        zero = self.rollout("zero", meta("zero"), context(), tokens(0))
        report = usage.summarize_rollouts([missing, zero])
        self.assertIsNone(report["threads"][0]["total_tokens"])
        self.assertTrue(report["threads"][0]["missing_usage"])
        self.assertEqual(report["threads"][1]["total_tokens"], 0)
        self.assertFalse(report["threads"][1]["missing_usage"])
        self.assertTrue(report["by_provider"]["OpenAI"]["partial"])
        self.assertIsNone(usage.summarize_rollouts([missing])["totals"]["total_tokens"])

    def test_rate_limit_only_event_does_not_erase_total(self):
        path = self.rollout("a", meta(), context(), tokens(),
                            {"type": "event_msg", "payload": {"type": "token_count", "info": None}})
        self.assertEqual(usage.summarize_rollouts([path])["totals"]["total_tokens"], 15)

    def test_latest_invalid_counter_is_unknown_not_earlier_total(self):
        for value in (None, True, -1, 1.5, "20", [], {}):
            with self.subTest(value=value):
                path = self.rollout("a", meta(), context(), tokens(), tokens(20, total_tokens=value))
                report = usage.summarize_rollouts([path])
                self.assertIsNone(report["threads"][0]["total_tokens"])
                self.assertEqual(report["threads"][0]["input_tokens"], 20)
                self.assertTrue(report["partial"])

    def test_missing_field_not_invented(self):
        event = tokens()
        del event["payload"]["info"]["total_token_usage"]["cached_input_tokens"]
        path = self.rollout("a", meta(), context(), event)
        report = usage.summarize_rollouts([path])
        self.assertIsNone(report["totals"]["cached_input_tokens"])
        self.assertEqual(report["totals"]["total_tokens"], 15)

    def test_unknown_and_mixed_models_not_credited_to_provider(self):
        for contexts, code in (
            ([context("other/model")], "unsupported_or_missing_model"),
            ([context("gpt-5.6-terra")], "unsupported_or_missing_model"),
            ([context(), context("xai/grok-4.6")], "mixed_model_attribution"),
            ([context(), context(None)], "missing_model_context"),
        ):
            with self.subTest(code=code):
                path = self.rollout("a", meta(), *contexts, tokens())
                report = usage.summarize_rollouts([path])
                self.assertIsNone(report["threads"][0]["provider"])
                self.assertEqual(report["totals"]["total_tokens"], 15)
                self.assertEqual(report["unattributed"]["total_tokens"], 15)
                self.assertEqual(report["by_provider"]["OpenAI"]["thread_count"], 0)
                self.assertIn(code, self.codes(report))

    def test_mixed_reasoning_is_not_last_effort(self):
        path = self.rollout("a", meta(), context(), tokens(), context(effort="max"), tokens(20))
        report = usage.summarize_rollouts([path])
        self.assertIsNone(report["threads"][0]["reasoning"])
        self.assertIn("mixed_reasoning", self.codes(report))

    def test_corrupt_truncated_and_invalid_utf8_salvages_known_usage(self):
        path = self.rollout("a", meta(), context(), tokens())
        with path.open("ab") as stream:
            stream.write(b'\xff\n{"secret":"DO_NOT_LEAK"\n')
        report = usage.summarize_rollouts([path])
        self.assertEqual(report["totals"]["total_tokens"], 15)
        self.assertIn("corrupt_jsonl", self.codes(report))
        self.assertTrue(report["partial"])
        self.assertNotIn("DO_NOT_LEAK", json.dumps(report))

    def test_unreadable_empty_and_malformed_shapes(self):
        path = self.rollout("a", [], {"type": []}, {"type": "event_msg", "payload": []})
        report = usage.summarize_rollouts([path, self.root / "DO_NOT_LEAK"])
        self.assertTrue(report["partial"])
        self.assertIsNone(report["totals"]["total_tokens"])
        self.assertIn("unreadable_input", self.codes(report))
        self.assertNotIn("DO_NOT_LEAK", json.dumps(report))
        self.assertIn("no_inputs", self.codes(usage.summarize_rollouts([])))

    def test_missing_or_conflicting_identity_cannot_contribute(self):
        missing = self.rollout("missing", context(), tokens())
        missing_report = usage.summarize_rollouts([missing])
        self.assertIsNone(missing_report["threads"][0]["id"])
        self.assertIsNone(missing_report["totals"]["total_tokens"])
        self.assertTrue(missing_report["partial"])
        conflicting = self.rollout("conflict", meta("a"), meta("unrelated"), context(), tokens())
        conflicting_report = usage.summarize_rollouts([conflicting])
        self.assertIsNone(conflicting_report["threads"][0]["id"])
        self.assertIsNone(conflicting_report["totals"]["total_tokens"])
        self.assertIn("conflicting_thread_ids", self.codes(conflicting_report))

    def test_child_session_id_is_shared_context_not_a_second_thread(self):
        parent = self.rollout("p", meta(), context(), tokens(10),
                              {"type": "event_msg", "payload": {"type": "item_completed", "item": {
                                  "type": "CollabAgentToolCall", "receiver_thread_ids": ["child"]}}})
        child_meta = meta("child", "parent")
        child_meta["payload"]["session_id"] = "parent"
        child_meta["payload"]["parent_thread_id"] = "parent"
        child = self.rollout(
            "c", child_meta, context("xai/grok-4.6", "xhigh"), tokens(40),
            response_record("resp_child", 40, 60),
        )
        report = usage.summarize_rollouts([parent, child])
        by_id = {row["id"]: row for row in report["threads"]}
        self.assertEqual(set(by_id), {"parent", "child"})
        self.assertEqual(by_id["child"]["parent_id"], "parent")
        self.assertEqual(by_id["child"]["total_tokens"], 60)
        self.assertEqual(by_id["child"]["response_count"], 1)
        self.assertEqual(by_id["child"]["response_usages"]["resp_child"]["total_tokens"], 60)
        self.assertEqual(report["totals"]["total_tokens"], 75)
        self.assertEqual(report["responses"]["response_count"], 1)
        self.assertNotIn("conflicting_thread_ids", self.codes(report))

    def test_inherited_second_meta_keeps_child_id(self):
        first = meta("child", "parent")
        first["payload"]["session_id"] = "parent"
        first["payload"]["parent_thread_id"] = "parent"
        first["payload"]["forked_from_id"] = "parent"
        restated = meta("parent")
        restated["payload"]["session_id"] = "parent"
        path = self.rollout(
            "c", first, restated, context("xai/grok-4.6", "xhigh"), tokens(40),
            response_record("resp_child", 40, 60),
        )
        row = usage.summarize_rollouts([path])["threads"][0]
        self.assertEqual(row["id"], "child")
        self.assertEqual(row["parent_id"], "parent")
        self.assertEqual(row["total_tokens"], 60)
        self.assertEqual(row["response_count"], 1)
        self.assertIn("forked_or_compacted_history", self.codes(usage.summarize_rollouts([path])))

    def test_conflicting_duplicates_invalidate_usage_in_every_order(self):
        a = self.rollout("a", meta(), context(), tokens(10))
        b = self.rollout("b", meta(), context(), tokens(20))
        c = self.rollout("c", meta(), context(), tokens(10))
        for paths in itertools.permutations([a, b, c]):
            report = usage.summarize_rollouts(list(paths))
            self.assertEqual(len(report["threads"]), 1)
            self.assertIsNone(report["totals"]["total_tokens"])
            self.assertIn("conflicting_duplicate_id", self.codes(report))

    def test_conflicting_duplicate_parent_and_model_not_claimed(self):
        a = self.rollout("a", meta("child", "one"), context(), tokens())
        b = self.rollout("b", meta("child", "two"), context("xai/grok-4.6"), tokens())
        report = usage.summarize_rollouts([a, b])
        row = report["threads"][0]
        self.assertIsNone(row["parent_id"])
        self.assertIsNone(row["model"])
        self.assertIsNone(row["provider"])
        self.assertIsNone(row["total_tokens"])

    def test_cumulative_decrease_uses_last_and_warns(self):
        path = self.rollout("a", meta(), context(), tokens(40), tokens(10))
        report = usage.summarize_rollouts([path])
        self.assertEqual(report["totals"]["total_tokens"], 15)
        self.assertIn("cumulative_usage_decreased", self.codes(report))

    def test_public_markdown_has_no_named_workspace_exceptions(self):
        for name in ("fixture", "neutral", "direct", "synthetic-private-consumer"):
            with self.subTest(name=name):
                turn = context()
                turn["payload"]["cwd"] = str(self.root / name)
                path = self.rollout("input.jsonl", meta(), turn, tokens())
                result = subprocess.run(
                    [sys.executable, "-B", str(SCRIPT), str(path), "--format", "markdown"],
                    cwd=self.root, text=True, capture_output=True, check=False,
                )
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertNotIn(f"| {name} |", result.stdout)
                self.assertNotIn(str(self.root / name), result.stdout)
                self.assertIn("workspace-", result.stdout)
                self.assertEqual(usage.summarize_rollouts([path])["totals"]["total_tokens"], 15)

    def test_redaction_and_actual_cli_output(self):
        event = meta()
        event["payload"].update(base_instructions="PRIVATE_PROMPT", auth="RAW_AUTH", cwd="PRIVATE_PATH")
        path = self.rollout("PRIVATE_FILENAME", event, context(), tokens(),
                            {"type": "response_item", "payload": {"content": "PRIVATE_MESSAGE"}})
        output = self.root / "report.json"
        result = subprocess.run([sys.executable, "-B", str(SCRIPT), str(path), "--output", str(output)],
                                cwd=self.root, text=True, capture_output=True, check=False)
        self.assertEqual(result.returncode, 0, result.stderr)
        rendered = output.read_text(encoding="utf-8")
        for secret in ("PRIVATE_PROMPT", "RAW_AUTH", "PRIVATE_PATH", "PRIVATE_FILENAME", "PRIVATE_MESSAGE"):
            self.assertNotIn(secret, rendered + result.stdout + result.stderr)
        self.assertEqual(json.loads(rendered)["totals"]["total_tokens"], 15)
        stdout = subprocess.run([sys.executable, "-B", str(SCRIPT), str(path)],
                                cwd=self.root, text=True, capture_output=True, check=False)
        self.assertEqual(json.loads(stdout.stdout), json.loads(rendered))

    def test_cli_cannot_overwrite_input(self):
        path = self.rollout("a", meta(), context(), tokens())
        before = path.read_bytes()
        result = subprocess.run([sys.executable, "-B", str(SCRIPT), str(path), "--output", str(path)],
                                text=True, capture_output=True, check=False)
        self.assertEqual(result.returncode, 2)
        self.assertEqual(path.read_bytes(), before)


def response_record(response_id, usage_amount, thread_total=None, timestamp="2026-09-08T00:00:00Z"):
    usage = {
        "input_tokens": usage_amount, "cached_input_tokens": usage_amount // 2,
        "output_tokens": usage_amount // 2, "reasoning_output_tokens": usage_amount // 5,
        "total_tokens": usage_amount + usage_amount // 2,
    }
    thread = {
        "input_tokens": thread_total or usage["input_tokens"],
        "cached_input_tokens": thread_total or usage["cached_input_tokens"] if False else usage["cached_input_tokens"],
        "output_tokens": usage["output_tokens"],
        "reasoning_output_tokens": usage["reasoning_output_tokens"],
        "total_tokens": thread_total or usage["total_tokens"],
    }
    if thread_total is not None:
        thread["total_tokens"] = thread_total
    return {
        "timestamp": timestamp,
        "type": "token_usage_record",
        "payload": {
            "response_id": response_id,
            "usage": usage,
            "turn_token_usage": dict(thread),
            "thread_token_usage": dict(thread),
        },
    }


def message(role, text, phase=None, timestamp="2026-09-08T00:00:01Z"):
    payload = {"type": "message", "role": role, "content": [{"type": "input_text", "text": text}]}
    if phase:
        payload["phase"] = phase
    return {"timestamp": timestamp, "type": "response_item", "payload": payload}


class AttributionTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)

    def rollout(self, name, *events):
        path = self.root / name
        path.write_text("".join(json.dumps(event) + "\n" for event in events), encoding="utf-8")
        return path

    def codes(self, report):
        return {warning["code"] for warning in report["warnings"]}

    def test_repeated_stable_response_ids_count_once(self):
        first = self.rollout("a", meta(), context(), tokens(10),
                             response_record("resp_aaa", 10, 15, "2026-09-08T00:00:00Z"),
                             response_record("resp_aaa", 10, 15, "2026-09-08T00:01:00Z"))
        copy = self.rollout("b", meta(), context(), tokens(10),
                            response_record("resp_aaa", 10, 15, "2026-09-08T00:02:00Z"))
        report = usage.summarize_rollouts([first, copy])
        self.assertEqual(report["responses"]["response_count"], 1)
        self.assertEqual(report["responses"]["total_tokens"], 15)
        self.assertEqual(report["totals"]["total_tokens"], 15)

    def test_conflicting_response_usage_invalidates_all_copies_in_every_order(self):
        first = self.rollout("a", meta(), context(), tokens(10),
                             response_record("resp_aaa", 10, 15))
        second = self.rollout("b", meta(), context(), tokens(10),
                              response_record("resp_aaa", 20, 15))
        third = self.rollout("c", meta(), context(), tokens(10),
                             response_record("resp_aaa", 10, 15))
        for paths in itertools.permutations([first, second, third]):
            report = usage.summarize_rollouts(list(paths))
            self.assertEqual(report["responses"]["response_count"], 1)
            self.assertIsNone(report["responses"]["total_tokens"])
            self.assertEqual(report["responses"]["conflicting_response_ids"], 1)
            self.assertEqual(report["totals"]["total_tokens"], 15)
            usages = report["threads"][0]["response_usages"]["resp_aaa"]
            self.assertIsNone(usages["total_tokens"])
            self.assertIn("conflicting_duplicate_id", self.codes(report))

    def test_missing_response_field_is_not_a_conflicting_identity(self):
        incomplete = response_record("resp_partial", 10, 15)
        incomplete["payload"]["usage"].pop("total_tokens")
        path = self.rollout("a", meta(), context(), tokens(10), incomplete)
        report = usage.summarize_rollouts([path])
        self.assertEqual(report["responses"]["conflicting_response_ids"], 0)
        self.assertIsNone(report["responses"]["total_tokens"])
        self.assertTrue(report["responses"]["partial"])
        self.assertIn("| Reconciled total | unknown |", usage.compact_markdown(report))

    def test_divergent_series_do_not_claim_a_reconciled_total(self):
        path = self.rollout("a", meta(), context(), tokens(100),
                            response_record("resp_one", 10, 15))
        report = usage.summarize_rollouts([path])
        self.assertEqual(report["totals"]["total_tokens"], 150)
        self.assertEqual(report["responses"]["total_tokens"], 15)
        markdown = usage.compact_markdown(report)
        self.assertIn("| Reconciled total | unknown |", markdown)
        self.assertIn("| Response input / cached / uncached | 10 / 5 / 5 |", markdown)

    def test_forked_history_does_not_add_compaction_usage(self):
        path = self.rollout(
            "a", meta(), context(), tokens(10),
            response_record("resp_one", 10, 15),
            {"type": "compacted", "payload": {
                "window_id": "win2", "compaction_response_id": "resp_one",
                "latest_token_usage_record": {"response_id": "resp_one", "usage": {
                    "input_tokens": 10, "cached_input_tokens": 5, "output_tokens": 5,
                    "reasoning_output_tokens": 2, "total_tokens": 15}}}},
        )
        report = usage.summarize_rollouts([path])
        self.assertEqual(report["threads"][0]["compacted_windows"], 1)
        self.assertEqual(report["responses"]["response_count"], 1)
        self.assertEqual(report["totals"]["total_tokens"], 15)
        self.assertIn("forked_or_compacted_history", self.codes(report))

    def test_cached_and_new_input_are_separated(self):
        path = self.rollout("a", meta(), context(), tokens(20))
        row = usage.summarize_rollouts([path])["threads"][0]
        self.assertEqual(row["input_tokens"], 20)
        self.assertEqual(row["cached_input_tokens"], 10)
        self.assertEqual(row["output_tokens"], 10)
        self.assertEqual(row["reasoning_output_tokens"], 4)
        self.assertLessEqual(row["reasoning_output_tokens"], row["output_tokens"])

    def test_reasoning_included_in_output_not_added(self):
        path = self.rollout("a", meta(), context(), tokens(20),
                            response_record("resp_out", 20, 30))
        report = usage.summarize_rollouts([path])
        usage_row = report["threads"][0]["response_usages"]["resp_out"]
        self.assertEqual(usage_row["output_tokens"], 10)
        self.assertEqual(usage_row["reasoning_output_tokens"], 4)
        self.assertEqual(usage_row["total_tokens"], 30)
        self.assertNotEqual(usage_row["total_tokens"], usage_row["output_tokens"] + usage_row["reasoning_output_tokens"])

    def test_missing_child_stays_partial(self):
        parent = self.rollout(
            "p", meta(), context(), tokens(10),
            {"type": "event_msg", "payload": {"type": "item_completed", "item": {
                "type": "CollabAgentToolCall", "receiver_thread_ids": ["child"]}}},
        )
        report = usage.summarize_rollouts([parent])
        self.assertEqual(report["missing_children"][0]["child_id"], "child")
        self.assertTrue(report["partial"])
        self.assertIn("missing_child", self.codes(report))
        self.assertTrue(report["threads"][0]["partial"])
        self.assertEqual(report["threads"][0]["total_tokens"], 15)

    def test_partial_concurrent_and_reset_window_limitations(self):
        parent = self.rollout(
            "p",
            {**meta(), "timestamp": "2026-09-08T00:00:00Z"},
            {**context(), "timestamp": "2026-09-08T00:00:01Z"},
            {**tokens(10), "timestamp": "2026-09-08T00:10:00Z"},
        )
        child = self.rollout(
            "c",
            {**meta("child", "parent"), "timestamp": "2026-09-08T00:05:00Z"},
            {**context("xai/grok-4.6", "xhigh"), "timestamp": "2026-09-08T00:05:01Z"},
            {**tokens(40), "timestamp": "2026-09-08T00:08:00Z"},
        )
        report = usage.summarize_rollouts([parent, child])
        self.assertTrue(report["overlapping_elapsed"])
        self.assertIn("concurrent_or_overlapping_elapsed", self.codes(report))
        markdown = usage.compact_markdown(report)
        self.assertIn("reset windows are incomparable", markdown)
        self.assertNotIn("%", markdown)

    def test_hook_text_and_actual_continuation(self):
        path = self.rollout(
            "a", meta(), context(), tokens(10),
            message("user", "<hook_prompt hook_run_id=abc>diagnostic</hook_prompt>"),
            message("user", "<turn_aborted> The user interrupted the previous turn", timestamp="2026-09-08T00:00:02Z"),
            message("assistant", "working", phase="commentary", timestamp="2026-09-08T00:00:03Z"),
            {"timestamp": "2026-09-08T00:00:04Z", "type": "event_msg", "payload": {"type": "task_started"}},
            {"timestamp": "2026-09-08T00:00:05Z", "type": "event_msg", "payload": {"type": "task_complete", "duration_ms": 1500}},
        )
        row = usage.summarize_rollouts([path])["threads"][0]
        self.assertEqual(row["hook_messages"], 1)
        self.assertGreater(row["hook_chars"], 10)
        self.assertEqual(row["continuation_notices"], 1)
        self.assertEqual(row["task_started"], 1)
        self.assertEqual(row["commentary_messages"], 1)
        self.assertEqual(row["actual_continuations"], 1)
        self.assertGreaterEqual(row["elapsed_seconds"], 1)

    def test_ordinary_turns_are_not_continuations(self):
        path = self.rollout(
            "a",
            {"timestamp": "2026-09-08T00:00:00Z", **meta()},
            {"timestamp": "2026-09-08T00:00:01Z", **context()},
            {"timestamp": "2026-09-08T00:00:02Z", **tokens(10)},
            {"timestamp": "2026-09-08T00:00:03Z", "type": "event_msg", "payload": {"type": "task_started"}},
            message("assistant", "status", phase="commentary", timestamp="2026-09-08T00:00:04Z"),
            {"timestamp": "2026-09-08T00:00:05Z", "type": "event_msg", "payload": {"type": "task_complete", "duration_ms": 900}},
        )
        row = usage.summarize_rollouts([path])["threads"][0]
        self.assertEqual(row["task_started"], 1)
        self.assertEqual(row["commentary_messages"], 1)
        self.assertEqual(row["continuation_notices"], 0)
        self.assertEqual(row["actual_continuations"], 0)
        markdown = usage.compact_markdown(usage.summarize_rollouts([path]))
        self.assertIn("Ordinary turns started", markdown)
        self.assertIn("| Actual continuations | 0 |", markdown)
        self.assertIn("Automatic context occurrences / chars", markdown)

    def test_intervening_ordinary_user_request_is_not_a_continuation(self):
        path = self.rollout(
            "a", meta(), context(), tokens(10),
            message("user", "<turn_aborted> The user interrupted the previous turn", timestamp="2026-09-08T00:00:02Z"),
            message("user", "please continue with a new question", timestamp="2026-09-08T00:00:03Z"),
            {"timestamp": "2026-09-08T00:00:04Z", "type": "event_msg", "payload": {"type": "task_started"}},
        )
        row = usage.summarize_rollouts([path])["threads"][0]
        self.assertEqual(row["continuation_notices"], 1)
        self.assertEqual(row["user_messages"], 1)
        self.assertEqual(row["actual_continuations"], 0)

    def test_two_triggers_share_one_resumed_turn(self):
        path = self.rollout(
            "a", meta(), context(), tokens(10),
            message("user", "<turn_aborted> The user interrupted the previous turn", timestamp="2026-09-08T00:00:02Z"),
            message("user", "resume after tool-host restart. if you are still working, continue.", timestamp="2026-09-08T00:00:03Z"),
            {"timestamp": "2026-09-08T00:00:04Z", "type": "event_msg", "payload": {"type": "task_started"}},
            response_record("resp_resume", 10, 15, "2026-09-08T00:00:05Z"),
        )
        row = usage.summarize_rollouts([path])["threads"][0]
        self.assertEqual(row["continuation_notices"], 2)
        self.assertEqual(row["actual_continuations"], 1)

    def test_markdown_omits_raw_identities_and_quota_conversion(self):
        path = self.rollout("PRIVATE_FILENAME", meta(), context(), tokens(10),
                            response_record("resp_secret", 10, 15))
        markdown = usage.compact_markdown(usage.summarize_rollouts([path]))
        self.assertIn("gpt-6-astra", markdown)
        self.assertIn("OpenAI", markdown)
        self.assertNotIn("resp_secret", markdown)
        self.assertNotIn("PRIVATE_FILENAME", markdown)
        self.assertIn("not a quota share", markdown)
        self.assertIn("not weekly quota", markdown)
        self.assertIn("root x1", markdown)
        self.assertIn("Reconciled total", markdown)
        self.assertIn("disagreement is unresolved", markdown)
        self.assertNotIn("harness-grok-reliability", markdown)

    def test_cli_markdown_and_private_hashes_omit_paths(self):
        path = self.rollout("PRIVATE_FILENAME", meta(), context(), tokens(10),
                            response_record("resp_secret", 10, 15),
                            message("user", "<hook_prompt hook_run_id=abc>secret</hook_prompt>"))
        output = self.root / "report.md"
        private = self.root / "sources.json"
        result = subprocess.run(
            [sys.executable, "-B", str(SCRIPT), str(path), "--format", "markdown",
             "--output", str(output), "--private-sources", str(private)],
            cwd=self.root, text=True, capture_output=True, check=False)
        self.assertEqual(result.returncode, 0, result.stderr)
        rendered = output.read_text(encoding="utf-8")
        sources = json.loads(private.read_text(encoding="utf-8"))
        blob = rendered + result.stdout + result.stderr + json.dumps(sources)
        for secret in ("PRIVATE_FILENAME", "resp_secret", "secret</hook_prompt>", str(path)):
            self.assertNotIn(secret, blob)
        self.assertEqual(len(sources["sources"]), 1)
        self.assertIn("sha256", sources["sources"][0])
        self.assertIn("gpt-6-astra", rendered)


if __name__ == "__main__":
    unittest.main(verbosity=2)
