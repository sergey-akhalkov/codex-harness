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
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools/code-tools'))
import dependencies as lifecycle
import graphify_update as updater


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source-installation', type=Path, required=True)
    args = parser.parse_args()
    source = args.source_installation.resolve()
    if not (source / 'uv-receipt.toml').is_file() or not (source / 'Scripts/python.exe').is_file():
        raise ValueError('An existing Graphify UV environment is required')
    source_identity = lifecycle.tree_identity(source)
    temporary = tempfile.TemporaryDirectory(prefix='harness-graphify-offline-update-')
    root = Path(temporary.name).resolve()
    assert root.parent == Path(tempfile.gettempdir()).resolve()
    user = root / 'user'
    installation = user / 'AppData/Roaming/uv/tools/graphifyy'
    state = user / '.codex/harness/dependencies'
    try:
        print('Copying the existing environment into an owned fixture.', flush=True)
        shutil.copytree(source, installation, ignore=shutil.ignore_patterns('__pycache__', '*.pyc', '*.pyo'))
        # Public wrapper targets must not point to the real user's environment.
        # Console entrypoint behavior is still exercised inside the staged env.
        receipt = updater.tomllib.loads((installation / 'uv-receipt.toml').read_text())
        version = receipt['tool']['requirements'][0]['specifier'].removeprefix('==')
        (installation / 'uv-receipt.toml').write_text(
            '[tool]\nrequirements = [{ name = "graphifyy", extras = ["mcp"], specifier = "==' + version + '" }]\nentrypoints = []\n')
        graph = root / 'owned graph.json'
        graph.write_text(json.dumps({'directed': False, 'multigraph': False, 'graph': {},
            'nodes': [{'id': 'graphify', 'label': 'GRAPHIFY_OFFLINE_FIXTURE', 'community': 0}], 'links': []}))
        settings = state.parent / 'graphify.json'
        settings.parent.mkdir(parents=True)
        settings.write_text(json.dumps({'graph_path': str(graph)}))
        settings_before, graph_before = settings.read_bytes(), graph.read_bytes()
        prior_identity = lifecycle.tree_identity(installation)
        lifecycle.TRANSACTION_ID = 'offline-fixture'
        print('Staging with real UV offline acquisition and MCP compatibility calls.', flush=True)
        with patch.dict(os.environ, {'UV_OFFLINE': '1', 'PYTHONDONTWRITEBYTECODE': '1'}):
            staged = updater.stage(user, state, version)
            assert staged['protected_manifest'] is None
            prepared = updater.prepare_manifest(staged['stage_manifest'])
            auxiliary = json.loads(Path(prepared['auxiliary_files']).read_text())
            assert auxiliary == []
            print('Promoting the owned fixture and validating actual relocated Graphify.', flush=True)
            result = updater.promote(user, state, staged['stage_manifest'], auxiliary)
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
                    temporary._finalizer.detach()
                    raise RuntimeError('Fixture cleanup pending: an unexpected link must be preserved')
        probe = [{'installation_root': str(root)}]
        lifecycle.discovery.Discovery(user).inspect_consumers(probe)
        if probe[0]['active_consumers']['state'] != 'observed' or probe[0]['active_consumers']['processes']:
            temporary._finalizer.detach()
            raise RuntimeError('Fixture cleanup pending: active or uninspectable fixture processes; ' + str(root))
        temporary.cleanup()


if __name__ == '__main__':
    main()
