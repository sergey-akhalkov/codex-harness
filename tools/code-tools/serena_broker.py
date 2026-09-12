"""Bounded project-specific Serena MCP workers behind authenticated local RPC.

The native worker's project never changes. Each client owns its project/mode
selection; matching selections and configuration bytes share a serialized
worker. Native tool schemas/results remain authoritative. The adopted guarded
entry point owns runtime provisioning policy; this broker owns process lifetime.
"""
from __future__ import annotations

from collections import deque
import hashlib
import hmac
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import importlib.metadata
import json
import os
from pathlib import Path
import queue
import secrets
from string import Template
import subprocess
import sys
import threading
import time
from typing import Any

import psutil
from ruamel.yaml import YAML

SOURCE = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(SOURCE))
from tools.process_ownership import JobGuard, mark_service_ready, spawn_service
from tools.lsp.broker import FileLock, MAX_MESSAGE, assert_activation_ready, exchange, private_directory, read_endpoint


def directory() -> Path:
    return Path(os.environ.get("HARNESS_SERENA_BROKER_DIR", str(Path(os.environ.get("CODEX_HOME", str(Path.home() / ".codex"))) / "harness/runtime/serena-broker"))).resolve()


def registry_path() -> Path:
    return Path(os.environ["HARNESS_CODE_TOOLS_REGISTRY"]).resolve()


def serena_home() -> Path:
    return Path(os.environ.get("SERENA_HOME", str(Path.home() / ".serena"))).resolve()


def source_identity() -> str:
    distribution = importlib.metadata.distribution("serena-agent")
    if distribution.version != "1.7.0":
        raise RuntimeError("Shared Serena compatibility requires adopted Serena 1.7.0")
    paths = [Path(__file__).resolve(), Path(__file__).with_name("serena_proxy.py"),
             Path(__file__).with_name("serena_entry.py"), SOURCE / "tools/process_ownership.py",
             SOURCE / "tools/lsp/broker.py", SOURCE / "global/tool-resources.json", registry_path()]
    paths += [Path(distribution.locate_file("serena/" + name)) for name in
              ("mcp.py", "agent.py", "cli.py", "tools/tools_base.py", "config/serena_config.py")]
    digest = hashlib.sha256()
    digest.update(json.dumps([sys.executable, sys.version, str(serena_home()),
                              distribution.version]).encode())
    for path in paths:
        digest.update(str(path).encode())
        digest.update(path.read_bytes())
    return digest.hexdigest()


def policy() -> tuple[int, float]:
    config = json.loads((SOURCE / "global/tool-resources.json").read_text(encoding="utf-8"))["serena"]
    capacity, idle = int(config["max_projects"]), float(config["idle_seconds"])
    if not 1 <= capacity <= 16 or not 1 <= idle <= 3600:
        raise ValueError("Invalid Serena resource policy")
    return capacity, idle


def yaml_file(path: Path) -> dict[str, Any]:
    if not path.exists():
        return {}
    value = YAML(typ="safe").load(path.read_text(encoding="utf-8-sig"))
    if value is None:
        return {}
    if not isinstance(value, dict):
        raise ValueError(f"Serena configuration must be a mapping: {path}")
    return value


def project_folder(root: Path, config: dict[str, Any]) -> Path:
    template = str(config.get("project_serena_folder_location", "$projectDir/.serena"))
    configured = Path(Template(template).substitute(projectDir=str(root), projectFolderName=root.name)).resolve()
    default = root / ".serena"
    return configured if configured.is_dir() or not default.is_dir() else default


def canonical_project(value: str, cwd: str, known: list[str] | None = None, removed: list[str] | None = None) -> str:
    """Match registered names first, then paths, as Serena 1.7 does."""
    config = yaml_file(serena_home() / "serena_config.yml")
    names = []
    for registered in dict.fromkeys([*(config.get("projects", []) or []), *(known or [])]):
        root = Path(registered).resolve()
        if value not in (removed or []) and root.is_dir() and yaml_file(project_folder(root, config) / "project.yml").get("project_name", root.name) == value:
            names.append(root)
    if len(names) > 1:
        raise ValueError(f"Multiple Serena projects named {value!r}; use an absolute project path")
    root = names[0] if names else Path(value).expanduser()
    if not root.is_absolute():
        root = Path(cwd) / root
    root = root.resolve()
    if not root.is_dir():
        raise ValueError(f"No registered Serena project or directory: {value}")
    return str(root)


