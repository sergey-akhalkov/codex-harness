"""Real installed Markdown LSP, disposable sibling files; no Codex/model calls."""
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools/lsp"))
from backend import Backend
from markdown_client import MAX_RESOURCE_BYTES


class MarkdownWorkspaceTests(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory(prefix="harness-markdown-links-")
        self.root = Path(self.scratch.name) / "workspace"
        self.sibling = Path(self.scratch.name) / "sibling docs"
        self.root.mkdir()
        self.sibling.mkdir()
        self.guide = self.sibling / "guide.md"
        self.guide.write_text("# Topic\n", encoding="utf-8")
        self.source = self.root / "example.md"
        self.source.write_text("# Example\n\n[sibling](../sibling%20docs/guide.md#topic)\n", encoding="utf-8")
        self.backend = Backend(self.root, "markdown", Path(self.scratch.name) / "state")

    def tearDown(self):
        self.backend.close()
        self.scratch.cleanup()

    def check(self, status):
        result = self.backend.diagnostics("example.md", timeout=15)
        self.assertEqual(result["status"], status, result)
        return result

    def test_sibling_link_clearance_changed_target_and_navigation(self):
        self.check("clean")
        self.assertTrue(self.backend.navigation("example.md", "symbols"))
        self.guide.write_text("# Changed\n", encoding="utf-8")
        result = self.check("diagnostics")
        self.assertTrue(any(row.get("code") == "link.no-such-header-in-file" for row in result["diagnostics"]), result)
        self.guide.write_text("# Topic\n", encoding="utf-8")
        self.check("clean")

    def test_missing_sibling_and_local_links_remain_diagnostics(self):
        self.source.write_text("# Example\n\n[bad](../sibling%20docs/missing.md)\n[local](missing.md)\n[self](#absent)\n", encoding="utf-8")
        result = self.check("diagnostics")
        self.assertGreaterEqual(len(result["diagnostics"]), 3, result)

    def test_linked_resource_reads_are_bounded(self):
        self.check("clean")
        with self.guide.open("wb") as stream:
            stream.truncate(MAX_RESOURCE_BYTES + 1)
        client = self.backend.markdown_client
        for operation in (lambda: client.read_file(self.guide),
                          lambda: client.revision({"uri": self.guide.as_uri()}),
                          lambda: client.parse({"uri": self.guide.as_uri()})):
            with self.assertRaisesRegex(ValueError, "8 MiB"):
                operation()

    def test_external_access_is_exact_read_only_and_revoked(self):
        self.check("clean")
        client = self.backend.markdown_client
        self.assertEqual(client.path({"uri": self.backend.document_uri(self.guide)}), self.guide.resolve())
        for target in (self.sibling / "unrelated.md", self.sibling.parent / "private.txt", self.sibling):
            with self.assertRaises(ValueError):
                client.path({"uri": target.as_uri()})
        with self.assertRaises(ValueError):
            client.path({"uri": "file://remote-host/share/guide.md"})
        with self.assertRaises(ValueError):
            client.path({"uri": "file:////remote-host/share/guide.md"})
        self.source.write_text("# Example\n", encoding="utf-8")
        self.check("clean")
        with self.assertRaises(ValueError):
            client.path({"uri": self.guide.as_uri()})
        self.check("clean")


if __name__ == "__main__":
    unittest.main()
