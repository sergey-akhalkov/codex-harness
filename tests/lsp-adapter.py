"""Standalone adapter checks. --real also starts the existing TypeScript LSP.

Creates disposable sources and CODEX_HOME under a temporary directory; never
updates packages, invokes Codex, or changes the user's projects/configuration.
"""
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
import time
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools/lsp"))
from backend import Backend, digest, language_for, installed_server
from journal import Journal, identity, workspace_events, command_fallback
from registry import generate
from server import DiagnosticsService

REAL = "--real" in sys.argv


class JournalTests(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory(prefix="harness-lsp-tests-")
        self.root = Path(self.scratch.name) / "workspace"
        self.root.mkdir()
        self.old_home = os.environ.get("CODEX_HOME")
        os.environ["CODEX_HOME"] = str(Path(self.scratch.name) / "codex-home")
        self.event = {"cwd": str(self.root), "session_id": "test-session", "agent_id": "child-a", "tool_use_id": "call-a"}
        self.source = self.root / "source.ts"
        self.source.write_text("export const value = 1;\n")

    def tearDown(self):
        if self.old_home is None:
            os.environ.pop("CODEX_HOME", None)
        else:
            os.environ["CODEX_HOME"] = self.old_home
        self.scratch.cleanup()

    def journal(self):
        return Journal(self.event)

    def test_late_adapter_adopts_preedit_baseline_and_partial_command_failure(self):
        entrypoint = Path(__file__).resolve().parents[1] / "tools/lsp/journal.py"
        subprocess.run([sys.executable, "-B", str(entrypoint), "--event", "pre"],
            input=json.dumps(self.event), text=True, capture_output=True, check=True)
        self.source.write_text("export const value: number = 'error';\n")
        (self.root / "untracked.ts").write_text("export const other = 2;\n")
        journal = self.journal()
        try:
            _, changes, problems = journal.changes({**self.event, "tool_response": {"exit_code": 1}})
            self.assertFalse(problems)
            self.assertEqual(set(changes), {"source.ts", "untracked.ts"})
            self.assertEqual(journal.stop(self.event)["decision"], "block")
            self.assertNotIn("decision", journal.stop({**self.event, "stop_hook_active": True}))
        finally:
            journal.close()

    def test_pre_does_not_absorb_previous_unacknowledged_edit(self):
        journal = self.journal()
        try:
            journal.pre(self.event)
            self.source.write_text("export const value = 2;\n")
            journal.pre({**self.event, "tool_use_id": "call-b"})
            self.assertIn("source.ts", journal.changes(self.event)[1])
        finally:
            journal.close()

    @unittest.skipUnless(REAL, "Use --real to start the installed TypeScript server")
    def test_approved_additional_root_keeps_independent_baseline_and_results(self):
        from unittest.mock import patch
        other = Path(self.scratch.name) / "approved additional root"
        other.mkdir()
        (other / "source.ts").write_text("export const value: number = 1;\n")
        entrypoint = Path(__file__).resolve().parents[1] / "tools/lsp/journal.py"
        with patch.dict(os.environ, {"HARNESS_LSP_WORKSPACE_ROOTS": json.dumps([str(other)])}):
            completed = subprocess.run([sys.executable, "-B", str(entrypoint), "--event", "pre"],
                input=json.dumps(self.event), text=True, capture_output=True, check=True)
            self.assertEqual(json.loads(completed.stdout), {})
        self.assertEqual(len(workspace_events(self.event)), 2, "Stop/MCP retain roots recorded independently by Pre")
        (other / "source.ts").write_text("export const value: number = 'broken';\n")
        service = DiagnosticsService()
        try:
            report = service.check(self.event)
            errors = [item for item in report["results"] if item["status"] == "diagnostics"]
            self.assertTrue(errors, report)
            self.assertEqual({item["workspace"] for item in errors}, {str(other.resolve())})
            self.assertEqual(report["status"], "diagnostics", report)
            self.assertEqual(self.source.read_text(), "export const value = 1;\n")
            (other / "source.ts").write_text("export const value: number = 2;\n")
            self.assertEqual(service.check(self.event)["status"], "clean")
        finally:
            service.close()

    def test_explicit_workdir_is_retained_without_scanning_arbitrary_command_paths(self):
        other = Path(self.scratch.name) / "shell workdir"
        other.mkdir()
        event = {**self.event, "tool_input": {"workdir": str(other), "cmd": "ignored arbitrary paths"}}
        self.assertEqual([Path(item["workspace"]) for item in workspace_events(event, remember=True)], [self.root, other])
        self.assertEqual(len(workspace_events(self.event)), 2)

    @unittest.skipUnless(REAL, "Use --real to start the installed TypeScript server")
    def test_command_fallback_reports_real_error_and_clearance_without_native_mcp(self):
        journal = self.journal()
        journal.pre(self.event)
        journal.close()
        self.source.write_text("export const value: number = 'broken';\n")
        error = command_fallback(self.event)
        self.assertIn('2322', error["hookSpecificOutput"]["additionalContext"])
        self.source.write_text("export const value: number = 2;\n")
        corrected = command_fallback({**self.event, "tool_use_id": "correction"})
        self.assertIn('"status": "clean"', corrected["hookSpecificOutput"]["additionalContext"])

    def test_native_completed_invocation_does_not_start_fallback_worker(self):
        from unittest.mock import patch
        journal = self.journal()
        journal.pre(self.event)
        token = journal.begin_check()
        journal.finish_check(token, {**self.event, "_origin": "native"})
        journal.close()
        with patch("journal.subprocess.Popen") as start:
            self.assertEqual(command_fallback(self.event), {})
            start.assert_not_called()

    def test_command_claim_prevents_native_duplicate_analysis(self):
        from unittest.mock import patch
        journal = self.journal()
        journal.pre(self.event)
        token = journal.claim(self.event, "command")
        service = DiagnosticsService()
        try:
            with patch.object(service, "_analyze") as analyze:
                report = service.check({**self.event, "_origin": "native"})
                self.assertEqual(report["status"], "delegated")
                analyze.assert_not_called()
                self.assertEqual(service.hook_result(report, self.event), {})
        finally:
            journal.release_claim(token)
            journal.close()
            service.close()

    def test_read_only_parent_stop_does_not_substitute_child_or_claim_clean(self):
        service = DiagnosticsService()
        try:
            stop = {**self.event, "event": "Stop"}
            report = service.check(stop)
            self.assertEqual(report["status"], "not-applicable")
            self.assertEqual(service.hook_result(report, stop), {})
            # A Post was observed but Pre was missing: completion must retain
            # that missing-baseline problem, even if no files can be identified.
            service.check({**self.event, "event": "PostToolUse"})
            self.assertEqual(service.check(stop)["status"], "unavailable")
        finally:
            service.close()

    def test_delphi_registry_keeps_compiler_target_when_merging_runtime_env(self):
        item = generate({"languages": [{"id": "delphi", "version": "v0.2.0", "command": ["pasls.exe"],
            "paths": {"pp": "C:/cache/fpc/bin/i386-win32/fpc.exe", "fpcdir": "C:/cache/fpc/source"},
            "lsp": {"env": {"PP": "C:/cache/fpc/bin/i386-win32/fpc.exe", "FPCDIR": "C:/cache/fpc/source"}}}]})["servers"]["pascal"]
        self.assertEqual(item["env"]["FPCTARGETCPU"], "i386")
        self.assertEqual(item["env"]["FPCTARGET"], "win32")

    def test_slow_startup_cannot_hold_batch_jobs_lock_past_deadline(self):
        from unittest.mock import Mock, patch
        gate, entered = threading.Event(), threading.Event()
        second = self.root / "second.ts"
        second.write_text("export const second = 1;\n")
        journal = self.journal()
        journal.pre(self.event)
        self.source.write_text("export const value = 2;\n")
        second.write_text("export const second = 2;\n")
        client = Mock()
        client.lock = threading.RLock()
        def create(*_):
            entered.set()
            gate.wait(2)
            return client
        service = DiagnosticsService()
        try:
            with patch("server.Backend", side_effect=create):
                began = time.monotonic()
                report = service.check(self.event, budget=0.05)
                self.assertLess(time.monotonic() - began, 0.5, report)
                self.assertTrue(entered.is_set())
                self.assertEqual(len(report["results"]), 2)
                self.assertTrue(all(row["status"] == "pending" for row in report["results"]), report)
                service.close()
                gate.set()
                for job in service.jobs.values():
                    job.result(timeout=2)
                client.close.assert_called_once()
        finally:
            gate.set()
            service.close()
            journal.close()

    def test_git_ignored_source_is_reconciled_and_scan_overflow_is_explicit(self):
        subprocess.run(["git", "init", "--quiet", str(self.root)], check=True, capture_output=True)
        (self.root / ".gitignore").write_text("ignored.ts\n")
        ignored = self.root / "ignored.ts"
        ignored.write_text("export const ignored = 1;\n")
        checked = subprocess.run(["git", "-C", str(self.root), "check-ignore", "ignored.ts"], check=True, capture_output=True, text=True)
        self.assertEqual(checked.stdout.strip(), "ignored.ts")
        journal = self.journal()
        try:
            journal.pre(self.event)
            ignored.write_text("export const ignored = 2;\n")
            self.assertIn("ignored.ts", journal.changes(self.event)[1])
            _, _, problems = journal.changes(self.event, budget=0)
            self.assertTrue(problems, "An exhausted scan cannot be interpreted as no changes")
        finally:
            journal.close()

    def test_project_rust_channel_cannot_select_an_executable_path(self):
        from unittest.mock import patch
        rust = Path(self.scratch.name) / "rustup"
        rust.mkdir()
        (rust / "settings.toml").write_text('default_toolchain = "stable-x86_64-pc-windows-msvc"\n')
        outside = Path(self.scratch.name) / "project supplied executable"
        (outside / "bin").mkdir(parents=True)
        (outside / "bin/rust-analyzer.exe").write_text("Inert test data; never executed")
        with patch.dict(os.environ, {"RUSTUP_HOME": str(rust)}):
            for channel in (outside.as_posix(), "../../project supplied executable", "C:relative", "*"):
                (self.root / "rust-toolchain.toml").write_text("[toolchain]\nchannel = '" + channel + "'\n")
                with self.assertRaises(ValueError):
                    installed_server("rust", self.root, discover_only=True)

    def test_new_nonconfiguration_source_before_stop_delivery_is_unresolved(self):
        from unittest.mock import patch
        journal = self.journal()
        journal.pre(self.event)
        journal.close()
        self.source.write_text("export const value = 2;\n")
        service = DiagnosticsService()
        def analyze(event, relative, revision):
            (self.root / "late.py").write_text("value: int = 'late write'\n")
            return {"file": relative, "revision": revision, "backend": "test", "status": "clean", "diagnostics": []}
        try:
            with patch.object(service, "_analyze", side_effect=analyze):
                report = service.check({**self.event, "event": "Stop", "stop_hook_active": True})
            self.assertEqual(report["status"], "unresolved", report)
            self.assertIn("late.py", {row["file"] for row in report["results"]})
            self.assertNotIn('"status": "clean"', json.dumps(service.hook_result(report, {"event": "Stop", "stop_hook_active": True})))
        finally:
            service.close()

    def test_repeated_stop_rechecks_bytes_before_reusing_native_completion(self):
        from unittest.mock import patch
        journal = self.journal()
        journal.pre(self.event)
        event = {**self.event, "event": "Stop", "tool_use_id": "", "stop_hook_active": True}
        token = journal.begin_check()
        journal.finish_check(token, {**event, "_origin": "native"})
        self.source.write_text("export const value: number = 'late';\n")
        try:
            with patch("journal.run_fallback_worker", return_value={"systemMessage": "unresolved new source generation"}) as worker:
                result = command_fallback(event)
            self.assertIn("unresolved", result["systemMessage"])
            worker.assert_called_once()
        finally:
            journal.close()

    @unittest.skipUnless(REAL, "Use --real for the installed JSON schema server")
    def test_changed_arbitrary_json_schema_rechecks_unchanged_instance(self):
        from unittest.mock import patch
        registry = Path(self.old_home or Path.home() / ".codex") / "harness/lsp-servers.json"
        if not registry.is_file():
            self.skipTest("Connected JSON server registry is required")
        schema = self.root / "arbitrary-schema.json"
        schema.write_text('{"type":"object","properties":{"example":{"type":"integer"}}}')
        (self.root / "example.json").write_text('{"$schema":"./arbitrary-schema.json","example":42}')
        journal = self.journal()
        journal.pre(self.event)
        journal.close()
        schema.write_text('{"type":"object","properties":{"example":{"type":"string"}}}')
        service = DiagnosticsService()
        try:
            with patch.dict(os.environ, {"HARNESS_LSP_REGISTRY": str(registry)}):
                report = service.check(self.event)
            self.assertTrue(any(row["file"] == "example.json" and row["status"] == "diagnostics" for row in report["results"]), report)
        finally:
            service.close()

    @unittest.skipUnless(REAL, "Use --real for concurrent installed backends")
    def test_concurrent_roots_keep_results_and_shutdown_independent(self):
        from concurrent.futures import ThreadPoolExecutor
        second = Path(self.scratch.name) / "second worktree"
        # A real linked Git worktree shares repository metadata while its
        # applicable source root and diagnostic process must remain separate.
        for arguments in (["init", "--quiet"], ["add", "source.ts"],
                ["-c", "user.name=LspFixture", "-c", "user.email=lsp-fixture@example.invalid", "commit", "--quiet", "-m", "baseline"],
                ["worktree", "add", "--quiet", "--detach", str(second), "HEAD"]):
            subprocess.run(["git", "-C", str(self.root), "-c", "core.autocrlf=false", *arguments],
                check=True, capture_output=True)
        self.assertTrue((second / ".git").is_file())
        source_b = second / "source.ts"
        source_b.write_text("export const value: string = 'initial';\n")
        event_b = {**self.event, "cwd": str(second), "session_id": "other-session"}
        for event in (self.event, event_b):
            journal = Journal(event)
            journal.pre(event)
            journal.close()
        self.source.write_text("export const value: number = 'broken';\n")
        source_b.write_text("export const value: string = 42;\n")
        service = DiagnosticsService()
        try:
            with ThreadPoolExecutor(max_workers=2) as pool:
                a, b = list(pool.map(service.check, (self.event, event_b)))
            self.assertEqual(a["workspace"], str(self.root))
            self.assertEqual(b["workspace"], str(second))
            self.assertIn("Type 'string' is not assignable to type 'number'", json.dumps(a))
            self.assertIn("Type 'number' is not assignable to type 'string'", json.dumps(b))
            client_b = next(value for key, value in service.backends.items() if key[0] == str(second))
            service.invalidate(self.event)
            source_b.write_text("export const value: string = 'correct';\n")
            self.assertEqual(service.check(event_b)["status"], "clean")
            self.assertIn(client_b, service.backends.values())
        finally:
            service.close()

    @unittest.skipUnless(REAL and os.name == "nt", "Windows process-tree timeout proof")
    def test_hung_fallback_is_bounded_preserves_edits_and_other_consumer(self):
        from unittest.mock import patch
        registry = Path(self.scratch.name) / "hung-registry.json"
        registry.write_text(json.dumps({"servers": {"typescript": {"command": [sys.executable, "-c", "import time; time.sleep(60)"], "adapter": "typescript"}}}))
        journal = self.journal()
        journal.pre(self.event)
        self.source.write_text("export const value: number = 'unverified';\n")
        other = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(60)"], creationflags=subprocess.CREATE_NO_WINDOW)
        began = time.monotonic()
        try:
            with patch.dict(os.environ, {"HARNESS_LSP_REGISTRY": str(registry)}):
                report = command_fallback(self.event, budget=4)
            self.assertLess(time.monotonic() - began, 8)
            self.assertIn("unresolved", json.dumps(report))
            self.assertIsNone(other.poll(), "Timeout must not kill an unrelated consumer")
            self.assertIn("unverified", self.source.read_text())
            self.assertIn("source.ts", journal.changes(self.event)[1])
        finally:
            other.terminate()
            other.wait(timeout=2)
            journal.close()

    def test_hash_detects_content_with_preserved_mtime_and_size(self):
        journal = self.journal()
        try:
            journal.pre(self.event)
            previous = self.source.stat()
            self.source.write_text("export const value = 2;\n")
            os.utime(self.source, ns=(previous.st_atime_ns, previous.st_mtime_ns))
            self.assertIn("source.ts", journal.changes(self.event)[1])
        finally:
            journal.close()

    def test_no_baseline_cannot_claim_clean_and_child_identity_is_separate(self):
        journal = self.journal()
        other = Journal({**self.event, "agent_id": "child-b"})
        try:
            journal.pre(self.event)
            self.assertNotEqual(journal.directory, other.directory)
            self.assertTrue(other.changes(self.event)[2])
        finally:
            journal.close()
            other.close()

    def test_rename_delete_and_current_result_acknowledgment(self):
        journal = self.journal()
        try:
            journal.pre(self.event)
            moved = self.root / "moved.ts"
            self.source.rename(moved)
            _, changed, _ = journal.changes(self.event)
            self.assertIsNone(changed["source.ts"])
            self.assertIn("moved.ts", changed)
            journal.accept({"file": "source.ts", "revision": None, "status": "deleted", "diagnostics": []})
            journal.accept({"file": "moved.ts", "revision": digest(moved), "status": "pending", "diagnostics": []})
            self.assertIn("moved.ts", journal.changes(self.event)[1])
            journal.accept({"file": "moved.ts", "revision": digest(moved), "status": "clean", "diagnostics": []})
            self.assertFalse(journal.changes(self.event)[1])
        finally:
            journal.close()

    def test_qt_translation_is_xml(self):
        self.source.write_text('<?xml version="1.0"?><TS version="2.1"></TS>')
        self.assertEqual(language_for(self.source), "xml")

    def test_child_pre_post_and_completion_share_native_transcript_identity(self):
        stream = str(Path(self.scratch.name) / "child-rollout.jsonl")
        pre = {**self.event, "agent_id": "native-child", "transcript_path": stream}
        post = {**self.event, "agent_id": None, "transcript_path": stream}
        stop = {**self.event, "agent_id": "native-child", "transcript_path": str(Path(self.scratch.name) / "parent.jsonl"),
                "agent_transcript_path": stream}
        self.assertEqual(identity(pre), identity(post))
        self.assertEqual(identity(pre), identity(stop))
        self.assertNotEqual(identity(pre), identity({**post, "transcript_path": str(Path(self.scratch.name) / "other-child.jsonl")}))

    def test_new_configuration_during_analysis_invalidates_old_clean_result(self):
        from unittest.mock import patch
        journal, service = self.journal(), DiagnosticsService()
        try:
            journal.pre(self.event)
            self.source.write_text('export const value = 2;\n')
            def adds_config(event, relative, revision):
                (self.root / "tsconfig.json").write_text('{"compilerOptions":{"strict":true}}')
                return {"file": relative, "revision": revision, "backend": "test", "status": "clean", "diagnostics": []}
            with patch.object(service, "_analyze", side_effect=adds_config):
                report = service.check(self.event)
            self.assertEqual(report["status"], "unresolved", report)
            self.assertEqual(report["results"][0]["status"], "stale", report)
            self.assertIn("source.ts", journal.changes(self.event)[1])
        finally:
            service.close()
            journal.close()

    def test_summary_prioritizes_later_error_and_bounds_diagnostic_messages(self):
        results = [{"file": f"warning-{index}.ts", "status": "diagnostics", "diagnostics": [{"message": "w" * 50000, "severity": 2}]}
                   for index in range(70)]
        results.append({"file": "critical.ts", "status": "diagnostics", "diagnostics": [{"message": "required error", "severity": 1}]})
        output = DiagnosticsService.hook_result({"results": results, "status": "diagnostics", "report_path": "complete.json"}, {})
        text = output["hookSpecificOutput"]["additionalContext"]
        summary = json.loads(text.split("instructions: ", 1)[1])
        self.assertEqual(summary["results"][0]["file"], "critical.ts")
        self.assertGreater(summary["omitted_diagnostics"], 0)
        self.assertLess(len(text), 26000)
        self.assertEqual(summary["report_path"], "complete.json")

    def test_registry_rejects_known_incompatible_variants_and_retains_missing_scope(self):
        generated = generate({"languages": [{"id": "toml", "command": ["node", "@taplo/cli/dist/cli.js"]},
            {"id": "cmake", "command": ["cmake-language-server.exe"]},
            {"id": "rust", "command": ["rust-analyzer.exe"]},
            {"id": "bash", "command": ["node", "bash-language-server/out/cli.js"], "paths": {"shellcheck": "shellcheck.exe"}}]})["servers"]
        self.assertEqual(len(generated), 18)
        self.assertEqual(generated["toml"]["status"], "unavailable")
        self.assertEqual(generated["cmake"]["status"], "unavailable")
        self.assertTrue(generated["rust"]["project_toolchain"])
        self.assertEqual(generated["yaml"]["status"], "unavailable")
        self.assertFalse(generated["yaml"]["required"])
        self.assertEqual(generated["bash"]["env"]["SHELLCHECK_PATH"], "shellcheck.exe")

    def test_stale_empty_push_cannot_replace_current_error(self):
        backend = Backend.__new__(Backend)
        backend.condition = threading.Condition()
        backend.generation = 0
        backend.published = {}
        uri = self.source.as_uri()
        diagnostic = {"message": "current error", "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 1}}}
        backend._notification("textDocument/publishDiagnostics", {"uri": uri, "version": 2, "diagnostics": [diagnostic]})
        backend._notification("textDocument/publishDiagnostics", {"uri": uri, "version": 1, "diagnostics": []})
        current = backend.published[backend.uri_key(uri)]
        self.assertEqual(current["version"], 2)
        self.assertEqual(current["diagnostics"], [diagnostic])

    def test_backend_error_log_with_empty_pull_is_failure_not_clearance(self):
        from unittest.mock import Mock
        backend = Backend.__new__(Backend)
        backend.condition, backend.lock = threading.Condition(), threading.RLock()
        backend.generation = backend.log_error_generation = 0
        backend.last_log_error = ""
        backend.documents = {}
        backend.language, backend.definition = "json", {"id": "json"}
        backend.capabilities = {"diagnosticProvider": {}}
        backend.dynamic_capabilities = set()
        backend.markdown_client = backend.delphi_project = None
        backend.sync = lambda relative, **_: (self.source, {"uri": self.source.as_uri(), "revision": digest(self.source), "text": "", "version": 1})
        backend.connection = Mock()
        def response(*_):
            backend._notification("window/logMessage", {"type": 1, "message": "Validation failed: TypeError"})
            return {"kind": "full", "items": []}
        backend.connection.send_request.side_effect = response
        result = backend.diagnostics("source.ts")
        self.assertEqual(result["status"], "failed", result)
        self.assertIn("TypeError", result["reason"])

    def test_batch_timeout_keeps_changes_for_later_completion(self):
        from unittest.mock import patch
        journal, service = self.journal(), DiagnosticsService()
        try:
            journal.pre(self.event)
            self.source.write_text("export const value = 2;\n")
            gate = threading.Event()
            def slow(event, relative, revision):
                gate.wait(2)
                return {"file": relative, "revision": revision, "backend": "test", "status": "clean", "diagnostics": []}
            with patch.object(service, "_analyze", side_effect=slow):
                report = service.check(self.event, budget=0.01)
                self.assertEqual(report["status"], "unresolved")
                self.assertEqual(report["results"][0]["status"], "pending")
                self.assertIn("source.ts", journal.changes(self.event)[1])
                gate.set()
                reconciled = service.check(self.event)
                self.assertEqual(reconciled["status"], "clean", reconciled)
        finally:
            service.close()
            journal.close()

    @unittest.skipUnless(REAL, "Run --real to exercise the existing installed TypeScript server")
    def test_actual_dependent_error_is_reported(self):
        (self.root / "tsconfig.json").write_text('{"compilerOptions":{"strict":true,"noEmit":true},"include":["*.ts"]}')
        self.source.write_text('export function check(value: string) { return value; }\n')
        (self.root / "caller.ts").write_text('import {check} from "./source";\ncheck("value");\n')
        journal, service = self.journal(), DiagnosticsService()
        try:
            journal.pre(self.event)
            self.source.write_text('export function check(value: number) { return value; }\n')
            report = service.check(self.event)
            self.assertTrue(any(result["file"] == "caller.ts" and result["diagnostics"] for result in report["results"]), report)
        finally:
            service.close()
            journal.close()

    def test_pending_caller_job_cannot_survive_changed_dependency_generation(self):
        from unittest.mock import patch
        self.source.unlink()
        library, caller = self.root / "lib.py", self.root / "main.py"
        library.write_text("generation = 0\n")
        caller.write_text("from lib import generation\n")
        journal, service = self.journal(), DiagnosticsService()
        gate, started = threading.Event(), threading.Event()
        caller_generations = []
        def analyze(event, relative, revision):
            diagnostics = []
            if relative == "main.py":
                generation = library.read_text()
                caller_generations.append(generation)
                if "1" in generation:
                    started.set()
                    gate.wait(3)
                if "2" in generation:
                    diagnostics = [{"message": "new dependency error", "severity": 1}]
            return {"file": relative, "revision": revision, "backend": "python",
                "status": "diagnostics" if diagnostics else "clean", "diagnostics": diagnostics}
        try:
            journal.pre(self.event)
            library.write_text("generation = 1\n")
            with patch.object(service, "_analyze", side_effect=analyze):
                pending = service.check(self.event, budget=0.2)
                self.assertTrue(started.is_set(), pending)
                self.assertEqual(pending["status"], "unresolved", pending)
                library.write_text("generation = 2\n")
                gate.set()
                for job in list(service.jobs.values()):
                    job.result(timeout=2)
                current = service.check(self.event)
            self.assertEqual(current["status"], "diagnostics", current)
            self.assertEqual(caller_generations, ["generation = 1\n", "generation = 2\n"])
            self.assertTrue(any(row["file"] == "main.py" and row["diagnostics"] for row in current["results"]), current)
        finally:
            gate.set()
            service.close()
            journal.close()

    @unittest.skipUnless(REAL, "Run --real to exercise the installed Python and XML backends")
    def test_unchanged_python_caller_and_xml_instance_follow_dependency_edits(self):
        from unittest.mock import patch
        registry = Path(self.old_home or str(Path.home() / ".codex")) / "harness/lsp-servers.json"
        if not registry.is_file():
            self.skipTest("Configured shared language registry is absent")
        cases = [
            ("lib.py", "main.py", "def answer() -> int:\n    return 42\n", "def answer() -> str:\n    return 'changed'\n",
             "from lib import answer\nvalue: int = answer()\n", "reportAssignmentType"),
            ("schema.xsd", "main.xml", '<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="value" type="xs:int"/></xs:schema>',
             '<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="value" type="xs:boolean"/></xs:schema>',
             '<value xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:noNamespaceSchemaLocation="schema.xsd">42</value>', None),
        ]
        self.source.unlink()
        with patch.dict(os.environ, {"HARNESS_LSP_REGISTRY": str(registry)}):
            for index, (dependency, caller, good, bad, source, code) in enumerate(cases):
                with self.subTest(language=dependency):
                    root = self.root / str(index)
                    root.mkdir()
                    (root / dependency).write_text(good)
                    (root / caller).write_text(source)
                    event = {**self.event, "workspace": str(root)}
                    journal, service = Journal(event), DiagnosticsService()
                    try:
                        journal.pre(event)
                        (root / dependency).write_text(bad)
                        report = service.check(event)
                        affected = [row for row in report["results"] if row["file"] == caller and row["status"] == "diagnostics"]
                        self.assertTrue(affected, report)
                        if code:
                            self.assertTrue(any(item.get("code") == code for row in affected for item in row["diagnostics"]), report)
                        (root / dependency).write_text(good)
                        cleared = service.check(event)
                        self.assertEqual(cleared["status"], "clean", cleared)
                        self.assertEqual((root / caller).read_text(), source)
                    finally:
                        service.close()
                        journal.close()

    @unittest.skipUnless(REAL, "Run --real to exercise current opened Python dependency buffers")
    def test_warm_multifile_batch_uses_current_open_dependency_buffers(self):
        from unittest.mock import patch
        registry = Path(self.old_home or str(Path.home() / ".codex")) / "harness/lsp-servers.json"
        if not registry.is_file():
            self.skipTest("Configured shared language registry is absent")
        self.source.unlink()
        caller, library = self.root / "a_main.py", self.root / "zlib_local.py"
        caller.write_text("from zlib_local import foo\nvalue: str = foo()\n")
        library.write_text("def foo() -> str:\n    return 'initial'\n")
        journal, service = self.journal(), DiagnosticsService()
        try:
            journal.pre(self.event)
            caller.write_text(caller.read_text() + "# first batch\n")
            with patch.dict(os.environ, {"HARNESS_LSP_REGISTRY": str(registry)}):
                first = service.check(self.event)
                self.assertEqual(first["status"], "clean", first)
                client = next(iter(service.backends.values()))
                self.assertEqual(set(client.documents), {"a_main.py", "zlib_local.py"})
                caller.write_text("from zlib_local import foo\nvalue: int = foo()\n")
                library.write_text("def foo() -> int:\n    return 42\n")
                second = service.check(self.event)
                self.assertEqual(second["status"], "clean", second)
                self.assertIn(client, service.backends.values(), "A new batch must reuse its warmed client")
                self.assertTrue(all(row["status"] == "clean" for row in second["results"]), second)
        finally:
            service.close()
            journal.close()

    @unittest.skipUnless(REAL, "Run --real to exercise the existing installed TypeScript server")
    def test_actual_typescript_error_clearance_and_navigation(self):
        (self.root / "tsconfig.json").write_text('{"compilerOptions":{"strict":true,"noEmit":true},"include":["*.ts"]}')
        journal = self.journal()
        service = DiagnosticsService()
        try:
            journal.pre(self.event)
            self.source.write_text('export const value: number = "wrong";\nconsole.log(value);\n')
            report = service.check(self.event)
            self.assertEqual(report["status"], "diagnostics", report)
            self.assertTrue(any(item.get("code") == 2322 for result in report["results"] for item in result["diagnostics"]))
            backend = service.backend(self.event, "typescript")
            symbols = backend.navigation("source.ts", "symbols")
            self.assertTrue(any(symbol["name"] == "value" for symbol in symbols), symbols)
            declaration = backend.navigation("source.ts", "definition", 1, 13)
            self.assertTrue(declaration, declaration)
            references = backend.navigation("source.ts", "references", 1, 13)
            self.assertGreaterEqual(len(references), 2, references)
            self.assertTrue(backend.navigation("source.ts", "hover", 1, 13))
            self.assertTrue(backend.navigation("source.ts", "workspace_symbols", query="value"))
            revision = digest(self.source)
            rename = backend.navigation("source.ts", "rename_preview", 1, 13, new_name="renamedValue")
            self.assertTrue(rename.get("changes") or rename.get("documentChanges"), rename)
            self.assertEqual(digest(self.source), revision, "Rename preview must never apply the returned edits")
            journal.pre({**self.event, "tool_use_id": "correction"})
            self.source.write_text('export const value: number = 42;\nconsole.log(value);\n')
            cleared = service.check(self.event)
            self.assertEqual(cleared["status"], "clean", cleared)
            self.assertFalse(cleared["results"][0]["diagnostics"])
            self.assertEqual(service.check(self.event)["status"], "unchanged")
        finally:
            service.close()
            journal.close()


if __name__ == "__main__":
    unittest.main(argv=[sys.argv[0]], verbosity=2)
