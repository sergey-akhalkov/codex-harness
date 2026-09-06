"""Graphify update ownership: host config, shared Workstation data and rollback.

Package acquisition and MCP responses are deterministic fixtures here. Directory
promotion, journals, graph selection, manifest ACL preparation and recovery are real.
"""
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools/code-tools'))
import dependencies as lifecycle
import graphify_proxy
import graphify_update as updater


class GraphifyUpdateTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='harness-graphify-update-')
        self.root = Path(self.temporary.name).resolve()
        assert self.root.parent == Path(tempfile.gettempdir()).resolve()
        self.user = self.root / 'user'
        self.codex = self.user / '.codex'
        self.state = self.codex / 'harness/dependencies'
        self.installation = self.user / 'AppData/Roaming/uv/tools/graphifyy'
        self.settings = self.codex / 'harness/graphify.json'
        self.settings.parent.mkdir(parents=True)
        self.graph = self.root / 'kit graph.json'
        self.graph.write_text('{"nodes": [{"id": "kit"}], "links": []}')
        self.settings.write_text(json.dumps({'graph_path': str(self.graph), 'credential_env': 'PRIVATE_REFERENCE'}))
        self.make_package(self.installation, '1.0.0')
        (self.installation / 'uv-receipt.toml').write_text(
            '[tool]\nrequirements = [{ name = "graphifyy", extras = ["mcp"], specifier = "==1.0.0" }]\nentrypoints = []\n')
        self.record = {'id': 'graphify', 'status': 'adopted', 'version': '1.0.0',
            'installation_root': str(self.installation),
            'paths': {'python': str(self.installation / 'Scripts/python.exe')},
            'shared_service': {'manifest': str(self.root / 'absent-workstation.json')},
            'active_consumers': {'state': 'observed', 'processes': []}}
        self.probes = []
        self.incompatible = None
        self.real_run = updater.run
        self.patches = [patch.object(updater, 'record_for', lambda _: self.record),
                        patch.object(updater, 'run', self.command),
                        patch.object(updater, 'graph_probe', self.probe)]
        for substitute in self.patches:
            substitute.start()
        lifecycle.TRANSACTION_ID = 'fixture-' + self.root.name

    def tearDown(self):
        for substitute in reversed(self.patches):
            substitute.stop()
        lifecycle.TRANSACTION_ID = None
        self.temporary.cleanup()

    def make_package(self, folder, version):
        (folder / 'Scripts').mkdir(parents=True)
        for name in ('python.exe', 'pythonw.exe', 'graphify.exe'):
            (folder / 'Scripts' / name).write_bytes(b'owned fixture only; never executed')
        package = folder / 'Lib/site-packages'
        entries = package / f'graphifyy-{version}.dist-info'
        entries.mkdir(parents=True)
        (entries / 'entry_points.txt').write_text('[console_scripts]\ngraphify = graphify.cli:main\n')
        (package / 'graphify').mkdir()
        (package / 'graphify/serve.py').write_text('version = ' + repr(version))

    def command(self, args, *, environment=None):
        values = list(map(str, args))
        if values[1:3] == ['tool', 'install']:
            self.make_package(Path(environment['UV_TOOL_DIR']) / 'graphifyy', '1.0.1')
            return ''
        if values[1:4] == ['-B', '-c', 'import sys; print(sys._base_executable)']:
            return sys.executable
        if values[1:3] == ['venv', '--allow-existing'] or values[1:3] == ['pip', 'install']:
            return ''
        if values[-1] == '--version':
            return 'graphify 1.0.1'
        return self.real_run(args, environment=environment)

    async def probe(self, python, graph):
        graph = Path(graph).resolve()
        self.probes.append((Path(python), graph))
        content = graph.read_text()
        if graph == self.incompatible and Path(python) != self.installation / 'Scripts/python.exe':
            content += ' incompatible'
        return {'stats': content, 'tools': ['graph_stats', 'query_graph', 'get_node', 'list_prs']}

    def workstation(self):
        graph = self.root / 'workstation graph.json'
        graph.write_text('{"nodes": [{"id": "workstation"}], "links": []}')
        module = self.installation / 'Lib/site-packages/graphify/serve.py'
        manifest = self.root / 'workstation.json'
        manifest.write_text(json.dumps({'foreign': {'preserved': True}, 'graphify': {'configuration': {
            'graph': {'path': str(graph)}, 'module': {'packageVersion': '1.0.0',
            'source': {'path': str(module), 'sha256': lifecycle.discovery.fingerprint(module), 'length': module.stat().st_size}}}}}))
        self.record['shared_service'] = {'graph_path': str(graph), 'manifest': str(manifest)}
        return graph, manifest

    def stage(self, **kwargs):
        return updater.stage(self.user, self.state, '1.0.1', **kwargs)

    def promotion(self, staged):
        prepared = updater.prepare_manifest(staged['stage_manifest'])
        auxiliary = json.loads(Path(prepared['auxiliary_files']).read_text())
        return updater.promote(self.user, self.state, staged['stage_manifest'], auxiliary)

    def test_host_graph_without_workstation_stages_promotes_and_recovers(self):
        before = lifecycle.tree_identity(self.installation)
        settings_before = self.settings.read_bytes()
        graph_before = self.graph.read_bytes()
        staged = self.stage()
        self.assertIsNone(staged['protected_manifest'])
        prepared = updater.prepare_manifest(staged['stage_manifest'])
        self.assertEqual(json.loads(Path(prepared['auxiliary_files']).read_text()), [])
        result = updater.promote(self.user, self.state, staged['stage_manifest'], [])
        self.assertEqual(result['state'], 'updated')
        self.assertNotEqual(lifecycle.tree_identity(self.installation), before)
        recovered = lifecycle.recover_transaction(result['transaction_journal'], rollback_committed=True)
        self.assertEqual(recovered['state'], 'restored')
        self.assertEqual(lifecycle.tree_identity(self.installation), before)
        self.assertEqual(self.settings.read_bytes(), settings_before)
        self.assertEqual(self.graph.read_bytes(), graph_before)

    def test_explicit_codex_home_with_independent_dependency_state(self):
        self.state = self.root / 'independent-state'
        staged = self.stage(codex_home=self.codex)
        self.assertEqual(Path(staged['graph']), self.graph)
        self.assertEqual(Path(staged['settings_path']), self.settings)
        registry = self.settings.with_name('code-tools.json')
        registry.write_text(json.dumps({'mcp': [self.record]}))
        with patch.dict(os.environ, {'HARNESS_CODE_TOOLS_REGISTRY': str(registry)}):
            self.assertEqual(graphify_proxy.configuration()[1], staged['graph'])

    def test_missing_graph_fails_before_package_acquisition(self):
        self.settings.unlink()
        with patch.object(updater, 'run') as acquisition:
            with self.assertRaisesRegex(RuntimeError, 'graph is missing'):
                self.stage()
            acquisition.assert_not_called()
        self.assertFalse(self.state.exists())

    def test_changed_host_settings_preserve_both_packages(self):
        staged = self.stage()
        before = lifecycle.tree_identity(self.installation)
        self.settings.write_text(json.dumps({'graph_path': str(self.graph), 'new_user_setting': True}))
        with self.assertRaisesRegex(ValueError, 'settings changed'):
            self.promotion(staged)
        self.assertEqual(lifecycle.tree_identity(self.installation), before)
        self.assertTrue(Path(staged['candidate']).is_dir())

    def test_workstation_only_without_settings_retains_manifest_transaction(self):
        graph, manifest = self.workstation()
        self.settings.unlink()
        original = manifest.read_bytes()
        staged = self.stage()
        self.assertEqual(staged['graph'], str(graph))
        result = self.promotion(staged)
        self.assertEqual(json.loads(manifest.read_text())['graphify']['configuration']['module']['packageVersion'], '1.0.1')
        self.assertEqual(lifecycle.recover_transaction(result['transaction_journal'], rollback_committed=True)['state'], 'restored')
        self.assertEqual(manifest.read_bytes(), original)

    def test_kit_override_validates_and_preserves_both_graphs(self):
        other_graph, manifest = self.workstation()
        originals = {file: file.read_bytes() for file in (self.graph, other_graph, manifest, self.settings)}
        staged = self.stage()
        self.assertEqual({entry['graph'] for entry in staged['graph_checks']}, {str(self.graph), str(other_graph)})
        result = self.promotion(staged)
        self.assertEqual(json.loads(manifest.read_text())['graphify']['configuration']['graph']['path'], str(other_graph))
        self.assertEqual(lifecycle.recover_transaction(result['transaction_journal'], rollback_committed=True)['state'], 'restored')
        for file, content in originals.items():
            self.assertEqual(file.read_bytes(), content)
        for graph in (self.graph, other_graph):
            self.assertGreaterEqual(sum(observed == graph for _, observed in self.probes), 3)

    def test_incompatible_workstation_graph_blocks_kit_selected_update(self):
        self.incompatible, _ = self.workstation()
        before = lifecycle.tree_identity(self.installation)
        with self.assertRaisesRegex(RuntimeError, 'query/statistics contract'):
            self.stage()
        self.assertEqual(lifecycle.tree_identity(self.installation), before)

    def test_new_workstation_consumer_after_staging_blocks_promotion(self):
        staged = self.stage()
        self.workstation()
        before = lifecycle.tree_identity(self.installation)
        with self.assertRaisesRegex(ValueError, 'manifest changed'):
            self.promotion(staged)
        self.assertEqual(lifecycle.tree_identity(self.installation), before)


if __name__ == '__main__':
    unittest.main()
