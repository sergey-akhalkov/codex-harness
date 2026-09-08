"""Serena routing checks; --real opts into owned, temporary native MCP/LSPs.

Use the adopted Serena Python. Native checks require HARNESS_CODE_TOOLS_REGISTRY,
retain a failure report/logs, never change user configuration, and terminate only
the broker/proxy PIDs whose creation identities were captured by this fixture.
"""
from concurrent.futures import ThreadPoolExecutor
import hashlib
import json
import os
from pathlib import Path
import queue
import socket
import subprocess
import sys
import tempfile
import threading
import time
from types import SimpleNamespace
import unittest
import urllib.error
from unittest.mock import patch

import psutil

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools/code-tools"))
import serena_broker as broker

REAL = "--real" in sys.argv
INITIALIZE = {"protocolVersion": "2024-11-05", "capabilities": {},
              "clientInfo": {"name": "serena-owned-check", "version": "1"}}
ARGS = ["start-mcp-server", "--context", "codex", "--project-from-cwd", "--enable-web-dashboard", "false"]


class FakeWorker:
    serial = 0
    def __init__(self, route, initialize, timeout):
        type(self).serial += 1
        self.route = dict(route)
        self.closed = False
        self.used = time.monotonic()
        self.process = SimpleNamespace(pid=self.serial, poll=lambda: 0 if self.closed else None)
        self.initialized = {"jsonrpc": "2.0", "id": 1, "result": {"capabilities": {"tools": {}}}}
        self.calls = []
    def rpc(self, method, params, timeout):
        self.used = time.monotonic()
        self.calls.append((method, params))
        return {"jsonrpc": "2.0", "id": len(self.calls), "result": {"project": self.route["project"]}}
    def close(self):
        self.closed = True


class RoutingTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="serena-routing-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.a, self.b = self.root / "alpha", self.root / "beta"
        for project in (self.a, self.b):
            (project / ".serena").mkdir(parents=True)
            (project / ".serena/project.yml").write_text(f"project_name: {project.name}\nlanguage_servers: [typescript]\n")
        self.environment = patch.dict(os.environ, {"SERENA_HOME": str(self.root / "home"), "CODEX_HOME": str(self.root / "codex")})
        self.environment.start()
        self.addCleanup(self.environment.stop)
        self.pool = broker.ProjectPool(FakeWorker)
        self.addCleanup(self.pool.close)

    def dispatch(self, operation, payload):
        return self.pool.dispatch(operation, payload, time.time() + 3)

    def connect(self, client, project=None, arguments=None):
        return self.dispatch("connect", {"client": client * 32, "cwd": str(project or self.a),
                             "arguments": arguments or ARGS, "initialize": INITIALIZE})

    def rpc(self, client, name, arguments=None):
        return self.dispatch("rpc", {"client": client * 32, "method": "tools/call",
                             "params": {"name": name, "arguments": arguments or {}}})

    def test_same_root_reuse_client_local_activation_and_private_metadata(self):
        self.connect("a")
        self.connect("b")
        self.assertEqual(len(self.pool.workers), 1)
        original = next(iter(self.pool.workers.values()))
        self.rpc("a", "activate_project", {"project": str(self.b)})
        first, second = self.rpc("a", "get_current_config"), self.rpc("b", "get_current_config")
        self.assertEqual(first["message"]["result"]["project"], str(self.b))
        self.assertEqual(second["message"]["result"]["project"], str(self.a))
        self.assertIn(original, self.pool.workers.values())
        self.assertEqual(original.calls[-1][1]["_meta"]["harness_serena_client"], "b" * 32)
        self.assertEqual(len(self.pool.workers), 2)

    def test_configuration_change_retires_owned_worker_and_modes_are_distinct(self):
        self.connect("a")
        first = next(iter(self.pool.workers.values()))
        (self.a / ".serena/project.local.yml").write_text("read_only: true\n")
        self.rpc("a", "get_current_config")
        self.assertTrue(first.closed)
        self.connect("b", arguments=[*ARGS, "--mode", "interactive"])
        self.assertEqual(len(self.pool.workers), 2)
        (self.a / "tsconfig.json").write_text('{"compilerOptions":{"strict":true}}')
        second = next(iter(self.pool.workers.values()))
        self.rpc("a", "get_current_config")
        self.assertTrue(second.closed)

    def test_names_positional_paths_canonical_subfolders_and_client_info(self):
        home = broker.serena_home()
        home.mkdir()
        (home / "serena_config.yml").write_text("projects:\n  - " + json.dumps(str(self.a)) + "\n")
        self.assertEqual(broker.canonical_project("alpha", str(self.b)), str(self.a))
        positional = broker.parse_route(["start-mcp-server", str(self.a), "--context", "codex"], str(self.b))
        self.assertEqual(positional["project"], str(self.a))
        child = self.a / "nested"
        child.mkdir()
        root_route = broker.parse_route(ARGS, str(self.a))
        child_route = broker.parse_route(ARGS, str(child))
        self.assertEqual(broker.configuration_key(root_route, INITIALIZE),
                         broker.configuration_key(child_route, {**INITIALIZE, "clientInfo": {"name": "other"}}))
        with self.assertRaisesRegex(ValueError, "cannot be combined"):
            broker.parse_route([*ARGS, str(self.b)], str(self.a))

    def test_failed_activation_preserves_selection(self):
        self.connect("a")
        with self.assertRaisesRegex(ValueError, "No registered"):
            self.rpc("a", "activate_project", {"project": str(self.root / "missing")})
        self.assertEqual(self.pool.clients["a" * 32]["route"]["project"], str(self.a))
        with patch.object(FakeWorker, "rpc", return_value={"result": {"isError": True}}):
            result = self.rpc("a", "activate_project", {"project": str(self.b)})
        self.assertFalse(result["tools_changed"])
        self.assertEqual(result["route"]["project"], str(self.a))

    def test_optional_config_mutation_forks_client_and_retains_known_names(self):
        self.connect("a")
        self.connect("b")
        shared = next(iter(self.pool.workers.values()))
        removed = self.rpc("a", "remove_project", {"project_name": "alpha"})
        self.assertEqual(removed["route"]["removed_projects"], ["alpha"])
        self.assertNotIn("removed_projects", self.pool.clients["b" * 32]["route"])
        self.assertEqual(shared.calls, [])
        self.rpc("b", "activate_project", {"project": str(self.b)})
        restored = self.rpc("b", "activate_project", {"project": "alpha"})
        self.assertEqual(restored["route"]["project"], str(self.a))

    def test_activation_transaction_blocks_new_admission_but_allows_status(self):
        self.connect("a")
        journal = Path(os.environ["CODEX_HOME"]) / "harness/activation-pending.json"
        journal.parent.mkdir(parents=True)
        journal.write_text('{"phase":"updating"}')
        self.assertEqual(self.dispatch("status", {})["clients"], 1)
        with self.assertRaisesRegex(RuntimeError, "Recover"):
            self.rpc("a", "get_current_config")
        journal.write_text('{"phase":"committed"}')
        self.assertEqual(self.rpc("a", "get_current_config")["route"]["project"], str(self.a))

    def test_capacity_idle_and_late_proxy_reconnect(self):
        self.pool.capacity = 1
        connected = self.connect("a")
        first = next(iter(self.pool.workers.values()))
        self.connect("b", self.b)
        self.assertTrue(first.closed)
        second = next(iter(self.pool.workers.values()))
        second.used -= self.pool.idle + 1
        self.pool.clients["a" * 32]["used"] -= self.pool.idle + 1
        self.pool.reap()
        self.assertTrue(second.closed)
        self.assertEqual(self.pool.workers, {})
        result = self.dispatch("rpc", {"client": "a" * 32, "route": connected["route"],
                     "initialize": INITIALIZE, "method": "tools/list"})
        self.assertEqual(result["route"]["project"], str(self.a))

    def test_concurrent_calls_serialize_and_status_does_not_keep_alive(self):
        self.connect("a")
        self.connect("b")
        active, peak = 0, 0
        original = FakeWorker.rpc
        def slow(worker, *args):
            nonlocal active, peak
            active += 1
            peak = max(peak, active)
            try:
                time.sleep(0.02)
                return original(worker, *args)
            finally:
                active -= 1
        with patch.object(FakeWorker, "rpc", slow), ThreadPoolExecutor(max_workers=2) as threads:
            list(threads.map(lambda client: self.rpc(client, "get_current_config"), ["a", "b"]))
        self.assertEqual(peak, 1)
        used = self.pool.used
        self.dispatch("status", {})
        self.assertEqual(self.pool.used, used)
        self.assertEqual(len(self.pool.workers), 1)


