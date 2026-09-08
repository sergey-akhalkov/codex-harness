"""Windows process-tree ownership for dedicated tool services and workers.

Use ``JobGuard(...).contain_current_process()`` once, in a dedicated service,
BEFORE starting its children. Never attach Codex, the user's shell, or a thin
client that starts an independently owned shared daemon. The guard is retained
until process exit; closing a self-containing job would terminate the service.

For a supervised worker, retain ``with JobGuard(...) as guard`` around
``guard.popen(...)`` and all use of its children. Closing the guard kills the
whole tree even if the immediate child has already exited. ``popen`` preserves
Popen streams/communication and admits children atomically through the Windows
10+ JOB_LIST startup attribute, then resumes the suspended primary thread.

JobGuard uses no background monitors, named jobs, inherited job handles, process
name searches, or stdout/stderr writes. Windows errors propagate; unsupported
platforms preserve ordinary Popen behavior and explicitly report enabled=False.
Memory limits bound aggregate committed memory, not just resident memory.
CPU percentage is a hard job rate (relative to an enclosing rate-limited job,
if any), not a percentage of one logical core.

Shared brokers use spawn_service to outlive their first thin client's Job. Its
temporary startup watchdog and exit-handle waiter do not alter worker ownership.

Contracts: https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects
https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-updateprocthreadattribute
"""

from __future__ import annotations

import ctypes
import json
import math
import os
from pathlib import Path
import subprocess
import sys
import threading
import time
from typing import Any

WINDOWS = os.name == "nt"
_SELF_GUARDS: list[JobGuard] = []
_DWORD = ctypes.c_uint32
_HANDLE = ctypes.c_void_p
_SIZE_T = ctypes.c_size_t
_KILL_ON_CLOSE = 0x2000
_JOB_MEMORY = 0x200
_CPU_HARD_CAP = 0x5
_CREATE_SUSPENDED = 0x4
_CREATE_BREAKAWAY = 0x01000000


class _BasicLimit(ctypes.Structure):
    _fields_ = [
        ("process_time", ctypes.c_int64), ("job_time", ctypes.c_int64),
        ("flags", _DWORD), ("min_working_set", _SIZE_T),
        ("max_working_set", _SIZE_T), ("active_limit", _DWORD),
        ("affinity", _SIZE_T), ("priority", _DWORD), ("scheduling", _DWORD),
    ]


class _ExtendedLimit(ctypes.Structure):
    _fields_ = [
        ("basic", _BasicLimit), ("io", ctypes.c_uint64 * 6),
        ("process_memory", _SIZE_T), ("job_memory", _SIZE_T),
        ("peak_process_memory", _SIZE_T), ("peak_job_memory", _SIZE_T),
    ]


class _CpuRate(ctypes.Structure):
    _fields_ = [("flags", _DWORD), ("rate", _DWORD)]


class _Accounting(ctypes.Structure):
    _fields_ = [
        ("user_time", ctypes.c_int64), ("kernel_time", ctypes.c_int64),
        ("period_user_time", ctypes.c_int64), ("period_kernel_time", ctypes.c_int64),
        ("page_faults", _DWORD), ("total_processes", _DWORD),
        ("active_processes", _DWORD), ("terminated_processes", _DWORD),
    ]


class _StartupInfo(ctypes.Structure):
    _fields_ = [
        ("cb", _DWORD), ("reserved", ctypes.c_wchar_p),
        ("desktop", ctypes.c_wchar_p), ("title", ctypes.c_wchar_p),
        ("x", _DWORD), ("y", _DWORD), ("x_size", _DWORD), ("y_size", _DWORD),
        ("x_chars", _DWORD), ("y_chars", _DWORD), ("fill", _DWORD),
        ("flags", _DWORD), ("show_window", ctypes.c_uint16),
        ("reserved_size", ctypes.c_uint16), ("reserved_ptr", _HANDLE),
        ("stdin", _HANDLE), ("stdout", _HANDLE), ("stderr", _HANDLE),
    ]


class _StartupInfoEx(ctypes.Structure):
    _fields_ = [("startup", _StartupInfo), ("attributes", _HANDLE)]


