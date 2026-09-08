"""Real installed Markdown LSP, disposable sibling files; no Codex/model calls."""
from pathlib import Path
import sys
import tempfile
import unittest
from dataclasses import dataclass
from typing import override, Protocol, TypedDict, runtime_checkable
from collections.abc import Mapping

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools/lsp"))
import backend as backend_module
from markdown_client import MAX_RESOURCE_BYTES


class DiagnosticResult(TypedDict):
    status: str
    diagnostics: list[dict[str, object]]


class MarkdownClientAPI(Protocol):
    def read_file(self, path: Path) -> object: ...
    def revision(self, params: Mapping[str, object]) -> object: ...
    def parse(self, params: Mapping[str, object]) -> object: ...
    def path(self, params: Mapping[str, object]) -> Path: ...


class BackendAPI(Protocol):
    markdown_client: MarkdownClientAPI | None
    def close(self) -> None: ...
    def diagnostics(self, relative: str, timeout: float = 20.0) -> DiagnosticResult: ...
    def navigation(self, relative: str, operation: str) -> object: ...
    def document_uri(self, path: Path) -> str: ...


@runtime_checkable
class BackendModule(Protocol):
    def Backend(self, workspace: Path, language: str, state: Path) -> BackendAPI: ...


def backend_api(module: object) -> BackendModule:
    assert isinstance(module, BackendModule)
    return module


globals_backend = backend_api(backend_module)


@dataclass
class _Fixture:
    scratch: tempfile.TemporaryDirectory[str]
    root: Path
    sibling: Path
    guide: Path
    source: Path
    backend: BackendAPI


class MarkdownWorkspaceTests(unittest.TestCase):
    _fixture: _Fixture | None = None

    @property
    def fixture(self) -> _Fixture:
        assert self._fixture is not None, "setUp has not completed"
        return self._fixture

    @override
    def setUp(self) -> None:
        scratch = tempfile.TemporaryDirectory(prefix="harness-markdown-links-")
        root = Path(scratch.name) / "workspace"
        sibling = Path(scratch.name) / "sibling docs"
        root.mkdir()
        sibling.mkdir()
        guide = sibling / "guide.md"
        _ = guide.write_text("# Topic\n", encoding="utf-8")
        source = root / "example.md"
        _ = source.write_text("# Example\n\n[sibling](../sibling%20docs/guide.md#topic)\n", encoding="utf-8")
        backend = globals_backend.Backend(root, "markdown", Path(scratch.name) / "state")
        self._fixture = _Fixture(scratch, root, sibling, guide, source, backend)

    @override
    def tearDown(self) -> None:
        self.fixture.backend.close()
        self.fixture.scratch.cleanup()

    def check(self, status: str) -> DiagnosticResult:
        result = self.fixture.backend.diagnostics("example.md", timeout=15)
        self.assertEqual(result["status"], status, result)
        return result

    def test_sibling_link_clearance_changed_target_and_navigation(self):
        _ = self.check("clean")
        self.assertTrue(self.fixture.backend.navigation("example.md", "symbols"))
        _ = self.fixture.guide.write_text("# Changed\n", encoding="utf-8")
        result = self.check("diagnostics")
        self.assertTrue(any(row.get("code") == "link.no-such-header-in-file" for row in result["diagnostics"]), result)
        _ = self.fixture.guide.write_text("# Topic\n", encoding="utf-8")
        _ = self.check("clean")

    def test_missing_sibling_and_local_links_remain_diagnostics(self):
        _ = self.fixture.source.write_text("# Example\n\n[bad](../sibling%20docs/missing.md)\n[local](missing.md)\n[self](#absent)\n", encoding="utf-8")
        result = self.check("diagnostics")
        self.assertGreaterEqual(len(result["diagnostics"]), 3, result)

    def test_linked_resource_reads_are_bounded(self):
        _ = self.check("clean")
        with self.fixture.guide.open("wb") as stream:
            _ = stream.truncate(MAX_RESOURCE_BYTES + 1)
        client = self.fixture.backend.markdown_client
        assert client is not None, "Markdown client did not start"
        for operation in (lambda: client.read_file(self.fixture.guide),
                          lambda: client.revision({"uri": self.fixture.guide.as_uri()}),
                          lambda: client.parse({"uri": self.fixture.guide.as_uri()})):
            with self.assertRaisesRegex(ValueError, "8 MiB"):
                _ = operation()

    def test_external_access_is_exact_read_only_and_revoked(self):
        _ = self.check("clean")
        client = self.fixture.backend.markdown_client
        assert client is not None, "Markdown client did not start"
        self.assertEqual(client.path({"uri": self.fixture.backend.document_uri(self.fixture.guide)}), self.fixture.guide.resolve())
        for target in (self.fixture.sibling / "unrelated.md", self.fixture.sibling.parent / "private.txt", self.fixture.sibling):
            with self.assertRaises(ValueError):
                _ = client.path({"uri": target.as_uri()})
        with self.assertRaises(ValueError):
            _ = client.path({"uri": "file://remote-host/share/guide.md"})
        with self.assertRaises(ValueError):
            _ = client.path({"uri": "file:////remote-host/share/guide.md"})
        _ = self.fixture.source.write_text("# Example\n", encoding="utf-8")
        _ = self.check("clean")
        with self.assertRaises(ValueError):
            _ = client.path({"uri": self.fixture.guide.as_uri()})
        _ = self.check("clean")


if __name__ == "__main__":
    _ = unittest.main()
