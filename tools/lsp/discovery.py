"""Content discovery, independent of language-server input limits.

No metadata-only clearance: hash bytes in bounded chunks, including large files.
scandir supplies type information without resolving every ordinary Windows path.
"""
from __future__ import annotations

import hashlib
from collections import deque
from collections.abc import Iterable
from concurrent.futures import Future, ThreadPoolExecutor
import os
from pathlib import Path
import stat
import time

from backend import EXTENSIONS


IGNORED_DIRS = {".git", ".hg", ".svn", "node_modules", ".venv", "venv", "__pycache__", ".serena", ".codex", "target", "bin", "obj"}
CONFIG_NAMES = {"tsconfig.json", "jsconfig.json", "Cargo.toml", "Cargo.lock", "rust-toolchain", "rust-toolchain.toml",
                "pyproject.toml", "pyrightconfig.json", "compile_commands.json", "compile_flags.txt", "CMakeLists.txt",
                "PSScriptAnalyzerSettings.psd1", ".editorconfig", "package.json", "package-lock.json", "pnpm-lock.yaml",
                "yarn.lock", "bun.lock", ".marksman.toml", "taplo.toml", ".taplo.toml", ".clangd", ".shellcheckrc"}
CONFIG_SUFFIXES = {".dproj", ".csproj", ".props", ".targets", ".sln", ".json", ".jsonc", ".xsd", ".dtd"}
MAX_ANALYSIS_BYTES = 8 * 1024 * 1024
# Shared across concurrent checks in this process. A stalled filesystem cannot
# multiply reader threads on every hook; queued work is bounded per snapshot.
READERS = ThreadPoolExecutor(max_workers=4, thread_name_prefix="harness-scan")


def is_config(path: str) -> bool:
    name = os.path.basename(path)
    return name in CONFIG_NAMES or os.path.splitext(name)[1].lower() in CONFIG_SUFFIXES


def check_deadline(deadline: float) -> None:
    if time.monotonic() >= deadline:
        raise TimeoutError("File reconciliation exceeded its time budget")


def content_digest(path: str | os.PathLike[str], deadline: float) -> str:
    """A replacement or write during hashing is not an observed revision."""
    check_deadline(deadline)
    hashed = hashlib.sha256()
    with open(path, "rb") as source:
        before = os.fstat(source.fileno())
        while True:
            check_deadline(deadline)
            chunk = source.read(256 * 1024)
            if not chunk:
                break
            hashed.update(chunk)
        after = os.fstat(source.fileno())
    current = os.stat(path)
    def fields(value: os.stat_result) -> tuple[int, int, int, int]:
        return value.st_dev, value.st_ino, value.st_size, value.st_mtime_ns
    # Windows path stat and handle fstat disagree on legacy ctime (creation
    # versus change time). Compare ctime only between the two handle samples.
    if fields(before) != fields(after) or before.st_ctime_ns != after.st_ctime_ns or fields(after) != fields(current):
        raise OSError("Source changed during snapshot")
    check_deadline(deadline)
    return hashed.hexdigest()


def snapshot(root: Path, extra: Iterable[str | os.PathLike[str]] = (), budget: float = 5.0,
             *, configurations_only: bool = False) -> tuple[dict[str, str], list[str]]:
    root = root.resolve()
    deadline = time.monotonic() + max(0, budget)
    files: dict[str, str] = {}
    problems: list[str] = []
    candidates: dict[str, tuple[int, int]] = {}
    visited: set[str] = set()
    prefix = str(root) + os.sep

    def candidate(path: str | os.PathLike[str], *, explicit: bool = False,
                  info: os.stat_result | None = None) -> None:
        check_deadline(deadline)
        path = os.fspath(path)
        name = os.path.basename(path)
        if configurations_only and not is_config(name):
            return
        if os.path.splitext(name)[1].lower() not in EXTENSIONS and not is_config(name):
            return
        if explicit:
            actual = Path(path).resolve()
            if not actual.is_relative_to(root):
                return
            path = str(actual)
        if path in candidates:
            return
        try:
            info = info or os.stat(path)
            if stat.S_ISREG(info.st_mode):
                candidates[path] = (0 if explicit else 1, info.st_size)
        except FileNotFoundError:
            pass  # Explicit deleted paths are reconciled against the journal.
        except OSError as error:
            problems.append(f"Cannot inspect {os.path.relpath(path, root)}: {type(error).__name__}")

    def walk(folder: str) -> None:
        check_deadline(deadline)
        if folder in visited:
            return
        visited.add(folder)
        try:
            with os.scandir(folder) as entries:
                directories: list[str] = []
                for entry in entries:
                    check_deadline(deadline)
                    path = entry.path
                    # Junctions and symlinks may leave the root or form cycles.
                    if entry.is_symlink() or (hasattr(entry, "is_junction") and entry.is_junction()):
                        actual = Path(path).resolve()
                        if not actual.is_relative_to(root):
                            continue
                        path = str(actual)
                    if entry.is_dir():
                        if entry.name not in IGNORED_DIRS:
                            directories.append(path)
                    elif entry.is_file():
                        candidate(path, info=entry.stat())
                for directory in directories:
                    walk(directory)
        except TimeoutError:
            raise
        except OSError as error:
            problems.append(f"Cannot enumerate {os.path.relpath(folder, root)}: {type(error).__name__}")

    try:
        # Explicit targets remain useful even if enumeration of a huge tree fails.
        for path in extra:
            candidate(path, explicit=True)
        # Reserve part of the allowance for reading discovered sources.
        scan_deadline = deadline
        deadline = min(deadline, time.monotonic() + max(0, budget) * 0.4)
        try:
            walk(str(root))
        except TimeoutError as error:
            problems.append(str(error))
        finally:
            deadline = scan_deadline
        # Small inputs cannot be starved by one huge archive in directory order.
        ordered = iter(sorted(candidates, key=lambda name: (candidates[name], name)))
        pending: deque[tuple[str, Future[str]]] = deque()
        try:
            while True:
                check_deadline(deadline)
                while len(pending) < 8:
                    path = next(ordered, None)
                    if path is None:
                        break
                    pending.append((path, READERS.submit(content_digest, path, deadline)))
                if not pending:
                    break
                path, future = pending.popleft()
                try:
                    files[path[len(prefix):].replace(os.sep, "/")] = future.result(timeout=max(0, deadline - time.monotonic()))
                except TimeoutError:
                    raise TimeoutError("File reconciliation exceeded its time budget") from None
                except OSError as error:
                    problems.append(f"Cannot read {os.path.relpath(path, root)}: {error}")
        finally:
            for _, future in pending:
                _ = future.cancel()
        check_deadline(deadline)
    except TimeoutError as error:
        problems.append(str(error))
    return files, sorted(set(problems))
