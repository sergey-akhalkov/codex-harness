"""Opt-in no-model test of suite home preparation and native arm comparison.

This validates discovery/config isolation mechanics against current sources.
It does not approve a changed hook version or bypass the suite freeze gate.
"""
from __future__ import annotations

import argparse
import importlib.util
import json
from pathlib import Path
import os
import tomllib
from typing import Any

REPO = Path(__file__).resolve().parents[1]


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--discovery-only", action="store_true")
    parser.add_argument("--codex-home", type=Path, default=Path(os.environ.get("CODEX_HOME", str(Path.home() / ".codex"))))
    args = parser.parse_args()
    if not args.discovery_only:
        print("SKIP: explicit --discovery-only required; no model execution exists in this test")
        return
    spec = importlib.util.spec_from_file_location("outcome_suite", REPO / "tools/outcome_suite.py")
    assert spec and spec.loader
    suite: Any = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(suite)
    root = suite.runner.private_root("codex-outcome-suite-discovery-test-")
    base = args.codex_home.resolve()
    auth_before = (base / "auth.json").stat()
    base_before = suite.digest(base / "config.toml")
    results = []
    for arm in ("baseline", "candidate"):
        folder = root / arm
        workspace = folder / "workspace"
        workspace.mkdir(parents=True)
        home = suite.prepare_home(base, folder, workspace)
        assert (home / "auth.json").is_symlink() and (home / "auth.json").resolve() == (base / "auth.json").resolve()
        discovery = suite.runner.configure_arm(workspace, home, arm)
        roots = {str(home): "$CODEX_HOME", str(home.parent): "$USER_HOME", str(workspace): "$WORKSPACE"}
        paths = {os.path.normcase(str(Path(s["path"]).absolute())) for s in discovery["skills"] if s["name"] in suite.runner.CANDIDATES}
        configs = {"base": tomllib.loads((home / "config.toml").read_text()), "native": discovery["config"]["config"]}
        comparison = suite.normalize({k: suite.without_treatment(v, paths) for k, v in configs.items()}, roots)
        results.append({"arm": arm, "home": str(home), "discovery": discovery, "comparison": comparison})
    suite.runner.write_json(root / "observed.json", results)
    assert results[0]["comparison"] == results[1]["comparison"], "Non-treatment config differs; inspect private observed.json"
    auth_after = (base / "auth.json").stat()
    assert (auth_before.st_mtime_ns, auth_before.st_size) == (auth_after.st_mtime_ns, auth_after.st_size)
    assert suite.digest(base / "config.toml") == base_before
    suite.runner.write_json(root / "report.json", {"status": "passed", "model_calls": 0,
        "scope": "configuration and native discovery only; hook acceptance unchanged", "arms": results})
    print(json.dumps({"status": "passed", "model_calls": 0, "evidence_root": str(root)}))


if __name__ == "__main__":
    main()
