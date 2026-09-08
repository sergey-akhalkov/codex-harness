"""Opt-in native outcome execution, without case preparation or case oracles.

run_native(case_root, prompt, codex_home, extra_config=None, timeout=600)
returns private evidence; it does NOT assert task correctness. Callers run their
acceptance afterwards and retain all rounds through outcome_report.finish_attempt.

The parent case catalogue owns --run-model-probes / --cases CLI orchestration,
case preparation and independent correctness oracles. select_cases provides its
strict opt-in/explicit-subset boundary. No import or discovery makes model calls.
"""
from __future__ import annotations

import hashlib
import importlib.util
import json
import os
from pathlib import Path
import queue
import re
import shutil
import subprocess
import tempfile
import threading
import time
import tomllib
from typing import Any

REPO = Path(__file__).resolve().parents[1]
CANDIDATES = {"project-verification", "reproduce-regression"}
MODEL = "gpt-6-astra"
EFFORT = "xhigh"


def select_cases(catalogue: dict[str, Any], cases: list[str] | None,
                 run_model_probes: bool = False) -> list[Any]:
    """Pure CLI boundary: an explicit opt-in and nonempty subset are necessary."""
    if not run_model_probes:
        return []
    if not cases:
        raise ValueError("--run-model-probes requires --cases with an explicit subset")
    if len(cases) != len(set(cases)) or any(c not in catalogue for c in cases):
        raise ValueError("--cases contains duplicate or unknown case identifiers")
    return [catalogue[c] for c in cases]


def write_json(path: Path, value: Any) -> None:
    # A partial write never replaces the last recoverable receipt.
    staged = path.with_suffix(path.suffix + ".tmp")
    staged.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    staged.replace(path)


def read_json(path: Path) -> dict[str, Any]:
    return json.loads(path.read_text(encoding="utf-8-sig"))


def private_root(prefix: str) -> Path:
    root = Path(tempfile.mkdtemp(prefix=prefix)).resolve()
    if root.is_relative_to(REPO):
        raise ValueError("Private evidence must be outside the source checkout")
    return root


def isolated_home(value: str | Path) -> Path:
    home = Path(value).resolve(strict=True)
    if not home.is_relative_to(Path(tempfile.gettempdir()).resolve()) or home.is_relative_to(REPO):
        raise ValueError("Outcome CODEX_HOME must be an isolated private temporary installation")
    return home


def native_executable(codex_home: Path) -> Path:
    state = read_json(codex_home / "harness/installation.json")
    command = Path(state["codexCommand"])
    if command.suffix.lower() == ".exe" and command.is_file():
        return command
    vendor = command.parent / "node_modules/@openai/codex/node_modules/@openai/codex-win32-x64/vendor"
    found = list(vendor.rglob("codex.exe"))
    if len(found) != 1:
        raise ValueError("Installed native Codex identity unavailable")
    return found[0]


def toml_value(value: Any) -> str:
    if isinstance(value, bool):
        return str(value).lower()
    if isinstance(value, str):
        return json.dumps(value, ensure_ascii=False)
    if isinstance(value, (float, int)):
        return str(value)
    if isinstance(value, list):
        return "[" + ", ".join(toml_value(v) for v in value) + "]"
    if isinstance(value, dict):
        return "{" + ", ".join(json.dumps(k) + "=" + toml_value(v) for k, v in value.items()) + "}"
    raise ValueError("Unsupported TOML override value")


def config_arguments(extra_config: dict[str, Any] | None) -> list[str]:
    result = []
    for key, value in (extra_config or {}).items():
        # No profile/model/provider substitution, including nested provider edits.
        if key.startswith(("model", "profile", "credential", "auth", "forced_login", "service_tier")):
            raise ValueError("Model, provider and billing overrides are not outcome treatment")
        result.extend(["-c", key + "=" + toml_value(value)])
    return result


