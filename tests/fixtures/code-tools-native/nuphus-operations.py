"""Real source-proxy operations against owned off-screen window and local page."""
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import time
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from typing import Protocol, TypeGuard, cast, override, runtime_checkable


class MCPClient(Protocol):
    def call(self, method: str, params: dict[str, object], timeout: float = 30) -> dict[str, object]: ...
    def close(self) -> None: ...


@runtime_checkable
class ProbeModule(Protocol):
    def MCP(self, executable: list[str], environment: dict[str, str]) -> MCPClient: ...
    def load_object(self, text: str) -> dict[str, object]: ...
    def object_value(self, value: object) -> dict[str, object]: ...
    def object_array(self, value: object) -> list[dict[str, object]]: ...
    def string_value(self, value: object) -> str: ...
    def is_array(self, value: object) -> TypeGuard[list[object]]: ...

spec = importlib.util.spec_from_file_location("nuphus_probe", Path(__file__).with_name("nuphus-probe.py"))
if spec is None or spec.loader is None:
    raise ImportError("Cannot load the Nuphus probe fixture")
probe_module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(probe_module)
if not isinstance(probe_module, ProbeModule):
    raise TypeError("Nuphus probe fixture has an incompatible interface")
load_object = probe_module.load_object
object_value = probe_module.object_value
object_array = probe_module.object_array
string_value = probe_module.string_value
is_array = probe_module.is_array
registry, source, probe = (Path(arg).resolve() for arg in sys.argv[1:4])
inventory = load_object(registry.read_text(encoding="utf-8-sig"))
python = string_value(object_value(next(item for item in object_array(inventory["mcp"]) if item["id"] == "serena")["paths"])["python"])
probe.mkdir(exist_ok=False)
home = probe / "codex-home"
home.mkdir()
environment = {"HARNESS_CODE_TOOLS_REGISTRY": str(registry), "CODEX_HOME": str(home),
               "NUPHUS_MCP_BROWSER_CDP_URL": "", "NUPHUS_MCP_ALLOW_PRIVATE_NAV": "1"}
mcp: MCPClient | None = None
window = None
http = None
checks: list[str] = []


def check(condition: bool, label: str) -> None:
    assert condition, label
    checks.append(label)
    print("PASS " + label, flush=True)


def tool(name: str, arguments: dict[str, object]) -> str:
    if mcp is None:
        raise RuntimeError("Owned MCP client has not been started")
    result = mcp.call("tools/call", {"name": name, "arguments": arguments}, timeout=45)
    text = "\n".join(string_value(item.get("text", "")) for item in object_array(result.get("content", [])))
    if result.get("isError"):
        raise RuntimeError(f"{name}: {text}")
    return text


