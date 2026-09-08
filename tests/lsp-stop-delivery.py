"""Stop/companion regressions: disposable sources, real SQLite, no models."""
from concurrent.futures import ThreadPoolExecutor
import copy
import json
import os
from pathlib import Path
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools/lsp"))
from journal import Journal, command_fallback, fallback_failure, invocation_key
from server import DiagnosticsService


class StopDeliveryTests(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory(prefix="harness-stop-delivery-")
        self.root = Path(self.scratch.name) / "workspace"
        self.root.mkdir()
        self.registry = Path(self.scratch.name) / "registry.json"
        self.registry.write_text('{"servers":{}}')
        self.environment = patch.dict(os.environ, {"CODEX_HOME": str(Path(self.scratch.name) / "home"),
            "HARNESS_LSP_WORKSPACE_ROOTS": "[]", "HARNESS_LSP_REGISTRY": str(self.registry)})
        self.environment.start()
        self.event = {"workspace": str(self.root), "session_id": "stop-test", "event": "Stop", "turn_id": "turn"}
        self.source = self.root / "document.md"
        self.source.write_text("# Before\n")
        journal = Journal(self.event)
        journal.pre({**self.event, "tool_use_id": "edit"})
        journal.close()
        self.source.write_text("# After\n")
        self.service = DiagnosticsService()

    def tearDown(self):
        self.service.close()
        self.environment.stop()
        self.scratch.cleanup()

    def defect(self, event, relative, revision):
        return {"file": relative, "revision": revision, "backend": "markdown", "status": "diagnostics",
            "diagnostics": [{"severity": 1, "code": "MD001", "message": "heading levels should only increment by one"}],
            "reason": ""}

    def failed(self, event, relative, revision):
        return {"file": relative, "revision": revision, "backend": "markdown", "status": "failed",
            "diagnostics": [], "reason": "Markdown resource requires an additional workspace root"}

    def check_with(self, analyzer, event=None):
        with patch.object(self.service, "_analyze", side_effect=analyzer):
            return self.service.check(event or self.event)

    def test_native_defect_companion_and_repeated_stop_continue_once(self):
        report = self.check_with(self.defect)
        first = self.service.hook_result(report, self.event)
        self.assertEqual(first.get("decision"), "block", first)
        with patch("journal.run_fallback_worker") as worker:
            self.assertEqual(command_fallback({**self.event, "tool_use_id": None}), {})
            worker.assert_not_called()
        repeat = self.check_with(self.defect)
        self.assertNotEqual(report["report_path"], repeat["report_path"])
        self.assertEqual(self.service.hook_result(repeat, {**self.event, "turn_id": "continuation"}), {})
        journal = Journal(self.event)
        try:
            stored = json.loads(journal.db.execute("SELECT body FROM results WHERE path=?", ("document.md",)).fetchone()[0])
            self.assertEqual(stored["status"], "diagnostics")
            self.assertTrue(stored["diagnostics"])
        finally:
            journal.close()

    def test_failed_analysis_is_informational_and_stays_unresolved(self):
        report = self.check_with(self.failed)
        first = self.service.hook_result(report, self.event)
        self.assertIn("systemMessage", first)
        self.assertNotEqual(first.get("decision"), "block", first)
        self.assertEqual(self.service.hook_result(report, {**self.event, "turn_id": "again"}), {})
        journal = Journal(self.event)
        try:
            self.assertIn("document.md", journal.changes(self.event)[1], "Failed analysis must stay unresolved")
        finally:
            journal.close()

    def test_concurrent_native_and_command_publish_only_once(self):
        report = self.check_with(self.defect)
        with ThreadPoolExecutor(max_workers=2) as pool:
            outputs = list(pool.map(lambda _: self.service.hook_result(report, self.event), range(2)))
        self.assertEqual(sum(bool(output) for output in outputs), 1, outputs)
        self.assertEqual(sum(output.get("decision") == "block" for output in outputs), 1)

    def test_success_is_informational_then_silent(self):
        with patch.object(self.service, "_analyze", side_effect=lambda event, name, revision:
                {"file": name, "revision": revision, "backend": "markdown", "status": "clean", "diagnostics": []}):
            report = self.service.check(self.event)
        output = self.service.hook_result(report, self.event)
        self.assertEqual(report["status"], "clean")
        self.assertIn("systemMessage", output)
        self.assertNotIn("decision", output)
        self.assertEqual(self.service.hook_result(report, self.event), {})

    def test_active_stop_never_blocks_and_new_revision_and_session_are_distinct(self):
        report = self.check_with(self.defect)
        event = {**self.event, "event": "SubagentStop", "stop_hook_active": True}
        active = self.service.hook_result(report, event)
        self.assertIn("systemMessage", active)
        self.assertNotEqual(active.get("decision"), "block", active)
        self.assertEqual(self.service.hook_result(report, event), {})
        changed = copy.deepcopy(report)
        changed["results"][0]["revision"] = "new-revision"
        self.assertEqual(self.service.hook_result(changed, self.event).get("decision"), "block")
        self.assertEqual(self.service.hook_result(report, {**self.event, "session_id": "other"}).get("decision"), "block")

    def test_companion_rechecks_later_source_config_and_registry(self):
        for kind in ("source", "config", "registry"):
            with self.subTest(kind=kind):
                self.check_with(self.defect)
                if kind == "source":
                    self.source.write_text("# Later\n")
                elif kind == "config":
                    (self.root / "package.json").write_text('{"name":"changed"}')
                else:
                    self.registry.write_text('{"servers":{},"changed":true}')
                with patch("journal.run_fallback_worker", return_value={"systemMessage": "new input"}) as worker:
                    self.assertIn("systemMessage", command_fallback(self.event))
                    worker.assert_called_once()

    def test_native_receipt_is_per_invocation_and_later_same_turn_write_is_rechecked(self):
        first = {**self.event, "tool_use_id": "stop-1"}
        second = {**self.event, "tool_use_id": "stop-2"}
        self.check_with(self.defect, first)
        self.check_with(self.defect, second)
        journal = Journal(first)
        try:
            receipt = journal.receipt(first) if hasattr(journal, "receipt") else journal.get("last_completed_check", {})
            self.assertEqual(receipt.get("invocation"), invocation_key(first), receipt)
        finally:
            journal.close()
        with patch("journal.run_fallback_worker") as worker:
            self.assertEqual(command_fallback(first), {})
            worker.assert_not_called()
        self.source.write_text("# After native receipt\n")
        with patch("journal.run_fallback_worker", return_value={"systemMessage": "later write"}) as worker:
            self.assertIn("systemMessage", command_fallback(first))
            worker.assert_called_once()

    def test_failed_final_scan_does_not_supply_reusable_receipt(self):
        from journal import snapshot
        count = 0
        def incomplete(*args, **kwargs):
            nonlocal count
            count += 1
            files, problems = snapshot(*args, **kwargs)
            return files, problems + ["incomplete scan"]
        with patch.object(self.service, "_analyze", side_effect=self.failed), patch("server.snapshot", side_effect=incomplete):
            self.service.check(self.event)
        self.assertGreater(count, 0)
        journal = Journal(self.event)
        try:
            receipt = journal.receipt(self.event) if hasattr(journal, "receipt") else journal.get("last_completed_check", {})
            self.assertTrue(receipt)
            self.assertFalse(receipt.get("checked_inputs"))
        finally:
            journal.close()
        with patch("journal.run_fallback_worker", return_value={}) as worker:
            self.assertEqual(command_fallback(self.event), {})
            worker.assert_called_once()
        self.source.write_text("# After incomplete scan\n")
        with patch("journal.run_fallback_worker", return_value={}) as worker:
            command_fallback(self.event)
            worker.assert_called_once()

    def test_same_defect_ignores_clean_cohort_and_transport_metadata(self):
        report = self.check_with(self.defect)
        self.service.hook_result(report, self.event)
        report["results"].append({"file": "other.md", "revision": "ok", "status": "clean", "diagnostics": []})
        report.update(elapsed_seconds=42, tool_use_id=None, report_path="other-report.json")
        self.assertEqual(self.service.hook_result(report, self.event), {})

    def test_report_write_exception_is_not_a_completed_native_receipt(self):
        original = Path.write_text
        def fail_report(path, *args, **kwargs):
            if path.name.startswith("report-"):
                raise OSError("report disk unavailable")
            return original(path, *args, **kwargs)
        with patch.object(self.service, "_analyze", side_effect=lambda event, name, revision:
                {"file": name, "revision": revision, "backend": "markdown", "status": "clean", "diagnostics": []}), \
                patch.object(Path, "write_text", fail_report):
            with self.assertRaisesRegex(OSError, "report disk"):
                self.service.check(self.event)
        journal = Journal(self.event)
        try:
            self.assertIsNone(journal.get("last_completed_check"))
            self.assertFalse(journal.get("completed_invocations") or {})
        finally:
            journal.close()

    def test_fallback_infrastructure_failure_is_informational_once(self):
        first = fallback_failure(self.event, "worker timeout")
        self.assertIn("systemMessage", first)
        self.assertNotEqual(first.get("decision"), "block", first)
        self.assertIn("No clean result", first["systemMessage"])
        self.assertEqual(fallback_failure(self.event, "worker timeout"), {})
        second = fallback_failure(self.event, "worker crashed")
        self.assertNotEqual(second.get("decision"), "block", second)
        self.assertTrue(second.get("systemMessage") or second == {})

    def test_parallel_events_keep_pending_and_loser_does_not_clear_it(self):
        def slow_pending(event, relative, revision):
            time.sleep(0.2)
            return {"file": relative, "revision": revision, "backend": "markdown", "status": "pending",
                "diagnostics": [], "reason": "Diagnostic batch time budget elapsed"}
        first = {**self.event, "event": "PostToolUse", "tool_use_id": "tool-a", "turn_id": "turn-a"}
        second = {**self.event, "event": "PostToolUse", "tool_use_id": "tool-b", "turn_id": "turn-b"}
        with patch.object(self.service, "_analyze", side_effect=slow_pending):
            with ThreadPoolExecutor(max_workers=2) as pool:
                reports = list(pool.map(self.service.check, [first, second]))
        self.assertTrue(any(report.get("status") in ("unresolved", "delegated") or
            any(item.get("status") == "pending" for item in report.get("results", [])) for report in reports), reports)
        journal = Journal(self.event)
        try:
            pending = [json.loads(body) for _, body in journal.db.execute("SELECT path, body FROM results")
                if json.loads(body).get("status") == "pending"]
            claim = journal.get("active_claim") or {}
        finally:
            journal.close()
        self.assertTrue(pending, "pending result must survive the overlapping invocation")
        self.assertFalse(claim.get("token"), claim)

    def test_distinct_post_receipts_survive_later_completion(self):
        entry = Journal(self.event)
        try:
            events = [{**self.event, "event": "PostToolUse", "tool_use_id": name} for name in ("first", "second")]
            for event in events:
                entry.finish_check(entry.begin_check(), event, checked_inputs=event["tool_use_id"])
            self.assertEqual(entry.receipt(events[0])["checked_inputs"], "first")
            self.assertEqual(entry.receipt(events[1])["checked_inputs"], "second")
        finally:
            entry.close()

    def test_missing_tool_identity_never_silently_delegates_other_work(self):
        event = {**self.event, "event": "PostToolUse"}
        entry = Journal(event)
        token = entry.claim(event, "command")
        try:
            report = self.service.check(event, budget=0.2)
            self.assertEqual(report["status"], "unresolved", report)
            self.assertTrue(report["problems"])
        finally:
            entry.release_claim(token)
            entry.close()

    def test_live_claim_cannot_be_stolen_after_lease_age(self):
        entry = Journal(self.event)
        token = entry.claim(self.event, "native")
        try:
            claim = entry.get("active_claim")
            claim["started"] = time.time() - 100
            entry.put("active_claim", claim)
            entry.db.commit()
            self.assertIsNone(entry.claim({**self.event, "tool_use_id": "other"}, "command"))
            entry.release_claim(token)
            self.assertTrue(entry.claim(self.event, "command"))
        finally:
            entry.close()

    def test_infrastructure_churn_cannot_rearm_the_same_actual_defect(self):
        report = self.check_with(self.defect)
        self.assertEqual(self.service.hook_result(report, self.event).get("decision"), "block")
        for active in (True, False, True, False):
            fallback_failure({**self.event, "stop_hook_active": active}, str(active))
            self.assertEqual(self.service.hook_result(report, {**self.event, "stop_hook_active": active}), {})


if __name__ == "__main__":
    unittest.main()
