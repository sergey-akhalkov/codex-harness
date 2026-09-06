"""Reuse original Nuphus, adapt six schema fields and isolate its lazy browser."""
from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import sys
import threading
import uuid
from urllib.request import urlopen

sys.dont_write_bytecode = True

import anyio
from mcp import ClientSession, StdioServerParameters
from mcp.client.stdio import stdio_client
from mcp.server.lowlevel import Server
from mcp.server.stdio import stdio_server

# Official npm @nuphus/nuphus-mcp-win32-x64@0.2.2 payload, tarball integrity
# checked independently. The locally patched binary is preserved and not run.
AUDITED_ORIGINALS = {"0.2.2": "9a07112f17a964d9c0b1a54653af95559d7de33cce1cb3dffd60dfc4c85ccfb0"}
SCHEMA_TOOLS = {"browser_click", "browser_type", "browser_drag_files"}


def adapt_schema(tool):
    if tool.name in SCHEMA_TOOLS:
        schema = tool.inputSchema
        branches = schema.get("anyOf")
        if schema.get("type") != "object" or not isinstance(branches, list) or len(branches) != 2:
            raise ValueError(f"Unreviewed Nuphus schema: {tool.name}")
        for branch, key in zip(branches, ("selector", "ref")):
            if branch.get("required") != [key] or set(branch) - {"required", "type"} or branch.get("type", "object") != "object":
                raise ValueError(f"Unreviewed Nuphus alternative schema: {tool.name}")
            branch["type"] = "object"
    return tool


def find_browser():
    candidates = []
    for base in (os.environ.get("PROGRAMFILES"), os.environ.get("PROGRAMFILES(X86)"), os.environ.get("LOCALAPPDATA")):
        if base:
            for suffix in ("Google/Chrome/Application/chrome.exe", "Microsoft/Edge/Application/msedge.exe", "Chromium/Application/chrome.exe"):
                candidates.append(Path(base) / suffix)
    for name in ("chrome", "chromium", "google-chrome", "msedge"):
        if found := shutil.which(name):
            candidates.append(Path(found))
    return next((str(path.resolve()) for path in candidates if path.is_file()), None)


