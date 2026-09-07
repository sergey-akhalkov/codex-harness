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
        for events in ([context(), tokens()], [meta("a"), meta("b"), context(), tokens()]):
            path = self.rollout("a", *events)
            report = usage.summarize_rollouts([path])
            self.assertIsNone(report["threads"][0]["id"])
            self.assertIsNone(report["totals"]["total_tokens"])
            self.assertTrue(report["partial"])

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


if __name__ == "__main__":
    unittest.main(verbosity=2)