class _ProcessInformation(ctypes.Structure):
    _fields_ = [
        ("process", _HANDLE), ("thread", _HANDLE),
        ("pid", _DWORD), ("tid", _DWORD),
    ]


def _check(success: Any, operation: str) -> None:
    if not success:
        code = ctypes.get_last_error()
        raise ctypes.WinError(code, f"{operation}: {ctypes.FormatError(code)}")


def _windows_api() -> Any:
    api = ctypes.WinDLL("kernel32", use_last_error=True)
    contracts = {
        "CreateJobObjectW": ([_HANDLE, ctypes.c_wchar_p], _HANDLE),
        "SetInformationJobObject": ([_HANDLE, ctypes.c_int, _HANDLE, _DWORD], ctypes.c_int),
        "QueryInformationJobObject": ([_HANDLE, ctypes.c_int, _HANDLE, _DWORD, _HANDLE], ctypes.c_int),
        "AssignProcessToJobObject": ([_HANDLE, _HANDLE], ctypes.c_int),
        "GetCurrentProcess": ([], _HANDLE),
        "OpenProcess": ([_DWORD, ctypes.c_int, _DWORD], _HANDLE),
        "GetExitCodeProcess": ([_HANDLE, ctypes.POINTER(_DWORD)], ctypes.c_int),
        "GetProcessTimes": ([_HANDLE, _HANDLE, _HANDLE, _HANDLE, _HANDLE], ctypes.c_int),
        "QueryFullProcessImageNameW": ([_HANDLE, _DWORD, ctypes.c_wchar_p, ctypes.POINTER(_DWORD)], ctypes.c_int),
        "GetSystemDirectoryW": ([ctypes.c_wchar_p, _DWORD], _DWORD),
        "SetStdHandle": ([_DWORD, _HANDLE], ctypes.c_int),
        "LocalFree": ([_HANDLE], _HANDLE),
        "IsProcessInJob": ([_HANDLE, _HANDLE, ctypes.POINTER(ctypes.c_int)], ctypes.c_int),
        "GetHandleInformation": ([_HANDLE, ctypes.POINTER(_DWORD)], ctypes.c_int),
        "SetHandleInformation": ([_HANDLE, _DWORD, _DWORD], ctypes.c_int),
        "CloseHandle": ([_HANDLE], ctypes.c_int),
        "TerminateProcess": ([_HANDLE, _DWORD], ctypes.c_int),
        "TerminateJobObject": ([_HANDLE, _DWORD], ctypes.c_int),
        "WaitForSingleObject": ([_HANDLE, _DWORD], _DWORD),
        "ResumeThread": ([_HANDLE], _DWORD),
        "InitializeProcThreadAttributeList": ([_HANDLE, _DWORD, _DWORD, ctypes.POINTER(_SIZE_T)], ctypes.c_int),
        "UpdateProcThreadAttribute": ([_HANDLE, _DWORD, _SIZE_T, _HANDLE, _SIZE_T, _HANDLE, _HANDLE], ctypes.c_int),
        "DeleteProcThreadAttributeList": ([_HANDLE], None),
        "CreateProcessW": ([ctypes.c_wchar_p, ctypes.c_wchar_p, _HANDLE, _HANDLE,
                            ctypes.c_int, _DWORD, _HANDLE, ctypes.c_wchar_p,
                            ctypes.POINTER(_StartupInfoEx), ctypes.POINTER(_ProcessInformation)], ctypes.c_int),
    }
    for name, (arguments, result) in contracts.items():
        function = getattr(api, name)
        function.argtypes = arguments
        function.restype = result
    return api


def _resume_thread(api: Any, process: _ProcessInformation) -> None:
    previous = api.ResumeThread(process.thread)
    if previous == 0xFFFFFFFF:
        _check(False, "Resume owned process")
    if previous != 1:
        raise RuntimeError(f"Owned process had unexpected suspend count {previous}")


