"""Explicit dependency planning and staged Codebase Memory updates.

Ordinary MCP/LSP launchers never import this module. Plan performs metadata reads only;
stage writes only an identified temporary candidate. Promotion requires a fresh process
check, exclusive shared-installation lock, real protocol evidence and a retained rollback.
"""
from __future__ import annotations

import argparse
import base64
from concurrent.futures import ThreadPoolExecutor
from contextlib import contextmanager
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
from types import SimpleNamespace
import queue
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import threading
import time
import tomllib
import urllib.request
import urllib.error
from urllib.parse import urlparse
import uuid
import zipfile

_spec = importlib.util.spec_from_file_location("harness_discovery", Path(__file__).with_name("discovery.py"))
discovery = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(discovery)

ALLOWED_HOSTS = {"registry.npmjs.org", "pypi.org", "files.pythonhosted.org", "api.github.com", "github.com",
                 "release-assets.githubusercontent.com", "objects.githubusercontent.com", "api.nuget.org",
                 "builds.dotnet.microsoft.com", "dotnetcli.blob.core.windows.net", "download.visualstudio.microsoft.com",
                 "gitee.com", "raw.githubusercontent.com", "static.rust-lang.org", "downloads.freepascal.org"}
ALLOWED_HOSTS.update({"huggingface.co", "us.aws.cdn.hf.co"})

# Compatibility fingerprints measured from cached models that passed actual
# offline Nuphus desktop_perceive. These are not checksums declared by upstream.
NUPHUS_MODELS = {
    "ch_PP-OCRv4_det.onnx": {
        "url": "https://huggingface.co/SWHL/RapidOCR/resolve/main/PP-OCRv4/ch_PP-OCRv4_det_infer.onnx",
        "sha256": "d2a7720d45a54257208b1e13e36a8479894cb74155a5efe29462512d42f49da9"},
    "ch_PP-OCRv4_rec.onnx": {
        "url": "https://huggingface.co/SWHL/RapidOCR/resolve/main/PP-OCRv4/ch_PP-OCRv4_rec_infer.onnx",
        "sha256": "48fc40f24f6d2a207a2b1091d3437eb3cc3eb6b676dc3ef9c37384005483683b"},
}

NPM_LANGUAGES = {
    "python": {"package": "basedpyright", "cache": "BasedPyrightLanguageServer", "bin": "basedpyright-langserver"},
    "json": {"package": "vscode-langservers-extracted", "cache": "JsonLanguageServer", "bin": "vscode-json-language-server"},
    "bash": {"package": "bash-language-server", "cache": "BashLanguageServer", "bin": "bash-language-server"},
}
TRANSACTION_ID = None


def atomic_json(path, value):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(path.name + ".tmp-" + uuid.uuid4().hex)
    with temporary.open("x", encoding="utf-8") as stream:
        json.dump(value, stream, indent=2)
        stream.flush()
        os.fsync(stream.fileno())
    os.replace(temporary, path)


def tree_identity(path, file_overrides=None):
    path = Path(path)
    if not path.is_dir() or path.is_symlink():
        return None
    digest = hashlib.sha256()
    files = {}
    for entry in path.rglob("*"):
        if entry.is_symlink() or not discovery.contained(entry, path):
            raise ValueError("Dependency tree contains an external link")
        if entry.is_file():
            files[entry.relative_to(path).as_posix()] = discovery.fingerprint(entry)
    files.update(file_overrides or {})
    # Preserve the original platform Path ordering used by existing journals.
    for name in sorted(files, key=Path):
        fingerprint = files[name]
        digest.update(name.encode("utf-8") + b"\0")
        digest.update(bytes.fromhex(fingerprint))
    return digest.hexdigest()


def transaction_path(state_dir, component):
    owner = TRANSACTION_ID or "standalone-" + uuid.uuid4().hex
    if not re.fullmatch(r"[A-Za-z0-9_-]{1,100}", owner):
        raise ValueError("Transaction id contains invalid characters")
    return Path(state_dir) / "transactions" / owner / (component + "-" + uuid.uuid4().hex + ".json")


def activate_new_directory(candidate, installation, state_dir, component):
    state_dir = discovery.canonical(state_dir)
    if any(Path(path).is_symlink() for path in (candidate, installation)):
        raise ValueError("New dependency transaction paths must not be symbolic links")
    candidate, installation = discovery.canonical(candidate), discovery.canonical(installation)
    if installation.exists() or not discovery.contained(candidate, Path(state_dir) / "staging"):
        raise ValueError("New dependency activation must have an empty destination and owned staging")
    journal_path = transaction_path(state_dir, component)
    backup = Path(state_dir) / "rollback" / (component + "-new-" + uuid.uuid4().hex)
    journal = {"schema_version": 1, "owner": "codex-harness-dependencies", "kind": "create-directory",
               "phase": "prepared", "installation": str(installation), "candidate": str(candidate), "backup": str(backup),
               "candidate_identity": tree_identity(candidate), "transaction_id": TRANSACTION_ID, "component": component}
    atomic_json(journal_path, journal)
    rename_checked(candidate, installation)
    journal.update(phase="committed", installed_identity=tree_identity(installation))
    atomic_json(journal_path, journal)
    return str(journal_path)


def activate_new_file(candidate, installation, state_dir, component):
    state_dir = discovery.canonical(state_dir)
    if any(Path(path).is_symlink() for path in (candidate, installation)):
        raise ValueError("New file transaction paths must not be symbolic links")
    candidate, installation = map(discovery.canonical, (candidate, installation))
    if installation.exists() or not candidate.is_file() or not discovery.contained(candidate, Path(state_dir) / "staging"):
        raise ValueError("New file activation requires owned staging and a previously absent target")
    journal_path = transaction_path(state_dir, component)
    backup = Path(state_dir) / "rollback" / (component + "-file-" + uuid.uuid4().hex)
    journal = {"schema_version": 1, "owner": "codex-harness-dependencies", "kind": "create-file", "phase": "prepared",
               "installation": str(installation), "candidate": str(candidate), "backup": str(backup),
               "candidate_identity": discovery.fingerprint(candidate), "transaction_id": TRANSACTION_ID}
    atomic_json(journal_path, journal)
    # link fails if another actor created the target; never replace their file.
    os.link(candidate, installation)
    candidate.unlink()
    journal["phase"] = "committed"
    atomic_json(journal_path, journal)
    return str(journal_path)


def lifecycle_context():
    return sys.modules.get(__name__) or SimpleNamespace(**globals())


def fetch(url, limit=128 * 1024 * 1024):
    parsed = urlparse(url)
    if parsed.scheme != "https" or parsed.hostname not in ALLOWED_HOSTS or parsed.username or parsed.password:
        raise ValueError("Dependency source must be an allowlisted official HTTPS endpoint")
    request = urllib.request.Request(url, headers={"User-Agent": "codex-harness-dependency-lifecycle", "Accept": "application/json"})
    try:
        response = urllib.request.urlopen(request, timeout=45)
    except urllib.error.HTTPError as error:
        # Use the existing authenticated read-only GitHub client when the public IP quota is exhausted.
        # Credentials remain owned by gh; no tokens are read, copied or printed.
        gh = shutil.which("gh")
        if error.code != 403 or parsed.hostname != "api.github.com" or not gh:
            raise
        fallback = subprocess.run([gh, "api", "--method", "GET", parsed.path + ("?" + parsed.query if parsed.query else "")],
                                  capture_output=True, timeout=45, env=dict(os.environ, GH_PROMPT_DISABLED="1"),
                                  creationflags=0x08000000 if os.name == "nt" else 0)
        if fallback.returncode or len(fallback.stdout) > limit:
            raise error
        return fallback.stdout
    with response:
        final = urlparse(response.url)
        if final.scheme != "https" or final.hostname not in ALLOWED_HOSTS:
            raise ValueError("Dependency redirect left the official source allowlist")
        value = response.read(limit + 1)
        if len(value) > limit:
            raise ValueError("Dependency response exceeds the bounded download size")
        return value


def fetch_json(url):
    data = fetch(url, 16 * 1024 * 1024)
    return tomllib.loads(data.decode("utf-8")) if url.endswith(".toml") else json.loads(data)


def stable_version(version):
    return bool(re.fullmatch(r"v?\d+(?:\.\d+)*", str(version)) or re.fullmatch(r"\d{4}-\d{2}-\d{2}", str(version)))


def version_key(version):
    match = re.search(r"\d+(?:[.-]\d+)+", str(version))
    return tuple(int(x) for x in re.split(r"[.-]", match.group())) if match else ()


