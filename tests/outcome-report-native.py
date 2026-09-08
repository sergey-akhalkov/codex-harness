"""Explicit native discovery checks; installer links in private homes, no models.

python tests/outcome-report-native.py --native-discovery --codex-command PATH
Leaves private evidence and installations recoverable; never touches global
registrations, authentication, provider settings or existing user HOME.
"""
from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import shutil
import subprocess
from typing import Any

REPO = Path(__file__).resolve().parents[1]


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--native-discovery", action="store_true")
    parser.add_argument("--codex-command", type=Path)
    args = parser.parse_args()
    if not args.native_discovery:
        print("SKIP: explicit --native-discovery required; this check never calls a model")
        return
    if not args.codex_command:
        parser.error("--codex-command must identify the installed native launcher")
    spec = importlib.util.spec_from_file_location("outcome_runner", REPO / "tools/outcome_runner.py")
    assert spec and spec.loader
    runner: Any = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(runner)
    root = runner.private_root("codex-outcome-native-discovery-test-")
    pwsh = shutil.which("pwsh")
    assert pwsh
    results = []
    for arm in ("baseline", "candidate"):
        user = root / arm / "user with spaces"
        home = user / ".codex"
        case = root / arm / "case"
        case.mkdir(parents=True)
        with (root / (arm + "-install.txt")).open("w", encoding="utf-8") as log:
            subprocess.run([pwsh, "-NoLogo", "-NoProfile", "-File", str(REPO / "install.ps1"),
                "-Mode", "Install", "-CoreOnly", "-PathScope", "Process", "-CodexHome", str(home),
                "-UserHome", str(user), "-CodexCommand", str(args.codex_command)],
                stdout=log, stderr=subprocess.STDOUT, check=True, timeout=90,
                creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
        # Native Windows uses the actual Known Folder even with HOME overrides.
        # Route the isolated CODEX_HOME discovery root to installer-owned links.
        # No source skill path is passed to a model or copied to this fixture.
        (home / "skills").mkdir(exist_ok=True)
        for registration in (user / ".agents/skills").iterdir():
            if registration.name.startswith("."):
                continue
            (home / "skills" / registration.name).symlink_to(registration, target_is_directory=True)
        results.append(runner.configure_arm(case, home, arm))
    # Native system skill bodies are generated separately in each private home;
    # compare their content identity, not these deliberately different paths.
    other = lambda r: sorted((s["name"], hashlib.sha256(Path(s["path"]).read_bytes()).hexdigest(), s["enabled"])
                             for s in r["skills"] if s["name"] not in runner.CANDIDATES)
    assert other(results[0]) == other(results[1]), "Other discovered skills differ between arms"
    runner.write_json(root / "report.json", {"status": "passed", "model_calls": 0, "arms": results})
    print(json.dumps({"status": "passed", "model_calls": 0, "evidence_root": str(root)}))


if __name__ == "__main__":
    main()
