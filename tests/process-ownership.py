"""Bounded native checks: adopted-python tests/process-ownership.py [-v].

Only starts owned fixtures. Each readiness/communication/cleanup has a deadline;
sleeping fixtures self-exit after 20 seconds even if the driver fails. Cleanup
uses retained Windows process handles, never names or recycled PID lookups.
Memory tests request at most 96 MiB of bytearrays, under a 64 MiB job cap.
CPU saturation uses one thread for two seconds at a 1% host hard cap.
Evidence (including failures and source hashes) is retained in a unique temp
root, printed by the test driver. No live services/configuration are modified.
"""

from __future__ import annotations

import ctypes
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
from typing import Any
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from tools import process_ownership as ownership

SCRIPT = Path(__file__).resolve()
PYTHON = sys.executable
EVIDENCE: dict[str, Any] = {}
ROOT = Path(tempfile.mkdtemp(prefix="process-ownership-")) if "--fixture" not in sys.argv else Path(sys.argv[3])


def ready(path: Path, timeout: float = 6) -> Any:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            return json.loads(path.read_text(encoding="utf-8"))
        except (FileNotFoundError, json.JSONDecodeError):
            time.sleep(0.02)
    raise TimeoutError(f"Readiness deadline elapsed: {path}")


def receipt(path: Path, value: Any) -> None:
    path.write_text(json.dumps(value), encoding="utf-8")


def line_ready(stream: Any) -> str:
    import threading
    signal = threading.Event()
    result = []
    def read():
        result.append(stream.readline())
        signal.set()
    threading.Thread(target=read, daemon=True).start()
    if not signal.wait(6):
        raise TimeoutError("Fixture pipe readiness elapsed")
    return result[0]


def job_private_bytes(guard: ownership.JobGuard) -> dict[str, Any]:
    """Independent committed-memory observation with both fixtures paused.

    GetProcessMemoryInfo PrivateUsage is committed private memory. Enumerate
    only this owned job (bounded to 16 processes); retain handles while reading.
    PeakJobMemoryUsed can include denied commitment attempts on this host and
    is recorded separately, never substituted for granted committed memory.
    """
    class ProcessIds(ctypes.Structure):
        _fields_ = [("assigned", ctypes.c_uint32), ("count", ctypes.c_uint32),
                    ("pids", ctypes.c_size_t * 16)]
    class Counters(ctypes.Structure):
        _fields_ = [("cb", ctypes.c_uint32), ("faults", ctypes.c_uint32)] + [
            (name, ctypes.c_size_t) for name in (
                "peak_ws", "ws", "peak_paged", "paged", "peak_nonpaged",
                "nonpaged", "pagefile", "peak_pagefile", "private")]
    api = guard._api
    api.OpenProcess.argtypes = [ctypes.c_uint32, ctypes.c_int, ctypes.c_uint32]
    api.OpenProcess.restype = ctypes.c_void_p
    memory_api = ctypes.WinDLL("psapi", use_last_error=True)
    memory_api.GetProcessMemoryInfo.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_uint32]
    memory_api.GetProcessMemoryInfo.restype = ctypes.c_int
    ids = ProcessIds()
    if not api.QueryInformationJobObject(guard._handle, 3, ctypes.byref(ids), ctypes.sizeof(ids), None):
        raise ctypes.WinError(ctypes.get_last_error())
    if ids.assigned != ids.count or ids.count > 16:
        raise AssertionError("Owned job process inventory truncated")
    rows = []
    for pid in ids.pids[:ids.count]:
        handle = api.OpenProcess(0x410, False, pid)
        if not handle:
            raise ctypes.WinError(ctypes.get_last_error())
        try:
            counters = Counters()
            counters.cb = ctypes.sizeof(counters)
            if not memory_api.GetProcessMemoryInfo(handle, ctypes.byref(counters), ctypes.sizeof(counters)):
                raise ctypes.WinError(ctypes.get_last_error())
            rows.append({"pid": pid, "private_bytes": int(counters.private)})
        finally:
            api.CloseHandle(handle)
    return {"processes": rows, "committed_private_bytes": sum(row["private_bytes"] for row in rows)}