def discover_skills(case_root: str | Path, codex_home: str | Path,
                    extra_config: dict[str, Any] | None = None, timeout: int = 30) -> dict[str, Any]:
    """Native base-config skills/list, with a fresh disk scan and private trace.

0.153.4 app-server has no file profiles. Reject a profile that changes skill
discovery, rather than claim this base-consumer listing proves profile discovery.
    """
    home = isolated_home(codex_home)
    case = Path(case_root).resolve(strict=True)
    profile_path = home / "harness.config.toml"
    profile = tomllib.loads(profile_path.read_text(encoding="utf-8-sig")) if profile_path.exists() else {}
    if set(profile) & {"skills", "project_root_markers", "credential_broker"} or any(
            case.is_relative_to(Path(p).resolve()) for p in profile.get("projects", {})):
        raise ValueError("File profile changes discovery: native base listing is insufficient")
    root = private_root("codex-outcome-discovery-")
    messages: queue.Queue[Any] = queue.Queue()
    deadline = time.monotonic() + timeout
    with (root / "stderr.txt").open("w", encoding="utf-8") as err:
        process = subprocess.Popen([str(native_executable(home)), "app-server", "--stdio",
            *config_arguments(extra_config)], cwd=case, env={**os.environ, "CODEX_HOME": str(home)},
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=err,
            text=True, encoding="utf-8", creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
        assert process.stdout is not None and process.stdin is not None

        def receive() -> None:
            assert process.stdout is not None
            try:
                with (root / "rpc.jsonl").open("w", encoding="utf-8") as trace:
                    for line in process.stdout:
                        trace.write(line)
                        trace.flush()
                        try:
                            messages.put(json.loads(line))
                        except ValueError:
                            messages.put({"malformed": True})
            finally:
                messages.put(None)

        reader = threading.Thread(target=receive, daemon=True)
        reader.start()

        def rpc(number: int, method: str, params: dict[str, Any]) -> Any:
            assert process.stdin is not None
            process.stdin.write(json.dumps({"id": number, "method": method, "params": params}) + "\n")
            process.stdin.flush()
            while True:
                response = messages.get(timeout=max(.01, deadline - time.monotonic()))
                if response is None or "malformed" in response:
                    raise RuntimeError(f"Incomplete native {method}; private evidence: {root}")
                if response.get("id") == number:
                    if "error" in response:
                        raise RuntimeError(f"Native {method} failed; private evidence: {root}")
                    return response["result"]

        try:
            init = rpc(1, "initialize", {"clientInfo": {"name": "outcome-discovery", "version": "1"},
                                         "capabilities": {"experimentalApi": True}})
            process.stdin.write('{"method":"initialized"}\n')
            process.stdin.flush()
            listed = rpc(2, "skills/list", {"cwds": [str(case)], "forceReload": True})
            entries = listed.get("data", [])
            if len(entries) != 1 or Path(entries[0]["cwd"]).resolve() != case or entries[0].get("errors"):
                raise ValueError(f"Incomplete native discovery; private evidence: {root}")
            result = {"skills": entries[0]["skills"], "native": init, "evidence_root": str(root),
                      "scope": "native-base; profile has no discovery overrides", "config": rpc(3, "config/read", {"cwd": str(case)})}
            write_json(root / "discovery.json", result)
            return result
        finally:
            process.stdin.close()
            try:
                process.wait(timeout=3)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=3)
            reader.join(timeout=3)
            process.stdout.close()


def configure_arm(case_root: str | Path, codex_home: str | Path, arm: str) -> dict[str, Any]:
    """Append native [[skills.config]] to an owned isolated config, then verify.

Installer source links must already exist. Enumerates actual Known Folder and
CODEX_HOME discovery, disables EVERY registration of both skills in baseline,
and preserves unrelated personal skills. Never writes a linked/global config.
    """
    if arm not in ("baseline", "candidate"):
        raise ValueError("Unknown comparison arm")
    home = isolated_home(codex_home)
    config = home / "config.toml"
    if config.is_symlink() or config.resolve().parent != home:
        raise ValueError("Arm config must be an owned regular file")
    before = discover_skills(case_root, home)
    candidates = [s for s in before["skills"] if s["name"] in CANDIDATES]
    if {s["name"] for s in candidates} != CANDIDATES:
        raise ValueError("Both candidate skills must first be connected by the installer")
    state = read_json(home / "harness/installation.json")
    links = state.get("links", [])
    for name in CANDIDATES:
        if not any(Path(link["destination"]).name == name and
                   Path(link["destination"]).resolve() == Path(link["source"]).resolve() and
                   Path(link["destination"]).is_symlink() for link in links):
            raise ValueError("Candidate lacks an installer-owned source link: " + name)
    text = config.read_text(encoding="utf-8-sig") if config.exists() else ""
    parsed = tomllib.loads(text)
    existing = {os.path.normcase(str(Path(s["path"]).absolute())): s["enabled"]
                for s in parsed.get("skills", {}).get("config", [])}
    for skill in candidates:
        path = str(Path(skill["path"]).absolute())
        prior = existing.get(os.path.normcase(path))
        enabled = arm == "candidate"
        if prior is not None and prior != enabled:
            raise ValueError("Fresh arm config required; existing candidate override conflicts")
        if prior is None:
            text += "\n[[skills.config]]\npath = " + toml_value(path) + "\nenabled = " + toml_value(enabled) + "\n"
    # Validate the entire document before the only mutation.
    tomllib.loads(text)
    config.write_text(text, encoding="utf-8")
    after = discover_skills(case_root, home)
    observed = [s for s in after["skills"] if s["name"] in CANDIDATES]
    if {s["name"] for s in observed} != CANDIDATES or any(s["enabled"] != (arm == "candidate") for s in observed):
        raise ValueError("Native discovery did not confirm the intended arm")
    others = lambda d: sorted((s["name"], s["path"], s["enabled"]) for s in d["skills"] if s["name"] not in CANDIDATES)
    if others(before) != others(after):
        raise ValueError("Arm preparation changed unrelated skill discovery")
    return {"arm": arm, "discovery_verified": True, **after}