def parse_route(arguments: list[str], cwd: str) -> dict[str, Any]:
    """Remove project selection; all other native CLI options retain order."""
    if not arguments or arguments[0] != "start-mcp-server":
        raise ValueError("Shared Serena supports the start-mcp-server entry point")
    forwarded, explicit, positional, from_cwd = [arguments[0]], None, None, False
    value_options = {"--context", "--mode", "--add-mode", "--language-backend", "--transport",
                     "--host", "--port", "--enable-web-dashboard", "--enable-gui-log-window",
                     "--open-web-dashboard", "--log-level", "--trace-lsp-communication", "--tool-timeout"}
    index = 1
    while index < len(arguments):
        item = arguments[index]
        if item == "--project-from-cwd":
            from_cwd = True
        elif item in ("--project", "--project-file"):
            index += 1
            if index >= len(arguments):
                raise ValueError(f"Missing {item} value")
            explicit = arguments[index]
        elif item.startswith(("--project=", "--project-file=")):
            explicit = item.split("=", 1)[1]
        elif item in value_options:
            index += 1
            if index >= len(arguments):
                raise ValueError(f"Missing {item} value")
            value = arguments[index]
            candidate = Path(cwd) / value
            if item in ("--context", "--mode", "--add-mode") and candidate.is_file():
                value = str(candidate.resolve())
            forwarded.extend((item, value))
        elif not item.startswith("-"):
            if positional is not None:
                raise ValueError("Serena accepts one positional project")
            positional = item
        else:
            if item.startswith(("--context=", "--mode=", "--add-mode=")):
                option, value = item.split("=", 1)
                candidate = Path(cwd) / value
                if candidate.is_file():
                    item = option + "=" + str(candidate.resolve())
            forwarded.append(item)
        index += 1
    explicit = positional or explicit
    if explicit is not None and from_cwd:
        raise ValueError("--project-from-cwd cannot be combined with --project")
    project = canonical_project(explicit, cwd) if explicit is not None else None
    if from_cwd:
        start = Path(cwd).resolve()
        project = next((str(path) for path in (start, *start.parents)
                        if (path / ".serena/project.yml").is_file() or (path / ".git").exists()), None)
    # A stdio proxy cannot silently switch to a network/native UI transport.
    for index, item in enumerate(forwarded):
        if item == "--transport" and (index + 1 == len(forwarded) or forwarded[index + 1] != "stdio"):
            raise ValueError("Shared Serena requires stdio transport")
        if item.startswith("--transport=") and item != "--transport=stdio":
            raise ValueError("Shared Serena requires stdio transport")
    return {"project": project, "cwd": str(Path(cwd).resolve()), "arguments": forwarded}


def configuration_key(route: dict[str, Any], initialize: dict[str, Any]) -> str:
    home = serena_home()
    global_config = home / "serena_config.yml"
    config = yaml_file(global_config)
    files = [global_config]
    root = route["project"]
    if root:
        folder = project_folder(Path(root), config)
        files += [folder / "project.yml", folder / "project.local.yml"]
        files += [Path(root) / name for name in ("tsconfig.json", "jsconfig.json", "pyrightconfig.json",
                  "pyproject.toml", "package.json", "Cargo.toml", "go.mod", "global.json")]
    # User context/mode/prompt overrides can change tool exposure or behavior.
    for name in ("contexts", "modes", "prompt_templates"):
        files += sorted((home / name).glob("*.yml"))
        files += sorted((home / name).glob("*.yaml"))
    files += [Path(value).resolve() for value in route["arguments"] if Path(value).is_file()]
    digest = hashlib.sha256()
    identity = {**{key: value for key, value in route.items() if key != "known_projects"},
                "project": os.path.normcase(root) if root else None,
                "cwd": None if root else os.path.normcase(route["cwd"]),
                "protocol": initialize.get("protocolVersion")}
    digest.update(json.dumps(identity, sort_keys=True).encode())
    for path in files:
        digest.update(str(path).encode())
        digest.update(path.read_bytes() if path.is_file() else b"<absent>")
    return digest.hexdigest()