def check_release(spec, getter=fetch_json):
    url = spec.get("metadata")
    checked = {"source": url, "checked_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()), "state": "unchecked", "version": None}
    if not url:
        checked["reason"] = "Backend channel requires a declared metadata contract; no current-version claim."
        return checked
    try:
        value = getter(url)
        if spec.get("metadata_format") == "rust-channel":
            # rust-analyzer-preview has the placeholder version 0.0.0 in rustup's
            # channel. The installed binary reports its compiler cohort instead.
            version = value["pkg"]["rust"]["version"].split()[0]
            checked["version_kind"] = "rust-toolchain-cohort"
            checked["component_metadata_version"] = value["pkg"]["rust-analyzer-preview"]["version"]
            checked["channel_date"] = value.get("date")
            checked["toolchain_policy"] = "Compare the existing component; never update project Rust toolchains as a side effect."
        elif "info" in value:
            version = value["info"]["version"]
        elif "tag_name" in value:
            if value.get("prerelease") or value.get("draft"):
                raise ValueError("Latest endpoint returned a prerelease/draft")
            version = value["tag_name"]
        elif "versions" in value:
            versions = [v for v in value["versions"] if stable_version(v)]
            version = max(versions, key=version_key) if versions else None
        else:
            version = value.get("version")
        if not version or not stable_version(version):
            raise ValueError("No stable version in official metadata")
        checked.update(state="checked", version=version)
    except Exception as error:
        checked.update(state="failed", reason=f"{type(error).__name__}: {error}")
    return checked


def plan(catalogue, inventory, getter=fetch_json):
    records = {x["id"]: x for x in inventory["mcp"] + inventory["languages"]}
    specs = [spec for spec in catalogue["mcp"] + catalogue["languages"] if spec["manager"] != "native"]
    with ThreadPoolExecutor(max_workers=6) as pool:
        releases = list(pool.map(lambda spec: check_release(spec, getter) if spec.get("required", True) or records[spec["id"]]["status"] != "missing" else {"state": "not-requested", "reason": "Conditional reuse-only backend is absent.", "version": None}, specs))
    result = []
    for spec, release in zip(specs, releases):
        installed = records[spec["id"]]
        action = "reuse"
        reason = None
        if not spec.get("required", True) and installed["status"] == "missing":
            action = "conditional-absent"
        elif installed["status"] == "missing":
            action = "install-required"
        elif installed["status"] in ("modified", "ambiguous", "broken"):
            action, reason = "preserve-and-audit", installed["status"]
        elif release["state"] != "checked":
            action, reason = "metadata-unresolved", release.get("reason")
        elif version_key(release["version"]) > version_key(installed["version"]):
            action = "stage-compatible-update"
            if installed["active_consumers"]["state"] != "observed":
                reason = "Fresh active-consumer inspection required before replacement."
            elif installed["active_consumers"]["processes"]:
                action, reason = "update-pending-consumers", "Shared installation has active consumers; no process will be stopped."
        result.append({"id": spec["id"], "identity": spec["package"], "required": spec.get("required", True),
                       "installed_version": installed["version"], "installation_root": installed["installation_root"],
                       "action": action, "reason": reason, "release": release,
                       "runtime_prerequisites": spec["runtime"], "rollback": "Retain the prior installation and affected manager metadata before promotion."})
    return {"schema_version": 1, "mode": "plan", "read_only": True, "items": result}


@contextmanager
def installation_lock(state_dir, installation):
    root = Path(state_dir) / "locks"
    root.mkdir(parents=True, exist_ok=True)
    key = hashlib.sha256(str(discovery.canonical(installation)).encode()).hexdigest()
    path = root / (key + ".lock")
    token = uuid.uuid4().hex
    try:
        descriptor = os.open(path, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o600)
    except FileExistsError as error:
        raise RuntimeError(f"Another updater holds the installation lock: {path}") from error
    try:
        with os.fdopen(descriptor, "w") as stream:
            json.dump({"pid": os.getpid(), "token": token, "installation": str(installation)}, stream)
        yield
    finally:
        if (discovery.read_json(path) or {}).get("token") == token:
            path.unlink()


def safe_extract_tar(data, destination):
    destination = discovery.canonical(destination)
    with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as archive:
        for member in archive.getmembers():
            target = destination / member.name
            if not discovery.contained(target, destination) or member.issym() or member.islnk() or not (member.isfile() or member.isdir()):
                raise ValueError("Package archive contains an unsafe path or special file")
        archive.extractall(destination, filter="data")


def verify_integrity(data, integrity):
    algorithm, encoded = integrity.split("-", 1)
    if algorithm not in ("sha256", "sha512"):
        raise ValueError("Unsupported official artifact integrity algorithm")
    digest = hashlib.new(algorithm, data).digest()
    if base64.b64encode(digest).decode() != encoded:
        raise ValueError("Official package integrity mismatch")


def rename_checked(source, destination):
    """Briefly tolerate Windows image/scanner handles after a completed executable probe."""
    for delay in (0, 0.1, 0.25, 0.5, 1.0):
        if delay:
            time.sleep(delay)
        try:
            os.replace(source, destination)
            return
        except PermissionError:
            if delay == 1.0:
                raise


def audit_npm_installation(package, version, installation):
    """Compare packaged files with the same official version without overwriting local work."""
    url = "https://registry.npmjs.org/" + package.replace("/", "%2f") + "/" + version
    metadata = fetch_json(url)
    if metadata.get("name") != package or metadata.get("version") != version:
        raise ValueError("Installed-version audit received a different package identity")
    data = fetch(metadata["dist"]["tarball"])
    verify_integrity(data, metadata["dist"]["integrity"])
    differences = []
    checked = 0
    with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as archive:
        for entry in archive:
            if not entry.isfile():
                continue
            relative = Path(entry.name).relative_to("package")
            target = Path(installation) / relative
            if not discovery.contained(target, installation) or target.is_symlink():
                raise ValueError("Installed package file escapes its installation")
            if not target.is_file():
                differences.append({"path": str(relative), "kind": "missing"})
                continue
            original = archive.extractfile(entry).read()
            actual = target.read_bytes()
            checked += 1
            if original == actual:
                continue
            kind = "modified"
            if relative.suffix == ".js":
                def code_lines(value):
                    return [line for line in value.decode("utf-8").splitlines() if not line.lstrip().startswith("//")]
                if code_lines(actual) == code_lines(original):
                    kind = "standalone-comment-or-newline-only"
            elif relative.name == "package.json":
                if json.loads(actual) == json.loads(original):
                    kind = "json-formatting-only"
            differences.append({"path": str(relative), "kind": kind,
                                "installed_sha256": hashlib.sha256(actual).hexdigest(), "upstream_sha256": hashlib.sha256(original).hexdigest()})
    unresolved = [d for d in differences if d["kind"] not in ("standalone-comment-or-newline-only", "json-formatting-only")]
    return {"state": "audited" if not unresolved else "modified", "checked_files": checked,
            "differences": differences, "unresolved": unresolved, "source": url,
            "preservation": "Promotion must retain the exact original installation as rollback, including local comments and formatting."}


def mcp_probe(command, workspace, timeout=25):
    """Actual MCP initialize/list/schema read in an isolated disposable consumer home."""
    workspace = Path(workspace)
    workspace.mkdir(parents=True, exist_ok=True)
    environment = dict(os.environ)
    for variable in ("HOME", "USERPROFILE", "APPDATA", "LOCALAPPDATA", "XDG_CACHE_HOME", "XDG_CONFIG_HOME", "XDG_DATA_HOME", "CBM_CACHE_DIR"):
        environment[variable] = str(workspace / variable.lower())
        Path(environment[variable]).mkdir(parents=True, exist_ok=True)
    # Windows uses the account's Known Folder for rendezvous, independently of
    # HOME/LOCALAPPDATA overrides. This supported endpoint option isolates the
    # daemon, CLI and index workers without weakening their ACL checks.
    # v0.10.8 src/daemon/bootstrap.c:198-233.
    runtime = workspace / ("r-" + uuid.uuid4().hex[:8])
    runtime.mkdir()
    environment["CBM_RUNTIME_DIR"] = str(runtime)
    evidence = None
    owned_daemon = None
    def daemon_status():
        status = subprocess.run([command[0], "daemon", "status"], cwd=workspace, env=environment,
                                capture_output=True, text=True, timeout=10, creationflags=0x08000000 if os.name == "nt" else 0)
        match = re.search(r"(?m)^\s*pid:\s*(\d+)\s*$", status.stdout)
        return status, int(match.group(1)) if match else None
    process = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                               text=True, encoding="utf-8", cwd=workspace, env=environment,
                               creationflags=0x08000000 if os.name == "nt" else 0)
    messages = queue.Queue()
    stderr = []
    def stdout_reader():
        for line in process.stdout:
            try:
                messages.put(json.loads(line))
            except ValueError:
                messages.put({"malformed_stdout": line[:120]})
    def stderr_reader():
        for line in process.stderr:
            stderr.append(line[:500])
            del stderr[:-20]
    threading.Thread(target=stdout_reader, daemon=True).start()
    stderr_thread = threading.Thread(target=stderr_reader, daemon=True)
    stderr_thread.start()
    def failure(reason):
        if process.poll() is not None:
            stderr_thread.join(timeout=0.5)
        tail = "".join(stderr)[-2500:]
        tail = re.sub(r"(?i)((?:authorization|api[_-]?key|access[_-]?token|password|secret)\s*[:=]\s*)[^\s,;]+", r"\1[redacted]", tail)
        return RuntimeError(f"{reason}; process exit={process.poll()}; stderr: {tail or '(empty)'}")
    def send(value):
        try:
            process.stdin.write(json.dumps(value) + "\n")
            process.stdin.flush()
        except OSError:
            raise failure("MCP input pipe closed") from None
    def request(number, method, parameters):
        send({"jsonrpc": "2.0", "id": number, "method": method, "params": parameters})
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            try:
                value = messages.get(timeout=min(0.2, max(0.01, deadline - time.monotonic())))
            except queue.Empty:
                if process.poll() is not None:
                    raise failure(f"MCP {method} exited before replying")
                continue
            if "malformed_stdout" in value:
                raise RuntimeError("MCP emitted non-JSON stdout")
            if value.get("id") == number:
                if "error" in value:
                    raise RuntimeError("MCP request failed: " + json.dumps(value["error"]))
                return value["result"]
        raise failure(f"MCP {method} timed out")
    try:
        initialized = request(1, "initialize", {"protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": {"name": "harness-update-probe", "version": "1"}})
        status, owned_daemon = daemon_status()
        if status.returncode or not owned_daemon:
            raise RuntimeError("The isolated initialized MCP daemon did not expose an authenticated PID")
        send({"jsonrpc": "2.0", "method": "notifications/initialized"})
        tool_result = request(2, "tools/list", {})
        names = {tool["name"] for tool in tool_result["tools"]}
        required = {"index_repository", "search_graph", "query_graph", "get_graph_schema"}
        if not required.issubset(names):
            raise RuntimeError("Codebase Memory required operations are missing")
        fixture = workspace / "fixture"
        fixture.mkdir(exist_ok=True)
        (fixture / "probe.py").write_text("def inventory_probe(value):\n    return value + 1\n\nanswer = inventory_probe(1)\n", encoding="utf-8")
        index_tool = next(tool for tool in tool_result["tools"] if tool["name"] == "index_repository")
        properties = index_tool["inputSchema"].get("properties", {})
        path_key = next((key for key in ("repo_path", "path", "repository_path") if key in properties), None)
        if path_key is None:
            raise RuntimeError("Unrecognized index_repository path contract: " + json.dumps(index_tool["inputSchema"]))
        indexed = request(3, "tools/call", {"name": "index_repository", "arguments": {path_key: str(fixture)}})
        if indexed.get("isError"):
            raise RuntimeError("Disposable indexing failed: " + json.dumps(indexed))
        projects = request(4, "tools/call", {"name": "list_projects", "arguments": {}})
        structured = projects.get("structuredContent")
        if not structured:
            structured = json.loads(next(item["text"] for item in projects["content"] if item["type"] == "text"))
        project_list = structured.get("projects", []) if isinstance(structured, dict) else structured
        if len(project_list) != 1:
            raise RuntimeError("Disposable index did not produce exactly one isolated project: " + json.dumps(projects))
        project = project_list[0]
        name = project if isinstance(project, str) else project.get("name") or project.get("project")
        schema = request(5, "tools/call", {"name": "get_graph_schema", "arguments": {"project": name}})
        if schema.get("isError") or not schema.get("content"):
            raise RuntimeError("Codebase Memory schema read failed: " + json.dumps(schema))
        query = request(6, "tools/call", {"name": "query_graph", "arguments": {"project": name, "query": "MATCH (n:Function) WHERE n.name = 'inventory_probe' RETURN n.name"}})
        if query.get("isError") or "inventory_probe" not in json.dumps(query):
            raise RuntimeError("Disposable structured graph query failed: " + json.dumps(query))
        evidence = {"state": "passed", "server": initialized.get("serverInfo"), "tool_count": len(names),
                "checked_operations": ["initialize", "tools/list", "index_repository", "list_projects", "get_graph_schema", "query_graph"],
                "project": name, "query_result": query, "isolation": {"runtime": str(runtime), "daemon_pid": owned_daemon}}
        return evidence
    finally:
        original_error = sys.exc_info()[1]
        try:
            process.stdin.close()
        except OSError:
            pass
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=5)
        process.stdout.close()
        process.stderr.close()
        if owned_daemon is None and Path(command[0]).stem.lower() == "codebase-memory-mcp":
            # Initialization may fail after spawning the daemon. The fresh,
            # private nonce namespace belongs only to this invocation; discover
            # its authenticated PID before using the cooperative stop contract.
            status, owned_daemon = daemon_status()
            if owned_daemon is None and "not running" not in status.stdout:
                raise RuntimeError("Failed probe could not verify absence of its private daemon: " + status.stderr[-1000:]) from original_error
        if owned_daemon:
            status, active = daemon_status()
            if active is not None:
                if active != owned_daemon:
                    raise RuntimeError("Probe daemon identity changed; preserve the new process") from original_error
                stopped = subprocess.run([command[0], "daemon", "stop"], cwd=workspace, env=environment,
                                         capture_output=True, text=True, timeout=10, creationflags=0x08000000 if os.name == "nt" else 0)
                if stopped.returncode:
                    raise RuntimeError("Owned probe daemon refused cooperative stop: " + stopped.stdout[-1000:] + stopped.stderr[-1000:]) from original_error
                deadline = time.monotonic() + 8
                while time.monotonic() < deadline:
                    status, active = daemon_status()
                    if active is None and "not running" in status.stdout:
                        break
                    time.sleep(0.1)
                else:
                    raise RuntimeError("Owned probe daemon did not finish cooperative shutdown; retained its runtime for recovery") from original_error
            if active is None and "not running" not in status.stdout:
                raise RuntimeError("Probe teardown could not verify daemon absence: " + status.stderr[-1000:]) from original_error
            if evidence is not None:
                evidence["isolation"]["teardown"] = "authenticated owned daemon stopped; no force stop"


