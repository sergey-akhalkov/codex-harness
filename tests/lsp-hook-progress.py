"""Regression for bounded hooks: real journal/files, controlled slow analysis.

Only creates disposable roots. No native sessions, global writes or packages.
"""
from collections import Counter
import os
from pathlib import Path
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools/lsp"))
from journal import Journal, snapshot
from server import DiagnosticsService


class HookProgressTests(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory(prefix="harness-hook-progress-")
        self.root = Path(self.scratch.name) / "workspace"
        self.root.mkdir()
        self.environment = patch.dict(os.environ, {"CODEX_HOME": str(Path(self.scratch.name) / "home"), "HARNESS_LSP_WORKSPACE_ROOTS": "[]"})
        self.environment.start()
        self.event = {"workspace": str(self.root), "session_id": "progress", "tool_use_id": "edit"}
        for index in range(6):
            (self.root / f"file{index}.ps1").write_text("$value = 1\n")
        journal = Journal(self.event)
        journal.pre(self.event)
        journal.close()
        (self.root / "file0.ps1").write_text("$value = 2\n")
        self.service = DiagnosticsService()
        self.calls = Counter()

    def tearDown(self):
        self.service.close()
        self.environment.stop()
        self.scratch.cleanup()

    def analyze(self, event, relative, revision):
        self.calls[relative] += 1
        time.sleep(0.065)
        return {"file": relative, "revision": revision, "backend": "powershell", "diagnostics": [], "status": "clean"}

    def drain(self):
        reports = []
        with patch.object(self.service, "_analyze", side_effect=self.analyze):
            for _ in range(12):
                report = self.service.check(self.event, budget=0.20)
                reports.append(report)
                self.assertLess(report.get("elapsed_seconds", 0), 0.5)
                if report["status"] == "unchanged":
                    return reports
        self.fail(f"Queue did not converge: {dict(self.calls)}; statuses={[r['status'] for r in reports]}")

    def test_partial_cohort_finishes_without_reanalyzing_completed_files(self):
        reports = self.drain()
        self.assertTrue(any(report["status"] == "unresolved" for report in reports))
        self.assertEqual(self.calls, Counter({f"file{i}.ps1": 1 for i in range(6)}))
        self.assertTrue(all(not r["problems"] for r in reports))

    def test_changed_dependency_invalidates_previously_completed_cohort(self):
        with patch.object(self.service, "_analyze", side_effect=self.analyze):
            first = self.service.check(self.event, budget=0.20)
        completed = {r["file"] for r in first["results"] if r["status"] == "clean"}
        self.assertTrue(completed)
        (self.root / "file5.ps1").write_text("$value = 3\n")
        self.drain()
        self.assertTrue(all(self.calls[name] >= 2 for name in completed))

    def test_final_snapshot_has_reserved_time_even_after_analysis_budget(self):
        real_snapshot = snapshot
        available = []

        def final_snapshot(*args, **kwargs):
            available.append(kwargs["budget"])
            return real_snapshot(*args, **kwargs)

        with patch.object(self.service, "_analyze", side_effect=self.analyze), patch("server.snapshot", side_effect=final_snapshot):
            self.service.check(self.event, budget=0.8)
        self.assertGreater(available[0], 0.1)

    def test_late_write_cannot_cache_old_clearance(self):
        changed = False

        def edit_during_analysis(event, relative, revision):
            nonlocal changed
            result = self.analyze(event, relative, revision)
            if not changed:
                changed = True
                (self.root / "late.ps1").write_text("$value = 4\n")
            return result

        with patch.object(self.service, "_analyze", side_effect=edit_during_analysis):
            report = self.service.check(self.event, budget=0.2)
        self.assertEqual(report["status"], "unresolved")
        self.assertTrue(all(item["status"] == "stale" for item in report["results"]))
        self.drain()
        self.assertIn("late.ps1", self.calls)

    def test_registry_change_before_delivery_invalidates_old_backend_results(self):
        registry = Path(self.scratch.name) / "registry.json"
        registry.write_text('{"version":1}')

        def replace_backend(event, relative, revision):
            result = self.analyze(event, relative, revision)
            registry.write_text('{"version":2}')
            return result

        with patch("server.registry_path", return_value=registry), patch.object(self.service, "_analyze", side_effect=replace_backend):
            report = self.service.check(self.event, budget=0.2)
        self.assertEqual(report["status"], "unresolved")
        self.assertTrue(any("registry changed" in item for item in report["problems"]))
        self.assertTrue(all(item["status"] == "stale" for item in report["results"]))


if __name__ == "__main__":
    unittest.main()
