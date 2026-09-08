"""Deterministic native-shaped process fixture. No remote calls or global writes."""
import ctypes
import json
from pathlib import Path
import re
import sys
import time


mode = sys.argv[1]
if mode == "oracle":
    value = json.loads(Path(sys.argv[-1]).read_text(encoding="utf-8"))
    expected = Path("input.txt").read_text(encoding="utf-8")
    good = (len(value["findings"]) == 1 and value["findings"][0]["path"] == "input.txt"
            and value["findings"][0]["line"] == 1 and value["findings"][0]["evidence"] == expected)
    raise SystemExit(0 if good else 1)

argv = sys.argv[2:]
final = Path(argv[argv.index("--output-last-message") + 1])
prompt = sys.stdin.buffer.read().decode("utf-8")
(final.parent / "captured.json").write_text(json.dumps({"argv": argv, "stdin": prompt}), encoding="utf-8")
identity = re.search(r"run_id exactly ([0-9a-f]{32})", prompt)
assert identity is not None
run_id = identity.group(1)
value = {"run_id": run_id, "findings": [{"path": "input.txt", "line": 1,
         "description": "Inspected input", "evidence": Path("input.txt").read_text(encoding="utf-8")}], "unresolved_issues": []}
if mode == "auth":
    print("authentication failed: 401 Unauthorized", file=sys.stderr)
    raise SystemExit(1)
if mode == "process":
    print("fixture process failure", file=sys.stderr)
    raise SystemExit(7)
if mode == "missing":
    print(json.dumps({"type": "turn.completed"}))
    raise SystemExit(0)
if mode == "wrong":
    value["findings"][0]["evidence"] = "wrong answer"
if mode == "stale":
    value["run_id"] = "previous-run"
if mode == "schema":
    value["findings"][0]["line"] = True
if mode == "unresolved":
    value["unresolved_issues"] = ["inspection incomplete"]
final.write_text("{" if mode == "malformed" else json.dumps(value), encoding="utf-8")
if mode == "timeout":
    print(json.dumps({"type": "turn.completed"}), flush=True)
    time.sleep(30)
if mode == "terminated":
    print(json.dumps({"type": "turn.completed"}), flush=True)
    api = ctypes.WinDLL("kernel32", use_last_error=True)
    api.GetCurrentProcess.restype = ctypes.c_void_p
    api.TerminateProcess.argtypes = [ctypes.c_void_p, ctypes.c_uint32]
    api.TerminateProcess.restype = ctypes.c_int
    if not api.TerminateProcess(api.GetCurrentProcess(), 0xC000013A):
        raise ctypes.WinError(ctypes.get_last_error())
    raise RuntimeError("Forced termination unexpectedly returned")
if mode == "output":
    print("x" * 8192, flush=True)
if mode == "final-limit":
    final.write_text("x" * 8192, encoding="utf-8")
    time.sleep(30)
if mode == "changed":
    Path("input.txt").write_text("changed", encoding="utf-8")
if mode == "events":
    print("{")
elif mode == "incomplete":
    print(json.dumps({"type": "turn.started"}))
elif mode == "task-failure":
    print(json.dumps({"type": "turn.failed", "error": {"message": "task failed"}}))
else:
    print(json.dumps({"type": "turn.completed"}))
