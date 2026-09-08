"""Pre-hook deadline and baseline regressions; only temporary homes and sources."""
from __future__ import annotations

import json
import os
from pathlib import Path
import sqlite3
import sys
import tempfile
import threading
import time
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools/lsp"))
import discovery
import journal


class FakeScan:
    def __init__(self, entries=()):
        self.entries = list(entries)

    def __enter__(self):
        return iter(self.entries)

    def __exit__(self, *_):
        return False


class PreBudgetTests(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory(prefix="harness-lsp-pre-budget-")
        self.root = Path(self.scratch.name) / "workspace"
        self.root.mkdir()
        self.environment = patch.dict(os.environ, {
            "CODEX_HOME": str(Path(self.scratch.name) / "codex-home"),
            "HARNESS_LSP_WORKSPACE_ROOTS": "[]",
        })
        self.environment.start()
        self.event = {"cwd": str(self.root), "session_id": "pre-budget", "tool_use_id": "tool-a"}
        self.source = self.root / "source.ts"
        self.source.write_text("export const value = 1;\n", encoding="utf-8")

    def tearDown(self):
        self.environment.stop()
        self.scratch.cleanup()

    def test_directory_only_walk_stops_at_deadline(self):
        visited = []

        def slow_scandir(folder):
            visited.append(folder)
            time.sleep(0.03)
            return FakeScan()

        with patch.object(discovery.os, "scandir", slow_scandir):
            files, problems = journal.snapshot(self.root, budget=0.01)
        self.assertEqual(files, {})
        self.assertEqual(len(visited), 1, visited)
        self.assertTrue(any("time budget" in item for item in problems), problems)

    def test_last_file_overrun_is_not_a_complete_scan(self):
        original = discovery.content_digest

        def slow_digest(path, deadline):
            time.sleep(0.03)
            return original(path, deadline)

        with patch.object(discovery, "content_digest", slow_digest):
            _, problems = journal.snapshot(self.root, budget=0.01)
        self.assertTrue(any("time budget" in item for item in problems), problems)

    def test_known_extras_are_hashed_once_and_preserved_mtime_edits_survive(self):
        entry = journal.Journal(self.event)
        try:
            entry.pre(self.event)
            before = self.source.stat()
            self.source.write_text("export const value = 2;\n", encoding="utf-8")
            os.utime(self.source, ns=(before.st_atime_ns, before.st_mtime_ns))
            with patch.object(discovery, "content_digest", wraps=discovery.content_digest) as digest:
                _, changed, problems = entry.changes({**self.event, "tool_input": {"path": "source.ts"}})
            self.assertFalse(problems)
            self.assertEqual(set(changed), {"source.ts"})
            self.assertEqual(digest.call_count, 1, "walk, explicit path, and known-file extra share one hash")
        finally:
            entry.close()

    def test_snapshot_does_not_hold_sqlite_write_lock(self):
        entry = journal.Journal(self.event)
        original = journal.snapshot
        other = sqlite3.connect(entry.directory / "journal.sqlite3", timeout=0.05)

        def snapshot_with_writer(*args, **kwargs):
            other.execute("BEGIN IMMEDIATE")
            other.execute("INSERT INTO meta VALUES ('concurrent_writer', 'true')")
            other.commit()
            return original(*args, **kwargs)

        try:
            with patch.object(journal, "snapshot", snapshot_with_writer):
                self.assertEqual(entry.pre(self.event), {})
            self.assertTrue(entry.get("concurrent_writer"))
            self.assertTrue(entry.get("baseline"))
        finally:
            other.close()
            entry.close()

    def test_concurrent_first_pre_keeps_earliest_committed_baseline(self):
        first = journal.Journal(self.event)
        second = journal.Journal(self.event)
        original = journal.snapshot

        def competing_snapshot(*args, **kwargs):
            with patch.object(journal, "snapshot", original):
                second.pre({**self.event, "tool_use_id": "tool-b"})
            self.source.write_text("export const value = 2;\n", encoding="utf-8")
            return original(*args, **kwargs)

        try:
            with patch.object(journal, "snapshot", competing_snapshot):
                first.pre(self.event)
            self.assertEqual(set(first.changes(self.event)[1]), {"source.ts"})
        finally:
            second.close()
            first.close()

    def test_empty_timed_out_scan_does_not_establish_complete_baseline(self):
        entry = journal.Journal(self.event)
        try:
            with patch.object(journal, "snapshot", return_value=({}, ["File reconciliation exceeded its time budget"])):
                output = entry.pre(self.event)
            self.assertIn("unverified", json.dumps(output))
            self.assertFalse(entry.get("baseline"))
            self.assertTrue(entry.get("coverage_gap"))
            entry.changes(self.event)
            self.assertTrue(entry.get("coverage_gap"))
        finally:
            entry.close()

    def test_partial_scan_keeps_observed_files_and_does_not_invent_deletions(self):
        sibling = self.root / "other.ts"
        sibling.write_text("export const other = 1;\n", encoding="utf-8")
        observed, _ = journal.snapshot(self.root)
        self.assertIn("source.ts", observed)
        entry = journal.Journal(self.event)
        try:
            partial = ({"source.ts": observed["source.ts"]}, ["File reconciliation exceeded its time budget"])
            with patch.object(journal, "snapshot", return_value=partial):
                output = entry.pre(self.event)
            self.assertIn("unverified", json.dumps(output))
            self.assertFalse(entry.get("baseline"))
            self.assertEqual(dict(entry.db.execute("SELECT path, revision FROM files")),
                {"source.ts": observed["source.ts"]})
            with patch.object(journal, "snapshot", return_value=partial):
                _, changed, problems = entry.changes(self.event)
            self.assertTrue(problems)
            self.assertNotIn("other.ts", changed)
            self.assertEqual({row[0] for row in entry.db.execute("SELECT path FROM files")}, {"source.ts"})
        finally:
            entry.close()

    def test_sql_execution_cannot_run_past_shared_deadline(self):
        entry = journal.Journal(self.event)
        began = time.monotonic()
        entry.db.deadline = began + 0.025
        try:
            with self.assertRaisesRegex(sqlite3.OperationalError, "interrupted"):
                entry.db.execute("WITH RECURSIVE work(n) AS (VALUES(0) UNION ALL SELECT n+1 FROM work WHERE n<10000000) SELECT sum(n) FROM work")
            self.assertLess(time.monotonic() - began, 0.5)
        finally:
            entry.close()

    def test_real_writer_waits_share_one_deadline_across_roots(self):
        other_root = Path(self.scratch.name) / "additional"
        other_root.mkdir()
        other_event = {**self.event, "cwd": str(other_root)}
        events = [self.event, other_event]
        databases = []
        for event in events:
            entry = journal.Journal(event)
            databases.append(entry.directory / "journal.sqlite3")
            entry.close()
        first_locked = threading.Event()
        release_first = threading.Event()

        def first_writer():
            connection = sqlite3.connect(databases[0])
            try:
                connection.execute("BEGIN IMMEDIATE")
                first_locked.set()
                release_first.wait(0.12)
                connection.rollback()
            finally:
                connection.close()

        writer = threading.Thread(target=first_writer)
        second_writer = sqlite3.connect(databases[1])
        second_writer.execute("BEGIN IMMEDIATE")
        writer.start()
        first_locked.wait(2)
        began = time.monotonic()
        deadline = began + 0.30
        try:
            journal.workspace_events(self.event, remember=True, deadline=deadline)
            self.assertGreater(time.monotonic() - began, 0.08)
            with self.assertRaises((sqlite3.OperationalError, TimeoutError)):
                journal.workspace_events(other_event, remember=True, deadline=deadline)
            self.assertLess(time.monotonic() - began, 0.75)
        finally:
            release_first.set()
            writer.join(2)
            second_writer.rollback()
            second_writer.close()
        restored = journal.Journal(other_event)
        try:
            self.assertFalse(restored.get("baseline"))
        finally:
            restored.close()


if __name__ == "__main__":
    unittest.main(verbosity=2)
