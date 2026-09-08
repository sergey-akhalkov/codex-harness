"""Opt-in real offline Graphify staging/promotion/recovery in an owned fake home.

Copies the selected installed environment for a disposable test, stages the same
version through existing UV's offline cache, performs actual MCP graph calls and
recovers the prior directory. No global installation or credential is changed.
"""
import argparse
import json
import os
from pathlib import Path
import shutil
import sys
import tempfile
import tomllib
from collections.abc import Callable
from typing import NotRequired, Protocol, TypedDict, cast, runtime_checkable
from unittest.mock import patch
from native_contracts import Json

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools/code-tools'))
import dependencies as lifecycle_module
import graphify_update as updater_module


class Consumers(TypedDict):
    state: str
    processes: list[object]


class Record(TypedDict):
    installation_root: str
    active_consumers: NotRequired[Consumers]


class ConsumerDiscovery(Protocol):
    def inspect_consumers(self, records: list[Record]) -> None: ...


class DiscoveryModule(Protocol):
    Discovery: Callable[[Path], ConsumerDiscovery]


class Recovery(TypedDict):
    complete: bool
    results: list[dict[str, object]]


@runtime_checkable
class Lifecycle(Protocol):
    TRANSACTION_ID: str | None
    discovery: DiscoveryModule
    def tree_identity(self, path: Path) -> str | None: ...
    def recover_dependencies(self, state_dir: Path, user_home: Path, transaction_id: str | None = None,
                             rollback_committed: bool = False) -> Recovery: ...


class Staged(TypedDict):
    protected_manifest: str | None
    stage_manifest: str


@runtime_checkable
class Updater(Protocol):
    def stage(self, user_home: Path, state_dir: Path, version: str) -> Staged: ...
    def prepare_manifest(self, stage_manifest: str) -> dict[str, str | None]: ...
    def promote(self, user_home: Path, state_dir: Path, stage_manifest: str,
                auxiliary_files: list[dict[str, str]]) -> dict[str, object]: ...


assert isinstance(lifecycle_module, Lifecycle)
assert isinstance(updater_module, Updater)
lifecycle: Lifecycle = lifecycle_module
updater: Updater = updater_module


class Arguments(argparse.Namespace):
    source_installation: Path = Path()


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    _ = parser.add_argument('--source-installation', type=Path, required=True)
    args = Arguments()
    _ = parser.parse_args(namespace=args)
    source = args.source_installation.resolve()
    if not (source / 'uv-receipt.toml').is_file() or not (source / 'Scripts/python.exe').is_file():
        raise ValueError('An existing Graphify UV environment is required')
    source_identity = lifecycle.tree_identity(source)
    # Cleanup is explicit: failed safety checks must retain the fixture.
    root = Path(tempfile.mkdtemp(prefix='harness-graphify-offline-update-')).resolve()
    assert root.parent == Path(tempfile.gettempdir()).resolve()
    user = root / 'user'
    installation = user / 'AppData/Roaming/uv/tools/graphifyy'
    state = user / '.codex/harness/dependencies'
    try:
        print('Copying the existing environment into an owned fixture.', flush=True)
        _ = shutil.copytree(source, installation, ignore=shutil.ignore_patterns('__pycache__', '*.pyc', '*.pyo'))
        # Public wrapper targets must not point to the real user's environment.
        # Console entrypoint behavior is still exercised inside the staged env.
        receipt = cast(dict[str, Json], tomllib.loads((installation / 'uv-receipt.toml').read_text()))
        tool = receipt['tool']
        assert isinstance(tool, dict)
        requirements = tool['requirements']
        assert isinstance(requirements, list) and requirements
        requirement = requirements[0]
        assert isinstance(requirement, dict)
        specifier = requirement['specifier']
        assert isinstance(specifier, str)
        version = specifier.removeprefix('==')
        _ = (installation / 'uv-receipt.toml').write_text(
            '[tool]\nrequirements = [{ name = "graphifyy", extras = ["mcp"], specifier = "==' + version + '" }]\nentrypoints = []\n')
        graph = root / 'owned graph.json'
        _ = graph.write_text(json.dumps({'directed': False, 'multigraph': False, 'graph': {},
            'nodes': [{'id': 'graphify', 'label': 'GRAPHIFY_OFFLINE_FIXTURE', 'community': 0}], 'links': []}))
        settings = state.parent / 'graphify.json'
        settings.parent.mkdir(parents=True)
        _ = settings.write_text(json.dumps({'graph_path': str(graph)}))
        settings_before, graph_before = settings.read_bytes(), graph.read_bytes()
        prior_identity = lifecycle.tree_identity(installation)
        lifecycle.TRANSACTION_ID = 'offline-fixture'
        print('Staging with real UV offline acquisition and MCP compatibility calls.', flush=True)
        with patch.dict(os.environ, {'UV_OFFLINE': '1', 'PYTHONDONTWRITEBYTECODE': '1'}):
            staged = updater.stage(user, state, version)
            assert staged['protected_manifest'] is None
            prepared = updater.prepare_manifest(staged['stage_manifest'])
            auxiliary_path = prepared['auxiliary_files']
            assert auxiliary_path is not None, 'Prepared manifest must provide its auxiliary file list'
            auxiliary = cast(list[dict[str, str]], json.loads(Path(auxiliary_path).read_text()))
            assert auxiliary == []
            print('Promoting the owned fixture and validating actual relocated Graphify.', flush=True)
            result = updater.promote(user, state, staged['stage_manifest'], auxiliary)
            assert result['state'] == 'updated' and result['transaction_journal'], result
            recovered = lifecycle.recover_dependencies(state, user, lifecycle.TRANSACTION_ID, rollback_committed=True)
        assert recovered['complete'] and all(item['state'] == 'restored' for item in recovered['results']), recovered
        assert lifecycle.tree_identity(installation) == prior_identity
        assert graph.read_bytes() == graph_before and settings.read_bytes() == settings_before
        assert lifecycle.tree_identity(source) == source_identity
        print(json.dumps({'state': 'passed', 'version': version, 'offline': True,
            'native_discovery_and_process_inspection': True, 'native_mcp_stage_and_promotion': True,
            'no_workstation_manifest': True, 'host_graph_preserved': True, 'recover_exact_prior_package': True,
            'source_installation_unchanged': True, 'transaction_result_key': 'transaction_journal',
            'limit': 'Same-version fixture exercises update mechanics; cross-version compatibility remains separately validated.'}), flush=True)
    finally:
        lifecycle.TRANSACTION_ID = None
        # Only the newly created, ordinary test tree may be recursively removed.
        for folder, directories, files in os.walk(root, followlinks=False):
            for name in [*directories, *files]:
                target = Path(folder) / name
                if target.is_symlink() or target.is_junction() or not target.resolve().is_relative_to(root):
                    raise RuntimeError('Fixture cleanup pending: an unexpected link must be preserved')
        probe: list[Record] = [{'installation_root': str(root)}]
        lifecycle.discovery.Discovery(user).inspect_consumers(probe)
        consumers = probe[0].get('active_consumers')
        if consumers is None or consumers['state'] != 'observed' or consumers['processes']:
            raise RuntimeError('Fixture cleanup pending: active or uninspectable fixture processes; ' + str(root))
        if root.is_symlink() or root.is_junction() or root.resolve().parent != Path(tempfile.gettempdir()).resolve():
            raise RuntimeError('Fixture cleanup pending: owned root identity changed; ' + str(root))
        shutil.rmtree(root)


if __name__ == '__main__':
    main()
