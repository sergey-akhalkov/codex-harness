"""Independent acceptance for the eight opt-in cases; no model calls.

Native transcripts establish what the agent executed. Product entrypoints and
reduced cases are exercised again. A self-reported success alone never passes.
Detailed records and subprocess output stay in private evidence directories.
"""
from __future__ import annotations

import importlib.util
import hashlib
import json
from pathlib import Path
import re
import shutil
import subprocess
import sys
import time
from typing import Any

REPO = Path(__file__).resolve().parents[1]


def digest(path: Path) -> str:
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def read_json(path: Path) -> Any:
    return json.loads(path.read_text(encoding="utf-8-sig"))


def process_run(argv: list[str], cwd: Path, timeout: int = 30) -> dict[str, Any]:
    spec = importlib.util.spec_from_file_location("outcome_process_case", REPO / ".agents/skills/reproduce-regression/scripts/process_case.py")
    assert spec and spec.loader
    helper = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(helper)
    row = helper.run_case(argv, cwd=cwd, timeout=timeout)
    row["exit_code"] = (row.get("native") or {}).get("ExitCode")
    for name in ("stdout", "stderr"):
        path = Path(row["streams"][name]["path"])
        row[name] = path.read_text(encoding="utf-8", errors="replace") if path.is_file() else ""
    return row


def completed_tools(result: dict[str, Any]) -> list[dict[str, Any]]:
    events = Path(result["evidence_root"]) / "events.jsonl"
    found = []
    if events.exists():
        for line in events.read_text(encoding="utf-8", errors="replace").splitlines():
            try:
                row = json.loads(line)
            except ValueError:
                continue
            item = row.get("item", {})
            if row.get("type") == "item.completed" and item.get("type") in ("command_execution", "mcp_tool_call"):
                found.append(item)
    return found


def tool_input(item: dict[str, Any]) -> str:
    return str(item.get("command", "")) + json.dumps(item.get("arguments", {}))


def used_skill(items: list[dict[str, Any]], name: str) -> bool:
    return any((item.get("exit_code") == 0 if item.get("type") == "command_execution" else item.get("status") == "completed")
               and re.search(r"[/\\]+" + re.escape(name) + r"[/\\]+SKILL\.md", tool_input(item), re.I) for item in items)


def successful_command(items: list[dict[str, Any]], pattern: str, output: str | None = None) -> bool:
    return any(item.get("type") == "command_execution" and item.get("exit_code") == 0
               and re.search(pattern, str(item.get("command", "")), re.I)
               and (output is None or re.search(output, str(item.get("aggregated_output", "")), re.I))
               for item in items)


def documentation(workspace: Path, previous: dict[str, str] | None = None) -> str:
    # Existing documentation homes plus the required concise outcome record.
    paths = [workspace / "README.md", workspace / "outcome.json"]
    for folder in ("docs", "doc", ".notes"):
        root = workspace / folder
        if root.is_dir():
            paths.extend(root.rglob("*.md"))
    return "\n".join(p.read_text(encoding="utf-8", errors="replace") for p in paths if p.is_file()
                     and (previous is None or p.suffix == ".md" and digest(p) != previous.get(p.relative_to(workspace).as_posix())))