def stage_codebase(version, state_dir):
    if not stable_version(version):
        raise ValueError("A stable explicit Codebase Memory version is required")
    state_dir = discovery.canonical(state_dir)
    stage = state_dir / "staging" / ("codebase-memory-" + version + "-" + uuid.uuid4().hex)
    stage.mkdir(parents=True)
    npm_url = "https://registry.npmjs.org/codebase-memory-mcp/" + version
    metadata = fetch_json(npm_url)
    if metadata["name"] != "codebase-memory-mcp" or metadata["version"] != version:
        raise ValueError("Unexpected npm package identity")
    raw_package = fetch(metadata["dist"]["tarball"])
    verify_integrity(raw_package, metadata["dist"]["integrity"])
    safe_extract_tar(raw_package, stage)
    release_url = "https://api.github.com/repos/DeusData/codebase-memory-mcp/releases/tags/v" + version
    release = fetch_json(release_url)
    arch = "arm64" if os.environ.get("PROCESSOR_ARCHITECTURE", "AMD64").lower() == "arm64" else "amd64"
    asset_name = "codebase-memory-mcp-windows-" + arch + ".zip"
    asset = next(a for a in release["assets"] if a["name"] == asset_name)
    raw_binary = fetch(asset["browser_download_url"])
    digest = asset.get("digest", "")
    if not digest.startswith("sha256:") or hashlib.sha256(raw_binary).hexdigest() != digest.split(":", 1)[1]:
        raise ValueError("Official release archive checksum is absent or mismatched")
    with zipfile.ZipFile(io.BytesIO(raw_binary)) as archive:
        executables = [n for n in archive.namelist() if Path(n).name == "codebase-memory-mcp.exe"]
        if len(executables) != 1:
            raise ValueError("Release does not contain exactly one expected native binary")
        binary = stage / "package/bin/codebase-memory-mcp.exe"
        binary.parent.mkdir(exist_ok=True)
        binary.write_bytes(archive.read(executables[0]))
    evidence = mcp_probe([str(binary)], state_dir / "probes" / ("cb-" + uuid.uuid4().hex[:8]))
    manifest = {"schema_version": 1, "id": "codebase-memory", "version": version, "stage": str(stage),
                "package": str(stage / "package"), "executable": str(binary), "evidence": evidence,
                "sources": [npm_url, metadata["dist"]["tarball"], release_url, asset["browser_download_url"]],
                "npm_integrity": metadata["dist"]["integrity"], "archive_digest": digest,
                "executable_sha256": discovery.fingerprint(binary), "state": "staged-compatible"}
    (stage / "stage.json").write_text(json.dumps(manifest, indent=2), encoding="utf-8")
    return manifest


def replace_protected_file(target, replacement, backup):
    """Consume a prepared protected file, preserving the destination's Windows DACL.

    ReplaceFileW flags=0 must not ignore ACL merge errors. Its same-volume backup
    also makes documented partial failures recoverable without copying secrets.
    https://learn.microsoft.com/windows/win32/api/winbase/nf-winbase-replacefilew
    """
    target, replacement, backup = map(Path, (target, replacement, backup))
    if backup.exists():
        raise ValueError("Protected file backup already exists")
    if os.name == "nt":
        import ctypes
        from ctypes import wintypes
        kernel = ctypes.WinDLL("kernel32", use_last_error=True)
        replace = kernel.ReplaceFileW
        replace.argtypes = [wintypes.LPCWSTR, wintypes.LPCWSTR, wintypes.LPCWSTR,
                            wintypes.DWORD, wintypes.LPVOID, wintypes.LPVOID]
        replace.restype = wintypes.BOOL
        if not replace(str(target), str(replacement), str(backup), 0, None, None):
            raise ctypes.WinError(ctypes.get_last_error())
    else:
        # Hard-link the old inode before the atomic rename; never widen its mode.
        os.link(target, backup)
        os.replace(replacement, target)


def prepare_auxiliary_files(files):
    result = []
    seen = set()
    for item in files or []:
        values = {key: discovery.canonical(item[key]) for key in ("path", "before_path", "after_path")}
        if len(set(values.values())) != 3 or values["path"] in seen:
            raise ValueError("Auxiliary file paths must be distinct")
        seen.add(values["path"])
        if any(not p.is_file() or p.is_symlink() for p in values.values()):
            raise ValueError("Auxiliary files must exist as ordinary files")
        if len({p.anchor.lower() for p in values.values()}) != 1:
            raise ValueError("Auxiliary files must share a volume")
        before_hash, after_hash = [discovery.fingerprint(values[key]) for key in ("before_path", "after_path")]
        if discovery.fingerprint(values["path"]) != before_hash:
            raise ValueError("Auxiliary target changed before promotion")
        token = uuid.uuid4().hex
        result.append({**{key: str(value) for key, value in values.items()},
                       "before_sha256": before_hash, "after_sha256": after_hash,
                       "swap_backup": str(values["path"].with_name(values["path"].name + ".prior-" + token)),
                       "recovery_backup": str(values["path"].with_name(values["path"].name + ".reverted-" + token))})
    return result


def auxiliary_recovery_issue(items):
    for item in items:
        target, prior = Path(item["path"]), Path(item["swap_backup"])
        if target.is_symlink() or prior.is_symlink():
            return "Auxiliary file became a link; preserve it."
        target_hash = discovery.fingerprint(target) if target.is_file() else None
        if target_hash == item["before_sha256"]:
            continue
        if target_hash not in (None, item["after_sha256"]):
            return "Auxiliary target has unrecorded changes; preserve it."
        if not prior.is_file() or discovery.fingerprint(prior) != item["before_sha256"]:
            return "Auxiliary prior file identity is unavailable; preserve all files."
        if Path(item["recovery_backup"]).exists():
            return "Auxiliary recovery destination is occupied; preserve it."
    return None


def restore_auxiliary_files(items):
    issue = auxiliary_recovery_issue(items)
    if issue:
        raise RuntimeError(issue)
    for item in reversed(items):
        target, prior = Path(item["path"]), Path(item["swap_backup"])
        if target.is_file() and discovery.fingerprint(target) == item["before_sha256"]:
            continue
        if target.exists():
            replace_protected_file(target, prior, item["recovery_backup"])
        else:
            os.replace(prior, target)


def atomic_promote(candidate, installation, backup, validator, journal_path=None, auxiliary_files=None):
    if any(Path(path).is_symlink() for path in (candidate, installation, backup)):
        raise ValueError("Dependency transaction paths must not be symbolic links")
    candidate, installation, backup = map(discovery.canonical, (candidate, installation, backup))
    paths = (candidate, installation, backup)
    if any(discovery.contained(left, right) for i, left in enumerate(paths) for j, right in enumerate(paths) if i != j):
        raise ValueError("Staging/rollback paths must be outside the active installation")
    if installation.is_symlink() or not installation.is_dir() or backup.exists():
        raise ValueError("Installation or rollback identity changed")
    if candidate.anchor.lower() != installation.anchor.lower() or backup.anchor.lower() != installation.anchor.lower():
        raise ValueError("Atomic promotion requires staging, installation and rollback on the same volume")
    backup.parent.mkdir(parents=True, exist_ok=True)
    journal_path = Path(journal_path) if journal_path else backup.with_name(backup.name + ".transaction.json")
    journal = {"schema_version": 1, "owner": "codex-harness-dependencies", "kind": "replace-directory",
               "phase": "prepared", "installation": str(installation), "candidate": str(candidate), "backup": str(backup),
               "prior_identity": tree_identity(installation), "candidate_identity": tree_identity(candidate),
               "transaction_id": TRANSACTION_ID, "auxiliary_files": prepare_auxiliary_files(auxiliary_files)}
    if not journal["candidate_identity"] or not journal["prior_identity"]:
        raise ValueError("Dependency tree identity is unavailable")
    atomic_json(journal_path, journal)  # durable BEFORE the first move
    os.replace(installation, backup)
    try:
        journal["phase"] = "prior-moved"
        atomic_json(journal_path, journal)
        if tree_identity(backup) != journal["prior_identity"]:
            raise RuntimeError("Prior installation changed while promotion acquired it")
        os.replace(candidate, installation)
        journal["phase"] = "candidate-moved"
        atomic_json(journal_path, journal)
        if tree_identity(installation) != journal["candidate_identity"]:
            raise RuntimeError("Candidate changed during promotion")
        for item in journal["auxiliary_files"]:
            if discovery.fingerprint(item["path"]) != item["before_sha256"] or discovery.fingerprint(item["after_path"]) != item["after_sha256"]:
                raise RuntimeError("Auxiliary file changed during promotion")
            replace_protected_file(item["path"], item["after_path"], item["swap_backup"])
        journal["phase"] = "auxiliary-promoted"
        atomic_json(journal_path, journal)
        result = validator(installation)
        journal["phase"] = "committed"
        journal["installed_identity"] = tree_identity(installation)
        atomic_json(journal_path, journal)
    except BaseException:
        # Do not restore only half a transaction over an unknown auxiliary edit.
        # A failed restore leaves the durable journal pending for explicit recovery.
        restore_auxiliary_files(journal["auxiliary_files"])
        if installation.exists():
            os.replace(installation, candidate)
        os.replace(backup, installation)
        journal["phase"] = "restored"
        atomic_json(journal_path, journal)
        raise
    return result


def journal_problem(journal):
    """Reject incomplete recovery instructions before any filesystem mutation."""
    if not isinstance(journal, dict):
        return "Dependency journal is not a JSON object; preserve it for repair."
    kind = journal.get("kind")
    if kind not in {"replace-directory", "create-directory", "create-file", "rustup-component-install", "additive-runtime"}:
        return "Unsupported dependency transaction kind; preserve it for repair."
    if journal.get("phase") not in {"prepared", "prior-moved", "candidate-moved", "auxiliary-promoted", "committed", "restoring", "restored"}:
        return "Dependency transaction phase is absent or unrecognized."
    paths = ["installation"]
    hashes = []
    if kind in {"replace-directory", "create-directory", "create-file"}:
        paths += ["candidate", "backup"]
        hashes += ["candidate_identity"]
    if kind == "replace-directory":
        hashes.append("prior_identity")
    if kind == "rustup-component-install":
        paths += ["rustup", "executable"]
        hashes += ["executable_sha256", "manifest_sha256"]
    for key in paths:
        value = journal.get(key)
        if not isinstance(value, str) or not Path(value).is_absolute() or Path(value).is_symlink():
            return "Dependency recovery path is missing, relative or a link: " + key
    for key in hashes + (["installed_identity"] if journal.get("installed_identity") else []):
        if not re.fullmatch(r"[a-fA-F0-9]{64}", str(journal.get(key, ""))):
            return "Dependency recovery identity is missing or invalid: " + key
    if not isinstance(journal.get("auxiliary_files", []), list):
        return "Auxiliary recovery descriptors are invalid."
    for item in journal.get("auxiliary_files", []):
        if not isinstance(item, dict):
            return "Auxiliary recovery descriptor is not an object."
        for key in ("path", "before_path", "after_path", "swap_backup", "recovery_backup"):
            if not isinstance(item.get(key), str) or not Path(item[key]).is_absolute():
                return "Auxiliary recovery path is missing or relative: " + key
        for key in ("before_sha256", "after_sha256"):
            if not re.fullmatch(r"[a-fA-F0-9]{64}", str(item.get(key, ""))):
                return "Auxiliary recovery identity is missing or invalid: " + key
    return None