def _environment_block(env: Any) -> Any:
    if env is None:
        return None
    entries = {}
    for key, value in env.items():
        key, value = os.fsdecode(key), os.fsdecode(value)
        if not key or "=" in key[1:] or "\0" in key or "\0" in value:
            raise ValueError("Invalid child environment entry")
        # Windows environment keys are case-insensitive, as in _winapi.
        entries.setdefault(key.upper(), (key, value))
    content = "\0".join(f"{key}={value}" for key, value in sorted(entries.values(), key=lambda item: item[0].upper()))
    return ctypes.create_unicode_buffer(content + "\0")


class JobGuard:
    """Own a Windows job; on other platforms ``enabled`` is False.

    Windows creation/admission errors fail closed. No enforcement fallback is
    attempted on Windows. Current-process containment is permanent and cannot
    be undone; ``close`` therefore rejects self-containing guards.
    """

    def __init__(self, memory_limit_bytes: int | None = None, cpu_rate_percent: float | None = None):
        if memory_limit_bytes is not None and (
            type(memory_limit_bytes) is not int or not 0 < memory_limit_bytes <= _SIZE_T(-1).value
        ):
            raise ValueError("memory_limit_bytes must be a positive size_t integer")
        if cpu_rate_percent is not None and (
            type(cpu_rate_percent) not in (int, float)
            or not math.isfinite(cpu_rate_percent)
            or not 0.01 <= cpu_rate_percent <= 100
        ):
            raise ValueError("cpu_rate_percent must be finite and between 0.01 and 100")
        self.enabled = WINDOWS
        self.memory_limit_bytes = memory_limit_bytes
        self.cpu_rate_percent = cpu_rate_percent
        self._lock = threading.RLock()
        self._handle: int | None = None
        self._closed = False
        self._contains_current = False
        self._api: Any = _windows_api() if self.enabled else None
        if not self.enabled:
            return
        self._handle = self._api.CreateJobObjectW(None, None)
        _check(self._handle, "Create process ownership job")
        try:
            _check(self._api.SetHandleInformation(self._handle, 1, 0), "Make job handle non-inheritable")
            limits = _ExtendedLimit()
            limits.basic.flags = _KILL_ON_CLOSE | (_JOB_MEMORY if memory_limit_bytes else 0)
            limits.job_memory = memory_limit_bytes or 0
            self._set_information(9, limits)
            if cpu_rate_percent is not None:
                self._set_information(15, _CpuRate(_CPU_HARD_CAP, math.floor(cpu_rate_percent * 100)))
        except BaseException:
            self.close()
            raise

    def _set_information(self, kind: int, value: Any) -> None:
        _check(self._api.SetInformationJobObject(self._handle, kind, ctypes.byref(value), ctypes.sizeof(value)), "Set ownership job limits")

    def _require_open(self) -> None:
        if self._closed:
            raise RuntimeError("Process ownership job is closed")

    def contain_current_process(self) -> JobGuard:
        """Attach this dedicated service before it creates descendants.

        Pins the guard until OS process exit, including if the caller drops its
        reference. Do not use in a root CLI, user shell, or shared-service client.
        """
        with self._lock:
            self._require_open()
            if self.enabled and not self._contains_current:
                _check(self._api.AssignProcessToJobObject(self._handle, self._api.GetCurrentProcess()), "Contain dedicated service")
                self._contains_current = True
                _SELF_GUARDS.append(self)
            return self

    def popen(self, args: Any, **kwargs: Any) -> subprocess.Popen[Any]:
        """Return a Popen-compatible child already admitted to this job.

        Uses CPython's Windows Popen pipe/communication machinery. Explicit
        CREATE_SUSPENDED and CREATE_BREAKAWAY_FROM_JOB are rejected: this API
        owns resume and must not break away from an enclosing owner.
        """
        with self._lock:
            self._require_open()
            if not self.enabled:
                return subprocess.Popen(args, **kwargs)
            if kwargs.get("creationflags", 0) & (_CREATE_SUSPENDED | _CREATE_BREAKAWAY):
                raise ValueError("Owned children cannot request suspension or job breakaway")
            return _OwnedPopen(self, args, **kwargs)

    def snapshot(self) -> dict[str, Any]:
        """Read actual OS limits/usage without printing to protocol streams."""
        with self._lock:
            self._require_open()
            result: dict[str, Any] = {"enforced": self.enabled, "memory_limit_bytes": self.memory_limit_bytes,
                      "cpu_rate_percent": self.cpu_rate_percent, "contains_current_process": self._contains_current}
            if not self.enabled:
                result["reason"] = "Windows Job Object enforcement is unavailable on this platform"
                return result
            limits, cpu, accounting, flags = _ExtendedLimit(), _CpuRate(), _Accounting(), _DWORD()
            for kind, value in ((9, limits), (15, cpu), (1, accounting)):
                _check(self._api.QueryInformationJobObject(self._handle, kind, ctypes.byref(value), ctypes.sizeof(value), None), "Query ownership job")
            _check(self._api.GetHandleInformation(self._handle, ctypes.byref(flags)), "Query job handle flags")
            result.update(memory_limit_bytes=int(limits.job_memory) if limits.basic.flags & _JOB_MEMORY else None,
                          kill_on_close=bool(limits.basic.flags & _KILL_ON_CLOSE),
                          handle_inheritable=bool(flags.value & 1),
                          cpu_rate_percent=cpu.rate / 100 if cpu.flags & 1 else None,
                          cpu_hard_cap=bool(cpu.flags & 4),
                          peak_job_memory_bytes=int(limits.peak_job_memory),
                          active_processes=int(accounting.active_processes))
            return result

    def close(self) -> None:
        """Terminate a child-only job and await tree exit for at most 5 seconds.

        Always closes the native handle, including if accounting/termination
        fails. A timeout is explicit; callers must retain recoverable temporary
        state rather than assume all children have released their files.
        """
        with self._lock:
            if self._closed:
                return
            if self._contains_current:
                raise RuntimeError("Current-process job must remain open until OS process exit")
            if self._handle:
                process_handles = []
                try:
                    deadline = time.monotonic() + 5
                    # Block descendant admission before the inventory. A limit
                    # of one preserves existing members but prevents any live
                    # member from creating another process during shutdown.
                    limits = _ExtendedLimit()
                    _check(self._api.QueryInformationJobObject(self._handle, 9, ctypes.byref(limits), ctypes.sizeof(limits), None), "Read closing job limits")
                    limits.basic.flags |= 8
                    limits.basic.active_limit = 1
                    self._set_information(9, limits)
                    process_handles = self._retain_process_handles()
                    _check(self._api.TerminateJobObject(self._handle, 1), "Terminate owned job tree")
                    for handle in process_handles:
                        remaining = max(0, math.ceil((deadline - time.monotonic()) * 1000))
                        status = self._api.WaitForSingleObject(handle, remaining)
                        if status == 258:
                            raise TimeoutError("Owned process did not finish termination within 5 seconds")
                        _check(status == 0, "Await owned process handle")
                    accounting = _Accounting()
                    while True:
                        _check(self._api.QueryInformationJobObject(self._handle, 1, ctypes.byref(accounting), ctypes.sizeof(accounting), None), "Await owned job tree exit")
                        if not accounting.active_processes:
                            break
                        if time.monotonic() >= deadline:
                            raise TimeoutError("Owned process tree did not finish termination within 5 seconds")
                        time.sleep(0.01)
                finally:
                    for handle in process_handles:
                        self._api.CloseHandle(handle)
                    _check(self._api.CloseHandle(self._handle), "Close ownership job")
                    self._handle = None
                    self._closed = True
            self._closed = True

    def _retain_process_handles(self) -> list[int]:
        """Capture job members without trusting a possibly recycled PID."""
        capacity = 16
        while capacity <= 4096:
            class ProcessIds(ctypes.Structure):
                _fields_ = [("assigned", _DWORD), ("count", _DWORD), ("pids", _SIZE_T * capacity)]
            members = ProcessIds()
            success = self._api.QueryInformationJobObject(self._handle, 3, ctypes.byref(members), ctypes.sizeof(members), None)
            if success:
                break
            if ctypes.get_last_error() != 234:  # ERROR_MORE_DATA
                _check(False, "Inventory owned process handles")
            capacity = max(capacity * 2, members.assigned)
        else:
            raise RuntimeError("Owned job process inventory exceeded 4096 members")
        handles = []
        try:
            for pid in members.pids[:members.count]:
                handle = self._api.OpenProcess(0x101000, False, pid)  # synchronize + query-limited
                if not handle and ctypes.get_last_error() == 87:
                    continue  # Member already finished exiting.
                _check(handle, "Retain owned process handle")
                included = ctypes.c_int()
                try:
                    _check(self._api.IsProcessInJob(handle, self._handle, ctypes.byref(included)), "Verify retained process ownership")
                    if included.value:
                        handles.append(handle)
                        handle = None
                finally:
                    if handle:
                        self._api.CloseHandle(handle)
            return handles
        except BaseException:
            for handle in handles:
                self._api.CloseHandle(handle)
            raise

    def __enter__(self) -> JobGuard:
        self._require_open()
        return self

    def __exit__(self, *_: Any) -> None:
        self.close()

    def __del__(self) -> None:
        # Self-containing guards must survive Python finalization too. Windows
        # closes their handles at process exit, after Python's exit handlers.
        if getattr(self, "_contains_current", False):
            return
        try:
            self.close()
        except Exception:
            pass


