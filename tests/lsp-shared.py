"""Shared diagnostics checks; --real starts only owned temporary broker/LSPs.

Uses the installed Serena Python and language packages. No Codex/model calls,
global configuration edits or termination of pre-existing processes.
"""
from concurrent.futures import ThreadPoolExecutor
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
import time
import unittest
import urllib.error
from unittest.mock import patch

import psutil

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools/lsp"))
import broker
from journal import Journal, state_directory, workspace_events
from server import DiagnosticsService

REAL = "--real" in sys.argv


class FakeBackend:
    def __init__(self, *args):
        self.args = args
        self.lock = threading.RLock()
        self.closed = False

    def close(self):
        with self.lock:
            self.closed = True


class PoolTests(unittest.TestCase):
    def test_sessions_share_only_backend_and_keep_journals_and_roots(self):
        with tempfile.TemporaryDirectory() as scratch, patch.dict(os.environ, {"CODEX_HOME": scratch}):
            root = Path(scratch) / "project"
            other = Path(scratch) / "other"
            root.mkdir()
            other.mkdir()
            first = {"workspace": str(root), "session_id": "one", "agent_id": "first", "_settings_revision": "a"}
            second = {**first, "session_id": "two", "agent_id": "second"}
            with patch("server.Backend", FakeBackend):
                service = DiagnosticsService(shared=True)
            try:
                with service.backend_lease(first, "typescript") as a:
                    pass
                with service.backend_lease(second, "typescript") as b:
                    self.assertIs(a, b)
                self.assertNotEqual(state_directory(first), state_directory(second))
                with patch.dict(os.environ, {"HARNESS_LSP_WORKSPACE_ROOTS": json.dumps([str(other)])}):
                    self.assertEqual(len(workspace_events({**first, "_workspace_roots": []})), 1)
                    self.assertEqual(len(workspace_events({**second, "_workspace_roots": [str(other)]})), 2)
                with service.backend_lease({**second, "_settings_revision": "b"}, "typescript") as c:
                    self.assertIsNot(b, c)
                    self.assertTrue(b.closed)
                with service.backend_lease({**second, "workspace": str(other)}, "typescript") as d:
                    self.assertIsNot(c, d)
            finally:
                service.close()

    def test_startup_race_has_one_backend_and_busy_lease_cannot_be_evicted(self):
        created = []
        def factory(*args):
            time.sleep(0.02)
            item = FakeBackend(*args)
            created.append(item)
            return item
        pool = broker.BackendPool(factory, maximum=1)
        key = ("root", "ts", "None", "registry", "config")
        def call(_):
            with pool.lease(key, ()) as backend:
                time.sleep(0.01)
                return id(backend)
        try:
            with ThreadPoolExecutor(max_workers=6) as threads:
                self.assertEqual(len(set(threads.map(call, range(6)))), 1)
            self.assertEqual(len(created), 1)
            with pool.lease(key, ()) as backend:
                with self.assertRaises(TimeoutError):
                    with pool.lease(("other", *key[1:]), (), timeout=0.02):
                        pass
                self.assertFalse(backend.closed)
            with pool.lease(("other", *key[1:]), ()):
                self.assertTrue(created[0].closed)
        finally:
            pool.close()

    def test_failure_releases_capacity_and_idle_expires(self):
        pool = broker.BackendPool(lambda: (_ for _ in ()).throw(RuntimeError("startup failed")), maximum=1, idle_seconds=0)
        key = ("root", "ts", "None", "registry", "config")
        with self.assertRaisesRegex(RuntimeError, "startup failed"):
            with pool.lease(key, ()):
                pass
        self.assertEqual(pool.status(), [])
        pool.factory = FakeBackend
        with pool.lease(key, ()) as backend:
            pool.reap()
            self.assertFalse(backend.closed)
        pool.reap()
        self.assertTrue(backend.closed)
        self.assertEqual(pool.status(), [])

    def test_failed_buffer_state_is_retired_after_its_last_lease(self):
        pool = broker.BackendPool(FakeBackend, maximum=1)
        key = ("root", "ts", "None", "registry", "config")
        with pool.lease(key, ()) as backend:
            backend.sync_failed = True
            self.assertFalse(backend.closed)
        self.assertTrue(backend.closed)
        with pool.lease(key, ()) as replacement:
            self.assertIsNot(backend, replacement)
        pool.close()

    def test_closing_backend_retains_capacity_until_process_exit(self):
        closing, release = threading.Event(), threading.Event()
        class SlowClose(FakeBackend):
            def close(self):
                closing.set()
                release.wait(2)
                super().close()
        pool = broker.BackendPool(SlowClose, maximum=1)
        key = ("root", "ts", "None", "registry", "config")
        with pool.lease(key, ()):
            pass
        with ThreadPoolExecutor(max_workers=1) as executor:
            eviction = executor.submit(pool.invalidate, "root")
            try:
                self.assertTrue(closing.wait(1))
                with self.assertRaises(TimeoutError):
                    with pool.lease(("other", *key[1:]), (), timeout=0.02):
                        pass
                self.assertEqual(pool.closing, 1)
            finally:
                release.set()
                eviction.result(timeout=2)
        with pool.lease(("other", *key[1:]), ()):
            pass
        pool.close()

    def test_transferred_claim_survives_original_client_cleanup(self):
        with tempfile.TemporaryDirectory() as scratch, patch.dict(os.environ, {"CODEX_HOME": scratch}):
            event = {"workspace": scratch, "session_id": "one", "tool_use_id": "tool"}
            journal = Journal(event)
            try:
                old = journal.claim(event, "command")
                new = journal.transfer_claim(old)
                self.assertNotEqual(old, new)
                journal.release_claim(old)
                self.assertEqual(journal.get("active_claim")["token"], new)
                self.assertIsNone(journal.claim(event, "native"))
                journal.release_claim(new)
                self.assertFalse(journal.get("active_claim"))
            finally:
                journal.close()

    def test_shutdown_failure_closes_admission(self):
        class BrokenClose(FakeBackend):
            def close(self):
                raise RuntimeError("process did not exit")
        pool = broker.BackendPool(BrokenClose, maximum=1)
        key = ("root", "ts", "None", "registry", "config")
        with pool.lease(key, ()):
            pass
        with self.assertRaisesRegex(RuntimeError, "process did not exit"):
            pool.invalidate("root")
        with self.assertRaisesRegex(RuntimeError, "shutting down"):
            with pool.lease(("other", *key[1:]), ()):
                pass

    def test_retire_without_receipt_reports_pending_for_live_startup(self):
        with tempfile.TemporaryDirectory() as scratch, patch.dict(os.environ, {"CODEX_HOME": scratch}):
            root = broker.directory()
            root.mkdir(parents=True)
            with broker.FileLock(root / "startup.lock") as lock:
                lock.acquire(0)
                self.assertEqual(broker.retire(0.1)["status"], "pending")
            self.assertEqual(broker.retire(0.1)["status"], "not-running")

    def test_unfinished_activation_blocks_startup_until_recovery_or_commit(self):
        with tempfile.TemporaryDirectory() as scratch, patch.dict(os.environ, {"CODEX_HOME": scratch}):
            pending = Path(scratch) / "harness/activation-pending.json"
            pending.parent.mkdir()
            with patch.object(broker, "source_identity", return_value="fixture"), patch.object(broker, "read_endpoint", return_value={"source": "fixture"}):
                for body in ('{"phase":"prepared"}', '{"phase":"incompleteUpdate"}', 'malformed'):
                    pending.write_text(body)
                    with self.assertRaisesRegex(RuntimeError, "Recover"):
                        broker.ensure_endpoint()
                pending.write_text('{"phase":"committed"}')
                self.assertEqual(broker.ensure_endpoint(), {"source": "fixture"})
                pending.unlink()  # Successful Recover restores the old connection.
                self.assertEqual(broker.ensure_endpoint(), {"source": "fixture"})

    def test_rapid_generations_cannot_drop_outstanding_work_from_admission(self):
        with tempfile.TemporaryDirectory() as scratch, patch.dict(os.environ, {"CODEX_HOME": scratch}):
            root = Path(scratch) / "project"
            root.mkdir()
            source = root / "main.ts"
            source.write_text("const value = 1;")
            event = {"workspace": str(root), "session_id": "one", "event": "PostToolUse", "tool_use_id": "initial"}
            journal = Journal(event)
            journal.pre(event)
            directory = str(journal.directory)
            journal.close()
            service = DiagnosticsService(shared=True)
            release = threading.Event()
            try:
                with service.guard:
                    for index in range(16):
                        key = (directory, f"pending-{index}.ts", "old-revision", "settings", "old-generation")
                        service.jobs[key] = service.pool.submit(release.wait, 15)
                with patch.object(service, "_analyze", side_effect=AssertionError("Admission cap bypassed")):
                    for generation in range(6):
                        source.write_text(f"const value = {generation + 2};")
                        service.check({**event, "tool_use_id": str(generation)}, budget=0.15)
                        self.assertEqual(len(service.jobs), 16)
                        self.assertLessEqual(service.pool._work_queue.qsize(), 16)
                        self.assertEqual(service.backends, {})
                        self.assertEqual(service.client_locks, {})
            finally:
                release.set()
                service.pool.shutdown(wait=True)
                service.close()


