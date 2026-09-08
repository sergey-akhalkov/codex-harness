"""Deterministic outcome boundary checks. Never launches a model."""
from __future__ import annotations

import copy
import importlib.util
import json
from pathlib import Path
import tempfile
from typing import Any
import unittest
from unittest.mock import patch

REPO = Path(__file__).resolve().parents[1]


def load(name: str) -> Any:
    spec = importlib.util.spec_from_file_location(name, REPO / "tools" / (name + ".py"))
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


report = load("outcome_report")
runner = load("outcome_runner")


def attempt(identity="a", arm="baseline", start=0, end=10) -> dict[str, Any]:
    return {"attempt_id": identity, "case_id": "focused", "arm": arm,
        "started_at": start, "ended_at": end, "discovery_verified": True,
        "matched": {key: "fixed" for key in report.MATCH_FIELDS},
        "native_runs": [{"started_at": start, "ended_at": start + 2, "status": "completed"}],
        "checks": [{"id": "acceptance", "started_at": start + 2, "ended_at": end,
            "required": True, "executed": True, "passed": True, "exit_code": 0, "evidence": "private/log"}],
        "children": [], "interventions": [], "retry_of": None}


class Accounting(unittest.TestCase):
    def test_preparation_retained_without_opposite_arm_wait(self):
        row = attempt(start=100, end=110)
        row.update(started_at=0, execution_started_at=100,
                   preparation={"started_at": 0, "ended_at": 5})
        result = report.summarize_attempts([row])["attempts"][0]
        self.assertEqual(result["preparation_seconds"], 5)
        self.assertEqual(result["elapsed_seconds"], 10)
        self.assertEqual(result["total_verified_seconds"], 15)
        self.assertEqual(result["first_useful_seconds"], 15)
        self.assertEqual(result["observed_wall_seconds"], 110)

    def test_failures_incomplete_rework_all_retained(self):
        first = attempt()
        first["checks"][0].update(passed=False, exit_code=1)
        incomplete = attempt("b", start=11, end=12)
        incomplete["checks"] = []
        fixed = attempt("c", start=14, end=25)
        fixed["retry_of"] = "b"
        incomplete["retry_of"] = "a"
        result = report.summarize_attempts([first, incomplete, fixed])
        self.assertEqual([r["status"] for r in result["attempts"]], ["failed", "incomplete", "accepted"])
        self.assertEqual(result["attempts"][2]["total_result_seconds"], 25)
        self.assertEqual(result["attempts"][2]["result_attempt_ids"], ["a", "b", "c"])
        self.assertIn("outcome_failed", result["attempts"][0]["excluded_reasons"])
        self.assertEqual(len(result["attempts"]), 3)

    def test_verification_and_overlap_not_summed(self):
        row = attempt()
        children: list[dict[str, Any]] = [{"started_at": 1, "ended_at": 8}, {"started_at": 1, "ended_at": 9}]
        row["children"] = children
        row["interventions"] = [{"started_at": 5, "ended_at": 7}]
        self.assertEqual(report.finish_attempt(row)["elapsed_seconds"], 10)
        self.assertEqual(report.wall_span([{"started_at": 0, "ended_at": 2}, {"started_at": 5, "ended_at": 6}]), 6)
        children[0]["ended_at"] = None
        self.assertIsNone(report.finish_attempt(row)["elapsed_seconds"])

    def test_incomplete_or_missing_usage_never_zero(self):
        self.assertIsNone(report.summarize_attempts([attempt()])["attempts"][0]["usage"]["total_tokens"])
        usage = runner.load_usage([])
        self.assertEqual(usage["status"], "unknown")
        self.assertIsNone(usage["totals"]["total_tokens"])
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / "partial.jsonl"
            path.write_text(json.dumps({"type": "session_meta", "payload": {"id": "thread"}}) + "\n")
            self.assertIsNone(runner.load_usage([path])["totals"]["total_tokens"])

    def test_prose_and_running_tool_are_not_first_useful(self):
        row: dict[str, Any] = {"children": [], "first_useful_signal": None}
        runner.observe_event(row, {"type": "item.completed", "item": {"type": "agent_message", "text": "Tests passed"}}, 1, "trace")
        runner.observe_event(row, {"type": "item.started", "item": {"type": "command_execution", "exit_code": 0}}, 2, "trace")
        runner.observe_event(row, {"type": "item.completed", "item": {"type": "command_execution", "exit_code": None}}, 3, "trace")
        self.assertIsNone(row["first_useful_signal"])
        runner.observe_event(row, {"type": "item.completed", "item": {"type": "command_execution", "exit_code": 0, "command": "pwd"}}, 3.5, "trace")
        self.assertIsNone(row["first_useful_signal"])
        self.assertEqual(row["first_command_result"]["at"], 3.5)
        runner.observe_event(row, {"type": "item.completed", "item": {"type": "command_execution", "exit_code": 1, "command": "node test.js"}}, 4, "trace", r"^node test\.js$")
        self.assertEqual(row["first_useful_signal"]["at"], 4)
        runner.observe_event(row, {"item": {"type": "collab_tool_call", "receiver_thread_ids": ["child"]}}, 5, "trace")
        self.assertEqual(row["children"], ["child"])

    def test_rework_cannot_drop_required_check(self):
        row = attempt()
        row["checks"][0].update(passed=False, exit_code=1, round=0)
        row["checks"].append({**row["checks"][0], "id": "weaker", "round": 1, "passed": True, "exit_code": 0})
        self.assertFalse(report.finish_attempt(row)["correct"])
        row["checks"].append({**row["checks"][0], "round": 1, "passed": True, "exit_code": 0})
        self.assertTrue(report.finish_attempt(row)["correct"])
        self.assertEqual(len(report.finish_attempt(row)["checks"]), 3)

    def test_every_material_field_and_unknowns_exclude(self):
        a, b = attempt(), attempt("b", "candidate")
        self.assertEqual(report.comparison_reasons(a, b), [])
        for field in report.MATCH_FIELDS:
            changed = copy.deepcopy(b)
            changed["matched"][field] = "changed"
            self.assertIn("mismatch:" + field, report.comparison_reasons(a, changed))
            del changed["matched"][field]
            self.assertIn("unknown:" + field, report.comparison_reasons(a, changed))
        b["matched"]["role_availability"] = "middle:unknown_agent_type"
        self.assertIn("unknown:role_availability", report.comparison_reasons(a, b))
        result = report.summarize_attempts([a, b])
        self.assertEqual(len(result["attempts"]), 2)
        self.assertIn("unknown:role_availability", report.concise_report(result))

    def test_bad_retry_and_nonfinite_timing(self):
        row = attempt()
        row["retry_of"] = "missing"
        self.assertIsNone(report.summarize_attempts([row])["attempts"][0]["total_result_seconds"])
        row["ended_at"] = float("nan")
        self.assertIsNone(report.finish_attempt(row)["elapsed_seconds"])

    def test_reloaded_incremental_report_recomputes_exclusions(self):
        a = report.summarize_attempts([attempt()])["attempts"][0]
        self.assertIn("no_opposite_arm", a["excluded_reasons"])
        result = report.summarize_attempts([a, attempt("b", "candidate")])
        self.assertTrue(result["comparisons"][0]["comparable"])

    def test_opt_in_explicit_subset_no_implicit_all(self):
        self.assertEqual(runner.select_cases({"one": 1, "two": 2}, None), [])
        self.assertEqual(runner.select_cases({"one": 1, "two": 2}, ["two"], True), [2])
        for selected in (None, [], ["absent"], ["one", "one"]):
            with self.assertRaises(ValueError):
                runner.select_cases({"one": 1}, selected, True)

    def test_native_launch_failure_retains_private_attempt(self):
        with patch.object(runner, "isolated_home", side_effect=ValueError("unavailable")):
            a = runner.run_native(REPO, "not executed", REPO)
            b = runner.run_native(REPO, "not executed", REPO)
        self.assertNotEqual(a["evidence_root"], b["evidence_root"])
        for row in (a, b):
            self.assertEqual(row["status"], "failed")
            self.assertIsNone(row["usage"]["total_tokens"])
            self.assertTrue((Path(row["evidence_root"]) / "native.json").is_file())
            self.assertFalse(Path(row["evidence_root"]).is_relative_to(REPO))

    def test_configuration_cannot_change_provider_or_model(self):
        for key in ("model", "model_provider", "model_providers.openai.base_url", "profile", "service_tier"):
            with self.assertRaises(ValueError):
                runner.config_arguments({key: "changed"})
        self.assertIn("skills.config=", runner.config_arguments({"skills.config": []})[1])


if __name__ == "__main__":
    unittest.main()
