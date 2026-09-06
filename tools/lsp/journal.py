"""Independent, bounded pre-edit baseline and completion fallback (stdlib only)."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import sqlite3
import subprocess
import sys
import time
import uuid

from backend import EXTENSIONS, digest, runtime_home


IGNORED_DIRS = {".git", ".hg", ".svn", "node_modules", ".venv", "venv", "__pycache__", ".serena", ".codex", "target", "bin", "obj"}
CONFIG_NAMES = {"tsconfig.json", "jsconfig.json", "Cargo.toml", "Cargo.lock", "rust-toolchain", "rust-toolchain.toml",
                "pyproject.toml", "pyrightconfig.json", "compile_commands.json", "compile_flags.txt", "CMakeLists.txt",
                "PSScriptAnalyzerSettings.psd1", ".editorconfig", "package.json", "package-lock.json", "pnpm-lock.yaml",
                "yarn.lock", "bun.lock", ".marksman.toml", "taplo.toml", ".taplo.toml", ".clangd", ".shellcheckrc"}
# Arbitrarily named JSON files may supply schemas or imported project data;
# XSD/DTD documents can affect unchanged XML instances. Their changes therefore
# conservatively invalidate this root's analysis under the same finite budget.
CONFIG_SUFFIXES = {".dproj", ".csproj", ".props", ".targets", ".sln", ".json", ".jsonc", ".xsd", ".dtd"}


def identity(event: dict) -> tuple[Path, str, str]:
    root = Path(event.get("workspace") or event.get("cwd") or os.getcwd()).resolve(strict=True)
    if not root.is_dir():
        raise ValueError("Workspace is not a directory")
    session = event.get("session_id")
    if not isinstance(session, str) or not session or session.startswith("${"):
        raise ValueError("Missing session_id; refusing to mix diagnostic sessions")
    agent = str(event.get("agent_id") or "main")
    if agent.startswith("${"):
        raise ValueError("Unresolved agent_id placeholder; refusing to mix diagnostic agents")
    # Native child Pre/Post use the parent session id, but the child's own
    # transcript pathname. Main events omit agent_id, making it unsuitable as
    # a mandatory MCP template field. No transcript content is read or parsed.
    stream = event.get("agent_transcript_path") or event.get("transcript_path")
    if stream:
        if not isinstance(stream, str) or stream.startswith("${"):
            raise ValueError("Invalid diagnostic transcript identity")
        canonical = os.path.normcase(str(Path(stream).resolve()))
        agent = "transcript:" + hashlib.sha256(canonical.encode()).hexdigest()[:24]
    return root, session, agent


def state_directory(event: dict) -> Path:
    root, session, agent = identity(event)
    key = json.dumps([os.path.normcase(str(root)), session, agent], ensure_ascii=False).encode()
    return runtime_home() / hashlib.sha256(key).hexdigest()


def workspace_events(event: dict, *, remember: bool = False) -> list[dict]:
    """Resolve explicitly granted roots, without parsing shell or transcript text.

    The launcher supplies --add-dir as a JSON array. A real native shell
    workdir is an explicit target directory; retain it for later Stop events.
    The root list is owned by this session/agent, not shared across consumers.
    """
    primary = identity(event)[0]
    values = json.loads(os.environ.get("HARNESS_LSP_WORKSPACE_ROOTS", "[]"))
    if not isinstance(values, list) or any(not isinstance(value, str) for value in values):
        raise ValueError("HARNESS_LSP_WORKSPACE_ROOTS must be a JSON array of directory paths")
    owner = Journal(event)
    try:
        if remember:
            owner.db.execute("BEGIN IMMEDIATE")
        values.extend(owner.get("additional_roots", []))
        inputs = event.get("tool_input") or {}
        if isinstance(inputs, dict):
            for name in ("workdir", "working_directory"):
                value = inputs.get(name)
                if isinstance(value, str) and value:
                    values.append(str((primary / value).resolve()))
        roots = [primary]
        for value in values:
            path = Path(value)
            if not path.is_absolute():
                raise ValueError("Additional workspace root must be absolute")
            path = path.resolve(strict=True)
            if not path.is_dir():
                raise ValueError("Additional workspace root is not a directory")
            if not any(path.is_relative_to(root) for root in roots):
                roots.append(path)
        if len(roots) > 32:
            raise ValueError("More than 32 diagnostic roots exceed the bounded reconciliation limit")
        if remember:
            owner.put("additional_roots", [str(root) for root in roots[1:]])
            owner.db.commit()
        return [{**event, "workspace": str(root)} for root in roots]
    finally:
        owner.close()


def is_config(path: str) -> bool:
    item = Path(path)
    return item.name in CONFIG_NAMES or item.suffix.lower() in CONFIG_SUFFIXES


def invocation_key(event: dict) -> list[str]:
    return [str(event.get("event") or event.get("hook_event_name") or "PostToolUse").lower(),
        str(event.get("turn_id") or ""), str(event.get("tool_use_id") or "")]


def explicit_paths(event: dict, root: Path) -> list[Path]:
    inputs = event.get("tool_input") or {}
    if not isinstance(inputs, dict):
        return []
    candidates = []
    for key in ("file_path", "path", "relative_path"):
        if isinstance(inputs.get(key), str):
            candidates.append(inputs[key])
    command = inputs.get("command", "")
    if isinstance(command, str) and event.get("tool_name") in ("apply_patch", "Edit", "Write"):
        candidates.extend(re.findall(r"^\*\*\* (?:Update|Add|Delete|Move to) (?:File: )?(.+)$", command, re.MULTILINE))
    result = []
    for value in candidates:
        path = (root / value).resolve()
        if path.is_relative_to(root):
            result.append(path)
    return result


def snapshot(root: Path, extra: list[Path] = (), budget: float = 5.0, *, configurations_only: bool = False) -> tuple[dict[str, str], list[str]]:
    """Hash actual bytes, including pre-dirty/untracked files; never follow outside roots."""
    files, problems, visited = {}, [], set()
    deadline = time.monotonic() + budget

    def inspect(path):
        if time.monotonic() > deadline:
            raise TimeoutError("File reconciliation exceeded its time budget")
        actual = path.resolve()
        if not actual.is_relative_to(root):
            return
        if configurations_only and not is_config(str(actual)):
            return
        if actual.suffix.lower() not in EXTENSIONS and not is_config(str(actual)):
            return
        try:
            if actual.is_file():
                before = actual.stat()
                if before.st_size > 8 * 1024 * 1024:
                    problems.append(f"Source exceeds 8 MiB scan bound: {actual.relative_to(root)}")
                    return
                revision = digest(actual)
                after = actual.stat()
                if (before.st_mtime_ns, before.st_size) != (after.st_mtime_ns, after.st_size):
                    problems.append(f"Source changed during snapshot: {actual.relative_to(root)}")
                else:
                    files[actual.relative_to(root).as_posix()] = revision
        except OSError as error:
            problems.append(f"Cannot read {path.name}: {type(error).__name__}")

    try:
        for folder, directories, names in os.walk(root, followlinks=True, onerror=lambda error: problems.append(str(error))):
            actual = Path(folder).resolve()
            if not actual.is_relative_to(root) or actual in visited:
                directories[:] = []
                continue
            visited.add(actual)
            directories[:] = [name for name in directories if name not in IGNORED_DIRS]
            for name in names:
                inspect(Path(folder) / name)
        for path in extra:
            inspect(path)
    except TimeoutError as error:
        problems.append(str(error))
    return files, problems


class Journal:
    def __init__(self, event: dict):
        self.root, self.session, self.agent = identity(event)
        self.directory = state_directory(event)
        self.directory.mkdir(parents=True, exist_ok=True)
        self.db = sqlite3.connect(self.directory / "journal.sqlite3", timeout=5)
        self.db.execute("PRAGMA journal_mode=WAL")
        self.db.executescript("""
            CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS files (path TEXT PRIMARY KEY, revision TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS results (path TEXT PRIMARY KEY, revision TEXT, body TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS invocations (id TEXT PRIMARY KEY, turn TEXT, started REAL);
            CREATE TABLE IF NOT EXISTS checks (id TEXT PRIMARY KEY, started REAL NOT NULL, finished REAL);
        """)

    def close(self):
        self.db.close()

    def get(self, key, default=None):
        row = self.db.execute("SELECT value FROM meta WHERE key=?", (key,)).fetchone()
        return json.loads(row[0]) if row else default

    def put(self, key, value):
        self.db.execute("INSERT OR REPLACE INTO meta VALUES (?,?)", (key, json.dumps(value)))

    def pre(self, event, budget=5.0):
        self.db.execute("BEGIN IMMEDIATE")
        try:
            self.put("last_pre_identity", {key: event.get(key) for key in
                ("hook_event_name", "session_id", "agent_id", "turn_id", "tool_use_id", "transcript_path", "agent_transcript_path")})
            if not self.get("baseline"):
                files, problems = snapshot(self.root, explicit_paths(event, self.root), budget=budget)
                if problems:
                    self.put("baseline_problem", problems)
                else:
                    self.db.executemany("INSERT OR REPLACE INTO files VALUES (?,?)", files.items())
                    self.put("baseline", True)
                    self.put("baseline_problem", [])
            invocation = event.get("tool_use_id")
            if invocation:
                self.db.execute("INSERT OR REPLACE INTO invocations VALUES (?,?,?)", (str(invocation), str(event.get("turn_id") or ""), time.time()))
            self.db.commit()
        except Exception:
            self.db.rollback()
            raise
        problems = self.get("baseline_problem", [])
        if problems:
            return {"hookSpecificOutput": {"hookEventName": "PreToolUse", "additionalContext":
                "Automatic diagnostics baseline unavailable; edits remain unverified: " + "; ".join(problems)}}
        return {}

    def changes(self, event, budget=5.0):
        extras = explicit_paths(event, self.root)
        # Include previously known files even beneath excluded directories.
        previous = dict(self.db.execute("SELECT path,revision FROM files"))
        extras.extend(self.root / name for name in previous)
        current, problems = snapshot(self.root, extras, budget=budget)
        if not self.get("baseline"):
            return current, {}, ["No pre-edit baseline; missed edits cannot be identified", *self.get("baseline_problem", []), *problems]
        changed = {name: revision for name, revision in current.items() if previous.get(name) != revision}
        # A dependent file may have unchanged bytes but still need analysis after
        # another file or configuration changed. Keep that unfinished work visible.
        for name, revision, body in self.db.execute("SELECT path,revision,body FROM results"):
            if json.loads(body).get("status") in ("pending", "stale", "failed", "unavailable") and name in current:
                changed[name] = current[name]
        if not problems:
            changed.update({name: None for name in previous if name not in current})
        return current, changed, problems

    def accept(self, result):
        name, revision = result["file"], result.get("revision")
        self.db.execute("BEGIN IMMEDIATE")
        try:
            self.db.execute("INSERT OR REPLACE INTO results VALUES (?,?,?)", (name, revision, json.dumps(result, ensure_ascii=False)))
            if result["status"] in ("clean", "diagnostics", "deleted"):
                if revision is None:
                    self.db.execute("DELETE FROM files WHERE path=?", (name,))
                else:
                    self.db.execute("INSERT OR REPLACE INTO files VALUES (?,?)", (name, revision))
            self.db.commit()
        except Exception:
            self.db.rollback()
            raise

    def begin_check(self):
        token = uuid.uuid4().hex
        self.db.execute("INSERT INTO checks VALUES (?,?,NULL)", (token, time.time()))
        self.db.commit()
        return token

    def claim(self, event, origin):
        self.db.execute("BEGIN IMMEDIATE")
        try:
            current = self.get("active_claim", {})
            if current.get("started", 0) > time.time() - 30:
                self.db.rollback()
                return None
            token = uuid.uuid4().hex
            self.put("active_claim", {"token": token, "invocation": invocation_key(event), "origin": origin, "started": time.time()})
            self.db.commit()
            return token
        except Exception:
            self.db.rollback()
            raise

    def release_claim(self, token):
        self.db.execute("BEGIN IMMEDIATE")
        try:
            if self.get("active_claim", {}).get("token") == token:
                self.put("active_claim", {})
            self.db.commit()
        except Exception:
            self.db.rollback()
            raise

    def finish_check(self, token, event=None):
        self.db.execute("UPDATE checks SET finished=? WHERE id=?", (time.time(), token))
        self.db.execute("DELETE FROM checks WHERE finished IS NOT NULL AND id NOT IN (SELECT id FROM checks ORDER BY started DESC LIMIT 128)")
        if event is not None:
            self.put("last_completed_check", {"invocation": invocation_key(event), "origin": event.get("_origin", "native"), "at": time.time()})
        self.db.commit()

    def stop(self, event, budget=29.0):
        began = time.monotonic()
        # Native MCP and command handlers can execute concurrently. Allow the
        # MCP handler to publish its completed reconciliation before the fallback
        # decides it was missing. A dead/missing adapter never holds this forever.
        grace = began + 0.4
        deadline = began + max(0.0, budget - 3.0)
        while time.monotonic() < deadline:
            active = self.db.execute("SELECT 1 FROM checks WHERE finished IS NULL AND started>? LIMIT 1", (time.time() - 30,)).fetchone()
            if not active and time.monotonic() >= grace:
                break
            time.sleep(0.05)
        _, changed, problems = self.changes(event, budget=max(0.01, min(3.0, budget - (time.monotonic() - began))))
        if not changed and not problems:
            return {}
        report = "Automatic diagnostics remain unresolved: " + json.dumps({"workspace": str(self.root), "agent_id": self.agent,
            "files": sorted(changed), "problems": problems}, ensure_ascii=False)
        # One continuation per *new* unresolved state; never loop on a missing server.
        signature = hashlib.sha256(json.dumps([changed, problems], sort_keys=True).encode()).hexdigest()
        if self.get("last_stop_signature") != signature and not event.get("stop_hook_active", False):
            self.put("last_stop_signature", signature)
            self.db.commit()
            return {"decision": "block", "reason": report + ". Preserve this limitation in the completion report; do not claim a clean check."}
        return {"systemMessage": report}


def command_fallback(event: dict, budget=25.0) -> dict:
    """Bounded native command fallback for children without a local MCP manager.

    Ordinary sessions keep their connected MCP and warm backends. A child uses
    a short-lived copy of the same adapter only when the native handler did not
    claim this invocation. Its process tree belongs solely to this invocation.
    """
    began, began_wall = time.monotonic(), time.time()
    events = workspace_events(event)
    while True:
        active, handled = False, True
        for index, scoped in enumerate(events):
            journal = Journal(scoped)
            try:
                active = active or bool(journal.db.execute("SELECT 1 FROM checks WHERE finished IS NULL AND started>? LIMIT 1", (time.time() - 30,)).fetchone())
                completed = journal.get("last_completed_check", {})
                matching = completed.get("origin") == "native" and completed.get("invocation") == invocation_key(scoped) and completed.get("at", 0) >= began_wall - 5
                if matching and invocation_key(scoped)[0] in ("stop", "subagentstop"):
                    # Native Stop can repeat with the same turn/tool identity.
                    # A previous successful Stop never covers a later write.
                    if journal.get("baseline") or journal.get("observed_post_tool_use") or journal.db.execute("SELECT 1 FROM invocations LIMIT 1").fetchone():
                        scan_budget = max(0.001, min(0.25, (budget - (time.monotonic() - began)) / (len(events) - index)))
                        _, changed, problems = journal.changes(scoped, budget=scan_budget)
                        matching = not changed and not problems
                handled = handled and matching
            finally:
                journal.close()
        if handled:
            return {}
        elapsed = time.monotonic() - began
        if (not active and elapsed >= 0.6) or elapsed >= budget - 1:
            break
        time.sleep(0.05)
    remaining = budget - (time.monotonic() - began)
    if remaining <= 1:
        return fallback_failure(event, "The native diagnostic check did not finish within the completion budget")
    claims = {}
    for scoped in events:
        journal = Journal(scoped)
        try:
            token = journal.claim(scoped, "command")
            if token:
                claims[str(journal.directory)] = token
        finally:
            journal.close()
        if not token:
            for previous in events:
                pending = Journal(previous)
                try:
                    if str(pending.directory) in claims:
                        pending.release_claim(claims[str(pending.directory)])
                finally:
                    pending.close()
            return fallback_failure(event, "Another check is already reconciling this workspace")
    payload = {**event, "_origin": "command", "_claims": claims, "_batch_budget": max(0.1, remaining - 4)}
    try:
        return run_fallback_worker(event, payload, remaining)
    finally:
        for scoped in events:
            journal = Journal(scoped)
            try:
                journal.release_claim(claims[str(journal.directory)])
            finally:
                journal.close()


def run_fallback_worker(event, payload, remaining):
    process = subprocess.Popen([sys.executable, "-B", "-u", str(Path(__file__).with_name("server.py")), "--once"],
        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        creationflags=subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0)
    try:
        stdout, stderr = process.communicate(json.dumps(payload).encode("utf-8"), timeout=remaining)
    except subprocess.TimeoutExpired:
        # Kill only the worker we started and its own LSP children. The native
        # MCP process and every other editor/session remain outside this tree.
        if os.name == "nt":
            subprocess.run(["taskkill", "/PID", str(process.pid), "/T", "/F"], capture_output=True, timeout=2,
                creationflags=subprocess.CREATE_NO_WINDOW, check=False)
        else:
            process.kill()
        stdout, stderr = process.communicate(timeout=1)
        if not stdout.strip():
            return fallback_failure(event, "Diagnostic fallback exceeded its finite time budget")
    try:
        output = json.loads(stdout.decode("utf-8"))
        if not isinstance(output, dict):
            raise ValueError("Hook result is not an object")
        return output
    except (ValueError, UnicodeError):
        return fallback_failure(event, "Diagnostic fallback did not return a complete result (exit " + str(process.returncode) + ")")


def fallback_failure(event: dict, reason: str) -> dict:
    message = "Automatic diagnostics remain unresolved: " + reason + ". No clean result is established."
    if str(event.get("event") or event.get("hook_event_name") or "").lower() in ("stop", "subagentstop"):
        return {"systemMessage": message} if event.get("stop_hook_active") else {"decision": "block", "reason": message}
    return {"hookSpecificOutput": {"hookEventName": "PostToolUse", "additionalContext": message}}


def main():
    sys.stdin.reconfigure(encoding="utf-8-sig")
    sys.stdout.reconfigure(encoding="utf-8")
    parser = argparse.ArgumentParser()
    parser.add_argument("--event", choices=("pre", "post", "stop"), required=True)
    arguments = parser.parse_args()
    journal = None
    try:
        event = json.load(sys.stdin)
        event.setdefault("hook_event_name", {"pre": "PreToolUse", "post": "PostToolUse", "stop": "Stop"}[arguments.event])
        if arguments.event != "pre":
            print(json.dumps(command_fallback(event), ensure_ascii=False))
            return
        began = time.monotonic()
        events = workspace_events(event, remember=arguments.event == "pre")
        outputs = []
        for index, scoped in enumerate(events):
            journal = Journal(scoped)
            budget = max(0.01, ((7.0 if arguments.event == "pre" else 29.0) - (time.monotonic() - began)) / (len(events) - index))
            outputs.append(journal.pre(scoped, budget=budget) if arguments.event == "pre" else journal.stop(scoped, budget=budget))
            journal.close()
            journal = None
        outputs = [output for output in outputs if output]
        if not outputs:
            result = {}
        elif arguments.event == "pre":
            result = {"hookSpecificOutput": {"hookEventName": "PreToolUse", "additionalContext": "\n".join(
                output["hookSpecificOutput"]["additionalContext"] for output in outputs)}}
        else:
            messages = "\n".join(output.get("reason") or output.get("systemMessage", "") for output in outputs)
            result = {"decision": "block", "reason": messages} if any(output.get("decision") == "block" for output in outputs) else {"systemMessage": messages}
    except Exception as error:
        message = f"Automatic diagnostics journal unavailable: {type(error).__name__}: {error}"
        result = ({"hookSpecificOutput": {"hookEventName": "PreToolUse" if arguments.event == "pre" else "PostToolUse", "additionalContext": message}} if arguments.event in ("pre", "post")
                  else {"decision": "block", "reason": message} if not locals().get("event", {}).get("stop_hook_active")
                  else {"systemMessage": message})
    finally:
        if journal:
            journal.close()
    print(json.dumps(result, ensure_ascii=False))


if __name__ == "__main__":
    main()