def load_usage(paths: list[Path]) -> dict[str, Any]:
    spec = importlib.util.spec_from_file_location("outcome_delegation_usage", REPO / "tools/delegation-usage.py")
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    result = module.summarize_rollouts(paths)
    # The existing parser represents an empty collection as zero observed tokens.
    # No rollout is NOT evidence of zero usage for an attempted execution.
    if not paths:
        result["totals"] = {**result["totals"], **dict.fromkeys(module.TOKEN_FIELDS)}
    result["status"] = "partial" if paths and result["partial"] else "known" if paths else "unknown"
    return result


def observe_event(result: dict[str, Any], event: dict[str, Any], at: float, evidence: str,
                  useful_command_pattern: str | None = None) -> None:
    """Keep raw completion separately; semantic usefulness needs caller policy."""
    if event.get("type") == "thread.started":
        result["thread_id"] = event["thread_id"]
    if event.get("type") == "turn.completed":
        result["turn_completed"] = True
    item = event.get("item", {})
    if event.get("type") == "item.completed" and item.get("type") == "command_execution" and type(item.get("exit_code")) is int:
        signal = {"at": at, "kind": "command_result", "item_id": item.get("id"),
                "evidence": evidence, "timestamp_scope": "observed on receipt; polling <=50ms"}
        if result.get("first_command_result") is None:
            result["first_command_result"] = signal
        if result.get("first_useful_signal") is None and useful_command_pattern and re.search(useful_command_pattern, item.get("command", ""), re.I):
            result["first_useful_signal"] = {**signal, "policy": useful_command_pattern}
    if item.get("type") == "collab_tool_call":
        for child in item.get("receiver_thread_ids", []):
            if child not in result["children"]:
                result["children"].append(child)


