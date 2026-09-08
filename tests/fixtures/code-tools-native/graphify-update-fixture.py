"""Exercise the actual Graphify promotion result and directory journal on owned fixtures.
Only graph protocol/process discovery are substituted; no installed package is updated.
"""
import argparse
import json
from pathlib import Path
import sys
from collections.abc import Callable, Coroutine
from typing import Protocol, override, runtime_checkable

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "tools/code-tools"))
import dependencies as dependencies_module
import graphify_update as graphify_module


class Discovery(Protocol):
    def fingerprint(self, path: Path) -> str: ...


@runtime_checkable
class Dependencies(Protocol):
    TRANSACTION_ID: str | None
    discovery: Discovery

    def recover_transaction(self, path: Path, rollback_committed: bool = False) -> dict[str, object]: ...
    def tree_identity(self, path: Path) -> str | None: ...


@runtime_checkable
class GraphifyUpdate(Protocol):
    record_for: Callable[[Path], dict[str, object]]
    graph_probe: Callable[[Path, Path], Coroutine[object, object, dict[str, str | list[str]]]]

    def promote(self, user_home: Path, state_dir: Path, stage_manifest: Path,
                auxiliary_files: list[dict[str, str]]) -> dict[str, object]: ...


assert isinstance(dependencies_module, Dependencies)
assert isinstance(graphify_module, GraphifyUpdate)
dependencies: Dependencies = dependencies_module
graphify_update: GraphifyUpdate = graphify_module

parser = argparse.ArgumentParser()
_ = parser.add_argument("operation", choices=("promote", "recover"))
_ = parser.add_argument("--user-home", type=Path)
_ = parser.add_argument("--state-dir", type=Path)
_ = parser.add_argument("--transaction-id")
_ = parser.add_argument("--journal", type=Path)


class FixtureArgs(argparse.Namespace):
    @override
    def __init__(self) -> None:
        super().__init__()
        self.operation: str = ""
        self.user_home: Path | None = None
        self.state_dir: Path | None = None
        self.transaction_id: str | None = None
        self.journal: Path | None = None


args = parser.parse_args(namespace=FixtureArgs())
if args.operation == "recover":
    if args.journal is None:
        parser.error("recover requires --journal")
    result = dependencies.recover_transaction(args.journal, rollback_committed=True)
    print(json.dumps({"complete": result["state"] == "restored", "results": [result]}))
else:
    if args.user_home is None or args.state_dir is None:
        parser.error("promote requires --user-home and --state-dir")
    if args.user_home.resolve() == Path.home().resolve() or "harness-activation-" not in str(args.user_home):
        raise ValueError("Only the activation suite's disposable user home is allowed")
    dependencies.TRANSACTION_ID = args.transaction_id
    install = args.user_home / "AppData/Roaming/uv/tools/graphifyy"
    candidate = args.state_dir / "staging/graphify-fixture/candidate"
    for directory, version in ((install, "old"), (candidate, "new")):
        (directory / "Scripts").mkdir(parents=True)
        _ = (directory / "Scripts/python.exe").write_bytes(b"fixture-only-not-an-executable")
        _ = (directory / "version.txt").write_text(version)
    graph = candidate.parent / "graph.json"
    _ = graph.write_text('{"nodes": [], "links": []}')
    evidence: dict[str, str | list[str]] = {"stats": "fixture graph", "tools": ["graph_stats"]}
    staged = {"id": "graphify", "version": "0.9.55", "old_version": "0.9.44",
              "installation": str(install), "candidate": str(candidate),
              "prior_identity": dependencies.tree_identity(install),
              "candidate_identity": dependencies.tree_identity(candidate),
              "graph": str(graph), "graph_sha256": dependencies.discovery.fingerprint(graph),
              "python_sha256": dependencies.discovery.fingerprint(install / "Scripts/python.exe"),
              "wrapper_fingerprints": {}, "protected_manifest": None, "evidence": evidence}
    manifest = candidate.parent / "stage.json"
    _ = manifest.write_text(json.dumps(staged))

    def fixture_record(_user_home: Path) -> dict[str, object]:
        return {"installation_root": str(install),
                "active_consumers": {"state": "observed", "processes": []}}

    graphify_update.record_for = fixture_record

    async def fixture_probe(_python: Path, _graph: Path) -> dict[str, str | list[str]]:
        return evidence

    graphify_update.graph_probe = fixture_probe
    result = graphify_update.promote(args.user_home, args.state_dir, manifest, [])
    if (install / "version.txt").read_text() != "new":
        raise AssertionError("Actual Graphify directory promotion did not happen")
    print(json.dumps(result))