class _OwnedPopen(subprocess.Popen[Any]):
    """Replace only Windows child creation; keep Popen's public stream API."""

    def __init__(self, guard: JobGuard, args: Any, **kwargs: Any):
        self._guard = guard
        self._handle: Any = None
        self._child_created = False
        try:
            super().__init__(args, **kwargs)
        except BaseException:
            # Cover failures in Popen initialization after child creation too.
            if getattr(self, "_child_created", False):
                self.kill()
                self.wait(timeout=5)
            raise

    def _execute_child(self, args, executable, preexec_fn, close_fds, pass_fds,
                       cwd, env, startupinfo, creationflags, shell,
                       p2cread, p2cwrite, c2pread, c2pwrite, errread, errwrite,
                       *unused):
        # Popen rejects unsupported Windows preexec_fn/pass_fds before this
        # hook. Its private hook is exercised against the adopted interpreter.
        if preexec_fn is not None or pass_fds:
            raise ValueError("preexec_fn/pass_fds are unsupported on Windows")
        if not isinstance(args, str):
            if isinstance(args, (bytes, os.PathLike)):
                if shell:
                    raise TypeError("shell arguments must not be bytes or path-like")
                args = [args]
            args = subprocess.list2cmdline(args)
        executable = os.fsdecode(executable) if executable is not None else None
        cwd = os.fsdecode(cwd) if cwd is not None else None
        info = subprocess.STARTUPINFO() if startupinfo is None else startupinfo.copy()
        use_stdio = -1 not in (p2cread, c2pwrite, errwrite)
        if use_stdio:
            info.dwFlags |= 0x100
            info.hStdInput, info.hStdOutput, info.hStdError = p2cread, c2pwrite, errwrite
        attributes = info.lpAttributeList or {}
        if set(attributes) - {"handle_list"}:
            raise ValueError("Unsupported STARTUPINFO attributes for owned process")
        handles = list(attributes.get("handle_list") or [])
        if handles or (use_stdio and close_fds):
            if use_stdio:
                handles += [int(p2cread), int(c2pwrite), int(errwrite)]
            handles = getattr(self, "_filter_handle_list")(handles)
            if handles:
                close_fds = False
        if self._guard._handle in handles:
            raise ValueError("Ownership job handle must never be inherited")
        if shell:
            info.dwFlags |= 1
            info.wShowWindow = 0
            comspec = executable or os.environ.get("ComSpec") or os.path.join(os.environ.get("SystemRoot", ""), "System32", "cmd.exe")
            if not os.path.isabs(comspec):
                raise FileNotFoundError("An absolute Windows shell path is required")
            executable, args = comspec, f'{comspec} /c "{args}"'
        if "\0" in args or (executable and "\0" in executable) or (cwd and "\0" in cwd):
            raise ValueError("Embedded NUL in process arguments")
        sys.audit("subprocess.Popen", executable, args, cwd, env)
        try:
            self._create_owned(executable, args, cwd, env, info, creationflags, not close_fds, handles)
        finally:
            getattr(self, "_close_pipe_fds")(p2cread, p2cwrite, c2pread, c2pwrite, errread, errwrite)

    def _create_owned(self, executable, command, cwd, env, info, flags, inherit, handles):
        api = self._guard._api
        values = [(0x2000D, (_HANDLE * 1)(self._guard._handle))]
        if handles:
            values.append((0x20002, (_HANDLE * len(handles))(*handles)))
        size = _SIZE_T()
        api.InitializeProcThreadAttributeList(None, len(values), 0, ctypes.byref(size))
        if not size.value:
            _check(False, "Size process ownership attributes")
        storage = ctypes.create_string_buffer(size.value)
        _check(api.InitializeProcThreadAttributeList(storage, len(values), 0, ctypes.byref(size)), "Initialize process ownership attributes")
        process = _ProcessInformation()
        try:
            for kind, value in values:
                _check(api.UpdateProcThreadAttribute(storage, 0, kind, ctypes.byref(value), ctypes.sizeof(value), None, None), "Set atomic process ownership attribute")
            startup = _StartupInfoEx()
            startup.startup.cb = ctypes.sizeof(startup)
            startup.startup.flags, startup.startup.show_window = info.dwFlags, info.wShowWindow
            startup.startup.stdin = int(info.hStdInput) if info.hStdInput is not None else None
            startup.startup.stdout = int(info.hStdOutput) if info.hStdOutput is not None else None
            startup.startup.stderr = int(info.hStdError) if info.hStdError is not None else None
            startup.attributes = ctypes.cast(storage, _HANDLE)
            command_buffer = ctypes.create_unicode_buffer(command)
            environment = _environment_block(env)
            _check(api.CreateProcessW(executable, command_buffer, None, None, inherit,
                                      flags | _CREATE_SUSPENDED | 0x80000 | 0x400,
                                      environment, cwd, ctypes.byref(startup), ctypes.byref(process)), "Create atomically owned suspended process")
            # The process already belongs to the job, even if this owner dies
            # here. Retain its native process handle for Popen before resume.
            self._handle = getattr(subprocess, "Handle")(process.process)
            self.pid = process.pid
            self._child_created = True
            _resume_thread(api, process)
        except BaseException:
            if process.process:
                api.TerminateProcess(process.process, 1)
                api.WaitForSingleObject(process.process, 5000)
                if not getattr(self, "_child_created", False):
                    api.CloseHandle(process.process)
            raise
        finally:
            if process.thread:
                api.CloseHandle(process.thread)
            api.DeleteProcThreadAttributeList(storage)


