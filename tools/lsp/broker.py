"""Authenticated local diagnostics broker and bounded, leased backend pool.

The broker owns language processes; clients own only requests. Session journals
remain with DiagnosticsService and never form part of the shared backend cache.
"""
from __future__ import annotations

from contextlib import contextmanager
import hashlib
import hmac
import importlib.util
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import secrets
import subprocess
import sys
import threading
import time
import urllib.error
import urllib.request

import psutil

PROTOCOL = 1
MAX_MESSAGE = 16 * 1024 * 1024


def setting(name: str, default: float, minimum: float = 1, maximum: float = 3600):
    try:
        return min(maximum, max(minimum, float(os.environ.get(name, str(default)))))
    except (ValueError, TypeError):
        return default


def directory():
    home = Path(os.environ.get("CODEX_HOME", Path.home() / ".codex")).resolve()
    return home / "harness/runtime/lsp-broker"


def assert_activation_ready():
    """Retired services must not restart inside an unfinished kit transaction."""
    home = Path(os.environ.get("CODEX_HOME", Path.home() / ".codex")).resolve()
    pending = home / "harness/activation-pending.json"
    try:
        record = json.loads(pending.read_text(encoding="utf-8-sig"))
    except FileNotFoundError:
        return
    except (OSError, ValueError) as error:
        raise RuntimeError("Kit activation state is unreadable; run install.ps1 -Mode Recover before starting shared services") from error
    if not isinstance(record, dict) or record.get("phase") != "committed":
        raise RuntimeError("Kit activation is incomplete; run install.ps1 -Mode Recover before starting shared services")


def source_identity():
    here = Path(__file__).resolve().parent
    paths = sorted(here.glob("*.py")) + sorted(here.glob("*.ps1"))
    paths += [here.parent / "process_ownership.py"]
    transport = importlib.util.find_spec("solidlsp")
    if transport and transport.origin:
        installed = Path(transport.origin).parent
        paths += [installed / "ls_process.py", installed / "util/subprocess_util.py"]
    digest = hashlib.sha256()
    digest.update(json.dumps([PROTOCOL, str(here), sys.executable, sys.version,
        os.environ.get("HARNESS_CODE_TOOLS_REGISTRY", "")]).encode())
    digest.update(os.environ.get("HARNESS_LSP_REGISTRY", "").encode())
    digest.update(json.dumps([setting("HARNESS_LSP_MAX_BACKENDS", 4, maximum=32),
        setting("HARNESS_LSP_BACKEND_IDLE_SECONDS", 300), setting("HARNESS_LSP_BROKER_IDLE_SECONDS", 300)]).encode())
    for path in paths:
        digest.update(str(path).encode())
        digest.update(path.read_bytes() if path.is_file() else b"missing")
    return digest.hexdigest()


def private_directory(path):
    path.mkdir(parents=True, exist_ok=True, mode=0o700)
    if os.name != "nt":
        path.chmod(0o700)
        return
    # Token material must not inherit an unusually permissive CODEX_HOME ACL.
    import csv
    result = subprocess.run(["whoami", "/user", "/fo", "csv", "/nh"], capture_output=True,
        text=True, timeout=5, creationflags=subprocess.CREATE_NO_WINDOW, check=True)
    sid = next(csv.reader(result.stdout.splitlines()))[1]
    if not sid.startswith("S-1-"):
        raise RuntimeError("Cannot establish diagnostics broker user identity")
    subprocess.run(["icacls", str(path), "/inheritance:r", "/grant:r", f"*{sid}:(OI)(CI)F"],
        capture_output=True, timeout=5, creationflags=subprocess.CREATE_NO_WINDOW, check=True)


