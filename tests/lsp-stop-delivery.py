"""Stop/companion regressions: disposable sources, real SQLite, no models."""
from concurrent.futures import ThreadPoolExecutor
import copy
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools/lsp"))
from journal import Journal, command_fallback, fallback_failure
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

    def analyze(self, event, relative, revision):
        return {"file": relative, "revision": revision, "backend": "markdown", "status": "failed",
            "diagnostics": [], "reason": "Markdown resource requires an additional workspace root"}

    def failure(self):
        with patch.object(self.service, "_analyze", side_effect=self.analyze):
            return self.service.check(self.event)

    def test_native_failure_companion_and_repeated_stop_deliver_once(self):
        report = self.failure()
        first = self.service.hook_result(report, self.event)
        self.assertEqual(first.get("decision"), "block", first)
        with patch("journal.run_fallback_worker") as worker:
            self.assertEqual(command_fallback({**self.event, "tool_use_id": None}), {})
            worker.assert_not_called()
        repeat = self.failure()
        self.assertNotEqual(report["report_path"], repeat["report_path"])
        self.assertEqual(self.service.hook_result(repeat, {**self.event, "turn_id": "continuation"}), {})
        journal = Journal(self.event)
        try:
            self.assertIn("document.md", journal.changes(self.event)[1], "Failure must stay unresolved")
        finally:
            journal.close()

    def test_concurrent_native_and_command_publish_only_once(self):
        report = self.failure()
        # Tables already exist, but each delivery opens an independent connection.
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

    def test_active_subagent_stop_and_new_revision_and_session(self):
        report = self.failure()
        event = {**self.event, "event": "SubagentStop", "stop_hook_active": True}
        self.assertIn("systemMessage", self.service.hook_result(report, event))
        self.assertEqual(self.service.hook_result(report, event), {})
        changed = copy.deepcopy(report)
        changed["results"][0]["revision"] = "new-revision"
        self.assertEqual(self.service.hook_result(changed, self.event).get("decision"), "block")
        self.assertEqual(self.service.hook_result(report, {**self.event, "session_id": "other"}).get("decision"), "block")

    def test_companion_rechecks_later_source_config_and_registry(self):
        for kind in ("source", "config", "registry"):
            with self.subTest(kind=kind):
                self.failure()
                if kind == "source":
                    self.source.write_text("# Later\n")
                elif kind == "config":
                    (self.root / "package.json").write_text('{"name":"changed"}')
                else:
                    self.registry.write_text('{"servers":{},"changed":true}')
                with patch("journal.run_fallback_worker", return_value={"systemMessage": "new input"}) as worker:
                    self.assertIn("systemMessage", command_fallback(self.event))
                    worker.assert_called_once()

    def test_failed_final_scan_does_not_supply_reusable_receipt(self):
        from journal import snapshot
        count = 0
        def incomplete(*args, **kwargs):
            nonlocal count
            count += 1
            files, problems = snapshot(*args, **kwargs)
            return files, problems + ["incomplete scan"]
        with patch.object(self.service, "_analyze", side_effect=self.analyze), patch("server.snapshot", side_effect=incomplete):
            self.service.check(self.event)
        self.assertGreater(count, 0)
        with patch("journal.run_fallback_worker", return_value={}) as worker:
            command_fallback(self.event)
            worker.assert_called_once()

    def test_same_failure_ignores_clean_cohort_and_transport_metadata(self):
        report = self.failure()
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
        finally:
            journal.close()

    def test_fallback_infrastructure_failure_is_explicit_once(self):
        first = fallback_failure(self.event, "worker timeout")
        self.assertEqual(first.get("decision"), "block")
        self.assertIn("No clean result", first["reason"])
        self.assertEqual(fallback_failure(self.event, "worker timeout"), {})
        self.assertEqual(fallback_failure(self.event, "worker crashed").get("decision"), "block")


if __name__ == "__main__":
    unittest.main()