_SERVICE_READY = threading.Event()


def mark_service_ready() -> None:
    """Called by a service only after publishing its authenticated endpoint."""
    _SERVICE_READY.set()


def _current_user_sid() -> str:
    api = _windows_api()
    security = ctypes.WinDLL('advapi32', use_last_error=True)
    security.OpenProcessToken.argtypes = [_HANDLE, _DWORD, ctypes.POINTER(_HANDLE)]
    security.OpenProcessToken.restype = ctypes.c_int
    security.GetTokenInformation.argtypes = [_HANDLE, ctypes.c_int, _HANDLE, _DWORD, ctypes.POINTER(_DWORD)]
    security.GetTokenInformation.restype = ctypes.c_int
    security.ConvertSidToStringSidW.argtypes = [_HANDLE, ctypes.POINTER(ctypes.c_wchar_p)]
    security.ConvertSidToStringSidW.restype = ctypes.c_int
    token = _HANDLE()
    _check(security.OpenProcessToken(api.GetCurrentProcess(), 8, ctypes.byref(token)), 'Read service owner token')
    try:
        size = _DWORD()
        security.GetTokenInformation(token, 1, None, 0, ctypes.byref(size))
        if not 0 < size.value <= 65536:
            raise RuntimeError('Invalid service owner token size')
        data = ctypes.create_string_buffer(size.value)
        _check(security.GetTokenInformation(token, 1, data, size, ctypes.byref(size)), 'Read service owner SID')
        sid = ctypes.c_wchar_p()
        _check(security.ConvertSidToStringSidW(_HANDLE.from_buffer(data), ctypes.byref(sid)), 'Format service owner SID')
        try:
            return sid.value
        finally:
            api.LocalFree(ctypes.cast(sid, _HANDLE))
    finally:
        api.CloseHandle(token)


