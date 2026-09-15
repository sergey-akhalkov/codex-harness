"""Owned desktop screenshot contract through the source Nuphus proxy."""
import json
import os
from pathlib import Path
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(Path(__file__).resolve().parent))
import importlib.util

spec = importlib.util.spec_from_file_location("nuphus_probe", Path(__file__).with_name("nuphus-probe.py"))
probe_module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(probe_module)

registry, source, probe = (Path(arg).resolve() for arg in sys.argv[1:4])
probe.mkdir(exist_ok=False)
home = probe / "codex-home"
home.mkdir()
environment = {
    "HARNESS_CODE_TOOLS_REGISTRY": str(registry),
    "CODEX_HOME": str(home),
    "NUPHUS_MCP_BROWSER_CDP_URL": "",
}
mcp = None
window = None
report = {"checks": []}

def check(condition, label):
    assert condition, label
    report["checks"].append(label)
    print("PASS " + label, flush=True)

try:
    python = sys.executable
    mcp = probe_module.MCP([python, "-B", "-u", str(source / "tools/code-tools/nuphus_proxy.py")], environment)
    state = probe / "window.json"
    stop = probe / "window.stop"
    pwsh = r"C:\Program Files\PowerShell\7\pwsh.exe"
    window = subprocess.Popen(
        [pwsh, "-NoLogo", "-NoProfile", "-File", str(Path(__file__).with_name("nuphus-window.ps1")),
         "-StatePath", str(state), "-StopPath", str(stop)],
        stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, creationflags=subprocess.CREATE_NO_WINDOW,
    )
    for _ in range(100):
        if state.is_file():
            break
        if window.poll() is not None:
            raise RuntimeError(window.stderr.read().decode())
        time.sleep(0.1)
    metadata = json.loads(state.read_text(encoding="utf-8"))
    hwnd = metadata["hwnd"]
    path_shot = probe / "owned-window.png"
    path_result = mcp.call("tools/call", {"name": "desktop_window_screenshot", "arguments": {"hwnd": hwnd, "path": str(path_shot)}}, timeout=45)
    path_items = path_result.get("content") or []
    path_text = "\n".join(item.get("text", "") for item in path_items if isinstance(item, dict))
    path_types = [item.get("type") for item in path_items if isinstance(item, dict)]
    check(not path_result.get("isError") and path_shot.is_file() and path_shot.stat().st_size > 100, "path screenshot writes owned file")
    check("text" in path_types and "image" not in path_types and "iVBORw0KGgo" not in path_text and str(path_shot) in path_text, "path screenshot stays path-only")
    visual = mcp.call("tools/call", {"name": "desktop_window_screenshot", "arguments": {"hwnd": hwnd}}, timeout=45)
    visual_items = visual.get("content") or []
    visual_types = [item.get("type") for item in visual_items if isinstance(item, dict)]
    visual_text = "\n".join(item.get("text", "") for item in visual_items if isinstance(item, dict))
    check(not visual.get("isError") and "image" in visual_types and "text" not in visual_types and "iVBORw0KGgo" not in visual_text, "no-path screenshot is a native image block")
    (probe / "report.json").write_text(json.dumps(report, indent=2), encoding="utf-8")
    (probe / "path-result.json").write_text(json.dumps({k: path_result[k] for k in path_result if k != "content"} | {"types": path_types, "text": path_text[:400]}, indent=2), encoding="utf-8")
    (probe / "visual-result.json").write_text(json.dumps({"isError": visual.get("isError"), "types": visual_types, "text": visual_text[:200]}, indent=2), encoding="utf-8")
finally:
    if mcp:
        mcp.close()
    if window:
        stop.touch()
        try:
            window.wait(timeout=5)
        except subprocess.TimeoutExpired:
            window.terminate()
            window.wait(timeout=5)
    print(f"Owned screenshot evidence: {probe}", flush=True)