class FileLock:
    """Kernel lock; the lock file is never unlinked/replaced beneath waiters."""
    def __init__(self, path):
        self.file = open(path, "a+b")
        self.held = False

    def acquire(self, timeout):
        deadline = time.monotonic() + timeout
        while True:
            try:
                if os.name == "nt":
                    import msvcrt
                    self.file.seek(0)
                    msvcrt.locking(self.file.fileno(), msvcrt.LK_NBLCK, 1)
                else:
                    import fcntl
                    fcntl.flock(self.file.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
                self.held = True
                return self
            except (OSError, BlockingIOError):
                if time.monotonic() >= deadline:
                    raise TimeoutError("Diagnostics broker startup/instance lock is busy")
                time.sleep(min(0.05, max(0, deadline - time.monotonic())))

    def close(self):
        if self.held:
            if os.name == "nt":
                import msvcrt
                self.file.seek(0)
                msvcrt.locking(self.file.fileno(), msvcrt.LK_UNLCK, 1)
            else:
                import fcntl
                fcntl.flock(self.file.fileno(), fcntl.LOCK_UN)
        self.file.close()
        self.held = False

    def __enter__(self):
        return self

    def __exit__(self, *_):
        self.close()


def read_endpoint(root):
    try:
        value = json.loads((root / "endpoint.json").read_text(encoding="utf-8"))
        if (value.get("protocol") != PROTOCOL or not isinstance(value.get("token"), str)
                or len(value["token"]) != 64 or not 0 < int(value["port"]) < 65536):
            return None
        process = psutil.Process(value["pid"])
        if abs(process.create_time() - value["started"]) > 0.001:
            return None
        return value
    except (OSError, ValueError, TypeError, KeyError, psutil.Error):
        return None


def exchange(endpoint, operation, payload, timeout):
    body = json.dumps({"operation": operation, "payload": payload,
        "deadline": time.time() + timeout}, ensure_ascii=False).encode("utf-8")
    if len(body) > MAX_MESSAGE:
        raise ValueError("Diagnostics broker request exceeds its 16 MiB transport bound")
    message = urllib.request.Request(f"http://127.0.0.1:{endpoint['port']}/rpc", data=body,
        headers={"Authorization": "Bearer " + endpoint["token"], "Content-Type": "application/json"})
    # Explicitly bypass proxy environment variables for the local authority.
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    with opener.open(message, timeout=max(0.05, timeout)) as response:
        raw = response.read(MAX_MESSAGE + 1)
    if len(raw) > MAX_MESSAGE:
        raise RuntimeError("Diagnostics broker response exceeds its transport bound")
    value = json.loads(raw)
    if value.get("error"):
        raise RuntimeError(value["error"])
    return value["result"]


def ensure_endpoint(timeout: float = 10):
    assert_activation_ready()
    began = time.monotonic()
    root = directory()
    expected = source_identity()
    endpoint = read_endpoint(root)
    if endpoint:
        if endpoint.get("source") != expected:
            raise RuntimeError("Diagnostics broker has older source/runtime; wait for its idle shutdown before reconnecting")
        return endpoint
    private_directory(root)
    with FileLock(root / "startup.lock") as lock:
        lock.acquire(max(0, timeout - (time.monotonic() - began)))
        assert_activation_ready()
        endpoint = read_endpoint(root)
        if endpoint:
            if endpoint.get("source") != expected:
                raise RuntimeError("Diagnostics broker source/runtime identity differs")
            return endpoint
        # A missing receipt must never allow a second daemon while a live owner
        # holds its lifetime lock (for example during an interrupted startup).
        with FileLock(root / "instance.lock") as lifetime:
            lifetime.acquire(max(0, timeout - (time.monotonic() - began)))
        environment = {**os.environ, "PYTHONDONTWRITEBYTECODE": "1", "PYTHONUTF8": "1",
            "HARNESS_LSP_WORKSPACE_ROOTS": "[]",
            "HARNESS_LSP_BROKER_START_DEADLINE": str(time.time() + max(0, timeout - (time.monotonic() - began)))}
        sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
        from process_ownership import spawn_service
        process = spawn_service(Path(__file__), arguments=["--serve"], env=environment,
            cwd=root, log_path=root / "broker.log", startup_timeout=max(0.01, timeout - (time.monotonic() - began)))
        while time.monotonic() - began < timeout:
            endpoint = read_endpoint(root)
            if endpoint and endpoint.get("source") == expected:
                return endpoint
            if process.poll() is not None:
                raise RuntimeError(f"Diagnostics broker exited during startup ({process.returncode}); see {root / 'broker.log'}")
            time.sleep(0.05)
    raise TimeoutError("Diagnostics broker startup deadline elapsed")


def request(operation, payload, timeout=29):
    # No automatic capability is selected. Cached clients must remain silent
    # even without a host marker, a registry, or a running broker.
    if operation == "hook":
        return {}
    began = time.monotonic()
    forwarded = dict(payload)
    forwarded.setdefault("_workspace_roots", json.loads(os.environ.get("HARNESS_LSP_WORKSPACE_ROOTS", "[]")))
    endpoint = ensure_endpoint(min(timeout, setting("HARNESS_LSP_STARTUP_TIMEOUT", 10)))
    remaining = timeout - (time.monotonic() - began)
    if remaining <= 0:
        raise TimeoutError("Diagnostics broker request deadline elapsed during startup")
    # Never retry a submitted mutation of journal/delivery state: a lost reply
    # is ambiguous, and the existing claim/receipt must own recovery.
    return exchange(endpoint, operation, forwarded, remaining)


def retire(timeout=30):
    """Drain the verified existing owner, including an outdated source revision.

    Retirement never launches a service or force-kills a process. Its authority
    is the private receipt's PID/start time and bearer token, not current source.
    """
    deadline = time.monotonic() + max(0.1, timeout)
    root = directory()
    while True:
        endpoint = read_endpoint(root)
        if endpoint is not None:
            break
        if not root.exists():
            return {"status": "not-running"}
        try:
            with FileLock(root / "startup.lock") as starting, FileLock(root / "instance.lock") as owner:
                starting.acquire(0)
                owner.acquire(0)
                return {"status": "not-running"}
        except TimeoutError:
            if time.monotonic() >= deadline:
                return {"status": "pending", "reason": "Owner is starting or closing without an authenticated ready endpoint"}
            time.sleep(0.05)
    try:
        exchange(endpoint, "retire", {}, min(5, max(0.1, deadline - time.monotonic())))
    except (OSError, RuntimeError, urllib.error.URLError) as error:
        if read_endpoint(directory()) == endpoint:
            return {"status": "pending", "pid": endpoint["pid"], "reason": str(error)}
    while True:
        try:
            process = psutil.Process(endpoint["pid"])
            alive = abs(process.create_time() - endpoint["started"]) < 0.001
        except psutil.Error:
            alive = False
        if not alive:
            return {"status": "retired", "pid": endpoint["pid"]}
        if time.monotonic() >= deadline:
            return {"status": "pending", "pid": endpoint["pid"], "reason": "Admitted operations are draining"}
        time.sleep(min(0.05, max(0, deadline - time.monotonic())))


class BackendPool:
    """Bound live backends, including startups, using explicit operation leases."""
    def __init__(self, factory, maximum=4, idle_seconds=300):
        self.factory = factory
        self.maximum = int(maximum)
        self.idle_seconds = idle_seconds
        self.condition = threading.Condition(threading.RLock())
        self.entries = {}
        self.closing = 0
        self.closed = False

    @staticmethod
    def _close(entry):
        if entry.get("backend"):
            entry["backend"].close()

    def _dispose(self, entries):
        failure = None
        for entry in entries:
            try:
                self._close(entry)
            except Exception as error:
                # Unknown physical shutdown must never free admission capacity.
                # The owner will close its Job; this pool cannot safely restart.
                failure = failure or error
                with self.condition:
                    self.closed = True
            finally:
                with self.condition:
                    self.closing -= 1
                    self.condition.notify_all()
        if failure is not None:
            raise failure

    @staticmethod
    def _healthy(backend):
        if getattr(backend, "sync_failed", False):
            return False
        process = getattr(getattr(getattr(backend, "connection", None), "_process", None), "_popen", None)
        return process is None or process.poll() is None

    @contextmanager
    def lease(self, key, create, timeout=20):
        deadline = time.monotonic() + timeout
        entry = None
        while entry is None:
            retired = []
            with self.condition:
                if self.closed:
                    raise RuntimeError("Shared language pool is shutting down")
                for old, candidate in list(self.entries.items()):
                    if old[:-2] == key[:-2] and old != key:
                        candidate["retired"] = True
                    if candidate.get("backend") and not self._healthy(candidate["backend"]):
                        candidate["retired"] = True
                    if candidate["users"] == 0 and candidate.get("retired"):
                        retired.append(self.entries.pop(old))
                candidate = self.entries.get(key)
                if candidate and not candidate.get("retired") and candidate.get("backend"):
                    entry = candidate
                    entry["users"] += 1
                elif candidate is None:
                    if len(self.entries) + self.closing + len(retired) >= self.maximum:
                        idle = [(old, item) for old, item in self.entries.items() if item["users"] == 0]
                        if idle:
                            oldest, _ = min(idle, key=lambda pair: pair[1]["used"])
                            retired.append(self.entries.pop(oldest))
                    if len(self.entries) + self.closing + len(retired) < self.maximum:
                        entry = {"backend": None, "users": 1, "used": time.monotonic(), "retired": False}
                        self.entries[key] = entry
                self.closing += len(retired)
                if entry is None and not retired:
                    remaining = deadline - time.monotonic()
                    if remaining <= 0:
                        raise TimeoutError("Shared language backend capacity is busy")
                    self.condition.wait(min(0.1, remaining))
            self._dispose(retired)
        try:
            if entry["backend"] is None:
                entry["backend"] = self.factory(*create)
                with self.condition:
                    self.condition.notify_all()
            client = entry["backend"]
            if not client.lock.acquire(timeout=max(0, deadline - time.monotonic())):
                raise TimeoutError("Shared language backend operation is busy")
            try:
                yield entry["backend"]
            finally:
                client.lock.release()
        except BaseException:
            if entry["backend"] is None:
                entry["retired"] = True
            raise
        finally:
            dispose = False
            with self.condition:
                entry["users"] -= 1
                entry["used"] = time.monotonic()
                if entry["backend"] and not self._healthy(entry["backend"]):
                    entry["retired"] = True
                if entry["users"] == 0 and (entry.get("retired") or self.closed):
                    if self.entries.get(key) is entry:
                        self.entries.pop(key)
                    dispose = True
                    self.closing += 1
                self.condition.notify_all()
            if dispose:
                self._dispose([entry])

    def invalidate(self, root):
        with self.condition:
            for key, entry in self.entries.items():
                if key[0] == root:
                    entry["retired"] = True
        self.reap()

    def forget(self, root, relative):
        # Take leases before releasing the pool lock; eviction cannot close a
        # connection between selection and didClose/didChangeWatchedFiles.
        with self.condition:
            selected = [entry for key, entry in self.entries.items() if key[0] == root and entry["backend"]]
            for entry in selected:
                entry["users"] += 1
        try:
            for entry in selected:
                with entry["backend"].lock:
                    entry["backend"].forget(relative)
        finally:
            with self.condition:
                for entry in selected:
                    entry["users"] -= 1
                    entry["used"] = time.monotonic()
                self.condition.notify_all()

    def reap(self):
        with self.condition:
            expired = [key for key, value in self.entries.items() if value["users"] == 0
                and (value.get("retired") or time.monotonic() - value["used"] >= self.idle_seconds)]
            retired = [self.entries.pop(key) for key in expired]
            self.closing += len(retired)
        self._dispose(retired)

    def status(self):
        with self.condition:
            return [{"key": list(key), "users": value["users"], "retired": value.get("retired", False),
                "pid": getattr(getattr(getattr(getattr(value["backend"], "connection", None), "_process", None), "_popen", None), "pid", None)}
                for key, value in self.entries.items()]

    def close(self):
        with self.condition:
            self.closed = True
            for entry in self.entries.values():
                entry["retired"] = True
            self.condition.notify_all()
        self.reap()


def serve():
    root = directory()
    # This process is intentionally outside the lifetime of thin MCP clients.
    sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
    from process_ownership import JobGuard, mark_service_ready
    guard = JobGuard()
    guard.contain_current_process()
    from server import DiagnosticsService
    with FileLock(root / "instance.lock") as lifetime:
        lifetime.acquire(0)
        service = DiagnosticsService(shared=True)
        if time.time() >= float(os.environ.get("HARNESS_LSP_BROKER_START_DEADLINE", "inf")):
            service.close()
            raise TimeoutError("Diagnostics broker startup expired before readiness")
        token = secrets.token_hex(32)
        source = source_identity()
        state_lock = threading.Lock()
        activity = {"active": 0, "used": time.monotonic(), "retiring": False}

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *_):
                pass

            def do_POST(self):
                if self.path != "/rpc" or not hmac.compare_digest(self.headers.get("Authorization", ""), "Bearer " + token):
                    self.send_error(403)
                    return
                try:
                    length = int(self.headers.get("Content-Length", "0"))
                    if not 0 < length <= MAX_MESSAGE:
                        raise ValueError("Invalid diagnostics request length")
                    self.connection.settimeout(5)
                    message = json.loads(self.rfile.read(length))
                    remaining = min(29, float(message["deadline"]) - time.time())
                    if remaining <= 0:
                        raise TimeoutError("Diagnostics request expired before admission")
                    operation = message["operation"]
                    if operation not in {"status", "retire"}:
                        assert_activation_ready()
                    payload = message.get("payload", {})
                    with state_lock:
                        if activity["retiring"] and operation not in {"status", "retire"}:
                            raise RuntimeError("Diagnostics broker is retiring; admitted operations are draining")
                        if operation == "retire":
                            activity["retiring"] = True
                        activity["active"] += 1
                    try:
                        if operation == "status":
                            result = {"pid": os.getpid(), "source": source, "retiring": activity["retiring"],
                                "backends": service.shared_pool.status()}
                        elif operation == "retire":
                            result = {"status": "draining", "pid": os.getpid()}
                        elif operation == "hook":
                            # Compatibility for already connected native clients.
                            result = {}
                        elif operation == "diagnostics":
                            result = service.explicit_diagnostics(**payload)
                        elif operation == "navigate":
                            result = service.navigate(**payload)
                        else:
                            raise ValueError("Unknown diagnostics broker operation")
                        response = {"result": result}
                    finally:
                        with state_lock:
                            activity["active"] -= 1
                            if operation != "status":
                                activity["used"] = time.monotonic()
                except Exception as error:
                    response = {"error": f"{type(error).__name__}: {error}"}
                body = json.dumps(response, ensure_ascii=False).encode("utf-8")
                if len(body) > MAX_MESSAGE:
                    body = b'{"error":"Diagnostics response exceeds transport bound"}'
                try:
                    self.send_response(200)
                    self.send_header("Content-Type", "application/json")
                    self.send_header("Content-Length", str(len(body)))
                    self.end_headers()
                    self.wfile.write(body)
                except (OSError, ConnectionError):
                    pass  # The operation/receipt owns recovery after client exit.

        class Server(ThreadingHTTPServer):
            daemon_threads = True
            request_queue_size = 8

            def __init__(self, *args):
                self.slots = threading.BoundedSemaphore(8)
                super().__init__(*args)

            def process_request(self, request_socket, address):
                if not self.slots.acquire(blocking=False):
                    self.shutdown_request(request_socket)
                    return
                with state_lock:
                    activity["active"] += 1
                try:
                    super().process_request(request_socket, address)
                except BaseException:
                    with state_lock:
                        activity["active"] -= 1
                    self.slots.release()
                    raise

            def process_request_thread(self, *args):
                try:
                    super().process_request_thread(*args)
                finally:
                    with state_lock:
                        activity["active"] -= 1
                    self.slots.release()

        server = Server(("127.0.0.1", 0), Handler)
        server.timeout = 0.5
        endpoint = {"protocol": PROTOCOL, "pid": os.getpid(), "started": psutil.Process().create_time(),
            "port": server.server_address[1], "token": token, "source": source}
        receipt = root / "endpoint.json"
        temporary = root / f"endpoint-{os.getpid()}.tmp"
        assert_activation_ready()
        temporary.write_text(json.dumps(endpoint), encoding="utf-8")
        temporary.replace(receipt)
        mark_service_ready()
        try:
            while True:
                server.handle_request()
                service.shared_pool.reap()
                with state_lock:
                    idle = activity["active"] == 0 and (activity["retiring"] or
                        time.monotonic() - activity["used"] >= setting("HARNESS_LSP_BROKER_IDLE_SECONDS", 300))
                with service.guard:
                    pending = any(not job.done() for job in service.jobs.values())
                if idle and not pending and not any(item["users"] for item in service.shared_pool.status()):
                    break
        finally:
            server.server_close()
            service.close()
            if read_endpoint(root) == endpoint:
                receipt.unlink(missing_ok=True)
        # guard intentionally remains live until interpreter exit.


if __name__ == "__main__":
    import argparse
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--serve", action="store_true")
    mode.add_argument("--retire", action="store_true")
    parser.add_argument("--timeout", type=float, default=30)
    arguments = parser.parse_args()
    if arguments.serve:
        serve()
    else:
        result = retire(max(0.1, min(300, arguments.timeout)))
        print(json.dumps(result))
        raise SystemExit(2 if result["status"] == "pending" else 0)