@unittest.skipUnless(REAL, "Use --real for owned daemon and installed TypeScript checks")
class BrokerTests(unittest.TestCase):
    def test_activation_admission_blocks_calls_but_allows_retirement(self):
        endpoint = broker.ensure_endpoint(25)
        self.owned.append((endpoint['pid'], endpoint['started']))
        pending = self.home / 'harness/activation-pending.json'
        pending.write_text(json.dumps({'phase': 'prepared'}))
        with self.assertRaisesRegex(RuntimeError, 'Recover'):
            broker.ensure_endpoint()
        self.assertEqual(broker.exchange(endpoint, 'status', {}, 2)['pid'], endpoint['pid'])
        with self.assertRaisesRegex(RuntimeError, 'Recover'):
            broker.exchange(endpoint, 'diagnostics', {}, 2)
        self.assertEqual(broker.retire(25)['status'], 'retired')
        pending.unlink()
        replacement = broker.ensure_endpoint(25)
        self.owned.append((replacement['pid'], replacement['started']))
        self.assertNotEqual(replacement['pid'], endpoint['pid'])

    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory(prefix="harness-lsp-shared-")
        self.root = Path(self.scratch.name)
        self.home = self.root / "home"
        (self.home / "harness").mkdir(parents=True)
        self.patch = patch.dict(os.environ, {"CODEX_HOME": str(self.home), "HARNESS_LSP_WORKSPACE_ROOTS": "[]",
            "HARNESS_LSP_BROKER_IDLE_SECONDS": "300", "HARNESS_LSP_BACKEND_IDLE_SECONDS": "300"})
        self.patch.start()
        self.owned = []
        for name in ("one", "two"):
            root = self.root / name
            root.mkdir()
            (root / "tsconfig.json").write_text('{"compilerOptions":{"strict":true,"noEmit":true},"include":["*.ts"]}')
            (root / "source.ts").write_text(f"export const {name}: number = 'bad';\n")

    def endpoint(self):
        endpoint = broker.ensure_endpoint(25)
        self.owned.append((endpoint["pid"], endpoint["started"]))
        return endpoint

    @staticmethod
    def alive(pid, started):
        try:
            return abs(psutil.Process(pid).create_time() - started) < 0.001
        except psutil.Error:
            return False

    def tearDown(self):
        # Only identities received from this test's private broker directory.
        retirement = {"status": "not-running"}
        endpoint = broker.read_endpoint(broker.directory())
        if endpoint:
            self.owned.append((endpoint["pid"], endpoint["started"]))
            retirement = broker.retire(25)
        for pid, started in set(self.owned):
            if self.alive(pid, started):
                process = psutil.Process(pid)
                children = process.children(recursive=True)
                process.kill()
                process.wait(timeout=10)
                # Windows Job containment owns the crash cleanup. Portable
                # fallback explicitly cleans only descendants captured above.
                if os.name != "nt":
                    for child in children:
                        try:
                            child.kill()
                        except psutil.Error:
                            pass
        self.patch.stop()
        # The venv launcher can release its inherited log handle just after
        # its contained child exits; this is a bounded ownership wait.
        deadline = time.monotonic() + 10
        while True:
            try:
                self.scratch.cleanup()
                break
            except PermissionError:
                if time.monotonic() >= deadline:
                    raise
                time.sleep(0.05)
        self.assertNotEqual(retirement["status"], "pending", retirement)

    def payload(self, project="one", session="first"):
        return {"workspace": str(self.root / project), "session_id": session, "file": "source.ts"}

    def test_actual_rpc_reuse_isolation_config_and_crash_recovery(self):
        with ThreadPoolExecutor(max_workers=6) as threads:
            endpoints = list(threads.map(lambda _: self.endpoint(), range(6)))
        self.assertEqual(len({item["pid"] for item in endpoints}), 1)
        endpoint = endpoints[0]
        first = broker.request("diagnostics", self.payload())
        self.assertEqual(first["status"], "diagnostics", first)
        self.assertTrue(any(item.get("code") == 2322 for item in first["diagnostics"]), first)
        before = broker.request("status", {})
        self.assertEqual(len(before["backends"]), 1)
        backend_pid = before["backends"][0]["pid"]
        self.assertIsInstance(backend_pid, int)
        second = broker.request("diagnostics", self.payload(session="second"))
        self.assertEqual(second["status"], "diagnostics", second)
        self.assertEqual(broker.request("status", {})["backends"][0]["pid"], backend_pid)
        other = broker.request("navigate", {**self.payload("two"), "operation": "symbols"})
        self.assertEqual([item["name"] for item in other["result"]], ["two"])
        self.assertEqual(len(broker.request("status", {})["backends"]), 2)
        def symbols(project):
            result = broker.request("navigate", {**self.payload(project, "concurrent"), "operation": "symbols"})
            return [item["name"] for item in result["result"]]
        with ThreadPoolExecutor(max_workers=4) as threads:
            self.assertEqual(list(threads.map(symbols, ["one", "two", "one", "two"])), [["one"], ["two"], ["one"], ["two"]])
        (self.root / "one/source.ts").write_text("export const one: number = 1;\n")
        clear = broker.request("diagnostics", self.payload(session="second"))
        self.assertEqual(clear["status"], "clean", clear)
        (self.root / "one/tsconfig.json").write_text('{"compilerOptions":{"strict":true,"noEmit":true,"noUnusedLocals":true},"include":["*.ts"]}')
        broker.request("diagnostics", self.payload())
        replacement = next(item for item in broker.request("status", {})["backends"] if item["key"][0] == os.path.normcase(str(self.root / "one")))
        self.assertNotEqual(replacement["pid"], backend_pid)
        (self.home / "harness/lsp-servers.json").write_text('{"servers":{}}')
        broker.request("diagnostics", self.payload())
        after_registry = next(item for item in broker.request("status", {})["backends"] if item["key"][0] == os.path.normcase(str(self.root / "one")))
        self.assertNotEqual(after_registry["pid"], replacement["pid"])
        with self.assertRaises(urllib.error.HTTPError):
            broker.exchange({**endpoint, "token": "0" * 64}, "status", {}, 3)
        process = psutil.Process(endpoint["pid"])
        children = [(p.pid, p.create_time()) for p in process.children(recursive=True)]
        self.assertTrue(children)
        process.kill()
        process.wait(timeout=10)
        if os.name == "nt":
            deadline = time.monotonic() + 10
            while any(self.alive(*item) for item in children) and time.monotonic() < deadline:
                time.sleep(0.05)
            self.assertFalse([item for item in children if self.alive(*item)])
        recovered = self.endpoint()
        self.assertNotEqual(recovered["pid"], endpoint["pid"])
        self.assertEqual(broker.request("diagnostics", self.payload())["status"], "clean")

    def test_thin_once_and_mcp_share_daemon_and_keep_hook_identity(self):
        endpoint = self.endpoint()
        # This case verifies thin transports reusing an established backend;
        # cold startup and its budget are exercised separately above.
        self.assertEqual(broker.request("diagnostics", self.payload())["status"], "diagnostics")
        event = {"workspace": str(self.root / "one"), "session_id": "hook", "tool_use_id": "change", "event": "PostToolUse"}
        journal = Journal(event)
        journal.pre(event)
        journal.close()
        (self.root / "one/source.ts").write_text("export const one: number = 'different';\n")
        output = subprocess.run([sys.executable, "-B", str(ROOT / "tools/lsp/server.py"), "--once"],
            input=json.dumps(event), text=True, capture_output=True, timeout=30, check=True)
        self.assertIn("2322", output.stdout, output.stderr + output.stdout)
        import anyio
        from mcp import ClientSession, StdioServerParameters
        from mcp.client.stdio import stdio_client
        async def call():
            async with stdio_client(StdioServerParameters(command=sys.executable,
                    args=["-B", str(ROOT / "tools/lsp/server.py")], env=dict(os.environ))) as streams:
                async with ClientSession(*streams) as session:
                    await session.initialize()
                    result = await session.call_tool("diagnostics", self.payload(session="native"))
                    self.assertFalse(result.isError, result)
                    self.assertIn("2322", str(result))
        anyio.run(call)
        self.assertEqual(self.endpoint()["pid"], endpoint["pid"])
        self.assertEqual(len(broker.request("status", {})["backends"]), 1)

    def test_disconnected_client_cannot_release_daemon_claim(self):
        endpoint = self.endpoint()
        event = {"workspace": str(self.root / "one"), "session_id": "disconnect", "tool_use_id": "change",
            "event": "PostToolUse", "_origin": "command"}
        journal = Journal(event)
        client = None
        try:
            journal.pre(event)
            (self.root / "one/source.ts").write_text("export const one: number = 'changed';\n")
            token = journal.claim(event, "command")
            event["_claims"] = {str(journal.directory): token}
            client = subprocess.Popen([sys.executable, "-B", str(ROOT / "tools/lsp/server.py"), "--once"],
                stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
            client.stdin.write(json.dumps(event))
            client.stdin.close()
            client.stdin = None
            deadline = time.monotonic() + 10
            while time.monotonic() < deadline:
                claim = journal.get("active_claim", {})
                if claim.get("pid") == endpoint["pid"] and claim.get("token") != token:
                    break
                time.sleep(0.01)
            self.assertEqual(claim.get("pid"), endpoint["pid"], claim)
            self.assertNotEqual(claim.get("token"), token)
            children = psutil.Process(client.pid).children(recursive=True)
            self.assertNotIn(endpoint["pid"], [item.pid for item in children])
            for child in children:
                child.kill()
            client.kill()
            client.communicate(timeout=5)
            journal.release_claim(token)
            remaining = journal.get("active_claim", {})
            self.assertTrue(not remaining or remaining.get("token") == claim["token"])
            deadline = time.monotonic() + 25
            while journal.get("active_claim", {}) and time.monotonic() < deadline:
                time.sleep(0.05)
            self.assertFalse(journal.get("active_claim", {}))
            self.assertTrue(journal.receipt(event))
            self.assertEqual(broker.request("hook", {**event, "_origin": "native", "_claims": {}}), {})
            self.assertEqual(self.endpoint()["pid"], endpoint["pid"])
        finally:
            if client is not None:
                if client.poll() is None:
                    for child in psutil.Process(client.pid).children(recursive=True):
                        child.kill()
                    client.kill()
                client.communicate(timeout=5)
            journal.close()

    def test_idle_exit_and_stale_source_rejected(self):
        with patch.dict(os.environ, {"HARNESS_LSP_BROKER_IDLE_SECONDS": "1"}):
            endpoint = self.endpoint()
        with patch("broker.source_identity", return_value="different"):
            with self.assertRaisesRegex(RuntimeError, "older source"):
                broker.ensure_endpoint(2)
        deadline = time.monotonic() + 10
        while self.alive(endpoint["pid"], endpoint["started"]) and time.monotonic() < deadline:
            time.sleep(0.1)
        self.assertFalse(self.alive(endpoint["pid"], endpoint["started"]))

    def test_authenticated_retire_accepts_old_source_and_closes_children(self):
        endpoint = self.endpoint()
        broker.request("diagnostics", self.payload())
        children = [(p.pid, p.create_time()) for p in psutil.Process(endpoint["pid"]).children(recursive=True)]
        self.assertTrue(children)
        with patch.dict(os.environ, {"HARNESS_LSP_MAX_BACKENDS": "2"}):
            with self.assertRaisesRegex(RuntimeError, "older source"):
                broker.ensure_endpoint(2)
            output = subprocess.run([sys.executable, "-B", str(ROOT / "tools/lsp/broker.py"),
                "--retire", "--timeout", "25"], capture_output=True, text=True, timeout=30, check=True)
            result = json.loads(output.stdout)
        self.assertEqual(result["status"], "retired", result)
        self.assertIsNone(broker.read_endpoint(broker.directory()))
        self.assertFalse([item for item in children if self.alive(*item)])
        self.assertEqual(broker.retire(1), {"status": "not-running"})

    def test_retirement_drains_admitted_operation_and_rejects_new(self):
        endpoint = self.endpoint()
        with ThreadPoolExecutor(max_workers=1) as executor:
            checking = executor.submit(broker.request, "diagnostics", self.payload())
            deadline = time.monotonic() + 10
            admitted = False
            while time.monotonic() < deadline:
                if any(row["users"] for row in broker.exchange(endpoint, "status", {}, 3)["backends"]):
                    admitted = True
                    break
                time.sleep(0.01)
            self.assertTrue(admitted, "The original diagnostics operation must be running before retirement")
            self.assertEqual(broker.exchange(endpoint, "retire", {}, 3)["status"], "draining")
            with self.assertRaisesRegex(RuntimeError, "retiring"):
                broker.exchange(endpoint, "diagnostics", self.payload(session="new"), 3)
            self.assertEqual(checking.result(timeout=29)["status"], "diagnostics")
        self.assertIn(broker.retire(25)["status"], {"retired", "not-running"})

    def test_backend_eviction_closes_entire_tree_before_replacement(self):
        with patch.dict(os.environ, {"HARNESS_LSP_MAX_BACKENDS": "1"}):
            self.endpoint()
            previous = []
            for project in ("one", "two", "one"):
                self.assertEqual(broker.request("diagnostics", self.payload(project))["status"], "diagnostics")
                self.assertFalse([row for row in previous if self.alive(*row)], "Evicted tree must exit before replacement is returned")
                state = broker.request("status", {})["backends"]
                self.assertEqual(len(state), 1)
                parent = psutil.Process(state[0]["pid"])
                previous = [(p.pid, p.create_time()) for p in [parent, *parent.children(recursive=True)]]
                self.assertGreater(len(previous), 1)


if __name__ == "__main__":
    unittest.main(argv=[arg for arg in sys.argv if arg != "--real"])