def available_commit_bytes() -> int:
    class MemoryStatus(ctypes.Structure):
        _fields_ = [("length", ctypes.c_uint32), ("load", ctypes.c_uint32)] + [
            (name, ctypes.c_uint64) for name in (
                "total_physical", "available_physical", "total_pagefile",
                "available_pagefile", "total_virtual", "available_virtual",
                "available_extended_virtual")]
    status = MemoryStatus()
    status.length = ctypes.sizeof(status)
    api = ctypes.WinDLL("kernel32", use_last_error=True)
    api.GlobalMemoryStatusEx.argtypes = [ctypes.POINTER(MemoryStatus)]
    api.GlobalMemoryStatusEx.restype = ctypes.c_int
    if not api.GlobalMemoryStatusEx(ctypes.byref(status)):
        raise ctypes.WinError(ctypes.get_last_error())
    return int(status.available_pagefile)


def fixture(role: str, root: Path) -> None:
    if role == "leaf":
        receipt(root / "leaf.json", {"pid": os.getpid()})
        time.sleep(20)
        return
    if role == "tree":
        subprocess.Popen([PYTHON, str(SCRIPT), "--fixture", "leaf", str(root)],
                         stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        leaf = ready(root / "leaf.json")
        receipt(root / "tree.json", {"child": os.getpid(), "grandchild": leaf["pid"]})
        time.sleep(20)
        return
    if role == "owner_atomic_pause":
        guard = ownership.JobGuard(memory_limit_bytes=128 * 1024 * 1024)

        def pause_before_resume(api, process):
            receipt(root / "suspended.json", {"owner": os.getpid(), "child": process.pid})
            time.sleep(20)
            raise RuntimeError("Fixture pre-resume deadline")

        ownership._resume_thread = pause_before_resume
        guard.popen([PYTHON, "-c", "import pathlib,sys; pathlib.Path(sys.argv[1]).write_text('executed')", str(root / "executed")],
                    stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        return
    guard = ownership.JobGuard(memory_limit_bytes=128 * 1024 * 1024, cpu_rate_percent=25)
    if role in ("owner_self", "owner_normal"):
        guard.contain_current_process()
        try:
            guard.close()
        except RuntimeError:
            pass
        else:
            raise AssertionError("Self-containing guard must reject early close")
        launch = subprocess.Popen
    else:
        launch = guard.popen
    launch([PYTHON, str(SCRIPT), "--fixture", "tree", str(root)],
           stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
           close_fds=False)
    tree = ready(root / "tree.json")
    receipt(root / "owner.json", {"owner": os.getpid(), **tree, "job": guard.snapshot()})
    if role == "owner_normal":
        ready(root / "stop.json")
        return
    time.sleep(20)


class RetainedProcess:
    """Open only PIDs supplied by a live owned fixture, then retain identity."""

    def __init__(self, pid: int):
        self.api = ctypes.WinDLL("kernel32", use_last_error=True)
        self.api.OpenProcess.argtypes = [ctypes.c_uint32, ctypes.c_int, ctypes.c_uint32]
        self.api.OpenProcess.restype = ctypes.c_void_p
        self.api.WaitForSingleObject.argtypes = [ctypes.c_void_p, ctypes.c_uint32]
        self.api.WaitForSingleObject.restype = ctypes.c_uint32
        self.api.TerminateProcess.argtypes = [ctypes.c_void_p, ctypes.c_uint32]
        self.api.TerminateProcess.restype = ctypes.c_int
        self.api.CloseHandle.argtypes = [ctypes.c_void_p]
        self.api.CloseHandle.restype = ctypes.c_int
        self.handle = self.api.OpenProcess(0x100001, False, pid)
        if not self.handle:
            raise ctypes.WinError(ctypes.get_last_error())
        if self.wait(0):
            self.close()
            raise AssertionError("Fixture process exited before identity capture")

    def wait(self, milliseconds: int) -> bool:
        result = self.api.WaitForSingleObject(self.handle, milliseconds)
        if result not in (0, 258):
            raise ctypes.WinError(ctypes.get_last_error())
        return result == 0

    def kill(self) -> None:
        if not self.wait(0) and not self.api.TerminateProcess(self.handle, 73):
            if ctypes.get_last_error() == 5 and self.wait(3000):
                return  # Windows has begun termination but not signaled yet.
            raise ctypes.WinError(ctypes.get_last_error())

    def close(self) -> None:
        if self.handle:
            self.api.CloseHandle(self.handle)
            self.handle = None


class PortableChecks(unittest.TestCase):
    def test_invalid_limits_fail_before_admission(self):
        invalid_memory: tuple[Any, ...] = (0, -1, True, 1.5, 1 << 100)
        for value in invalid_memory:
            with self.subTest(memory=value), self.assertRaises(ValueError):
                ownership.JobGuard(memory_limit_bytes=value)
        for value in (0, -1, True, float("nan"), float("inf"), 101):
            with self.subTest(cpu=value), self.assertRaises(ValueError):
                ownership.JobGuard(cpu_rate_percent=value)

    def test_unsupported_platform_preserves_popen_and_reports_limit(self):
        with mock.patch.object(ownership, "WINDOWS", False):
            with ownership.JobGuard(memory_limit_bytes=1024) as guard:
                self.assertFalse(guard.enabled)
                self.assertFalse(guard.snapshot()["enforced"])
                self.assertIn("unavailable", guard.snapshot()["reason"])
                with guard.popen([PYTHON, "-c", "print('fallback')"], stdout=subprocess.PIPE, stderr=subprocess.PIPE) as child:
                    output, errors = child.communicate(timeout=5)
                self.assertEqual((output, errors, child.returncode), (b"fallback\r\n" if os.name == "nt" else b"fallback\n", b"", 0))
            with self.assertRaises(RuntimeError):
                guard.popen([PYTHON, "-c", "pass"])


@unittest.skipUnless(os.name == "nt", "Native Job Objects require Windows")
class WindowsChecks(unittest.TestCase):
    def test_protocol_arguments_environment_and_streams(self):
        root = ROOT / "stdio space unicode ё"
        root.mkdir()
        code = "import json,os,sys; print(json.dumps([sys.argv[1:],os.getcwd(),os.environ['OWNERSHIP_TEST'],sys.stdin.read()],ensure_ascii=False)); print('diagnostic',file=sys.stderr)"
        arguments = ["space value", 'quote"value', "trailing\\", "ё"]
        with ownership.JobGuard(memory_limit_bytes=128 * 1024 * 1024, cpu_rate_percent=25) as guard:
            child = guard.popen([PYTHON, "-c", code, *arguments], cwd=root,
                                env={**os.environ, "OWNERSHIP_TEST": "ю", "PYTHONIOENCODING": "utf-8"},
                                stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                text=True, encoding="utf-8")
            output, errors = child.communicate("json-rpc\n", timeout=6)
            self.assertEqual(json.loads(output), [arguments, str(root), "ю", "json-rpc\n"])
            self.assertEqual(errors, "diagnostic\n")
            self.assertEqual(child.returncode, 0)
            snapshot = guard.snapshot()
            self.assertFalse(snapshot["handle_inheritable"])
            self.assertTrue(snapshot["kill_on_close"])
            self.assertTrue(snapshot["cpu_hard_cap"])
            self.assertEqual(snapshot["cpu_rate_percent"], 25)
            EVIDENCE["stdio_and_limits"] = snapshot
            with guard.popen('echo shell-ok', shell=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True) as shell:
                self.assertEqual(shell.communicate(timeout=5), ("shell-ok\n", None))
            with self.assertRaises(ValueError):
                guard.popen([PYTHON, "-c", "pass"], creationflags=0x01000000)

    def _owner_case(self, role: str, normal: bool = False):
        root = ROOT / role
        root.mkdir()
        retained: list[RetainedProcess] = []
        with (root / "stdout.log").open("wb") as stdout, (root / "stderr.log").open("wb") as stderr:
            owner = subprocess.Popen([PYTHON, str(SCRIPT), "--fixture", role, str(root)],
                                     stdin=subprocess.DEVNULL, stdout=stdout, stderr=stderr)
            try:
                state = ready(root / ("suspended.json" if role == "owner_atomic_pause" else "owner.json"))
                for name in ("owner", "child", "grandchild"):
                    if name in state:
                        retained.append(RetainedProcess(state[name]))
                if normal:
                    receipt(root / "stop.json", True)
                    self.assertEqual(owner.wait(timeout=5), 0)
                else:
                    retained[0].kill()
                self.assertTrue(all(process.wait(5000) for process in retained))
                if role == "owner_atomic_pause":
                    self.assertFalse((root / "executed").exists())
                else:
                    self.assertFalse(state["job"]["handle_inheritable"])
                owner.wait(timeout=5)
                EVIDENCE[role] = {"result": "owned tree signaled", "state": state, "owner_exit": owner.returncode}
            finally:
                for process in retained:
                    process.kill()
                    process.wait(3000)
                    process.close()
                if owner.poll() is None:
                    owner.kill()
                    owner.wait(timeout=5)
        self.assertEqual((root / "stdout.log").read_bytes(), b"")
        self.assertEqual((root / "stderr.log").read_bytes(), b"")

    def test_current_service_crash_kills_grandchild(self):
        self._owner_case("owner_self")

    def test_normal_service_exit_kills_grandchild(self):
        self._owner_case("owner_normal", normal=True)

    def test_worker_owner_crash_kills_grandchild(self):
        self._owner_case("owner_spawn")

    def test_owner_crash_before_resume_leaves_no_suspended_orphan(self):
        self._owner_case("owner_atomic_pause")

    def test_initialization_failure_reclaims_child(self):
        captured = []

        def refuse_resume(api, process):
            captured.append(RetainedProcess(process.pid))
            raise OSError("Injected resume failure")

        with ownership.JobGuard() as guard:
            with mock.patch.object(ownership, "_resume_thread", refuse_resume):
                with self.assertRaisesRegex(OSError, "Injected resume failure"):
                    guard.popen([PYTHON, "-c", "raise AssertionError('must never execute')"],
                                stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            try:
                self.assertEqual(len(captured), 1)
                self.assertTrue(captured[0].wait(3000))
                self.assertEqual(guard.snapshot()["active_processes"], 0)
                with self.assertRaises(OSError):
                    guard.popen([str(ROOT / "does-not-exist.exe")], stdout=subprocess.PIPE)
            finally:
                for process in captured:
                    process.kill()
                    process.close()

    def test_close_waits_for_grandchild_after_immediate_child_exits(self):
        root = ROOT / "close_after_child_exit"
        root.mkdir()
        code = "import subprocess,sys; subprocess.Popen([sys.executable,sys.argv[1],'--fixture','leaf',sys.argv[2]],stdin=subprocess.DEVNULL,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)"
        with ownership.JobGuard() as guard:
            child = guard.popen([PYTHON, "-c", code, str(SCRIPT), str(root)],
                                stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            leaf = RetainedProcess(ready(root / "leaf.json")["pid"])
            try:
                self.assertEqual(child.communicate(timeout=5), (b"", b""))
                self.assertEqual(child.returncode, 0)
                self.assertFalse(leaf.wait(0))
                start = time.monotonic()
                guard.close()
                self.assertTrue(leaf.wait(0), "close returned before descendant process exit")
                EVIDENCE["explicit_close"] = {"elapsed_seconds": time.monotonic() - start,
                                             "descendant_signaled_on_return": True}
            finally:
                leaf.kill()
                leaf.close()

    def test_aggregate_memory_denies_excess_and_preserves_other_job(self):
        limit = 64 * 1024 * 1024
        headroom = available_commit_bytes()
        EVIDENCE["memory_fixture_starting_commit_headroom_bytes"] = headroom
        if headroom < 512 * 1024 * 1024:
            self.skipTest("Host commit headroom below512MiB; cannot safely attribute1455 to job cap")
        hold = "import sys,time; blocks=[bytearray(1024*1024) for _ in range(24)]; print('ready',flush=True); sys.stdin.read(1)"
        allocate = """import ctypes,json,sys
api=ctypes.WinDLL('kernel32',use_last_error=True)
api.VirtualAlloc.argtypes=[ctypes.c_void_p,ctypes.c_size_t,ctypes.c_uint32,ctypes.c_uint32]
api.VirtualAlloc.restype=ctypes.c_void_p
blocks=[]
for _ in range(72):
    address=api.VirtualAlloc(None,1024*1024,0x3000,4)
    if not address:
        result={'denied':True,'allocated_mib':len(blocks),'error':ctypes.get_last_error()}
        break
    blocks.append(address)
else:
    result={'denied':False,'allocated_mib':len(blocks),'error':0}
print(json.dumps(result),flush=True)
sys.stdin.read(1)
"""
        with ownership.JobGuard(memory_limit_bytes=limit) as guard, ownership.JobGuard() as other_guard:
            first = guard.popen([PYTHON, "-u", "-c", hold], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
            self.addCleanup(first.communicate, timeout=5)
            other = other_guard.popen([PYTHON, "-c", "import sys; print(sys.stdin.read())"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
            self.addCleanup(other.communicate, timeout=5)
            self.assertEqual(line_ready(first.stdout), "ready\n")
            second = guard.popen([PYTHON, "-c", allocate], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
            self.addCleanup(second.communicate, timeout=5)
            result = json.loads(line_ready(second.stdout))
            snapshot = guard.snapshot()
            committed = job_private_bytes(guard)
            EVIDENCE["aggregate_memory"] = {"allocation": result, "job": snapshot, "committed": committed,
                                           "peak_limit_difference_bytes": snapshot["peak_job_memory_bytes"] - limit}
            self.assertTrue(result["denied"])
            self.assertEqual(result["error"], 1455)  # ERROR_COMMITMENT_LIMIT
            self.assertEqual(snapshot["memory_limit_bytes"], limit)
            self.assertLess(result["allocated_mib"], 40)
            self.assertGreaterEqual(len(committed["processes"]), 2)
            self.assertLessEqual(committed["committed_private_bytes"], limit)
            self.assertGreater(committed["committed_private_bytes"], 48 * 1024 * 1024)
            output, errors = second.communicate("x", timeout=5)
            self.assertEqual((second.returncode, errors, output), (0, "", ""))
            first.communicate("x", timeout=5)
            guard.close()
            self.assertIsNone(other.poll(), "Closing another job killed an unrelated fixture")
            self.assertEqual(other.communicate("separate", timeout=5), ("separate\n", ""))

    def test_cpu_hard_cap_exercised_by_bounded_work(self):
        code = "import json,time; start=time.process_time(); end=time.monotonic()+2\nwhile time.monotonic()<end: pass\nprint(json.dumps({'cpu_seconds':time.process_time()-start}))"
        with ownership.JobGuard(cpu_rate_percent=1) as guard:
            child = guard.popen([PYTHON, "-c", code], stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
            self.addCleanup(child.communicate, timeout=5)
            output, errors = child.communicate(timeout=15)
            self.assertEqual((child.returncode, errors), (0, ""))
            result = json.loads(output)
            snapshot = guard.snapshot()
            self.assertTrue(snapshot["cpu_hard_cap"])
            self.assertEqual(snapshot["cpu_rate_percent"], 1)
            # Account for 100 ms scheduling granularity, interpreter startup,
            # and small CPU-count variation. Very wide hosts may not constrain
            # one busy thread; retain measurements without claiming saturation.
            if (os.cpu_count() or 1) <= 16:
                self.assertLess(result["cpu_seconds"], 0.8)
            EVIDENCE["cpu"] = {**result, "logical_cpus": os.cpu_count(), "job": snapshot}


if __name__ == "__main__":
    if "--fixture" in sys.argv:
        fixture(sys.argv[2], Path(sys.argv[3]))
    else:
        program = unittest.main(exit=False)
        EVIDENCE.update(python=PYTHON, runtime=sys.version, root=str(ROOT),
                        success=program.result.wasSuccessful(),
                        skipped=[(str(test), reason) for test, reason in program.result.skipped],
                        failures=[(str(test), detail) for test, detail in program.result.failures + program.result.errors],
                        source_sha256={str(path): hashlib.sha256(path.read_bytes()).hexdigest() for path in (SCRIPT, SCRIPT.parents[1] / "tools" / "process_ownership.py")})
        receipt(ROOT / "report.json", EVIDENCE)
        print(f"Process ownership evidence: {ROOT / 'report.json'}")
        raise SystemExit(0 if program.result.wasSuccessful() else 1)