def recover_transaction(path, rollback_committed=False):
    path = Path(path)
    journal = discovery.read_json(path)
    if not isinstance(journal, dict) or journal.get("owner") != "codex-harness-dependencies":
        raise ValueError("Unsupported or foreign dependency transaction")
    problem = journal_problem(journal)
    if problem:
        return {"state": "pending", "journal": str(path), "reason": problem}
    if journal["phase"] == "restored":
        return {"state": "restored", "journal": str(path)}
    if journal["phase"] == "committed" and not rollback_committed:
        return {"state": "committed", "journal": str(path)}
    if journal["kind"] == "rustup-component-install":
        return load_lsp_provision().recover_rust(journal, path, lifecycle=lifecycle_context())
    if journal["kind"] == "additive-runtime":
        return {"state": "pending", "journal": str(path), "reason": "An explicit runtime prerequisite may have been added; its preserved installer and before-state require a manager-aware inverse. No unrelated runtime is removed."}
    installation, candidate, backup = [discovery.canonical(journal[key]) for key in ("installation", "candidate", "backup")]
    auxiliaries = journal.get("auxiliary_files", [])
    auxiliary_issue = auxiliary_recovery_issue(auxiliaries)
    if auxiliary_issue:
        return {"state": "pending", "journal": str(path), "reason": auxiliary_issue}
    if discovery.contained(backup, installation) or discovery.contained(candidate, installation) or installation.is_symlink():
        raise ValueError("Unsafe dependency recovery paths")
    if journal["kind"] == "create-file":
        if not installation.exists():
            journal["phase"] = "restored"
            atomic_json(path, journal)
            return {"state": "restored", "journal": str(path)}
        if not installation.is_file() or discovery.fingerprint(installation) != journal["candidate_identity"] or backup.exists():
            return {"state": "pending", "journal": str(path), "reason": "New file changed or rollback destination is occupied; preserve it."}
        backup.parent.mkdir(parents=True, exist_ok=True)
        journal["phase"] = "restoring"
        atomic_json(path, journal)
        rename_checked(installation, backup)
        journal["phase"] = "restored"
        atomic_json(path, journal)
        return {"state": "restored", "journal": str(path), "retained_file": str(backup)}
    if journal["kind"] == "create-directory":
        if not installation.exists():
            journal["phase"] = "restored"
            atomic_json(path, journal)
            return {"state": "restored", "journal": str(path), "reason": "Prior absence restored; candidate retained in lifecycle state."}
        if tree_identity(installation) not in (journal["candidate_identity"], journal.get("installed_identity")) or backup.exists():
            return {"state": "pending", "journal": str(path), "reason": "New installation changed or rollback path is occupied; preserve it."}
        backup.parent.mkdir(parents=True, exist_ok=True)
        journal["phase"] = "restoring"
        atomic_json(path, journal)
        rename_checked(installation, backup)
        journal["phase"] = "restored"
        atomic_json(path, journal)
        return {"state": "restored", "journal": str(path), "retained_candidate": str(backup)}
    prior, current = tree_identity(backup), tree_identity(installation)
    if not backup.exists() and current == journal["prior_identity"]:
        restore_auxiliary_files(auxiliaries)
        journal["phase"] = "restored"
        atomic_json(path, journal)
        return {"state": "restored", "journal": str(path), "reason": "Prior installation already occupies its original path."}
    if prior != journal["prior_identity"]:
        return {"state": "pending", "journal": str(path), "reason": "Prior installation identity differs; preserve all files for review."}
    if installation.exists() and current not in (journal["candidate_identity"], journal.get("installed_identity")):
        return {"state": "pending", "journal": str(path), "reason": "Active installation has unrecorded changes; recovery will not overwrite them."}
    if installation.exists() and candidate.exists():
        return {"state": "pending", "journal": str(path), "reason": "Candidate path is occupied; no file is overwritten."}
    journal["phase"] = "restoring"
    atomic_json(path, journal)
    restore_auxiliary_files(auxiliaries)
    if installation.exists():
        os.replace(installation, candidate)
    os.replace(backup, installation)
    journal["phase"] = "restored"
    atomic_json(path, journal)
    return {"state": "restored", "journal": str(path), "installation": str(installation)}


def recover_dependencies(state_dir, user_home, transaction_id=None, rollback_committed=False):
    state_dir = discovery.canonical(state_dir)
    root = state_dir / "transactions"
    if transaction_id:
        if not re.fullmatch(r"[A-Za-z0-9_-]{1,100}", transaction_id):
            raise ValueError("Invalid recovery transaction id")
        root /= transaction_id
    results = []
    current = discovery.Discovery(user_home, processes=True)
    for path in sorted(root.glob("**/*.json"), reverse=True):
        journal = discovery.read_json(path)
        if not isinstance(journal, dict) or journal.get("owner") != "codex-harness-dependencies":
            continue
        problem = journal_problem(journal)
        if problem:
            results.append({"state": "pending", "journal": str(path), "reason": problem})
            continue
        installation = discovery.canonical(journal["installation"])
        if journal.get("kind") == "additive-runtime":
            dotnet = current.find_command("dotnet")
            if not dotnet or installation != discovery.canonical(Path(dotnet).parent) or not discovery.contained(journal.get("installer", "."), state_dir):
                results.append({"state": "pending", "journal": str(path), "reason": "Additive runtime journal differs from the discovered .NET installation."})
            else:
                results.append(recover_transaction(path, rollback_committed))
            continue
        allowed_mcp = [discovery.canonical(root / name) for root in current.npm_roots for name in ("codebase-memory-mcp", "@nuphus/nuphus-mcp")]
        rust_component = journal.get("kind") == "rustup-component-install" and discovery.contained(installation, current.rustup / "toolchains")
        clangd_cache = installation.parent == current.home / ".cache/opencode/bin" and installation.name.startswith("clangd_")
        models = discovery.canonical((os.environ.get("NUPHUS_MODELS_DIR") if current.environment else None) or current.home / "AppData/Roaming/Nuphus/models")
        model_file = journal.get("kind") == "create-file" and installation.parent == models and installation.name in {"ch_PP-OCR_keys_v1.txt", *NUPHUS_MODELS}
        if installation not in allowed_mcp and not discovery.contained(installation, current.serena) and not discovery.contained(installation, current.uv) and not rust_component and not clangd_cache and not model_file:
            results.append({"state": "pending", "journal": str(path), "reason": "Installation is outside discovered shared dependency roots."})
            continue
        if rust_component:
            if journal.get("rustup") != current.find_command("rustup") or discovery.canonical(journal.get("executable", ".")) != installation / "bin/rust-analyzer.exe":
                results.append({"state": "pending", "journal": str(path), "reason": "Rust recovery manager or executable identity differs from discovery."})
                continue
        elif not discovery.contained(journal["candidate"], state_dir) or not discovery.contained(journal["backup"], state_dir):
            results.append({"state": "pending", "journal": str(path), "reason": "Recovery artifacts are outside the lifecycle state root."})
            continue
        bad_auxiliary = False
        for item in journal.get("auxiliary_files", []):
            target = discovery.canonical(item["path"])
            if target != current.graphify_manifest:
                bad_auxiliary = True
            for key in ("swap_backup", "recovery_backup"):
                if discovery.canonical(item[key]).parent != target.parent:
                    bad_auxiliary = True
            for key in ("before_path", "after_path"):
                artifact = discovery.canonical(item[key])
                if not discovery.contained(artifact, state_dir) and artifact.parent != target.parent:
                    bad_auxiliary = True
        if bad_auxiliary:
            results.append({"state": "pending", "journal": str(path), "reason": "Auxiliary recovery path is outside the discovered Graphify manifest or protected artifact roots."})
            continue
        with installation_lock(state_dir, installation):
            # The process snapshot matcher can inspect a missing active installation too.
            probe = [{"installation_root": str(installation)}] if rust_component else [{"installation_root": str(installation)}, {"installation_root": journal["backup"]}, {"installation_root": journal["candidate"]}]
            current.inspect_consumers(probe)
            if any(p["active_consumers"]["state"] != "observed" or p["active_consumers"]["processes"] for p in probe):
                results.append({"state": "pending", "journal": str(path), "reason": "Active/unresolved consumers prevent dependency recovery."})
            else:
                results.append(recover_transaction(path, rollback_committed))
    return {"schema_version": 1, "operation": "recover", "results": results, "complete": all(x["state"] in ("restored", "committed") for x in results)}


def promote_codebase(manifest_path, user_home, state_dir):
    manifest_path = discovery.canonical(manifest_path)
    manifest = discovery.read_json(manifest_path)
    if not manifest or manifest.get("state") != "staged-compatible" or manifest.get("evidence", {}).get("state") != "passed":
        raise ValueError("A successful staged protocol proof is required")
    stage = discovery.canonical(manifest["stage"])
    state_dir = discovery.canonical(state_dir)
    if not discovery.contained(stage, state_dir / "staging") or not discovery.contained(manifest_path, stage):
        raise ValueError("Candidate is outside this lifecycle's staging root")
    if discovery.fingerprint(manifest["executable"]) != manifest["executable_sha256"]:
        raise ValueError("Candidate binary changed after verification")
    inventory = discovery.Discovery(user_home, processes=True).run()
    installed = next(x for x in inventory["mcp"] if x["id"] == "codebase-memory")
    if installed["status"] != "adopted":
        raise RuntimeError("Existing Codebase Memory installation needs ownership/repair review")
    target = Path(installed["installation_root"])
    if version_key(installed["version"]) >= version_key(manifest["version"]):
        return {"state": "retained", "reason": "Installed version is already equal or newer; no downgrade."}
    # Hidden npm lockfiles would need an additional transaction; do not leave one stale.
    if (target.parent / ".package-lock.json").exists():
        raise RuntimeError("Existing npm hidden lockfile requires a manager-aware transaction")
    audit = audit_npm_installation("codebase-memory-mcp", installed["version"], target)
    if audit["state"] != "audited":
        return {"state": "pending", "reason": "Unresolved local package modifications must be preserved before replacement.", "audit": audit}
    with installation_lock(state_dir, target):
        fresh = discovery.Discovery(user_home, processes=True).run()
        current = next(x for x in fresh["mcp"] if x["id"] == "codebase-memory")
        if current["installation_root"] != str(target) or current["version"] != installed["version"]:
            raise RuntimeError("Shared installation changed while obtaining the update lock")
        if current["active_consumers"]["state"] != "observed" or current["active_consumers"]["processes"]:
            return {"state": "pending", "reason": "Active or unresolved shared consumers; no processes stopped."}
        backup = state_dir / "rollback" / ("codebase-memory-" + installed["version"] + "-" + uuid.uuid4().hex)
        def validate(path):
            result = mcp_probe([str(path / "bin/codebase-memory-mcp.exe")], state_dir / "probes" / ("cb-post-" + uuid.uuid4().hex[:8]))
            # OpenCode's existing npm wrapper still resolves the same entry point.
            node = current["paths"]["node"]
            wrapper = subprocess.run([node, str(path / "bin.js"), "--version"], capture_output=True, text=True, timeout=20,
                                     creationflags=0x08000000 if os.name == "nt" else 0)
            if wrapper.returncode or manifest["version"] not in wrapper.stdout:
                raise RuntimeError("Existing npm/OpenCode launcher compatibility check failed")
            result["existing_npm_launcher_version"] = wrapper.stdout.strip()
            return result
        try:
            journal = transaction_path(state_dir, "codebase-memory")
            result = atomic_promote(manifest["package"], target, backup, validate, journal)
        except BaseException as error:
            manifest.update(state="rolled-back", failure=f"{type(error).__name__}: {error}")
            manifest_path.write_text(json.dumps(manifest, indent=2), encoding="utf-8")
            raise
        manifest.update(state="promoted", installation=str(target), rollback=str(backup), post_promotion=result, prior_installation_audit=audit, transaction_journal=str(journal))
        manifest_path.write_text(json.dumps(manifest, indent=2), encoding="utf-8")
        return {"state": "updated", "version": manifest["version"], "installation": str(target), "rollback": str(backup), "evidence": result, "transaction_journal": str(journal)}