def run_native(case_root: str | Path, prompt: str, codex_home: str | Path,
               extra_config: dict[str, Any] | None = None, timeout: int = 600,
               *, useful_command_pattern: str | None = None) -> dict[str, Any]:
    """Execute exactly one Astra/xhigh turn under the existing Windows job owner.

Explicit library calls spend quota. CLI callers require --run-model-probes.
The prompt is passed unchanged; skill source paths are never appended. Detailed
evidence is retained even on launch failure/timeout, separate from model-writable
case state. The caller owns acceptance and any explicit retry/rework decision.
    """
    root = private_root("codex-outcome-native-")
    result: dict[str, Any] = {"evidence_root": str(root), "status": "incomplete", "started_at": time.time(),
        "ended_at": None, "thread_id": None, "children": [], "rollout_paths": [],
        "first_useful_signal": None, "usage": {"status": "unknown", "total_tokens": None},
        "final_path": str(root / "final.txt"), "result_path": str(root / "result.json")}
    write_json(root / "native.json", result)
    process: subprocess.Popen[Any] | None = None
    try:
        home = isolated_home(codex_home)
        case = Path(case_root).resolve(strict=True)
        if os.name != "nt" or not isinstance(timeout, int) or not 1 <= timeout <= 604800:
            raise ValueError("Bounded native Windows execution required")
        pwsh = shutil.which("pwsh")
        launcher = home / "harness/bin/codex.ps1"
        if not pwsh or not launcher.is_file():
            raise ValueError("Isolated installed launcher unavailable")
        extra_args = config_arguments(extra_config)
        request = {"executable": pwsh, "arguments": ["-NoLogo", "-NoProfile", "-File", str(launcher),
            "exec", "--strict-config", "--skip-git-repo-check", "--json", "-C", str(case),
            "-m", MODEL, "-c", 'model_reasoning_effort="' + EFFORT + '"', *extra_args,
            "--output-last-message", str(root / "final.txt"), prompt], "workingDirectory": str(case),
            "stdoutPath": str(root / "events.jsonl"), "stderrPath": str(root / "stderr.txt"),
            "startedPath": str(root / "started.json"), "memoryLimitMiB": 2048,
            "timeoutSeconds": timeout, "environment": {"CODEX_HOME": str(home)}}
        write_json(root / "request.json", request)
        result["executable_sha256"] = hashlib.sha256(native_executable(home).read_bytes()).hexdigest()
        result["model"], result["effort"] = MODEL, EFFORT
        with (root / "supervisor.txt").open("w", encoding="utf-8") as log:
            process = subprocess.Popen([pwsh, "-NoLogo", "-NoProfile", "-File", str(REPO / "tools/opencodex-process.ps1"),
                "-RequestPath", str(root / "request.json"), "-ResultPath", str(root / "result.json")],
                stdout=log, stderr=subprocess.STDOUT, creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
            offset = 0
            deadline = time.monotonic() + timeout + 30
            with (root / "observed.jsonl").open("w", encoding="utf-8") as observed:
                while True:
                    done = process.poll() is not None
                    events = root / "events.jsonl"
                    if events.exists():
                        with events.open("rb") as stream:
                            stream.seek(offset)
                            for line in stream:
                                if not line.endswith(b"\n"):
                                    if done:
                                        result.setdefault("evidence_errors", []).append("truncated_native_event")
                                    break
                                offset += len(line)
                                try:
                                    event = json.loads(line)
                                except ValueError:
                                    result.setdefault("evidence_errors", []).append("malformed_native_event")
                                    continue
                                at = time.time()
                                observed.write(json.dumps({"at": at, "event": event}) + "\n")
                                observe_event(result, event, at, str(root / "observed.jsonl"), useful_command_pattern)
                        observed.flush()
                    if done:
                        break
                    if time.monotonic() >= deadline:
                        # Killing the supervisor closes its job handle and owns
                        # cleanup of its whole child tree; no name-based kill.
                        process.kill()
                        process.wait(timeout=5)
                        result["status"] = "timeout"
                        result["reason"] = "supervisor_deadline"
                        break
                    time.sleep(.05)
        if (root / "result.json").exists():
            receipt = read_json(root / "result.json")
            result["process"] = receipt
            result["status"] = "completed" if receipt.get("Status") == "exited" and receipt.get("ExitCode") == 0 else "failed"
            if receipt.get("Status") == "timeout":
                result["status"] = "timeout"
            elif result["status"] == "completed" and not result.get("turn_completed"):
                result["status"] = "incomplete"
        thread_ids = [result["thread_id"], *result["children"]]
        paths = []
        for thread_id in thread_ids:
            if not isinstance(thread_id, str) or not re.fullmatch(r"[0-9a-fA-F-]{36}", thread_id):
                continue
            found = list((home / "sessions").rglob("*" + thread_id + ".jsonl"))
            if len(found) == 1:
                paths.extend(found)
            else:
                result.setdefault("evidence_errors", []).append("missing_or_ambiguous_rollout:" + thread_id)
        result["rollout_paths"] = [str(p) for p in paths]
        result["usage"] = load_usage(paths)
        result["observed_threads"] = [{k: t.get(k) for k in ("id", "parent_id", "model", "reasoning", "provider")}
                                      for t in result["usage"].get("threads", [])]
        if any(t.get("model") not in (MODEL, "openai/" + MODEL) or t.get("reasoning") != EFFORT
               or t.get("provider") != "OpenAI" for t in result["observed_threads"]):
            result.setdefault("evidence_errors", []).append("observed_model_policy_unverified")
        if result["children"]:
            result.setdefault("evidence_errors", []).append("unexpected_delegation")
        if not result["thread_id"]:
            result.setdefault("evidence_errors", []).append("missing_thread")
    except BaseException as error:
        result.update(status="incomplete" if isinstance(error, (KeyboardInterrupt, SystemExit)) else "failed",
                      error_type=type(error).__name__)
        (root / "failure.txt").write_text(str(error), encoding="utf-8")
        if isinstance(error, (KeyboardInterrupt, SystemExit)):
            raise
    finally:
        if process is not None and process.poll() is None:
            process.kill()
            process.wait(timeout=5)
        result["ended_at"] = time.time()
        result["elapsed_seconds"] = result["ended_at"] - result["started_at"]
        write_json(root / "native.json", result)
    return result
