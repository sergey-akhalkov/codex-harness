"""Controlled hook before/after comparison in owned homes; no model calls."""
from __future__ import annotations
import argparse
import concurrent.futures
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import time
from typing import Any

REPO = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--inputs", type=Path, required=True)
    args = parser.parse_args()
    pwsh = shutil.which("pwsh")
    if not pwsh:
        raise SystemExit("PowerShell 7 prerequisite missing")
    root = args.inputs.resolve() / "hook-comparison"
    root.mkdir(exist_ok=False)
    host = Path(os.environ.get("CODEX_HOME", Path.home() / ".codex"))
    inventory = json.loads((host / "harness/code-tools.json").read_text(encoding="utf-8-sig"))
    spec = importlib.util.spec_from_file_location("registry", REPO / "tools/lsp/registry.py")
    assert spec and spec.loader
    registry = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(registry)
    registry_path = root / "registry.json"
    registry_path.write_text(json.dumps(registry.generate(inventory)), encoding="utf-8")
    records = []
    for arm in ("before", "after"):
        armroot = root / arm
        source = armroot / "source"
        shutil.copytree(REPO / "tools/lsp", source / "tools/lsp", ignore=shutil.ignore_patterns("__pycache__"))
        shutil.copyfile(REPO / "tools/hook.ps1", source / "tools/hook.ps1")
        if arm == "before":
            for file in (args.inputs / "hooks-before/tools").rglob("*"):
                if file.is_file():
                    destination = source / "tools" / file.relative_to(args.inputs / "hooks-before/tools")
                    destination.parent.mkdir(parents=True, exist_ok=True)
                    shutil.copyfile(file, destination)
        home = armroot / "home"
        (home / "harness").mkdir(parents=True)
        (home / "harness/installation.json").write_text(json.dumps({"sourceRoot": str(source)}), encoding="utf-8")
        (home / "harness/code-tools.json").write_text(json.dumps(inventory), encoding="utf-8")
        environment = {**os.environ, "CODEX_HOME": str(home), "HARNESS_LSP_REGISTRY": str(registry_path),
                       "HARNESS_LSP_WORKSPACE_ROOTS": "[]"}
        identities = {str(p.relative_to(source)): hashlib.sha256(p.read_bytes()).hexdigest()
                      for p in source.rglob("*") if p.is_file()}

        def hook(workspace, phase, session, invocation, **extra):
            event = dict(cwd=str(workspace), session_id=session, turn_id="turn", tool_use_id=invocation,
                         tool_name="Bash", tool_input={}, hook_event_name={"pre":"PreToolUse","post":"PostToolUse","stop":"Stop"}[phase], **extra)
            start = time.time()
            try:
                completed = subprocess.run([pwsh, "-NoProfile", "-File", str(source / "tools/hook.ps1"), "-Event", phase],
                    input=json.dumps(event), text=True, encoding="utf-8", capture_output=True, env=environment,
                    cwd=workspace, timeout=35)
                result: dict[str, Any] = dict(exit_code=completed.returncode, stdout=completed.stdout, stderr=completed.stderr)
                try:
                    result["output"] = json.loads(completed.stdout)
                except ValueError:
                    result["output"] = None
            except subprocess.TimeoutExpired:
                result = dict(exit_code=None, output=None, status="outer-timeout")
            return dict(arm=arm, phase=phase, session=session, invocation=invocation, started_at=start,
                        ended_at=time.time(), elapsed=time.time()-start, **result)

        for scenario in ("unchanged", "single", "concurrent"):
            workspace = armroot / scenario
            workspace.mkdir()
            for index in range(256):
                (workspace / f"context-{index}.txt").write_text("same content\n" * 8)
            note = workspace / "note.json"
            note.write_text('{"value":1}\n')
            if scenario == "unchanged":
                (workspace / "archive.json").write_bytes(b'"' + b'a' * (9 * 1024 * 1024) + b'"')
            session = f"{arm}-{scenario}"
            records.append(hook(workspace, "pre", session, "read"))
            records.append(hook(workspace, "post", session, "read"))
            if scenario == "single":
                records.append(hook(workspace, "pre", session, "edit"))
                note.write_text('{"value": }\n')
                records.append(hook(workspace, "post", session, "edit"))
                records.append(hook(workspace, "stop", session, "stop-error"))
                records.append(hook(workspace, "pre", session, "fix"))
                note.write_text('{"value":2}\n')
                records.append(hook(workspace, "post", session, "fix"))
                records.append(hook(workspace, "stop", session, "stop-clear"))
                records.append(hook(workspace, "pre", session, "large-edit"))
                (workspace / "large.json").write_bytes(b'"' + b'b' * (9 * 1024 * 1024) + b'"')
                records.append(hook(workspace, "post", session, "large-edit"))
            if scenario == "concurrent":
                with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
                    records.extend(pool.map(lambda name: hook(workspace, "pre", session + name, "parallel"), ("-parent", "-child")))
                note.write_text('{"value":3}\n')
                with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
                    records.extend(pool.map(lambda name: hook(workspace, "post", session + name, "parallel"), ("-parent", "-child")))
            (root / "progress.json").write_text(json.dumps(records, indent=2), encoding="utf-8")
            print(f"{arm} {scenario}: {len(records)} events retained", flush=True)
        (armroot / "source-identity.json").write_text(json.dumps(identities, indent=2), encoding="utf-8")
    after = [row for row in records if row["arm"] == "after"]
    checks = {
        "bounded_valid_hooks": all(row.get("exit_code") == 0 and isinstance(row.get("output"), dict) for row in after),
        "unchanged_clean": all(row.get("output") == {} for row in after if "unchanged" in row["session"]),
        "finding_delivered": any("Value expected" in json.dumps(row.get("output")) and "note.json" in json.dumps(row.get("output")) for row in after if row["invocation"] == "edit" and row["phase"] == "post"),
        "finding_blocks_stop": any(row.get("output", {}).get("decision") == "block" and "Value expected" in row["output"].get("reason", "") for row in after if row["invocation"] == "stop-error"),
        "resolved_stop_clear": all(row.get("output") == {} for row in after if row["invocation"] == "stop-clear"),
        "large_input_explicit": any("skip" in json.dumps(row.get("output")).lower() for row in after if row["invocation"] == "large-edit" and row["phase"] == "post"),
    }
    (root / "report.json").write_text(json.dumps(dict(records=records, checks=checks,
        limitation="Controlled command-hook workload; detailed reports retained in isolated homes. No skills/model effect attributed."), indent=2), encoding="utf-8")
    print(json.dumps(checks), flush=True)
    if not all(checks.values()):
        raise SystemExit("Hook comparison incomplete; inspect retained evidence")


if __name__ == "__main__":
    main()