def provision_npm(language, user_home, state_dir, version=None):
    """Provision only selected missing JS backends in the shared Serena resource cache.

    npm owns package identity and dependency lockfiles. Nothing is installed under CODEX_HOME;
    its staging directory is temporary and renamed into the shared cache after resolution.
    """
    if language not in NPM_LANGUAGES:
        raise ValueError("No declared npm provisioning contract for this selected language")
    contract = NPM_LANGUAGES[language]
    current = discovery.Discovery(user_home)
    record = next(x for x in current.run()["languages"] if x["id"] == language)
    if record["status"] == "adopted":
        return {"state": "reused", "language": language, "record": record}
    if record["status"] != "missing":
        return {"state": "pending", "language": language, "reason": "Existing installation must be audited or repaired first.", "record": record}
    if not current.node:
        return {"state": "prerequisite-missing", "language": language, "prerequisite": "Existing Node.js/npm installation"}
    npm_cli = Path(current.node).parent / "node_modules/npm/bin/npm-cli.js"
    if not npm_cli.is_file():
        return {"state": "prerequisite-missing", "language": language, "prerequisite": "npm CLI beside the resolved Node.js installation"}
    metadata_url = "https://registry.npmjs.org/" + contract["package"].replace("/", "%2f") + "/" + (version or "latest")
    metadata = fetch_json(metadata_url)
    selected = metadata["version"]
    if metadata["name"] != contract["package"] or not stable_version(selected):
        raise ValueError("npm returned an unexpected or unstable package identity")
    destination = current.serena / contract["cache"] / "shared"
    state_dir = discovery.canonical(state_dir)
    with installation_lock(state_dir, destination):
        if destination.exists():
            raise RuntimeError("Shared provisioning destination already exists; discover or repair it instead of replacing")
        stage = state_dir / "staging" / (language + "-" + selected + "-" + uuid.uuid4().hex)
        stage.mkdir(parents=True)
        command = [current.node, str(npm_cli), "install", "--prefix", str(stage), "--no-audit", "--no-fund", "--ignore-scripts", "--save-exact", contract["package"] + "@" + selected]
        installed = subprocess.run(command, capture_output=True, text=True, timeout=180,
                                   creationflags=0x08000000 if os.name == "nt" else 0)
        if installed.returncode:
            raise RuntimeError("Explicit npm provisioning failed: " + installed.stderr[-3000:])
        candidate = discovery.npm_candidate(contract["package"], stage / "node_modules", current.node, contract["bin"], "serena-cache")
        if not candidate or candidate["status"] != "adopted":
            raise RuntimeError("npm did not produce the expected package/entrypoint")
        if stage.anchor.lower() != destination.anchor.lower():
            raise RuntimeError("Staging and shared cache must be on the same volume for atomic provisioning")
        destination.parent.mkdir(parents=True, exist_ok=True)
        journal_path = activate_new_directory(stage, destination, state_dir, language)
        actual = discovery.npm_candidate(contract["package"], destination / "node_modules", current.node, contract["bin"], "serena-cache")
        result = {"state": "installed-unverified", "language": language, "version": selected, "source": metadata_url,
                  "installation": str(destination), "record": actual,
                  "transaction_journal": journal_path,
                  "remaining": "Actual LSP initialization, project navigation and diagnostics evidence required."}
        return result


def provision_markdown(user_home, state_dir):
    """Add only the official Markdown LSP's parser to the existing shared JS prefix."""
    current = discovery.Discovery(user_home, processes=True)
    json_record = next(x for x in current.run()["languages"] if x["id"] == "json")
    if json_record["status"] == "missing":
        provision_npm("json", user_home, state_dir)
        current = discovery.Discovery(user_home, processes=True)
        json_record = next(x for x in current.run()["languages"] if x["id"] == "json")
    if json_record["status"] != "adopted" or json_record["identity"] != "vscode-langservers-extracted":
        return {"state": "pending", "reason": "The shared VS Code language-server package needs repair or a compatible declared selection."}
    module = Path(json_record["paths"]["module_root"])
    if not (module / "bin/vscode-markdown-language-server").is_file():
        return {"state": "pending", "reason": "Installed VS Code server package lacks its Markdown entrypoint."}
    destination = module.parent.parent
    if not discovery.contained(destination, current.serena) or not (destination / "package-lock.json").is_file():
        return {"state": "pending", "reason": "Parser provisioning requires the existing owned shared prefix and npm lockfile."}
    def parser_entry(prefix):
        root = Path(prefix) / "node_modules/markdown-it"
        data = discovery.read_json(root / "package.json")
        if not data or data.get("name") != "markdown-it":
            return None
        entry = discovery.canonical(root / data.get("main", "index.mjs"))
        return entry if discovery.contained(entry, root) and entry.is_file() else None
    parser = parser_entry(destination)
    if parser:
        return {"state": "reused", "language": "markdown", "parser_path": str(parser)}
    npm_cli = Path(current.node).parent / "node_modules/npm/bin/npm-cli.js"
    metadata = fetch_json("https://registry.npmjs.org/markdown-it/latest")
    if metadata.get("name") != "markdown-it" or not stable_version(metadata.get("version")):
        raise ValueError("Unexpected Markdown parser package metadata")
    state_dir = discovery.canonical(state_dir)
    with installation_lock(state_dir, destination):
        probe = [{"installation_root": str(destination)}]
        current.inspect_consumers(probe)
        if probe[0]["active_consumers"]["state"] != "observed" or probe[0]["active_consumers"]["processes"]:
            return {"state": "pending", "reason": "Shared VS Code LSP package has active or unresolved consumers; no process stopped."}
        prior_identity = tree_identity(destination)
        stage = state_dir / "staging" / ("markdown-parser-" + metadata["version"] + "-" + uuid.uuid4().hex)
        shutil.copytree(destination, stage)
        prior_lock = discovery.read_json(stage / "package-lock.json")
        command = [current.node, str(npm_cli), "install", "--prefix", str(stage), "--no-audit", "--no-fund", "--ignore-scripts", "--save-exact", "markdown-it@" + metadata["version"]]
        installed = subprocess.run(command, capture_output=True, text=True, timeout=180,
                                   creationflags=0x08000000 if os.name == "nt" else 0)
        if installed.returncode:
            raise RuntimeError("Explicit Markdown parser provisioning failed: " + installed.stderr[-2000:])
        next_lock = discovery.read_json(stage / "package-lock.json")
        for name, package in prior_lock["packages"].items():
            if name and next_lock["packages"].get(name) != package:
                raise RuntimeError("Parser installation unexpectedly changed an existing dependency lock record")
        def validate(path):
            entry = parser_entry(path)
            if not entry:
                raise RuntimeError("Markdown parser package entry is absent or escapes its package")
            code = "const {default:Parser}=await import(process.argv[1]); const tokens=new Parser().parse('# Heading\\n\\n[link](target.md)',{}); if(!tokens.some(t=>t.type==='heading_open'))process.exit(2); console.log(JSON.stringify({state:'passed',tokens:tokens.length}));"
            tested = subprocess.run([current.node, "--input-type=module", "--eval", code, entry.as_uri()], capture_output=True, text=True, timeout=20,
                                    creationflags=0x08000000 if os.name == "nt" else 0)
            if tested.returncode:
                raise RuntimeError("Markdown parser import/token contract failed: " + tested.stderr[-1500:])
            return json.loads(tested.stdout)
        evidence = validate(stage)
        if tree_identity(destination) != prior_identity:
            raise RuntimeError("Shared VS Code LSP package changed during staging")
        backup = state_dir / "rollback" / ("json-before-markdown-parser-" + uuid.uuid4().hex)
        journal = transaction_path(state_dir, "markdown-parser")
        atomic_promote(stage, destination, backup, validate, journal)
        return {"state": "installed-unverified", "language": "markdown", "version": metadata["version"],
                "identity": "markdown-it", "source": "https://registry.npmjs.org/markdown-it/latest",
                "installation": str(destination), "parser_path": str(parser_entry(destination)), "rollback": str(backup),
                "transaction_journal": str(journal), "evidence": evidence,
                "remaining": "Actual Markdown LSP heading/link navigation and diagnostic/clearance checks."}


def update_typescript(user_home, state_dir, version):
    current = discovery.Discovery(user_home, processes=True)
    inventory = current.run()
    record = next(x for x in inventory["languages"] if x["id"] == "typescript")
    if record["status"] != "adopted":
        return {"state": "pending", "id": "typescript", "reason": "Existing TypeScript server identity needs provisioning or repair."}
    prefix = Path(record["installation_root"]).parent.parent
    if not discovery.contained(prefix, current.serena) or not (prefix / "package-lock.json").is_file():
        return {"state": "pending", "id": "typescript", "reason": "The existing shared npm prefix and lockfile must be identified before update."}
    if version_key(version) <= version_key(record["version"]):
        return {"state": "reused", "id": "typescript", "version": record["version"]}
    audit = audit_npm_installation("typescript-language-server", record["version"], record["installation_root"])
    if audit["state"] != "audited":
        return {"state": "pending", "id": "typescript", "reason": "Local language-server changes need preservation/audit.", "audit": audit}
    state_dir = discovery.canonical(state_dir)
    stage_root = state_dir / "staging" / ("typescript-" + version + "-" + uuid.uuid4().hex)
    stage_root.mkdir(parents=True)
    with installation_lock(state_dir, prefix):
        probe = [{"installation_root": str(prefix)}]
        current.inspect_consumers(probe)
        if probe[0]["active_consumers"]["state"] != "observed" or probe[0]["active_consumers"]["processes"]:
            return {"state": "pending", "id": "typescript", "reason": "Existing TypeScript consumers prevent shared replacement; no process stopped."}
        before = tree_identity(prefix)
        typescript_identity = tree_identity(prefix / "node_modules/typescript")
        candidate = stage_root / "candidate"
        shutil.copytree(prefix, candidate)
        npm = Path(current.node).parent / "node_modules/npm/bin/npm-cli.js"
        process = subprocess.run([current.node, str(npm), "install", "--prefix", str(candidate), "--no-audit", "--no-fund", "--ignore-scripts", "--save-exact", "typescript-language-server@" + version],
                                 capture_output=True, text=True, timeout=180, creationflags=0x08000000)
        if process.returncode:
            raise RuntimeError("Staged TypeScript server update failed: " + process.stderr[-2000:])
        if tree_identity(candidate / "node_modules/typescript") != typescript_identity:
            raise RuntimeError("Language-server update changed the existing TypeScript compiler; preserve the old prefix")
        serena = next(x for x in inventory["mcp"] if x["id"] == "serena")
        interpreter = serena["paths"].get("python")
        if not interpreter:
            return {"state": "pending", "id": "typescript", "reason": "Existing Serena runtime is required for actual LSP compatibility proof."}
        def validate(path):
            command = [current.node, str(path / "node_modules/typescript-language-server/lib/cli.mjs"), "--stdio"]
            registry = stage_root / ("registry-" + uuid.uuid4().hex + ".json")
            report = registry.with_name(registry.stem + "-result.json")
            atomic_json(registry, {"servers": {language: {"command": command, "adapter": "typescript", "initialization_options": {"preferences": {"disableAutomaticTypingAcquisition": True}}} for language in ("typescript", "javascript")}})
            checked = subprocess.run([interpreter, str(Path(__file__).resolve().parents[2] / "tests/lsp-languages.py"), "typescript", "javascript", "--registry", str(registry), "--report", str(report)],
                                     capture_output=True, text=True, timeout=120, creationflags=0x08000000)
            result = discovery.read_json(report)
            if checked.returncode or not result or not all(item.get("passed") for item in result):
                raise RuntimeError("Staged TypeScript/JavaScript real diagnostics, clearance and symbol proof failed: " + checked.stderr[-2000:])
            return {"state": "passed", "report": str(report), "languages": [item["language"] for item in result]}
        evidence = validate(candidate)
        current.inspect_consumers(probe)
        if tree_identity(prefix) != before or probe[0]["active_consumers"]["state"] != "observed" or probe[0]["active_consumers"]["processes"]:
            return {"state": "pending", "id": "typescript", "reason": "Shared prefix or consumers changed after staging; candidate retained.", "evidence": evidence}
        backup = state_dir / "rollback" / ("typescript-" + record["version"] + "-" + uuid.uuid4().hex)
        journal = transaction_path(state_dir, "typescript")
        after = atomic_promote(candidate, prefix, backup, validate, journal)
        return {"state": "updated", "id": "typescript", "version": version, "installation": str(prefix), "rollback": str(backup),
                "transaction_journal": str(journal), "prior_audit": audit, "staged": evidence, "evidence": after,
                "typescript_compiler_unchanged": typescript_identity}