class NativeWorker:
    """Bounded JSON-line MCP transport; every call is serialized by the pool."""

    def __init__(self, route: dict[str, Any], initialize: dict[str, Any], timeout: float):
        self.guard = JobGuard()
        self.pending: dict[int, queue.Queue[Any]] = {}
        self.pending_lock = threading.Lock()
        self.write_lock = threading.Lock()
        self.serial = 0
        self.errors: deque[bytes] = deque(maxlen=32)
        self.process: Any = None
        self.used = time.monotonic()
        self.route = dict(route)
        inventory = json.loads(registry_path().read_text(encoding="utf-8-sig"))
        python = next(item for item in inventory["mcp"] if item["id"] == "serena")["paths"]["python"]
        arguments = list(route["arguments"])
        if route["project"]:
            arguments += ["--project", route["project"]]
        environment = {**os.environ, "PYTHONUTF8": "1", "PYTHONIOENCODING": "utf-8",
                       "HARNESS_SERENA_SHARED_WORKER": "1",
                       "HARNESS_SERENA_REMOVED_PROJECTS": json.dumps(route.get("removed_projects", []))}
        try:
            self.process = self.guard.popen([python, "-u", str(Path(__file__).with_name("serena_entry.py")), *arguments],
                                           cwd=route["project"] or route["cwd"], env=environment,
                                           stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                           creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
            threading.Thread(target=self._read, daemon=True).start()
            threading.Thread(target=self._stderr, daemon=True).start()
            # Native Serena1.7 does not request sampling/roots/elicitation. The
            # shared transport exposes no unsupported callbacks to the worker.
            self.initialized = self.rpc("initialize", {**initialize, "capabilities": {}}, timeout)
            if "error" in self.initialized:
                raise RuntimeError(f"Serena initialization failed: {self.initialized['error']}")
            self.send({"jsonrpc": "2.0", "method": "notifications/initialized"})
        except BaseException:
            self.close()
            raise

    def send(self, message: dict[str, Any]) -> None:
        raw = json.dumps(message, ensure_ascii=False).encode("utf-8") + b"\n"
        if len(raw) > MAX_MESSAGE:
            raise ValueError("Serena message exceeds 16 MiB transport bound")
        with self.write_lock:
            self.process.stdin.write(raw)
            self.process.stdin.flush()

    def _stderr(self) -> None:
        try:
            while block := self.process.stderr.read(2048):
                self.errors.append(block)
        except (OSError, ValueError):
            pass  # Owned close may race the drain thread after process exit.

    def _read(self) -> None:
        try:
            while True:
                raw = self.process.stdout.readline(MAX_MESSAGE + 1)
                if not raw:
                    raise EOFError("Serena worker closed its protocol stream")
                if len(raw) > MAX_MESSAGE:
                    raise ValueError("Serena response exceeds 16 MiB transport bound")
                message = json.loads(raw)
                if "method" in message:
                    if "id" in message:
                        self.send({"jsonrpc": "2.0", "id": message["id"],
                                   "error": {"code": -32601, "message": "Client capability not advertised"}})
                    continue
                with self.pending_lock:
                    destination = self.pending.get(message.get("id"))
                if destination is not None:
                    destination.put_nowait(message)
        except Exception as error:
            with self.pending_lock:
                for destination in self.pending.values():
                    if destination.empty():
                        destination.put_nowait(error)

    def rpc(self, method: str, params: dict[str, Any], timeout: float) -> dict[str, Any]:
        self.serial += 1
        identifier = self.serial
        response: queue.Queue[Any] = queue.Queue(maxsize=1)
        with self.pending_lock:
            self.pending[identifier] = response
        try:
            if self.process.poll() is not None:
                raise RuntimeError("Serena worker is no longer running")
            def write() -> None:
                try:
                    self.send({"jsonrpc": "2.0", "id": identifier, "method": method, "params": params})
                except Exception as error:
                    try:
                        response.put_nowait(error)
                    except queue.Full:
                        pass
            # A stopped worker can fill its stdin pipe. Include that write in
            # the request timeout; owned close unblocks the writer on failure.
            threading.Thread(target=write, daemon=True).start()
            value = response.get(timeout=max(0.01, timeout))
            if isinstance(value, Exception):
                raise value
            return value
        except Exception as error:
            self.close()
            if isinstance(error, queue.Empty):
                raise TimeoutError("Serena worker exceeded its request deadline") from error
            raise
        finally:
            with self.pending_lock:
                self.pending.pop(identifier, None)
            self.used = time.monotonic()

    def close(self) -> None:
        self.guard.close()
        if self.process is not None:
            self.process.wait(timeout=5)
            for stream in (self.process.stdin, self.process.stdout, self.process.stderr):
                if stream:
                    stream.close()


class ProjectPool:
    def __init__(self, factory: Any = NativeWorker):
        self.capacity, self.idle = policy()
        self.factory = factory
        self.workers: dict[str, Any] = {}
        self.clients: dict[str, dict[str, Any]] = {}
        self.lock = threading.RLock()
        self.used = time.monotonic()

    def worker(self, route: dict[str, Any], initialize: dict[str, Any], timeout: float) -> Any:
        key = configuration_key(route, initialize)
        if key in self.workers and self.workers[key].process.poll() is None:
            self.workers[key].used = time.monotonic()
            return self.workers[key]
        # Retire changed configuration for this selection before replacement.
        for old_key, old in list(self.workers.items()):
            if old.process.poll() is not None or (old.route == route and old_key != key):
                old.close()
                del self.workers[old_key]
        while len(self.workers) >= self.capacity:
            oldest = min(self.workers, key=lambda candidate: self.workers[candidate].used)
            self.workers.pop(oldest).close()
        created = self.factory(route, initialize, timeout)
        # Native startup may create project metadata/register the project.
        key = configuration_key(route, initialize)
        self.workers[key] = created
        return created

    def dispatch(self, operation: str, payload: dict[str, Any], deadline: float) -> Any:
        if operation == "status":
            # Endpoint liveness must remain readable while a worker starts or
            # executes a long tool. Snapshot membership before inspecting the
            # immutable route/PID fields; provider work stays serialized below.
            workers = tuple(self.workers.items())
            return {"clients": len(self.clients), "workers": [
                {"pid": item.process.pid, "project": item.route["project"], "key": key}
                for key, item in workers]}
        remaining = max(0, deadline - time.time())
        if not self.lock.acquire(timeout=remaining):
            raise TimeoutError("Serena shared operation is busy")
        try:
            remaining = deadline - time.time()
            if remaining <= 0:
                raise TimeoutError("Serena request expired before admission")
            if operation != "disconnect":
                assert_activation_ready()
            self.used = time.monotonic()
            client = payload["client"]
            if not isinstance(client, str) or len(client) != 32:
                raise ValueError("Invalid Serena client identity")
            if operation == "disconnect":
                self.clients.pop(client, None)
                return True
            if operation == "connect":
                if client not in self.clients and len(self.clients) >= 128:
                    raise RuntimeError("Serena client capacity reached")
                route = payload.get("route") or parse_route(payload["arguments"], payload["cwd"])
                if route["project"]:
                    route["known_projects"] = list(dict.fromkeys([*route.get("known_projects", []), route["project"]]))
                initialize = payload["initialize"]
                native = self.worker(route, initialize, remaining)
                self.clients[client] = {"route": route, "initialize": initialize, "used": self.used}
                return {"message": native.initialized, "route": route}
            if operation != "rpc":
                raise ValueError(f"Unknown Serena operation: {operation}")
            if client not in self.clients:
                # A connected idle proxy can outlive its cached broker state.
                if len(self.clients) >= 128:
                    raise RuntimeError("Serena client capacity reached")
                self.clients[client] = {"route": payload["route"], "initialize": payload["initialize"], "used": self.used}
            state = self.clients[client]
            state["used"] = self.used
            method, params = payload["method"], payload.get("params", {})
            route = dict(state["route"])
            activation = method == "tools/call" and params.get("name") == "activate_project"
            removal = method == "tools/call" and params.get("name") == "remove_project"
            if activation:
                route["project"] = canonical_project(params["arguments"]["project"], route["cwd"],
                                                      route.get("known_projects"), route.get("removed_projects"))
                params = {**params, "arguments": {**params["arguments"], "project": route["project"]}}
            if removal:
                # Optional remove_project mutates in-memory native config. A
                # client performing it becomes incompatible before mutation.
                route["mutation_owner"] = client
            if method == "tools/call":
                params = {**params, "_meta": {**params.get("_meta", {}), "harness_serena_client": client}}
            # Serena1.7 removed switch_modes. CLI modes are part of the key; do
            # not invent a tool or mutate modes on another client's worker.
            native = self.worker(route, state["initialize"], max(0.01, deadline - time.time()))
            result = native.rpc(method, params, max(0.01, deadline - time.time()))
            activated = activation and "error" not in result and not result.get("result", {}).get("isError")
            if activated:
                route["known_projects"] = list(dict.fromkeys([*route.get("known_projects", []), route["project"]]))
                state["route"] = route
            if removal and "error" not in result and not result.get("result", {}).get("isError"):
                route["removed_projects"] = sorted(set([*route.get("removed_projects", []), params["arguments"]["project_name"]]))
                state["route"] = route
            return {"message": result, "route": state["route"], "tools_changed": activated}
        finally:
            self.lock.release()

    def reap(self) -> None:
        if not self.lock.acquire(blocking=False):
            return
        try:
            now = time.monotonic()
            for key, worker in list(self.workers.items()):
                if now - worker.used >= self.idle or worker.process.poll() is not None:
                    worker.close()
                    del self.workers[key]
            for client, state in list(self.clients.items()):
                if now - state["used"] >= self.idle:
                    del self.clients[client]
        finally:
            self.lock.release()

    def close(self) -> None:
        with self.lock:
            failure = None
            for worker in self.workers.values():
                try:
                    worker.close()
                except Exception as error:
                    failure = error
            self.workers.clear()
            if failure:
                raise failure


_private_roots: set[Path] = set()


def ensure_endpoint(expected: str, timeout: float = 30) -> dict[str, Any]:
    root = directory()
    if root not in _private_roots:
        private_directory(root)
        _private_roots.add(root)
    deadline = time.monotonic() + timeout
    with FileLock(root / "startup.lock") as lock:
        lock.acquire(max(0, deadline - time.monotonic()))
        assert_activation_ready()
        endpoint = read_endpoint(root)
        if endpoint:
            if endpoint.get("source") != expected:
                raise RuntimeError("Serena broker has older source/runtime; retire it before reconnecting")
            status = exchange(endpoint, "status", {}, min(3, max(0.1, deadline - time.monotonic())))
            if status["source"] != expected or status["retiring"]:
                raise RuntimeError("Serena broker is retiring or has incompatible source")
            return endpoint
        # A live but unready server owns the lifetime lock; never start a twin.
        with FileLock(root / "instance.lock") as lifetime:
            lifetime.acquire(0)
        log_path = root / "broker.log"
        if log_path.exists() and log_path.stat().st_size > 1024 * 1024:
            log_path.replace(root / "broker.previous.log")
        child = spawn_service(Path(__file__), arguments=["--serve"], cwd=root, log_path=log_path,
                              startup_timeout=max(0.01, deadline - time.monotonic()))
        while time.monotonic() < deadline:
            endpoint = read_endpoint(root)
            if endpoint and endpoint.get("source") == expected:
                return endpoint
            if child.poll() is not None:
                raise RuntimeError(f"Serena broker exited during startup ({child.returncode}); see {log_path}")
            time.sleep(0.05)
        raise TimeoutError("Serena broker startup exceeded deadline; existing owner retained")


def serve() -> None:
    root = directory()
    private_directory(root)
    with FileLock(root / "instance.lock") as lifetime:
        lifetime.acquire(0)
        assert_activation_ready()
        JobGuard().contain_current_process()
        source = source_identity()
        token = secrets.token_hex(32)
        pool = ProjectPool()
        retiring = threading.Event()
        slots = threading.BoundedSemaphore(8)
        activity = threading.Lock()
        active = 0
        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *_):
                pass
            def do_POST(self):
                if self.path != "/rpc" or not hmac.compare_digest(self.headers.get("Authorization", ""), "Bearer " + token):
                    self.send_error(403)
                    return
                try:
                    self.connection.settimeout(5)
                    length = int(self.headers.get("Content-Length", 0))
                    if not 0 < length <= MAX_MESSAGE:
                        raise ValueError("Invalid Serena request length")
                    message = json.loads(self.rfile.read(length))
                    operation = message["operation"]
                    deadline = min(float(message["deadline"]), time.time() + 240)
                    if retiring.is_set() and operation not in ("status", "retire"):
                        raise RuntimeError("Serena broker is retiring")
                    if operation == "retire":
                        retiring.set()
                        result = {"status": "draining", "pid": os.getpid()}
                    else:
                        result = pool.dispatch(operation, message.get("payload", {}), deadline)
                        if operation == "status":
                            result.update(source=source, retiring=retiring.is_set(), pid=os.getpid())
                    response = {"result": result}
                except Exception as error:
                    response = {"error": f"{type(error).__name__}: {error}"}
                raw = json.dumps(response, ensure_ascii=False).encode("utf-8")
                if len(raw) > MAX_MESSAGE:
                    raw = b'{"error":"Serena response exceeds transport bound"}'
                try:
                    self.send_response(200)
                    self.send_header("Content-Type", "application/json")
                    self.send_header("Content-Length", str(len(raw)))
                    self.end_headers()
                    self.wfile.write(raw)
                except OSError:
                    pass
        class Server(ThreadingHTTPServer):
            daemon_threads = True
            def process_request(self, request, address):
                nonlocal active
                # Bound request-line/header stalls before do_POST can run.
                request.settimeout(5)
                if not slots.acquire(blocking=False):
                    self.shutdown_request(request)
                    return
                with activity:
                    active += 1
                try:
                    super().process_request(request, address)
                except BaseException:
                    with activity:
                        active -= 1
                    slots.release()
                    raise
            def process_request_thread(self, *args):
                nonlocal active
                try:
                    super().process_request_thread(*args)
                finally:
                    with activity:
                        active -= 1
                    slots.release()
        server = Server(("127.0.0.1", 0), Handler)
        server.timeout = 0.5
        endpoint = {"protocol": 1, "pid": os.getpid(), "started": psutil.Process().create_time(),
                    "port": server.server_address[1], "token": token, "source": source}
        temporary = root / f"endpoint-{os.getpid()}.tmp"
        assert_activation_ready()
        temporary.write_text(json.dumps(endpoint), encoding="utf-8")
        temporary.replace(root / "endpoint.json")
        mark_service_ready()
        try:
            while True:
                server.handle_request()
                pool.reap()
                with activity:
                    if active == 0:
                        if retiring.is_set() or time.monotonic() - pool.used >= pool.idle:
                            break
        finally:
            server.server_close()
            pool.close()
            if read_endpoint(root) == endpoint:
                (root / "endpoint.json").unlink(missing_ok=True)


def retire(timeout: float = 30) -> dict[str, Any]:
    if not directory().exists():
        return {"status": "absent"}
    endpoint = read_endpoint(directory())
    if not endpoint:
        # A live startup owner may not have published its endpoint yet.
        with FileLock(directory() / "instance.lock") as lifetime:
            lifetime.acquire(0)
        return {"status": "absent"}
    owner = psutil.Process(endpoint["pid"])
    if owner.create_time() != endpoint["started"]:
        raise RuntimeError("Serena owner creation identity changed before retirement")
    exchange(endpoint, "retire", {}, min(5, timeout))
    owner.wait(timeout=timeout)
    return {"status": "retired", "pid": endpoint["pid"]}


if __name__ == "__main__":
    if sys.argv[1:] == ["--serve"]:
        serve()
    elif sys.argv[1:] == ["--retire"]:
        print(json.dumps(retire()))
    else:
        raise SystemExit("Use --serve or --retire")