class ProxyClient:
    def __init__(self, cwd):
        self.process = subprocess.Popen([sys.executable, "-B", str(ROOT / "tools/code-tools/serena_proxy.py"), *ARGS],
                  cwd=cwd, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                  creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
        self.serial = 0
        self.output = queue.Queue()
        self.errors = []
        def read():
            try:
                for raw in self.process.stdout:
                    self.output.put(json.loads(raw))
            except Exception as error:
                self.output.put(error)
        threading.Thread(target=read, daemon=True).start()
        threading.Thread(target=lambda: self.errors.append(self.process.stderr.read()), daemon=True).start()
    def rpc(self, method, params=None):
        self.serial += 1
        self.process.stdin.write(json.dumps({"jsonrpc": "2.0", "id": self.serial, "method": method,
                                           "params": params or {}}).encode() + b"\n")
        self.process.stdin.flush()
        deadline = time.monotonic() + 90
        while True:
            message = self.output.get(timeout=max(0.01, deadline - time.monotonic()))
            if isinstance(message, Exception):
                raise message
            if message.get("id") == self.serial:
                if "error" in message:
                    raise RuntimeError(message["error"])
                return message["result"]
    def close(self):
        self.process.stdin.close()
        try:
            self.process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            self.process.kill()
            self.process.wait(timeout=5)
        for stream in (self.process.stdout, self.process.stderr):
            stream.close()


class TransportTests(unittest.TestCase):
    def test_disconnected_admitted_request_drains_before_retirement(self):
        started, release, completed, closed = (threading.Event() for _ in range(4))
        errors = []
        class Pool:
            idle = 300
            used = time.monotonic()
            def dispatch(self, operation, payload, deadline):
                started.set()
                if not release.wait(3):
                    raise TimeoutError("Owned request release deadline")
                completed.set()
                return True
            def reap(self):
                pass
            def close(self):
                if not completed.is_set():
                    errors.append("Pool closed before admitted request completed")
                closed.set()
        with tempfile.TemporaryDirectory(prefix="serena-drain-") as scratch:
            runtime = Path(scratch)
            def serve():
                try:
                    broker.serve()
                except Exception as error:
                    errors.append(str(error))
            with patch.object(broker, "directory", return_value=runtime), \
                 patch.object(broker, "private_directory", lambda path: path.mkdir(parents=True, exist_ok=True)), \
                 patch.object(broker, "source_identity", return_value="owned-drain-fixture"), \
                 patch.object(broker, "ProjectPool", Pool), \
                 patch.object(broker, "JobGuard", return_value=SimpleNamespace(contain_current_process=lambda: None)):
                thread = threading.Thread(target=serve, daemon=True)
                thread.start()
                try:
                    deadline = time.monotonic() + 3
                    endpoint = None
                    while time.monotonic() < deadline and endpoint is None:
                        endpoint = broker.read_endpoint(runtime)
                        time.sleep(0.01)
                    self.assertIsNotNone(endpoint, errors)
                    body = json.dumps({"operation": "blocked", "payload": {}, "deadline": time.time() + 5}).encode()
                    headers = (f"POST /rpc HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {endpoint['token']}\r\n"
                               f"Content-Length: {len(body)}\r\nConnection: close\r\n\r\n").encode()
                    with socket.create_connection(("127.0.0.1", endpoint["port"]), timeout=3) as client:
                        client.sendall(headers + body)
                    self.assertTrue(started.wait(1))
                    self.assertEqual(broker.exchange(endpoint, "retire", {}, 2)["status"], "draining")
                    self.assertFalse(closed.wait(0.05))
                    release.set()
                    thread.join(3)
                    self.assertFalse(thread.is_alive())
                    self.assertTrue(completed.is_set())
                    self.assertTrue(closed.is_set())
                    self.assertEqual(errors, [])
                finally:
                    release.set()
                    thread.join(3)


@unittest.skipUnless(REAL, "--real is required for owned native worker checks")
class NativeTests(unittest.TestCase):
    def test_real_proxy_reuse_project_isolation_prompt_history_and_crash_cleanup(self):
        scratch = Path(tempfile.mkdtemp(prefix="serena-shared-native-"))
        report = {"root": str(scratch), "python": sys.executable, "version": sys.version,
                  "source": {}, "passed": False}
        for path in (ROOT / "tools/code-tools").glob("serena_*.py"):
            report["source"][str(path)] = hashlib.sha256(path.read_bytes()).hexdigest()
        clients = []
        owner = None
        with patch.dict(os.environ, {"CODEX_HOME": str(scratch / "codex"), "SERENA_HOME": str(scratch / "home"),
                                     "HARNESS_SERENA_BROKER_DIR": str(scratch / "broker")}):
            try:
                projects = [scratch / "alpha", scratch / "beta"]
                for project, symbol in zip(projects, ("AlphaOwned", "BetaOwned")):
                    (project / ".serena").mkdir(parents=True)
                    (project / ".serena/project.yml").write_text(f"project_name: {project.name}\nlanguage_servers: [typescript]\nencoding: utf-8\n")
                    (project / "sample.ts").write_text(f"export function {symbol}(): number {{ return 1; }}\n")
                endpoint = broker.ensure_endpoint(broker.source_identity())
                owner = psutil.Process(endpoint["pid"])
                with self.assertRaises(urllib.error.HTTPError) as rejected:
                    broker.exchange({**endpoint, "token": "0" * 64}, "status", {}, 5)
                self.assertEqual(rejected.exception.code, 403)
                for _ in range(2):
                    client = ProxyClient(projects[0])
                    clients.append(client)
                    client.rpc("initialize", INITIALIZE)
                status = broker.exchange(endpoint, "status", {}, 5)
                report["initial"] = status
                self.assertEqual(len(status["workers"]), 1)
                first_pid = status["workers"][0]["pid"]
                for client in clients:
                    instructions = client.rpc("tools/call", {"name": "initial_instructions", "arguments": {}})
                    self.assertIn("alpha", json.dumps(instructions).lower())
                clients[0].rpc("tools/call", {"name": "activate_project", "arguments": {"project": str(projects[1])}})
                for client, symbol, absent in zip(clients, ("BetaOwned", "AlphaOwned"), ("AlphaOwned", "BetaOwned")):
                    result = client.rpc("tools/call", {"name": "get_symbols_overview", "arguments": {"relative_path": "sample.ts"}})
                    self.assertFalse(result.get("isError"), result)
                    self.assertIn(symbol, json.dumps(result))
                    self.assertNotIn(absent, json.dumps(result))
                status = broker.exchange(endpoint, "status", {}, 5)
                report["activated"] = status
                self.assertEqual(len(status["workers"]), 2)
                self.assertIn(first_pid, [worker["pid"] for worker in status["workers"]])
                descendants = owner.children(recursive=True)
                report["owned_descendants"] = [{"pid": item.pid, "started": item.create_time()} for item in descendants]
                self.assertGreaterEqual(len(descendants), 4)
                owner.kill()
                owner.wait(timeout=10)
                _, alive = psutil.wait_procs(descendants, timeout=10)
                self.assertEqual(alive, [], [item.pid for item in alive])
                owner = None
                endpoint = broker.ensure_endpoint(broker.source_identity())
                owner = psutil.Process(endpoint["pid"])
                self.assertEqual(broker.retire()["status"], "retired")
                self.assertFalse(owner.is_running())
                owner = None
                # Native eviction and idle retirement must await tree exit,
                # including the servers beneath the venv interpreter shim.
                pool = broker.ProjectPool()
                pool.capacity = 1
                try:
                    captured = []
                    for project in projects:
                        route = broker.parse_route(ARGS, str(project))
                        native = pool.worker(route, INITIALIZE, 60)
                        for previous in captured:
                            self.assertFalse(previous.is_running())
                        process = psutil.Process(native.process.pid)
                        captured = [process, *process.children(recursive=True)]
                    native.used -= pool.idle + 1
                    pool.reap()
                    self.assertEqual(pool.workers, {})
                    self.assertFalse(any(process.is_running() for process in captured))
                finally:
                    pool.close()
                report["passed"] = True
            finally:
                for client in clients:
                    client.close()
                if owner is not None and owner.is_running():
                    broker.exchange(endpoint, "retire", {}, 5)
                    owner.wait(timeout=15)
                report["proxy_stderr"] = [b"".join(client.errors).decode("utf-8", errors="replace")[-12000:] for client in clients]
                (scratch / "report.json").write_text(json.dumps(report, indent=2), encoding="utf-8")
                print("Serena native evidence: " + str(scratch / "report.json"), file=sys.stderr)


if __name__ == "__main__":
    unittest.main(argv=[item for item in sys.argv if item != "--real"], verbosity=2)