class _ServiceProcess:
    """A retained, verified WMI-created process handle; never a termination API."""
    def __init__(self, pid: int, executable: str, started: float):
        self.pid, self.returncode = pid, None
        self._finished = threading.Event()
        api = _windows_api()
        handle = api.OpenProcess(0x101000, False, pid)
        _check(handle, 'Open independently owned service')
        try:
            created, exited, kernel, user = (ctypes.c_uint64() for _ in range(4))
            _check(api.GetProcessTimes(handle, ctypes.byref(created), ctypes.byref(exited),
                                      ctypes.byref(kernel), ctypes.byref(user)), 'Verify service creation time')
            image = ctypes.create_unicode_buffer(32768)
            size = _DWORD(len(image))
            _check(api.QueryFullProcessImageNameW(handle, 0, image, ctypes.byref(size)), 'Verify service executable')
            if (created.value / 10_000_000 - 11644473600 < started - 1
                    or Path(image.value).resolve() != Path(executable).resolve()):
                raise RuntimeError('Independent service process identity differs; preserving the process')
        except BaseException:
            api.CloseHandle(handle)
            raise
        def observe():
            try:
                _check(api.WaitForSingleObject(handle, 0xFFFFFFFF) == 0, 'Wait for independent service')
                code = _DWORD()
                _check(api.GetExitCodeProcess(handle, ctypes.byref(code)), 'Read service exit status')
                self.returncode = code.value
            finally:
                api.CloseHandle(handle)
                self._finished.set()
        threading.Thread(target=observe, name='shared-service-exit', daemon=True).start()

    def poll(self):
        return self.returncode if self._finished.is_set() else None

    def wait(self, timeout=None):
        if not self._finished.wait(timeout):
            raise subprocess.TimeoutExpired('independent shared service', timeout)
        return self.returncode


