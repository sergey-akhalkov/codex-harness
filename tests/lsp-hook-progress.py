"""Regression for bounded hooks: real journal/files, controlled slow analysis.

Only creates disposable roots. No native sessions, global writes or packages.
"""
from collections import Counter
import os
from contextlib import ExitStack
from pathlib import Path
import sys
import tempfile
import time
import unittest
from dataclasses import dataclass
from typing import override, Protocol, TypedDict, NotRequired, runtime_checkable
from collections.abc import Iterable, Mapping
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools/lsp"))
import journal as journal_module
import discovery as discovery_module
import server as server_module


class AnalysisResult(TypedDict):
    file: str
    revision: str
    backend: str
    diagnostics: list[dict[str, object]]
    status: str


class Report(TypedDict):
    status: str
    results: list[AnalysisResult]
    problems: list[str]
    elapsed_seconds: NotRequired[float]


class JournalAPI(Protocol):
    def pre(self, event: Mapping[str, object], budget: float = 5.0) -> object: ...
    def close(self) -> None: ...


class Service(Protocol):
    def check(self, event: Mapping[str, object], budget: float = 27.0) -> Report: ...
    def close(self) -> None: ...


@runtime_checkable
class JournalModule(Protocol):
    def Journal(self, event: Mapping[str, object]) -> JournalAPI: ...


@runtime_checkable
class DiscoveryModule(Protocol):
    def snapshot(self, root: Path, extra: Iterable[str] = (), budget: float = 5.0, *, configurations_only: bool = False) -> tuple[dict[str, str], list[str]]: ...


@runtime_checkable
class ServerModule(Protocol):
    def DiagnosticsService(self) -> Service: ...


def checked_module[T](module: object, interface: type[T]) -> T:
    assert isinstance(module, interface)
    return module


journal_api = checked_module(journal_module, JournalModule)
discovery_api = checked_module(discovery_module, DiscoveryModule)
server_api = checked_module(server_module, ServerModule)


@dataclass
class _Fixture:
    scratch: tempfile.TemporaryDirectory[str]
    root: Path
    environment: ExitStack
    event: dict[str, str]
    service: Service
    calls: Counter[str]


class HookProgressTests(unittest.TestCase):
    _fixture: _Fixture | None = None

    @property
    def fixture(self) -> _Fixture:
        assert self._fixture is not None, "setUp has not completed"
        return self._fixture

    @override
    def setUp(self) -> None:
        scratch = tempfile.TemporaryDirectory(prefix="harness-hook-progress-")
        root = Path(scratch.name) / "workspace"
        root.mkdir()
        environment = ExitStack()
        environment.enter_context(patch.dict(os.environ, {"CODEX_HOME": str(Path(scratch.name) / "home"), "HARNESS_LSP_WORKSPACE_ROOTS": "[]"}))
        event = {"workspace": str(root), "session_id": "progress", "tool_use_id": "edit"}
        for index in range(6):
            _ = (root / f"file{index}.ps1").write_text("$value = 1\n")
        journal = journal_api.Journal(event)
        _ = journal.pre(event)
        journal.close()
        _ = (root / "file0.ps1").write_text("$value = 2\n")
        service = server_api.DiagnosticsService()
        calls: Counter[str] = Counter()
        self._fixture = _Fixture(scratch, root, environment, event, service, calls)

    @override
    def tearDown(self) -> None:
        self.fixture.service.close()
        self.fixture.environment.close()
        self.fixture.scratch.cleanup()

    def analyze(self, _event: Mapping[str, object], relative: str, revision: str) -> AnalysisResult:
        self.fixture.calls[relative] += 1
        time.sleep(0.065)
        return {"file": relative, "revision": revision, "backend": "powershell", "diagnostics": [], "status": "clean"}

    def drain(self):
        reports: list[Report] = []
        with patch.object(self.fixture.service, "_analyze", side_effect=self.analyze):
            for _ in range(12):
                report = self.fixture.service.check(self.fixture.event, budget=0.20)
                reports.append(report)
                self.assertLess(report.get("elapsed_seconds", 0), 0.5)
                if report["status"] == "unchanged":
                    return reports
        self.fail(f"Queue did not converge: {dict(self.fixture.calls)}; statuses={[r['status'] for r in reports]}")

    def test_partial_cohort_finishes_without_reanalyzing_completed_files(self):
        reports = self.drain()
        self.assertTrue(any(report["status"] == "unresolved" for report in reports))
        self.assertEqual(self.fixture.calls, Counter({f"file{i}.ps1": 1 for i in range(6)}))
        self.assertTrue(all(not r["problems"] for r in reports))

    def test_changed_dependency_invalidates_previously_completed_cohort(self):
        with patch.object(self.fixture.service, "_analyze", side_effect=self.analyze):
            first = self.fixture.service.check(self.fixture.event, budget=0.20)
        completed = {r["file"] for r in first["results"] if r["status"] == "clean"}
        self.assertTrue(completed)
        _ = (self.fixture.root / "file5.ps1").write_text("$value = 3\n")
        _ = self.drain()
        self.assertTrue(all(self.fixture.calls[name] >= 2 for name in completed))

    def test_final_snapshot_has_reserved_time_even_after_analysis_budget(self):
        real_snapshot = discovery_api.snapshot
        available: list[float] = []

        def final_snapshot(root: Path, extra: Iterable[str] = (), budget: float = 5.0, *, configurations_only: bool = False):
            available.append(budget)
            return real_snapshot(root, extra, budget, configurations_only=configurations_only)

        with patch.object(self.fixture.service, "_analyze", side_effect=self.analyze), patch("server.snapshot", side_effect=final_snapshot):
            _ = self.fixture.service.check(self.fixture.event, budget=0.8)
        self.assertGreater(available[0], 0.1)

    def test_late_write_cannot_cache_old_clearance(self):
        changed = False

        def edit_during_analysis(event: Mapping[str, object], relative: str, revision: str) -> AnalysisResult:
            nonlocal changed
            result = self.analyze(event, relative, revision)
            if not changed:
                changed = True
                _ = (self.fixture.root / "late.ps1").write_text("$value = 4\n")
            return result

        with patch.object(self.fixture.service, "_analyze", side_effect=edit_during_analysis):
            report = self.fixture.service.check(self.fixture.event, budget=1.0)
        self.assertEqual(report["status"], "unresolved")
        self.assertTrue(all(item["status"] == "stale" for item in report["results"]))
        _ = self.drain()
        self.assertIn("late.ps1", self.fixture.calls)

    def test_registry_change_before_delivery_invalidates_old_backend_results(self):
        registry = Path(self.fixture.scratch.name) / "registry.json"
        _ = registry.write_text('{"version":1}')

        def replace_backend(event: Mapping[str, object], relative: str, revision: str) -> AnalysisResult:
            result = self.analyze(event, relative, revision)
            _ = registry.write_text('{"version":2}')
            return result

        with patch("server.registry_path", return_value=registry), patch.object(self.fixture.service, "_analyze", side_effect=replace_backend):
            # Trigger the registry write before delivery; tiny batch deadlines
            # are exercised by drain(), independently of this freshness check.
            report = self.fixture.service.check(self.fixture.event, budget=1.0)
        self.assertEqual(report["status"], "unresolved")
        self.assertTrue(any("registry changed" in item for item in report["problems"]))
        self.assertTrue(all(item["status"] == "stale" for item in report["results"]))


if __name__ == "__main__":
    _ = unittest.main()