def provision_marksman(user_home, state_dir):
    current = discovery.Discovery(user_home)
    record = next(x for x in current.run()["languages"] if x["id"] == "markdown")
    if record["status"] != "missing":
        return {"state": "reused" if record["status"] == "adopted" else "pending", "language": "markdown", "record": record}
    url = "https://api.github.com/repos/artempyanykh/marksman/releases/latest"
    release = fetch_json(url)
    if release.get("prerelease") or not stable_version(release["tag_name"]):
        raise ValueError("Marksman release is not stable")
    asset = next(a for a in release["assets"] if a["name"] == "marksman.exe")
    version = release["tag_name"]
    destination = current.serena / "Marksman" / ("marksman-" + version)
    with installation_lock(state_dir, destination):
        if destination.exists():
            raise RuntimeError("Marksman target exists but discovery could not adopt it")
        raw = fetch(asset["browser_download_url"])
        digest = asset.get("digest", "")
        if not digest.startswith("sha256:") or hashlib.sha256(raw).hexdigest() != digest.split(":", 1)[1]:
            raise ValueError("Marksman release checksum is absent or mismatched")
        stage = Path(state_dir) / "staging" / ("marksman-" + uuid.uuid4().hex)
        stage.mkdir(parents=True)
        binary = stage / "marksman.exe"
        binary.write_bytes(raw)
        result = subprocess.run([str(binary), "--version"], capture_output=True, text=True, timeout=15, creationflags=0x08000000)
        if result.returncode:
            raise RuntimeError("Marksman executable probe failed: " + result.stderr)
        destination.parent.mkdir(parents=True, exist_ok=True)
        journal_path = activate_new_directory(stage, destination, state_dir, "markdown")
        record = discovery.native_candidate(destination / "marksman.exe", "serena-cache", version)
        return {"state": "installed-unverified", "language": "markdown", "version": version,
                "source": url, "checksum": digest, "record": record, "remaining": "Actual LSP project verification.", "transaction_journal": journal_path}


def provision_cmake(user_home, state_dir):
    # The former Python backend has no diagnostic/symbol handlers and cannot meet this scope.
    return provision_github_zip("cmake", user_home, state_dir)


def provision_taplo(user_home, state_dir):
    current = discovery.Discovery(user_home)
    existing = list((current.serena / "Taplo").glob("**/taplo.exe"))
    if existing:
        return {"state": "reused", "id": "toml", "record": discovery.native_candidate(existing[0], "serena-cache")}
    url = "https://api.github.com/repos/tamasfe/taplo/releases/latest"
    release = fetch_json(url)
    version = release["tag_name"]
    if not stable_version(version) or release.get("prerelease"):
        raise ValueError("Taplo release is not stable")
    asset = next(a for a in release["assets"] if a["name"] == "taplo-windows-x86_64.zip")
    raw = fetch(asset["browser_download_url"])
    stage = Path(state_dir) / "staging" / ("taplo-" + uuid.uuid4().hex)
    stage.mkdir(parents=True)
    safe_extract_zip(raw, stage)
    executables = list(stage.glob("**/taplo.exe"))
    if len(executables) != 1:
        raise ValueError("Taplo archive executable identity is ambiguous")
    probe = subprocess.run([str(executables[0]), "lsp", "--help"], capture_output=True, text=True, timeout=15, creationflags=0x08000000)
    if probe.returncode or "stdio" not in probe.stdout:
        raise RuntimeError("Taplo release lacks a working LSP entry point: " + probe.stderr)
    relative = executables[0].relative_to(stage)
    destination = current.serena / "Taplo" / ("taplo-" + version)
    with installation_lock(state_dir, current.serena / "Taplo"):
        legacy = current.serena / "Taplo/shared"
        rollback = None
        if legacy.exists():
            # This exact lifecycle-owned candidate failed real LSP initialization: npm is CLI-only.
            marker = discovery.read_json(legacy / ".harness-provisioning.json")
            if not marker or marker.get("language") != "toml":
                raise RuntimeError("Unowned legacy Taplo directory must not be replaced")
            rollback = Path(state_dir) / "rollback" / ("taplo-npm-no-lsp-" + uuid.uuid4().hex)
            rollback.parent.mkdir(parents=True, exist_ok=True)
            rename_checked(legacy, rollback)
        destination.parent.mkdir(parents=True, exist_ok=True)
        journal_path = activate_new_directory(stage, destination, state_dir, "toml")
    return {"state": "installed-unverified", "id": "toml", "version": version, "source": asset["browser_download_url"],
            "sha256": hashlib.sha256(raw).hexdigest(), "hash_kind": "observed official artifact fingerprint; release does not publish a digest",
            "record": discovery.native_candidate(destination / relative, "serena-cache", version), "rollback": str(rollback) if rollback else None, "transaction_journal": journal_path,
            "rejected_alternative": "@taplo/cli0.7.0 npm build reports the LSP is not part of this build."}


def safe_extract_zip(raw, target):
    with zipfile.ZipFile(io.BytesIO(raw)) as archive:
        for member in archive.infolist():
            if not discovery.contained(Path(target) / member.filename, target) or (member.external_attr >> 16) & 0o170000 == 0o120000:
                raise ValueError("ZIP contains an unsafe path or symlink")
        archive.extractall(target)


def provision_github_zip(component, user_home, state_dir):
    contracts = {
        "xml": ("redhat-developer/vscode-xml", "Lemminx", "lemminx-win32.zip", "lemminx*.exe"),
        "shellcheck": ("koalaman/shellcheck", "BashLanguageServer", None, "shellcheck.exe"),
        "cmake": ("neocmakelsp/neocmakelsp", "NeoCMakeLanguageServer", "neocmakelsp-x86_64-pc-windows-msvc.zip", "neocmakelsp.exe"),
    }
    if component not in contracts:
        raise ValueError("No declared release contract")
    repository, cache, asset_name, pattern = contracts[component]
    current = discovery.Discovery(user_home)
    existing = list((current.serena / cache).glob("**/" + pattern))
    if existing:
        return {"state": "reused", "id": component, "record": discovery.native_candidate(existing[0], "serena-cache")}
    url = "https://api.github.com/repos/" + repository + "/releases/latest"
    release = fetch_json(url)
    version = release["tag_name"]
    if release.get("prerelease") or not stable_version(version):
        raise ValueError("Backend release is not stable")
    asset = next(a for a in release["assets"] if a["name"] == (asset_name or "shellcheck-" + version + ".zip"))
    destination = current.serena / cache / (component + "-" + version)
    with installation_lock(state_dir, destination):
        if destination.exists():
            raise RuntimeError("Backend destination exists without a discoverable executable")
        raw = fetch(asset["browser_download_url"])
        digest = asset.get("digest", "")
        if not digest.startswith("sha256:") or hashlib.sha256(raw).hexdigest() != digest.split(":", 1)[1]:
            raise ValueError("Backend official archive digest is absent or mismatched")
        stage = Path(state_dir) / "staging" / (component + "-" + uuid.uuid4().hex)
        stage.mkdir(parents=True)
        safe_extract_zip(raw, stage)
        matches = list(stage.glob("**/" + pattern))
        if len(matches) != 1:
            raise ValueError("Backend archive did not contain exactly one expected executable")
        relative = matches[0].relative_to(stage)
        destination.parent.mkdir(parents=True, exist_ok=True)
        journal_path = activate_new_directory(stage, destination, state_dir, component)
        return {"state": "installed-unverified", "id": component, "version": version, "source": url,
                "digest": digest, "record": discovery.native_candidate(destination / relative, "serena-cache", version), "transaction_journal": journal_path}


def ensure_dotnet10(current, state_dir):
    dotnet = current.find_command("dotnet")
    if not dotnet:
        return {"state": "prerequisite-missing", "prerequisite": "Existing .NET installation for runtime-only addition"}
    before = subprocess.run([dotnet, "--list-runtimes"], capture_output=True, text=True, timeout=15).stdout
    sdks_before = subprocess.run([dotnet, "--list-sdks"], capture_output=True, text=True, timeout=15).stdout
    if "Microsoft.NETCore.App 10." in before:
        return {"state": "reused", "dotnet": dotnet, "runtimes": before.strip(), "sdks": sdks_before.strip()}
    url = "https://builds.dotnet.microsoft.com/dotnet/release-metadata/10.0/releases.json"
    metadata = fetch_json(url)
    release = next(r for r in metadata["releases"] if r["runtime"]["version"] == metadata["latest-runtime"])
    asset = next(f for f in release["runtime"]["files"] if f["rid"] == "win-x64" and f["name"].endswith(".exe"))
    raw = fetch(asset["url"])
    if hashlib.sha512(raw).hexdigest().lower() != asset["hash"].lower():
        raise ValueError("Official .NET runtime installer SHA512 mismatch")
    directory = Path(state_dir) / "downloads"
    directory.mkdir(parents=True, exist_ok=True)
    installer = directory / ("dotnet-runtime-" + metadata["latest-runtime"] + "-win-x64.exe")
    installer.write_bytes(raw)
    environment = dict(os.environ, HARNESS_DEPENDENCY_INSTALLER=str(installer))
    signature = subprocess.run([current.pwsh, "-NoProfile", "-NonInteractive", "-Command",
                                "$s = Get-AuthenticodeSignature -LiteralPath $env:HARNESS_DEPENDENCY_INSTALLER; [pscustomobject]@{Status=[string]$s.Status;Subject=$s.SignerCertificate.Subject} | ConvertTo-Json -Compress"],
                               capture_output=True, text=True, timeout=20, env=environment, creationflags=0x08000000)
    signer = json.loads(signature.stdout)
    if signer.get("Status") != "Valid" or "Microsoft Corporation" not in signer.get("Subject", ""):
        raise ValueError(".NET runtime installer does not have a valid Microsoft signature")
    with installation_lock(state_dir, Path(dotnet).parent / "shared/Microsoft.NETCore.App/10"):
        journal_path = transaction_path(state_dir, "dotnet-runtime")
        journal = {"schema_version": 1, "owner": "codex-harness-dependencies", "kind": "additive-runtime", "phase": "prepared",
                   "installation": str(Path(dotnet).parent), "installer": str(installer), "installer_sha512": asset["hash"],
                   "runtime_version": metadata["latest-runtime"], "runtimes_before": before.strip(), "sdks_before": sdks_before.strip(),
                   "transaction_id": TRANSACTION_ID, "inverse": "Manager-aware runtime-only inverse must preserve every prior SDK/runtime; no blind directory removal."}
        atomic_json(journal_path, journal)
        installed = subprocess.run([str(installer), "/install", "/quiet", "/norestart"], capture_output=True, text=True, timeout=240, creationflags=0x08000000)
        if installed.returncode not in (0, 3010):
            raise RuntimeError("Runtime-only .NET installation failed with exit " + str(installed.returncode))
        after = subprocess.run([dotnet, "--list-runtimes"], capture_output=True, text=True, timeout=15).stdout
        sdks_after = subprocess.run([dotnet, "--list-sdks"], capture_output=True, text=True, timeout=15).stdout
        if "Microsoft.NETCore.App 10." not in after or sdks_after != sdks_before or any(line not in after for line in before.splitlines()):
            raise RuntimeError(".NET runtime postcondition failed: preserve existing runtimes and all SDKs")
        journal.update(phase="committed", runtimes_after=after.strip(), sdks_after=sdks_after.strip())
        atomic_json(journal_path, journal)
        return {"state": "installed", "dotnet": dotnet, "version": metadata["latest-runtime"], "source": url,
                "signature": signer, "runtimes_before": before.strip(), "runtimes_after": after.strip(),
                "unchanged_sdks": sdks_after.strip(), "restart_requested": installed.returncode == 3010,
                "transaction_journal": str(journal_path), "rollback_state": "manager-aware-inverse-required"}


