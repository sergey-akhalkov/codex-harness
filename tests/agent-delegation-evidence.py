"""Offline regressions for failed native acceptance evidence. No model calls."""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("delegation_probe", Path(__file__).with_name("agent-delegation.py"))
assert spec and spec.loader
probe = importlib.util.module_from_spec(spec)
spec.loader.exec_module(probe)


class EvidenceTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.case = self.root / "direct"
        self.case.mkdir()
        (self.root / "sessions").mkdir()
        self.native_events = [{"type": "thread.started", "thread_id": "parent"}]
        self.parent = [
            {"type": "session_meta", "payload": {"id": "parent", "model_provider": "openai"}},
            {"type": "turn_context", "payload": {"model": "gpt-6-astra", "effort": "xhigh"}},
            {"type": "event_msg", "payload": {"type": "token_count", "info": {
                "total_token_usage": dict(input_tokens=20, cached_input_tokens=10,
                    output_tokens=5, reasoning_output_tokens=2, total_tokens=25)}}},
        ]
        self.result = dict(AssignedBeforeResume=True, ExitCode=0, ElapsedMilliseconds=100)

    def verify_failure(self):
        probe.write_json(self.case / "result.json", self.result)
        for path, rows in [(self.case / "events.jsonl", self.native_events),
                           (self.root / "sessions/rollout-parent.jsonl", self.parent)]:
            path.write_text("".join(json.dumps(row) + "\n" for row in rows), encoding="utf-8")
        with self.assertRaises(Exception):
            probe.verify_case(self.case, self.root)
        report = probe.read_json(self.case / "report.json")
        self.assertEqual(report["status"], "failed")
        return report

    def test_failed_process_keeps_available_cost(self):
        self.result["ExitCode"] = 125
        report = self.verify_failure()
        self.assertEqual(report["usage"]["totals"]["total_tokens"], 25)
        self.assertEqual(report["process"]["ExitCode"], 125)

    def test_missing_thread_is_unknown_not_zero(self):
        self.native_events = []
        report = self.verify_failure()
        self.assertIsNone(report["usage"])
        self.assertFalse(report["accounting_complete"])

    def test_missing_child_keeps_parent_cost_but_incomplete_accounting(self):
        self.native_events.append({"type": "item.completed", "item": {
            "tool": "spawn_agent", "receiver_thread_ids": ["missing-child"]}})
        report = self.verify_failure()
        self.assertEqual(report["usage"]["totals"]["total_tokens"], 25)
        self.assertFalse(report["accounting_complete"])

    def test_earlier_wrong_binding_cannot_hide_behind_final_binding(self):
        self.parent.insert(1, {"type": "turn_context", "payload": {
            "model": "gpt-6-astra", "effort": "high"}})
        report = self.verify_failure()
        self.assertEqual(report["reason"], "Parent binding changed or absent")
        self.assertEqual(report["usage"]["totals"]["total_tokens"], 25)


if __name__ == "__main__":
    unittest.main()