class OwnedBrowser:
    """Reserve a private endpoint; spawn an existing browser only on first use."""
    def __init__(self):
        self.lock = threading.RLock()
        self.process = None
        self.directory = None
        self.directory_parent = None
        self.directory_resolved = None
        self.reservation = socket.socket()
        self.reservation.bind(("127.0.0.1", 0))
        self.port = self.reservation.getsockname()[1]
        self.endpoint = f"http://127.0.0.1:{self.port}"

    def ensure_started(self):
        with self.lock:
            self._ensure_started()

    def _ensure_started(self):
        if self.process and self.process.poll() is None:
            return
        if self.process:
            raise RuntimeError("Owned browser exited; start a new MCP session. No shared-browser fallback is allowed.")
        executable = find_browser()
        if not executable:
            raise RuntimeError("An existing Chrome/Chromium/Edge installation is required. Runtime browser installation is disabled.")
        runtime = Path(os.environ.get("CODEX_HOME", str(Path.home() / ".codex"))) / "harness/runtime/nuphus"
        runtime.mkdir(parents=True, exist_ok=True)
        self.directory = runtime / f"{os.getpid()}_{uuid.uuid4().hex}"
        self.directory.mkdir()
        self.directory = Path(os.path.abspath(self.directory))
        self.directory_parent = self.directory.parent.resolve()
        self.directory_resolved = self.directory.resolve()
        self.reservation.close()
        self.process = subprocess.Popen(
            [executable, "--headless=new", "--do-not-de-elevate", f"--remote-debugging-port={self.port}", "--remote-debugging-address=127.0.0.1",
             f"--user-data-dir={self.directory / 'browser'}", "--no-first-run", "--no-default-browser-check",
             "--disable-background-networking", "--disable-component-update", "about:blank"],
            stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True, encoding="utf-8", errors="replace",
            creationflags=subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0)
        # Confirm the endpoint announced by this exact owned process, rather
        # than accepting any listener that won a race for the released port.
        import queue
        announced = queue.Queue()
        startup_errors = []
        def read_stderr():
            for line in self.process.stderr:
                startup_errors.append(line.rstrip())
                del startup_errors[:-5]
                if "DevTools listening on " in line:
                    announced.put(line.split("DevTools listening on ", 1)[1].strip())
        threading.Thread(target=read_stderr, daemon=True).start()
        try:
            expected = announced.get(timeout=15)
            with urlopen(self.endpoint + "/json/version", timeout=5) as response:
                actual = json.load(response)["webSocketDebuggerUrl"]
            if actual != expected:
                raise RuntimeError("Owned browser endpoint identity mismatch")
        except Exception as error:
            detail = str(error) or type(error).__name__
            if startup_errors:
                detail += "; " + " | ".join(startup_errors)[-700:]
            self.close()
            raise RuntimeError(f"Owned browser did not become ready ({detail}); shared-browser fallback is disabled") from error

    def close(self):
        with self.lock:
            self._close()

    def _close(self):
        self.reservation.close()
        if self.process and self.process.poll() is None:
            if os.name == "nt":
                # The open process handle confirms this PID still belongs to
                # our browser. Terminate only that owned tree, never by name.
                subprocess.run(["taskkill", "/PID", str(self.process.pid), "/T", "/F"],
                               stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=10,
                               creationflags=subprocess.CREATE_NO_WINDOW)
            else:
                self.process.terminate()
            self.process.wait(timeout=10)
        if self.directory:
            # The directory was created by this process. Refuse link traversal;
            # locked files after a forced exit remain for lifecycle recovery.
            def remove_owned(path):
                absolute = Path(os.path.abspath(path))
                if not absolute.is_relative_to(self.directory):
                    raise OSError("cleanup target escaped the exact owned directory")
                if self.directory.parent.resolve() != self.directory_parent:
                    raise OSError("cleanup parent identity changed")
                parent = absolute.parent.resolve()
                if absolute != self.directory and not parent.is_relative_to(self.directory_resolved):
                    raise OSError("cleanup child parent escaped the owned directory")
                if path.is_symlink() or path.is_junction():
                    path.rmdir() if path.is_dir() else path.unlink()
                elif path.is_dir():
                    if not absolute.resolve().is_relative_to(self.directory_resolved):
                        raise OSError("cleanup resolved directory escaped ownership")
                    for child in path.iterdir():
                        remove_owned(child)
                    path.rmdir()
                elif path.exists():
                    path.unlink()
            try:
                remove_owned(self.directory)
            except OSError as error:
                print(f"Nuphus cleanup pending: {self.directory} ({error})", file=sys.stderr)


async def main():
    registry = json.loads(Path(os.environ["HARNESS_CODE_TOOLS_REGISTRY"]).read_text(encoding="utf-8-sig"))
    record = next(item for item in registry["mcp"] if item["id"] == "nuphus")
    executable = Path(record["paths"].get("original_native_executable") or record["paths"]["native_executable"])
    expected = AUDITED_ORIGINALS.get(record.get("version"))
    if not expected or hashlib.sha256(executable.read_bytes()).hexdigest() != expected:
        raise RuntimeError("Nuphus original executable has not passed the supported package integrity audit")
    environment = {**os.environ, "NUPHUS_MCP_NO_MODEL_DOWNLOAD": "1"}
    owned = None if environment.get("NUPHUS_MCP_BROWSER_CDP_URL", "").strip() else OwnedBrowser()
    if owned:
        environment["NUPHUS_MCP_BROWSER_CDP_URL"] = owned.endpoint
    try:
        async with stdio_client(StdioServerParameters(command=str(executable), env=environment)) as upstream:
            async with ClientSession(*upstream) as remote:
                with anyio.fail_after(15):
                    await remote.initialize()
                server = Server("harness-nuphus")

                @server.list_tools()
                async def list_tools():
                    return [adapt_schema(tool) for tool in (await remote.list_tools()).tools]

                @server.call_tool()
                async def call_tool(name, arguments):
                    if owned and name.startswith("browser_"):
                        await anyio.to_thread.run_sync(owned.ensure_started)
                    return await remote.call_tool(name, arguments)

                async with stdio_server() as client:
                    await server.run(*client, server.create_initialization_options())
    finally:
        if owned:
            owned.close()


if __name__ == "__main__":
    try:
        anyio.run(main)
    except Exception as error:
        print(f"Nuphus source adapter failed: {error}", file=sys.stderr)
        raise SystemExit(1)
