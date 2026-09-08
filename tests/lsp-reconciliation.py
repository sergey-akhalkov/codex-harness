"""Discovery, recovery and analysis-limit regressions; temporary homes only."""
from __future__ import annotations

import os
from abc import ABC
from contextlib import ExitStack
from pathlib import Path
import sys
import tempfile
import unittest
from dataclasses import dataclass
from typing import override, Protocol, TypedDict, NotRequired, runtime_checkable
from collections.abc import Iterable, Mapping
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools/lsp"))
import discovery as discovery_module
import journal as journal_module
import server as server_module


class AnalysisResult(TypedDict):
    file: str
    revision: str
    backend: str
    status: str
    diagnostics: list[dict[str, object]]
    reason: NotRequired[str]


class Report(TypedDict):
    status: str
    results: NotRequired[list[AnalysisResult]]
    problems: NotRequired[list[str]]
    coverage: NotRequired[dict[str, object]]


class Database(Protocol):
    def execute(self, sql: str) -> Iterable[tuple[str, str]]: ...


class JournalAPI(Protocol):
    db: Database
    def pre(self, event: Mapping[str, object], budget: float = 5.0) -> object: ...
    def get(self, key: str, default: object = None) -> object: ...
    def changes(self, event: Mapping[str, object], budget: float = 5.0) -> tuple[dict[str, str], dict[str, str], list[str]]: ...
    def close(self) -> None: ...


class Service(Protocol):
    def _analyze(self, event: Mapping[str, object], relative: str, revision: str) -> AnalysisResult: ...
    def check(self, event: Mapping[str, object], budget: float = 27.0) -> Report: ...
    def close(self) -> None: ...


class AnalysisInspection(Service, ABC):
    """Expose the protected analysis entry point for this limit regression."""

    @staticmethod
    def analyze(service: Service, event: Mapping[str, object], relative: str, revision: str) -> AnalysisResult:
        return service._analyze(event, relative, revision)


@runtime_checkable
class DiscoveryModule(Protocol):
    MAX_ANALYSIS_BYTES: int
    def snapshot(self, root: Path) -> tuple[dict[str, str], list[str]]: ...


@runtime_checkable
class JournalModule(Protocol):
    def Journal(self, event: Mapping[str, object]) -> JournalAPI: ...
    def snapshot(self, root: Path) -> tuple[dict[str, str], list[str]]: ...


@runtime_checkable
class ServerModule(Protocol):
    def DiagnosticsService(self) -> Service: ...


def checked_module[T](module: object, interface: type[T]) -> T:
    assert isinstance(module, interface)
    return module


discovery = checked_module(discovery_module, DiscoveryModule)
journal = checked_module(journal_module, JournalModule)
server = checked_module(server_module, ServerModule)


@dataclass
class _Fixture:
    scratch: tempfile.TemporaryDirectory[str]
    root: Path
    registry: Path
    environment: ExitStack
    event: dict[str, str]
    small: Path
    archive: Path
    service: Service