_CREATE_SERVICE = r'''
$ErrorActionPreference = 'Stop'
[Console]::InputEncoding = [Text.UTF8Encoding]::new($false)
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
try {
    $request = [Console]::In.ReadToEnd() | ConvertFrom-Json
    $startup = New-CimInstance -ClassName Win32_ProcessStartup -ClientOnly -Property @{
        ShowWindow = [uint16]0
        CreateFlags = [uint32]0x09000400
        EnvironmentVariables = [string[]]$request.Environment
    }
    $result = Invoke-CimMethod -ClassName Win32_Process -MethodName Create -Arguments @{
        CommandLine = [string]$request.CommandLine
        CurrentDirectory = [string]$request.CurrentDirectory
        ProcessStartupInformation = $startup
    }
    if ($result.ReturnValue -ne 0) { throw "Win32_Process.Create status $($result.ReturnValue)" }
    [Console]::Out.WriteLine(($result | Select-Object ProcessId, ReturnValue | ConvertTo-Json -Compress))
} catch {
    [Console]::Error.WriteLine('Independent service creation failed: ' + $_.FullyQualifiedErrorId)
    exit 1
}
'''


def spawn_service(script, *, arguments=(), env=None, cwd, log_path, startup_timeout=30):
    """Start one Python service outside a thin client's Windows Job lifetime.

    Local WMI creation is explicitly outside the caller's Job; BREAKAWAY also
    avoids the provider host's quota Job. The helper itself is atomically owned.
    No credentials/environment are put in command lines or handoff files. The
    service joins its own Job before running code and must mark_service_ready
    within the finite startup deadline. The caller supplies a private runtime
    directory and retains its existing startup/lifetime locks and identity RPC.
    https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects
    https://learn.microsoft.com/en-us/windows/win32/cimwin32prov/win32-processstartup
    """
    script, cwd, log_path = Path(script).resolve(), Path(cwd).resolve(), Path(log_path).resolve()
    if not script.is_file() or not cwd.is_dir() or log_path.parent != cwd:
        raise ValueError('Service requires an existing script and private runtime log directory')
    if not 0 < startup_timeout <= 120:
        raise ValueError('Service startup timeout must be within 120 seconds')
    environment = dict(os.environ if env is None else env)
    if any(not isinstance(key, str) or not isinstance(value, str) or '=' in key or '\0' in key + value
           for key, value in environment.items()):
        raise ValueError('Invalid service environment')
    began = time.time()
    command = [sys.executable, '-B', '-u', str(Path(__file__).resolve()), '--service',
               str(log_path), str(began + startup_timeout), _current_user_sid() if WINDOWS else '-',
               str(script), *map(str, arguments)]
    if not WINDOWS:
        with log_path.open('ab') as log:
            process = subprocess.Popen(command, stdin=subprocess.DEVNULL, stdout=log, stderr=log,
                                       env=environment, cwd=cwd, start_new_session=True, close_fds=True)
        threading.Thread(target=process.wait, daemon=True).start()
        return process
    api = _windows_api()
    system = ctypes.create_unicode_buffer(32768)
    _check(api.GetSystemDirectoryW(system, len(system)), 'Locate native Windows service launcher')
    powershell = Path(system.value) / 'WindowsPowerShell/v1.0/powershell.exe'
    request = json.dumps({'CommandLine': subprocess.list2cmdline(command), 'CurrentDirectory': str(cwd),
                          'Environment': [key + '=' + value for key, value in sorted(environment.items())]})
    if len(request.encode('utf-8')) > 1024 * 1024:
        raise ValueError('Service startup request exceeds 1 MiB')
    with JobGuard() as guard:
        helper = guard.popen([str(powershell), '-NoLogo', '-NoProfile', '-NonInteractive', '-Command', _CREATE_SERVICE],
                             stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                             text=True, encoding='utf-8', creationflags=subprocess.CREATE_NO_WINDOW)
        try:
            output, error = helper.communicate(request, timeout=startup_timeout)
        finally:
            guard.close()
            helper.wait(timeout=5)
            for stream in (helper.stdin, helper.stdout, helper.stderr):
                stream.close()
        if helper.returncode:
            raise RuntimeError(error[-1000:] or 'Independent Windows service creation failed')
    receipt = json.loads(output)
    return _ServiceProcess(int(receipt['ProcessId']), sys.executable, began)


