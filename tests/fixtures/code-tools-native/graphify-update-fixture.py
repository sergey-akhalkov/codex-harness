"""Exercise the actual Graphify promotion result and directory journal on owned fixtures.
Only graph protocol/process discovery are substituted; no installed package is updated.
"""
import argparse
import importlib.util
import json
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "tools/code-tools"))
import dependencies
import graphify_update

parser = argparse.ArgumentParser()
parser.add_argument("operation", choices=("promote", "recover"))
parser.add_argument("--user-home", type=Path)
parser.add_argument("--state-dir", type=Path)
parser.add_argument("--transaction-id")
parser.add_argument("--journal", type=Path)
args = parser.parse_args()
if args.operation == "recover":
    result = dependencies.recover_transaction(args.journal, rollback_committed=True)
    print(json.dumps({"complete": result["state"] == "restored", "results": [result]}))
else:
    if args.user_home.resolve() == Path.home().resolve() or "harness-activation-" not in str(args.user_home):
        raise ValueError("Only the activation suite's disposable user home is allowed")
    dependencies.TRANSACTION_ID = args.transaction_id
    install = args.user_home / "AppData/Roaming/uv/tools/graphifyy"
    candidate = args.state_dir / "staging/graphify-fixture/candidate"
    for directory, version in ((install, "old"), (candidate, "new")):
        (directory / "Scripts").mkdir(parents=True)
        (directory / "Scripts/python.exe").write_bytes(b"fixture-only-not-an-executable")
        (directory / "version.txt").write_text(version)
    graph = candidate.parent / "graph.json"
    graph.write_text('{"nodes": [], "links": []}')
    evidence = {"stats": "fixture graph", "tools": ["graph_stats"]}
    staged = {"id": "graphify", "version": "0.9.55", "old_version": "0.9.44",
              "installation": str(install), "candidate": str(candidate),
              "prior_identity": dependencies.tree_identity(install),
              "candidate_identity": dependencies.tree_identity(candidate),
              "graph": str(graph), "graph_sha256": dependencies.discovery.fingerprint(graph),
              "python_sha256": dependencies.discovery.fingerprint(install / "Scripts/python.exe"),
              "wrapper_fingerprints": {}, "protected_manifest": None, "evidence": evidence}
    manifest = candidate.parent / "stage.json"
    manifest.write_text(json.dumps(staged))
    graphify_update.record_for = lambda _: {"installation_root": str(install),
        "active_consumers": {"state": "observed", "processes": []}}

    async def fixture_probe(*_):
        return evidence

    graphify_update.graph_probe = fixture_probe
    result = graphify_update.promote(args.user_home, args.state_dir, manifest, [])
    if (install / "version.txt").read_text() != "new":
        raise AssertionError("Actual Graphify directory promotion did not happen")
    print(json.dumps(result))