def provision_roslyn(user_home, state_dir):
    current = discovery.Discovery(user_home)
    record = next(x for x in current.run()["languages"] if x["id"] == "csharp")
    if record["status"] != "missing":
        return {"state": "reused" if record["status"] == "adopted" else "pending", "id": "csharp", "record": record}
    url = "https://api.nuget.org/v3-flatcontainer/roslyn-language-server.win-x64/index.json"
    versions = fetch_json(url)["versions"]
    version = max((v for v in versions if stable_version(v)), key=version_key)
    package_url = "https://api.nuget.org/v3-flatcontainer/roslyn-language-server.win-x64/" + version + "/roslyn-language-server.win-x64." + version + ".nupkg"
    raw = fetch(package_url)
    stage = Path(state_dir) / "staging" / ("roslyn-" + uuid.uuid4().hex)
    stage.mkdir(parents=True)
    safe_extract_zip(raw, stage)
    config_paths = list(stage.glob("**/Microsoft.CodeAnalysis.LanguageServer.runtimeconfig.json"))
    if len(config_paths) != 1:
        raise ValueError("Roslyn package runtime contract is ambiguous")
    runtime = json.loads(config_paths[0].read_text())["runtimeOptions"]
    if runtime["framework"]["name"] != "Microsoft.NETCore.App" or not runtime["framework"]["version"].startswith("10."):
        raise ValueError("Roslyn package requires an unverified runtime contract")
    runtime_result = ensure_dotnet10(current, state_dir)
    if runtime_result["state"] not in ("installed", "reused"):
        return {"state": "prerequisite-missing", "id": "csharp", "runtime": runtime_result}
    destination = current.serena / "CSharpLanguageServer" / ("roslyn-" + version)
    relative = config_paths[0].parent.relative_to(stage)
    with installation_lock(state_dir, destination):
        if destination.exists():
            raise RuntimeError("Roslyn destination exists but was not adopted")
        destination.parent.mkdir(parents=True, exist_ok=True)
        journal_path = activate_new_directory(stage, destination, state_dir, "csharp")
    executable = destination / relative / "Microsoft.CodeAnalysis.LanguageServer.exe"
    return {"state": "installed-unverified", "id": "csharp", "version": version, "source": package_url,
            "sha256": hashlib.sha256(raw).hexdigest(), "runtime": runtime_result,
            "record": discovery.native_candidate(executable, "serena-cache", version), "remaining": "Analyze a real SDK8 project without changing its toolchain.", "transaction_journal": journal_path}


def provision_nuphus_dictionary(user_home, state_dir):
    current = discovery.Discovery(user_home)
    root = discovery.canonical((os.environ.get("NUPHUS_MODELS_DIR") if current.environment else None) or current.home / "AppData/Roaming/Nuphus/models")
    target = root / "ch_PP-OCR_keys_v1.txt"
    if target.is_file():
        return {"state": "reused", "id": "nuphus-dictionary", "path": str(target), "sha256": discovery.fingerprint(target)}
    sources = ["https://gitee.com/paddlepaddle/PaddleOCR/raw/main/ppocr/utils/ppocr_keys_v1.txt",
               "https://raw.githubusercontent.com/PaddlePaddle/PaddleOCR/main/ppocr/utils/ppocr_keys_v1.txt"]
    failures = []
    with installation_lock(state_dir, target):
        for url in sources:
            try:
                raw = fetch(url, 1024 * 1024)
                if len(raw) < 10000 or len(raw.decode("utf-8").splitlines()) < 1000:
                    raise ValueError("PaddleOCR dictionary failed the upstream size/text requirement")
                root.mkdir(parents=True, exist_ok=True)
                candidate = Path(state_dir) / "staging" / ("nuphus-dictionary-" + uuid.uuid4().hex + ".txt")
                candidate.parent.mkdir(parents=True, exist_ok=True)
                with candidate.open("xb") as stream:
                    stream.write(raw)
                    stream.flush()
                    os.fsync(stream.fileno())
                journal = activate_new_file(candidate, target, state_dir, "nuphus-dictionary")
                return {"state": "installed-unverified", "id": "nuphus-dictionary", "path": str(target),
                        "source": url, "sha256": hashlib.sha256(raw).hexdigest(), "hash_kind": "observed artifact fingerprint; upstream does not predeclare a checksum",
                        "fallback_failures": failures, "transaction_journal": journal, "remaining": "Actual offline desktop_perceive using all three cached OCR artifacts."}
            except (OSError, ValueError) as error:
                failures.append({"source": url, "reason": f"{type(error).__name__}: {error}"})
    raise RuntimeError("Explicit PaddleOCR dictionary provisioning failed: " + json.dumps(failures))


def provision_nuphus_models(user_home, state_dir):
    """Explicit missing-model maintenance; ordinary MCP startup stays offline.

    Preserve every existing cache artifact. A missing artifact is staged from
    the official URL and must match bytes already proved compatible with the
    selected native OCR runtime before it can be activated.
    """
    current = discovery.Discovery(user_home)
    root = discovery.canonical((os.environ.get("NUPHUS_MODELS_DIR") if current.environment else None) or current.home / "AppData/Roaming/Nuphus/models")
    results = []
    for name, metadata in NUPHUS_MODELS.items():
        target = root / name
        with installation_lock(state_dir, target):
            if target.exists():
                if target.is_symlink() or not target.is_file() or target.stat().st_size < 1_000_000:
                    results.append({"state": "pending", "id": name, "path": str(target), "reason": "Existing OCR artifact is not an ordinary model of the upstream minimum size; preserve it for repair."})
                    continue
                actual = discovery.fingerprint(target)
                results.append({"state": "reused", "id": name, "path": str(target), "sha256": actual,
                                "compatibility": "matches previously exercised offline OCR model" if actual == metadata["sha256"] else "existing alternate artifact preserved; real native OCR validation required"})
                continue
            raw = fetch(metadata["url"])
            if len(raw) < 1_000_000 or hashlib.sha256(raw).hexdigest() != metadata["sha256"]:
                raise ValueError("Official OCR model differs from the previously exercised compatibility fingerprint; candidate is not activated")
            candidate = Path(state_dir) / "staging" / (name + "-" + uuid.uuid4().hex)
            candidate.parent.mkdir(parents=True, exist_ok=True)
            with candidate.open("xb") as stream:
                stream.write(raw)
                stream.flush()
                os.fsync(stream.fileno())
            root.mkdir(parents=True, exist_ok=True)
            journal = activate_new_file(candidate, target, state_dir, "nuphus-" + name)
            results.append({"state": "installed-unverified", "id": name, "path": str(target), "source": metadata["url"],
                            "sha256": metadata["sha256"], "hash_kind": "measured compatibility fingerprint from actual offline OCR; not an upstream declared checksum",
                            "transaction_journal": journal, "remaining": "Actual offline desktop_perceive with the selected native runtime."})
    dictionary = provision_nuphus_dictionary(user_home, state_dir)
    states = [item["state"] for item in results] + [dictionary["state"]]
    state = "pending" if "pending" in states else "reused" if all(value == "reused" for value in states) else "installed-unverified"
    return {"state": state, "id": "nuphus-ocr", "models": results, "dictionary": dictionary,
            "runtime_downloads": "disabled; provisioning is explicit only"}


def provision_fpc(user_home, state_dir):
    """Extract an official compiler driver and RTL resources for pasls CodeTools.

    Never run the installer, register an IDE, change PATH, or replace Delphi's SDK.
    The extractor is temporary lifecycle tooling, not a runtime dependency.
    """
    current = discovery.Discovery(user_home)
    discovered = next(x for x in current.run()["languages"] if x["id"] == "delphi")
    if discovered["paths"].get("pp") and discovered["paths"].get("fpcdir"):
        return {"state": "reused", "id": "delphi-fpc", "pp": discovered["paths"]["pp"], "fpcdir": discovered["paths"]["fpcdir"]}
    if discovered.get("fpc_candidates"):
        return {"state": "pending", "id": "delphi-fpc", "reason": "An existing FPC compiler needs an unambiguous matching source tree; preserve it instead of installing another compiler."}
    destination = current.serena / "PascalLanguageServer/prerequisites/fpc-3.2.2"
    executable = destination / "bin/i386-win32/fpc.exe"
    if executable.is_file() and (destination / "source/rtl/win32/system.pp").is_file():
        return {"state": "reused", "id": "delphi-fpc", "pp": str(executable), "fpcdir": str(destination / "source")}
    stage = Path(state_dir) / "staging/fpc-3.2.2-prerequisite"
    stage.mkdir(parents=True, exist_ok=True)
    urls = {"installer": "https://downloads.freepascal.org/fpc/dist/3.2.2/i386-win32/fpc-3.2.2.i386-win32.exe",
            "source": "https://downloads.freepascal.org/fpc/dist/3.2.2/source/fpc-3.2.2.source.zip",
            "extractor": "https://github.com/dscharrer/innoextract/releases/download/1.9/innoextract-1.9-windows.zip"}
    installer = stage / "fpc-3.2.2.i386-win32.exe"
    expected = "7ec78b1790ecac7685f440b17f9e03865bc09846b7c068a9270c4d37704b5ac8"
    if not installer.exists():
        installer.write_bytes(fetch(urls["installer"]))
    if discovery.fingerprint(installer) != expected:
        raise ValueError("FPC installer differs from the audited official HTTPS artifact fingerprint")
    extractor = stage / "innoextract/innoextract.exe"
    if not extractor.exists():
        safe_extract_zip(fetch(urls["extractor"]), extractor.parent)
    candidate = stage / ("candidate-" + uuid.uuid4().hex)
    candidate.mkdir()
    command = [str(extractor), "--silent", "--exclude-temp", "--include", str(Path("app/bin/i386-win32/fpc.exe")),
               "--include", str(Path("app/bin/i386-win32/ppc386.exe")), "--include", str(Path("app/units/i386-win32")),
               "--output-dir", str(candidate), str(installer)]
    extracted = subprocess.run(command, capture_output=True, text=True, timeout=180, creationflags=0x08000000)
    if extracted.returncode:
        raise RuntimeError("Official FPC archive extraction failed: " + extracted.stderr[-1500:])
    source_zip = stage / "fpc-3.2.2.source.zip"
    if not source_zip.exists():
        source_zip.write_bytes(fetch(urls["source"]))
    app = candidate / "app"
    with zipfile.ZipFile(source_zip) as archive:
        for entry in archive.infolist():
            parts = Path(entry.filename).parts
            if len(parts) > 1 and parts[1] in ("rtl", "compiler", "packages"):
                relative = Path(*parts[1:])
                target = app / "source" / relative
                if not discovery.contained(target, app / "source") or entry.external_attr >> 16 & 0o170000 == 0o120000:
                    raise ValueError("Unsafe FPC source archive path")
                if not entry.is_dir():
                    target.parent.mkdir(parents=True, exist_ok=True)
                    target.write_bytes(archive.read(entry))
    compiler = app / "bin/i386-win32/fpc.exe"
    evidence = {}
    for argument, expected_value in (("-iV", "3.2.2"), ("-iTO", "win32"), ("-iTP", "i386")):
        probe = subprocess.run([str(compiler), "-n", argument], capture_output=True, text=True, timeout=15, creationflags=0x08000000)
        if probe.returncode or probe.stdout.strip().lower() != expected_value:
            raise RuntimeError("FPC configuration-driver probe failed: " + argument + " " + probe.stderr[-1000:])
        evidence[argument] = probe.stdout.strip()
    if not (app / "source/rtl/win32/system.pp").is_file():
        raise RuntimeError("FPC RTL source tree is incomplete")
    (compiler.parent / "fpc.cfg").write_text('-Fu' + str(destination / 'units/i386-win32/*') + '\n', encoding="utf-8")
    with installation_lock(state_dir, destination):
        destination.parent.mkdir(parents=True, exist_ok=True)
        journal = activate_new_directory(app, destination, state_dir, "delphi-fpc")
    return {"state": "installed-unverified", "id": "delphi-fpc", "version": "3.2.2", "pp": str(executable),
            "fpcdir": str(destination / "source"), "sources": urls, "installer_sha256": expected,
            "source_sha256": discovery.fingerprint(source_zip), "hash_kind": "observed official HTTPS artifact fingerprints",
            "transaction_journal": journal, "evidence": evidence,
            "remaining": "Actual pasls Delphi dialect/navigation/diagnostic verification with the existing Delphi SDK."}