try:
    mcp = probe_module.MCP([python, "-B", "-u", str(source / "tools/code-tools/nuphus_proxy.py")], environment)
    schemas = mcp.call("tools/list", {})
    listed_tools = object_array(schemas["tools"])
    check(len(listed_tools) == 38, "source proxy exposes all 38 upstream tools")
    for item in listed_tools:
        if item["name"] in ("browser_click", "browser_type", "browser_drag_files"):
            check(all(branch["type"] == "object" for branch in object_array(object_value(item["inputSchema"])["anyOf"])), "native schema repair: " + string_value(item["name"]))
    check(not (home / "harness/runtime/nuphus").exists(), "MCP initialization and schema discovery do not launch a browser")
    state = probe / "window.json"
    stop = probe / "window.stop"
    command = next(item for item in object_array(inventory["languages"]) if item["id"] == "powershell")["command"]
    if not is_array(command) or not command:
        raise ValueError("PowerShell command is missing")
    pwsh = string_value(command[0])
    window = subprocess.Popen[bytes]([pwsh, "-NoLogo", "-NoProfile", "-File", str(Path(__file__).with_name("nuphus-window.ps1")),
                               "-StatePath", str(state), "-StopPath", str(stop)],
                              stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, creationflags=subprocess.CREATE_NO_WINDOW)
    for _ in range(100):
        if state.is_file():
            break
        if window.poll() is not None:
            assert window.stderr is not None, "Owned window stderr pipe was not created"
            stderr_bytes = cast(object, window.stderr.read())
            assert isinstance(stderr_bytes, bytes)
            raise RuntimeError(stderr_bytes.decode())
        time.sleep(.1)
    metadata = load_object(state.read_text())
    before = tool("desktop_window_info", {"hwnd": metadata["hwnd"]})
    check("Harness owned Nuphus fixture" in before, "desktop reads exact owned window handle and title")
    resized = tool("desktop_window_resize", {"hwnd": metadata["hwnd"], "width": 420, "height": 240, "confirm": True})
    after = tool("desktop_window_info", {"hwnd": metadata["hwnd"]})
    check(before != after and "420" in after and "240" in after, "bounded native desktop resize changes only the owned window")
    page = probe / "page.html"
    _ = page.write_text('<!doctype html><title>Harness owned page</title><h1>HARNESS OWNED PAGE</h1><label>Harness input<input id="entry"></label><button id="apply" onclick="document.getElementById(\'status\').textContent=document.getElementById(\'entry\').value">Apply owned change</button><p id="status">Before</p>', encoding="utf-8")
    class FixtureHandler(BaseHTTPRequestHandler):
        def do_GET(self) -> None:
            if self.path != "/":
                self.send_error(404)
                return
            body = page.read_bytes()
            self.send_response(200)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            _ = self.wfile.write(body)
        @override
        def log_message(self, format: str, *args: object) -> None:
            pass
    http = ThreadingHTTPServer(("127.0.0.1", 0), FixtureHandler)
    threading.Thread(target=http.serve_forever, daemon=True).start()
    _ = tool("browser_navigate", {"url": f"http://127.0.0.1:{http.server_port}/", "confirm": True})
    snapshot = tool("browser_snapshot", {})
    check("Harness input" in snapshot and "Apply owned change" in snapshot, "browser reads owned local page through native CDP")
    _ = tool("browser_type", {"selector": "#entry", "text": "OWNED_CHANGE_CONFIRMED", "confirm": True})
    _ = tool("browser_click", {"selector": "#apply", "confirm": True})
    value = tool("browser_evaluate", {"script": "document.getElementById('status').textContent", "confirm": True})
    check("OWNED_CHANGE_CONFIRMED" in value, "repaired browser_type and browser_click perform actual owned DOM change")
    image = probe / "owned-page.png"
    _ = tool("browser_screenshot", {"path": str(image), "confirm": True})
    check(image.is_file() and image.stat().st_size > 100, "native browser screenshot contains owned page artifact")
    perceive = mcp.call("tools/call", {"name": "desktop_perceive", "arguments": {"path": str(image)}}, timeout=45)
    _ = (probe / "perceive.json").write_text(json.dumps(perceive, indent=2), encoding="utf-8")
    print("OCR result: " + json.dumps(perceive)[:600], flush=True)
    if not perceive.get("isError"):
        ocr = load_object(string_value(object_array(perceive["content"])[0]["text"]))
        ocr_count = ocr.get("ocr_count", 0)
        check(isinstance(ocr_count, (int, float)) and ocr_count > 0 and all("center" in item for item in object_array(ocr["elements"])),
              "native OCR reuses existing models and returns actual owned-page element coordinates")
    _ = tool("browser_close", {"confirm": True})
    mcp.close()
    mcp = None
    check(not any((home / "harness/runtime/nuphus").iterdir()), "source proxy cleans its owned browser process/profile on shutdown")
    _ = (probe / "report.json").write_text(json.dumps({"checks": checks, "ocr_is_error": perceive.get("isError", False)}, indent=2), encoding="utf-8")
finally:
    if mcp:
        mcp.close()
    if window:
        (probe / "window.stop").touch()
        try:
            _ = window.wait(timeout=5)
        except subprocess.TimeoutExpired:
            window.terminate()
            _ = window.wait(timeout=5)
    if http:
        http.shutdown()
        http.server_close()
    print(f"Owned Nuphus evidence: {probe}", flush=True)