def _service_entry():
    import runpy
    # run_path executes in this interpreter; no resident bootstrap interpreter.
    sys.modules['process_ownership'] = sys.modules[__name__]
    sys.modules['tools.process_ownership'] = sys.modules[__name__]
    log_path, deadline, owner, script, *arguments = sys.argv[2:]
    JobGuard().contain_current_process()
    if WINDOWS and _current_user_sid() != owner:
        raise RuntimeError('Independent service did not start as the requesting Windows user')
    # WMI need not supply CRT standard descriptors. Reserve them before opening
    # the log, so dup2 cannot overwrite one of its own source descriptors.
    for descriptor in range(3):
        try:
            os.fstat(descriptor)
        except OSError:
            os.open(os.devnull, os.O_RDWR)
    log = open(log_path, 'a', encoding='utf-8', buffering=1)
    incoming = open(os.devnull, 'r', encoding='utf-8')
    sys.stdin, sys.stdout, sys.stderr = incoming, log, log
    for stream, descriptor in ((incoming, 0), (log, 1), (log, 2)):
        os.dup2(stream.fileno(), descriptor)
    if WINDOWS:
        import msvcrt
        api = _windows_api()
        for descriptor, handle_id in ((0, -10), (1, -11), (2, -12)):
            _check(api.SetStdHandle(handle_id & 0xFFFFFFFF, msvcrt.get_osfhandle(descriptor)), 'Set service log handle')
    def startup_watchdog():
        if not _SERVICE_READY.wait(max(0, float(deadline) - time.time())):
            print('Shared service startup deadline elapsed before endpoint publication', file=sys.stderr, flush=True)
            os._exit(124)  # OS closes our Job handle and reclaims the complete tree.
    threading.Thread(target=startup_watchdog, name='service-startup-deadline', daemon=True).start()
    sys.argv = [script, *arguments]
    sys.path.insert(0, str(Path(script).resolve().parent))
    runpy.run_path(script, run_name='__main__')


if __name__ == '__main__' and len(sys.argv) > 1 and sys.argv[1] == '--service':
    _service_entry()