def verify_case(case_id: str, workspace: Path, setup: dict[str, Any], result: dict[str, Any],
                evidence: Path) -> dict[str, Any]:
    started = time.time()
    evidence.mkdir(parents=True, exist_ok=True)
    checks: dict[str, bool] = {}
    runs: list[dict[str, Any]] = []
    details: dict[str, Any] = {"checks": checks, "runs": runs}
    try:
        items = completed_tools(result)
        checks["native_completed"] = result.get("status") == "completed"
        checks["immutable_inputs"] = all((workspace / name).is_file() and digest(workspace / name) == expected
                                         for name, expected in setup["immutable"].items())
        report = read_json(workspace / "outcome.json")
        checks["result_record"] = isinstance(report, dict) and bool(report.get("command")) and bool(report.get("scope"))
        docs = documentation(workspace)
        updated_docs = documentation(workspace, setup.get("documents", {}))
        checks["accurate_status"] = report.get("status") == ("blocked" if case_id == "missing" else "passed")
        target_skill = "reproduce-regression" if case_id in ("reduction", "process") else "project-verification"
        uses = {name: used_skill(items, name) for name in ("project-verification", "reproduce-regression")}
        details["skill_use"] = uses
        if setup.get("arm") == "candidate" and case_id != "negative":
            checks["positive_activation"] = uses[target_skill]
        elif setup.get("arm") == "baseline" or case_id == "negative":
            checks["negative_activation"] = not any(uses.values())

        def run(argv: list[str], timeout: int = 30) -> dict[str, Any]:
            value = process_run(argv, workspace, timeout)
            # Raw potentially large streams already live in the helper root.
            runs.append({k: v for k, v in value.items() if k not in ("stdout", "stderr")})
            return value

        if case_id in ("freshness", "entrypoint"):
            # Read before our independent execution: our own CLI run is not
            # evidence that the native agent exercised the entrypoint.
            audit = workspace / "execution-audit.jsonl"
            executions = [json.loads(line) for line in audit.read_text(encoding="utf-8").splitlines()] if audit.is_file() else []
            builds = [i for i, row in enumerate(executions) if row == {"entrypoint":"build", "version":2}]
            checks["build_executed"] = bool(builds)
            checks["cli_executed"] = bool(builds) and any(i > builds[0] and row == {"entrypoint":"cli", "version":2} for i, row in enumerate(executions))
            cli = run([sys.executable, str(workspace / "cli.py")])
            checks["actual_entrypoint_v2"] = cli["status"] == "exited" and cli["exit_code"] == 0 and cli["stdout"].strip() == "2"
            checks["generated_matches_source"] = read_json(workspace / "built.json") == read_json(workspace / "source.json")
            if case_id == "freshness":
                record = (workspace / "docs/validation.md").read_text(encoding="utf-8")
                checks["historical_record_retained"] = "source version 1" in record and "stdout 1" in record
                checks["current_record_distinct"] = bool(re.search(r"(?:version|верси\w*)\s*[:=]?\s*2|stdout\s*[:=]?\s*2", record, re.I))
                checks["historical_limit"] = bool(re.search(r"historic|earlier|previous|outdated|устар|истори|предыдущ", record, re.I))
        elif case_id in ("focused", "second"):
            node = shutil.which("node")
            if not node:
                raise RuntimeError("Node prerequisite absent during acceptance")
            if case_id == "focused":
                pattern = r"test:focused:library|run-focused-test\.ts[^\r\n]*test-library\.ts|acceptance[/\\]+focused-check\.ps1"
                checks["native_command_executed"] = successful_command(items, pattern, r"(?m)^OK: library tests=183\r?$")
                checks["evidence_in_documentation"] = bool(re.search(pattern, updated_docs, re.I))
                # Full invocation is independently calibrated before native attempts;
                # immutable source plus actual completed native command is reused here.
                checks["calibrated_command"] = setup.get("focused_calibration_passed") is True
            else:
                checks["native_command_executed"] = successful_command(items, r"npm(?:\.cmd)?\s+(?:run\s+)?lint\b|eslint[^\r\n]+(?:src|\.ts)")
                lint = run([node, str(workspace / "node_modules/eslint/bin/eslint.js"), "--ext", ".ts", "src"], 90)
                checks["independent_lint"] = lint["status"] == "exited" and lint["exit_code"] == 0
                scope = str(report.get("scope", ""))
                checks["lint_only_scope"] = "lint" in scope.lower() and bool(re.search(r"only|not|no |не |только|лишь", scope, re.I))
                checks["evidence_in_documentation"] = bool(re.search(r"npm(?:\.cmd)? (?:run )?lint|eslint", updated_docs, re.I))
        elif case_id == "reduction":
            node = shutil.which("node")
            if not node:
                raise RuntimeError("Node prerequisite absent during acceptance")
            minimal = workspace / "tools/outcome-minimal.mjs"
            original = workspace / "tools/outcome-original.mjs"
            wrapper = workspace / "tools/run-focused-test.ts"
            checks["reduction_exists"] = minimal.is_file()
            if minimal.is_file():
                original_direct = run([node, str(original)])
                original_wrapped = run([node, str(wrapper), str(original)])
                reduced_direct = run([node, str(minimal)])
                reduced_wrapped = run([node, str(wrapper), str(minimal)])
                wrong = run([node, str(wrapper), str(workspace / "tools/outcome-wrong.mjs")])
                checks["reference_success"] = all(row["status"] == "exited" and row["exit_code"] == 0 for row in (original_direct, reduced_direct))
                checks["same_failure"] = all(row["status"] == "exited" and row["exit_code"] != 0 and "ENOBUFS" in row["stderr"] for row in (original_wrapped, reduced_wrapped))
                checks["smaller_input"] = (reduced_direct["streams"]["stdout"]["bytes"] + reduced_direct["streams"]["stderr"]["bytes"] < original_direct["streams"]["stdout"]["bytes"] + original_direct["streams"]["stderr"]["bytes"])
                checks["wrong_failure_distinct"] = wrong["exit_code"] != 0 and "SyntaxError" in wrong["stderr"] and "ENOBUFS" not in wrong["stderr"]
                checks["wrong_reduction_rejected"] = bool(re.search(r"(?:reject|not|different|отклон|не |друг)[\s\S]{0,180}(?:SyntaxError|wrong)|(?:SyntaxError|wrong)[\s\S]{0,180}(?:reject|not|different|отклон|не |друг)", docs, re.I))
        elif case_id == "process":
            # Owned independent sentinel; never terminate a discovered process.
            sentinel = subprocess.Popen([sys.executable, "-c", "import time;time.sleep(120)"], creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
            try:
                audit = workspace / "process-audit.jsonl"
                offset = audit.stat().st_size if audit.exists() else 0
                executed = run([sys.executable, str(workspace / "check_process.py")], 45)
                checks["executable_check"] = executed["status"] == "exited" and executed["exit_code"] == 0
                observations = read_json(workspace / "process-results.json")
                fresh_audit = audit.read_bytes()[offset:].decode("utf-8") if audit.exists() else ""
                events = [json.loads(line) for line in fresh_audit.splitlines()]
                checks["all_real_targets_executed"] = {e["mode"] for e in events if e.get("event") == "start"} >= {"flood", "fail", "no-ready", "hang"}
                checks["hang_did_not_exit_naturally"] = not any(e.get("event") == "natural-end" and e.get("mode") in ("hang", "no-ready") for e in events)
                for mode, status, code in (("flood", "exited", 0), ("fail", "exited", 7), ("no-ready", "readiness-timeout", None), ("hang", "timeout", None)):
                    row = observations.get(mode, {})
                    checks[mode + "_status"] = row.get("status") == status and row.get("exit_code") == code
                for stream, byte in (("stdout", b"a"), ("stderr", b"b")):
                    path = Path(observations["flood"][stream + "_path"])
                    if not path.is_absolute():
                        path = workspace / path
                    checks["complete_" + stream] = path.is_file() and path.read_bytes() == byte * 2097152
                time.sleep(7)  # Past the owned descendant's delayed write deadline.
                checks["descendant_cleaned"] = not (workspace / "descendant-survived.txt").exists()
                checks["unrelated_process_survives"] = sentinel.poll() is None
            finally:
                if sentinel.poll() is None:
                    sentinel.terminate()
                sentinel.wait(timeout=5)
        elif case_id == "missing":
            command = read_json(workspace / "verification.json")["command"]
            checks["prerequisite_still_absent"] = not Path(command[0]).exists()
            checks["blocker_named"] = Path(command[0]).name.lower() in json.dumps(report).lower()
            checks["no_substitute_or_install"] = not any(re.search(r"\b(?:pip|npm|uv|winget|choco)\s+(?:install|add)|Invoke-WebRequest|curl\s+https?", tool_input(item), re.I) for item in items)
        elif case_id == "negative":
            checks["exact_edit"] = (workspace / "README.md").read_text(encoding="utf-8") == "# Guide\n\nRun verification; see [details](guide.md).\n"
            checks["local_link"] = (workspace / "guide.md").is_file()
            checks["no_unrelated_execution"] = not any(re.search(r"\b(?:pytest|jest|vitest|eslint)\b|npm(?:\.cmd)?\s+(?:run\s+)?(?:test|build|lint)|agent-delegation|outcome_suite|spawn_agent", tool_input(item), re.I) for item in items)
            checks["no_delegation"] = not result.get("children")
        else:
            raise ValueError("Unknown case: " + case_id)
    except (OSError, ValueError, KeyError, TypeError, RuntimeError) as error:
        checks["oracle_completed"] = False
        details["error"] = type(error).__name__ + ": " + str(error)
    path = evidence / "oracle.json"
    passed = bool(checks) and all(checks.values())
    record = {"id": "outcome", "executed": True, "exit_code": 0 if passed else 1, "passed": passed,
              "started_at": started, "ended_at": time.time(), "evidence": str(path), "details": details}
    path.write_text(json.dumps(record, indent=2), encoding="utf-8")
    return record