def load_lsp_provision():
    spec = importlib.util.spec_from_file_location("harness_lsp_provision", Path(__file__).with_name("lsp_provision.py"))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def apply_selected(user_home, state_dir, codex_home=None):
    inventory = discovery.Discovery(user_home, processes=True, probe_versions=True).run()
    requested = plan(discovery.read_json(discovery.CATALOGUE), inventory)
    results = []
    for item in requested["items"]:
        identifier = item["id"]
        try:
            if item["action"] == "conditional-absent":
                result = {"state": "conditional-absent", "id": identifier}
            elif identifier == "markdown" and item["action"] in ("install-required", "stage-compatible-update", "reuse"):
                result = provision_markdown(user_home, state_dir)
            elif identifier == "delphi" and item["action"] == "reuse":
                result = provision_fpc(user_home, state_dir)
            elif identifier == "bash" and item["action"] == "reuse":
                result = provision_github_zip("shellcheck", user_home, state_dir)
            elif identifier == "nuphus" and item["action"] == "reuse":
                result = provision_nuphus_models(user_home, state_dir)
            elif item["action"] == "reuse":
                result = {"state": "reused", "id": identifier, "version": item["installed_version"]}
            elif item["action"] == "update-pending-consumers":
                result = {"state": "retained-compatible", "id": identifier, "version": item["installed_version"], "available_version": item["release"]["version"],
                          "update_state": "pending-consumers", "reason": "Installed shared dependency retained while active consumers use it; operational compatibility must also pass the separate real consumer checks. The available update remains pending and no process is stopped."}
            elif identifier == "rust" and item["action"] == "stage-compatible-update":
                result = {"state": "retained-compatible", "id": identifier, "version": item["installed_version"], "available_cohort": item["release"]["version"],
                          "update_state": "held-toolchain-policy", "reason": "The adopted rustup component belongs to the installed compiler cohort. Updating that toolchain as a side effect would change project/compiler behavior; preserve it. Official component metadata itself uses placeholder 0.0.0."}
            elif identifier == "nuphus" and item["action"] == "preserve-and-audit":
                record = next(x for x in inventory["mcp"] if x["id"] == "nuphus")
                original = record["paths"].get("original_native_executable")
                if not original or record["status"] != "modified":
                    result = {"state": "pending", "id": identifier, "reason": "No unambiguous preserved original native Nuphus executable."}
                else:
                    audit = audit_npm_installation(record["provenance"]["platform_package"], record["provenance"]["platform_version"], Path(original).parent.parent)
                    if audit["state"] != "audited" or audit["differences"]:
                        result = {"state": "pending", "id": identifier, "reason": "Original Nuphus companion differs from its official package.", "audit": audit}
                    else:
                        ocr = provision_nuphus_models(user_home, state_dir)
                        result = {"state": "retained-compatible" if ocr["state"] == "reused" else ocr["state"], "id": identifier, "version": item["installed_version"],
                                  "selection": "Preserved official original executable through the repository schema proxy", "audit": audit, "dictionary": ocr["dictionary"], "models": ocr["models"],
                                  "reason": "Locally modified npm wrapper and patched binary are preserved. Only the byte-matching original native package is selected; real desktop/browser checks remain separately required."}
            elif identifier in NPM_LANGUAGES and item["action"] == "install-required":
                result = provision_npm(identifier, user_home, state_dir)
                if identifier == "bash":
                    result["analyzer"] = provision_github_zip("shellcheck", user_home, state_dir)
            elif identifier in ("codebase-memory", "nuphus") and item["action"] == "install-required":
                source_directory = str(Path(__file__).parent)
                if source_directory not in sys.path:
                    sys.path.insert(0, source_directory)
                spec = importlib.util.spec_from_file_location("harness_mcp_provision", Path(__file__).with_name("mcp_provision.py"))
                provider = importlib.util.module_from_spec(spec)
                spec.loader.exec_module(provider)
                provider.lifecycle = lifecycle_context()
                result = provider.provision(identifier, user_home, state_dir, item["release"]["version"])
                if identifier == "nuphus" and result["state"] in ("reused", "installed-unverified"):
                    ocr = provision_nuphus_models(user_home, state_dir)
                    result.update(dictionary=ocr["dictionary"], models=ocr["models"])
                    if ocr["state"] != "reused":
                        result["state"] = ocr["state"]
            elif identifier in ("rust", "typescript", "javascript", "powershell", "delphi", "cpp") and item["action"] == "install-required":
                result = load_lsp_provision().provision(identifier, user_home, state_dir, item["release"]["version"], lifecycle=lifecycle_context())
            elif identifier == "cmake" and item["action"] == "install-required":
                result = provision_cmake(user_home, state_dir)
            elif identifier == "toml" and item["action"] == "install-required":
                result = provision_taplo(user_home, state_dir)
            elif identifier == "xml" and item["action"] == "install-required":
                result = provision_github_zip("xml", user_home, state_dir)
            elif identifier == "csharp" and item["action"] == "install-required":
                result = provision_roslyn(user_home, state_dir)
            elif identifier == "codebase-memory" and item["action"] == "stage-compatible-update":
                staged = stage_codebase(item["release"]["version"], state_dir)
                result = promote_codebase(Path(staged["stage"]) / "stage.json", user_home, state_dir)
            elif identifier in ("typescript", "javascript") and item["action"] == "stage-compatible-update":
                result = update_typescript(user_home, state_dir, item["release"]["version"])
            elif identifier == "graphify" and item["action"] == "stage-compatible-update":
                spec = importlib.util.spec_from_file_location("harness_graphify_update", Path(__file__).with_name("graphify_update.py"))
                updater = importlib.util.module_from_spec(spec)
                spec.loader.exec_module(updater)
                updater.lifecycle.TRANSACTION_ID = TRANSACTION_ID
                staged = updater.stage(user_home, state_dir, item["release"]["version"], codex_home=codex_home)
                prepared = updater.prepare_manifest(staged["stage_manifest"])
                result = updater.promote(user_home, state_dir, staged["stage_manifest"], discovery.read_json(prepared["auxiliary_files"]))
            else:
                result = {"state": "pending", "id": identifier, "reason": item.get("reason") or "Required backend-specific provisioning/update and compatibility proof is not yet implemented.", "plan": item}
        except Exception as error:
            result = {"state": "failed", "id": identifier, "reason": f"{type(error).__name__}: {error}"}
        if result.get("id") and result["id"] != identifier:
            result["dependency_id"] = result["id"]
        result["id"] = identifier
        record_native_provisioning(result)
        results.append(result)
    return {"schema_version": 1, "operation": "apply", "results": results,
            "complete": all(r["state"] in ("reused", "updated", "conditional-absent", "retained-compatible") for r in results),
            "all_updates_applied": not any(r.get("update_state") or r["state"] in ("pending", "failed") for r in results),
            "note": "Installed-unverified records require real consumer/LSP checks; pending records remain unfinished."}


def record_native_provisioning(result):
    if result.get("state") != "installed-unverified":
        return
    path = result.get("record", {}).get("paths", {}).get("executable")
    if not path or not Path(path).is_file():
        return
    marker = Path(path).parent / ".harness-provisioning.json"
    existing = discovery.read_json(marker)
    if existing and existing.get("owner") != "codex-harness-dependencies":
        raise RuntimeError("A foreign dependency provenance marker must not be overwritten")
    record = {"owner": "codex-harness-dependencies", "id": result.get("id") or result.get("language"),
              "version": result.get("version"), "source": result.get("source"), "digest": result.get("digest") or result.get("checksum"),
              "executable_sha256": discovery.fingerprint(path), "verification": "installation provenance only; operations require real LSP evidence"}
    journal_path = result.get("transaction_journal")
    if journal_path:
        journal = discovery.read_json(journal_path)
        relative = marker.relative_to(Path(journal["installation"])).as_posix()
        marker_hash = hashlib.sha256(json.dumps(record, indent=2).encode("utf-8")).hexdigest()
        journal["installed_identity"] = tree_identity(journal["installation"], {relative: marker_hash})
        atomic_json(journal_path, journal)
    atomic_json(marker, record)


def main():
    global TRANSACTION_ID
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("operation", choices=["plan", "apply", "update", "recover", "stage-codebase", "promote-codebase", "provision-npm", "provision-markdown", "provision-marksman", "provision-cmake", "provision-xml", "provision-shellcheck", "provision-roslyn", "provision-nuphus-dictionary", "provision-nuphus-models", "provision-taplo", "provision-fpc", "update-typescript"])
    parser.add_argument("--user-home", default=str(Path.home()))
    parser.add_argument("--state-dir")
    parser.add_argument("--codex-home", help="Explicit Codex home for global Graphify selection when dependency state is elsewhere")
    parser.add_argument("--version")
    parser.add_argument("--manifest")
    parser.add_argument("--id")
    parser.add_argument("--transaction-id")
    parser.add_argument("--rollback-committed", action="store_true")
    parser.add_argument("--output")
    args = parser.parse_args()
    TRANSACTION_ID = args.transaction_id
    state = args.state_dir or str(Path(args.user_home) / ".codex/harness/dependencies")
    if args.operation == "plan":
        inventory = discovery.Discovery(args.user_home, processes=True, probe_versions=True).run()
        result = plan(discovery.read_json(discovery.CATALOGUE), inventory)
    elif args.operation == "recover":
        result = recover_dependencies(state, args.user_home, args.transaction_id, args.rollback_committed)
    elif args.operation in ("apply", "update"):
        result = apply_selected(args.user_home, state, args.codex_home)
    elif args.operation == "stage-codebase":
        result = stage_codebase(args.version, state)
    elif args.operation == "promote-codebase":
        result = promote_codebase(args.manifest, args.user_home, state)
    elif args.operation == "provision-npm":
        result = provision_npm(args.id, args.user_home, state, args.version)
    elif args.operation == "provision-marksman":
        result = provision_marksman(args.user_home, state)
    elif args.operation == "provision-markdown":
        result = provision_markdown(args.user_home, state)
    elif args.operation == "provision-cmake":
        result = provision_cmake(args.user_home, state)
    elif args.operation == "provision-roslyn":
        result = provision_roslyn(args.user_home, state)
    elif args.operation == "provision-nuphus-dictionary":
        result = provision_nuphus_dictionary(args.user_home, state)
    elif args.operation == "provision-nuphus-models":
        result = provision_nuphus_models(args.user_home, state)
    elif args.operation == "provision-taplo":
        result = provision_taplo(args.user_home, state)
    elif args.operation == "provision-fpc":
        result = provision_fpc(args.user_home, state)
    elif args.operation == "update-typescript":
        result = update_typescript(args.user_home, state, args.version or fetch_json("https://registry.npmjs.org/typescript-language-server/latest")["version"])
    else:
        result = provision_github_zip(args.operation.removeprefix("provision-"), args.user_home, state)
    record_native_provisioning(result)
    value = json.dumps(result, ensure_ascii=False, indent=2) + "\n"
    if args.output:
        Path(args.output).write_text(value, encoding="utf-8")
    else:
        print(value, end="")


if __name__ == "__main__":
    main()