class ReconciliationTests(unittest.TestCase):
    _fixture: _Fixture | None = None

    @property
    def fixture(self) -> _Fixture:
        assert self._fixture is not None, "setUp has not completed"
        return self._fixture

    @override
    def setUp(self) -> None:
        scratch = tempfile.TemporaryDirectory(prefix="harness-lsp-reconciliation-")
        root = Path(scratch.name) / "workspace"
        root.mkdir()
        registry = Path(scratch.name) / "registry.json"
        _ = registry.write_text('{"servers":{}}')
        environment = ExitStack()
        environment.enter_context(patch.dict(os.environ, {
            "CODEX_HOME": str(Path(scratch.name) / "home"),
            "HARNESS_LSP_WORKSPACE_ROOTS": "[]",
            "HARNESS_LSP_REGISTRY": str(registry),
        }))
        event = {"workspace": str(root), "session_id": "reconciliation", "turn_id": "turn"}
        small = root / "note.ts"
        _ = small.write_text("export const value = 1;\n", encoding="utf-8")
        archive = root / "archive.json"
        _ = archive.write_bytes(b"{" + (b"0" * (discovery.MAX_ANALYSIS_BYTES + 1)) + b"}")
        service = server.DiagnosticsService()
        self._fixture = _Fixture(scratch, root, registry, environment, event, small, archive, service)

    @override
    def tearDown(self) -> None:
        self.fixture.service.close()
        self.fixture.environment.close()
        self.fixture.scratch.cleanup()

    def test_snapshot_reexport_is_discovery(self):
        self.assertIs(journal.snapshot, discovery.snapshot)

    def test_large_unchanged_archive_does_not_invalidate_baseline(self):
        entry = journal.Journal(self.fixture.event)
        try:
            self.assertEqual(entry.pre(self.fixture.event), {})
            self.assertTrue(entry.get("baseline"))
            files, problems = journal.snapshot(self.fixture.root)
            self.assertFalse(problems)
            self.assertIn("archive.json", files)
            self.assertIn("note.ts", files)
            _ = self.fixture.small.write_text("export const value = 2;\n", encoding="utf-8")
            current, changed, again = entry.changes(self.fixture.event)
            self.assertFalse(again)
            self.assertIn("note.ts", changed)
            self.assertNotIn("archive.json", changed)
            self.assertEqual(current["archive.json"], files["archive.json"])
        finally:
            entry.close()

    def test_large_changed_archive_is_discovered_and_analysis_is_skipped(self):
        entry = journal.Journal(self.fixture.event)
        try:
            _ = entry.pre(self.fixture.event)
            previous = dict(entry.db.execute("SELECT path, revision FROM files"))
            _ = self.fixture.archive.write_bytes(b"{" + (b"1" * (discovery.MAX_ANALYSIS_BYTES + 1)) + b"}")
            _current, changed, problems = entry.changes(self.fixture.event)
            self.assertFalse(problems)
            self.assertIn("archive.json", changed)
            self.assertNotEqual(changed["archive.json"], previous["archive.json"])
            result = AnalysisInspection.analyze(self.fixture.service, self.fixture.event, "archive.json", changed["archive.json"])
            self.assertEqual(result["status"], "skipped")
            self.assertIn("8 MiB", result.get("reason", ""))
        finally:
            entry.close()

    def test_missing_baseline_adopts_forward_observation_with_durable_gap(self):
        post = {**self.fixture.event, "event": "PostToolUse", "tool_use_id": "first-edit",
            "tool_name": "apply_patch", "tool_input": {"path": "note.ts"}}
        entry = journal.Journal(post)
        try:
            _current, changed, _problems = entry.changes(post)
            self.assertIn("note.ts", changed)
            self.assertTrue(entry.get("coverage_gap"))
            self.assertTrue(entry.get("baseline"))
            gap = entry.get("coverage_gap")
        finally:
            entry.close()
        def clean_analysis(_event: Mapping[str, object], name: str, revision: str) -> AnalysisResult:
            return {"file": name, "revision": revision, "backend": "typescript", "status": "clean", "diagnostics": []}

        with patch.object(self.fixture.service, "_analyze", side_effect=clean_analysis):
            report = self.fixture.service.check(post)
        self.assertEqual(report["status"], "unresolved")
        self.assertTrue(report.get("coverage", {}).get("historical_gap") or any("historical" in item.lower() or "coverage" in item.lower()
            for item in report.get("problems", [])), report)
        _ = self.fixture.small.write_text("export const value = 3;\n", encoding="utf-8")
        later = {**post, "tool_use_id": "second-edit"}
        entry = journal.Journal(later)
        try:
            self.assertEqual(entry.get("coverage_gap"), gap)
            _, changed, _ = entry.changes(later)
            self.assertIn("note.ts", changed)
        finally:
            entry.close()
        def error_analysis(_event: Mapping[str, object], name: str, revision: str) -> AnalysisResult:
            return {"file": name, "revision": revision, "backend": "typescript", "status": "diagnostics",
                "diagnostics": [{"severity": 1, "code": "2322", "message": "type error"}], "reason": ""}

        with patch.object(self.fixture.service, "_analyze", side_effect=error_analysis):
            later_report = self.fixture.service.check(later)
        self.assertTrue(any(item.get("file") == "note.ts" and item.get("status") == "diagnostics"
            for item in later_report.get("results", [])), later_report)

    def test_read_only_stop_without_baseline_does_not_mark_every_file_changed(self):
        stop = {**self.fixture.event, "event": "Stop"}
        entry = journal.Journal(stop)
        try:
            current, changed, _problems = entry.changes(stop)
            self.assertTrue(entry.get("coverage_gap"))
            self.assertNotIn("archive.json", changed)
            self.assertNotIn("note.ts", changed)
            self.assertGreater(len(current), 0)
        finally:
            entry.close()

    def test_outside_root_reparse_is_not_observed(self):
        foreign = Path(self.fixture.scratch.name) / "foreign"
        foreign.mkdir()
        _ = (foreign / "leak.ts").write_text("export const leak = 1;\n", encoding="utf-8")
        link = self.fixture.root / "escaped"
        try:
            os.symlink(foreign, link, target_is_directory=True)
        except OSError:
            try:
                import _winapi
                _winapi.CreateJunction(str(foreign), str(link))
            except Exception:
                self.skipTest("directory reparse points are unavailable")
        files, _problems = journal.snapshot(self.fixture.root)
        self.assertNotIn("escaped/leak.ts", files)
        self.assertFalse(any("leak.ts" in name for name in files), files)


if __name__ == "__main__":
    _ = unittest.main(verbosity=2)
