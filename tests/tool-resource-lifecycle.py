"""Isolated native-configuration transaction acceptance; never invokes live CBM."""
from pathlib import Path
import json
import os
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'tools/code-tools'))
import resources


class LifecycleTests(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory(prefix='harness-resource-lifecycle-')
        self.addCleanup(self.scratch.cleanup)
        self.root = Path(self.scratch.name)
        self.values = {'auto_index': 'true', 'auto_watch': 'true', 'ui_enabled': 'false'}
        self.original = dict(self.values)
        self.writes = []
        for context in (
            patch.dict(os.environ, {'HARNESS_TOOL_RESOURCES_DIR': str(self.root / 'state'), 'CBM_CACHE_DIR': str(self.root / 'cache')}),
            patch.object(resources, 'native', return_value='owned-fixture-cbm'),
            patch.object(resources, 'read_configuration', side_effect=lambda: dict(self.values)),
            patch.object(resources, 'config', side_effect=self.write),
            patch.object(resources, 'stop_daemon'),
        ):
            context.start()
            self.addCleanup(context.stop)
        self.pending = self.root / 'codex/harness/tool-resources-pending.json'

    def write(self, executable, key, value):
        self.assertEqual(executable, 'owned-fixture-cbm')
        self.writes.append((key, value))
        self.values[key] = value

    def invoke(self, mode, **kwargs):
        return resources.configuration(mode, pending=self.pending, **kwargs)

    def receipt(self):
        return self.root / 'state/cbm-configuration.json'

    def test_first_apply_persists_ui_and_user_revert_is_preserved(self):
        self.invoke('apply')
        self.assertIn(('ui_enabled', 'false'), self.writes)
        self.assertEqual(json.loads(self.receipt().read_text())['state'], 'complete')
        self.writes.clear()
        self.invoke('apply')
        self.assertEqual(self.writes, [])
        self.values['auto_index'] = 'true'  # Exactly the pre-install value.
        with self.assertRaisesRegex(RuntimeError, 'preserving user'):
            self.invoke('apply')
        result = self.invoke('restore')
        self.assertEqual(result['preserved_user_settings'], ['auto_index'])
        self.assertEqual(self.values, self.original)
        self.assertFalse(self.receipt().exists())

    def test_crash_after_native_write_recovers_only_transaction(self):
        def interrupted(executable, key, value):
            self.write(executable, key, value)
            raise RuntimeError('simulated client death after native commit')
        with patch.object(resources, 'config', side_effect=interrupted):
            with self.assertRaisesRegex(RuntimeError, 'client death'):
                self.invoke('apply')
        self.assertTrue(self.pending.exists())
        with self.assertRaisesRegex(RuntimeError, 'Recover'):
            self.invoke('check')
        self.invoke('recover')
        self.assertEqual(self.values, self.original)
        self.assertFalse(self.receipt().exists())

    def test_failed_update_restores_last_install_not_preinstall(self):
        self.invoke('apply')
        installed = dict(self.values)
        prior_receipt = self.receipt().read_bytes()
        newer = resources.policy()
        newer['codebase_memory']['configuration']['auto_index'] = 'true'
        with patch.object(resources, 'policy', return_value=newer):
            self.invoke('apply', defer_commit=True, transaction_id='update')
        self.assertEqual(self.values['auto_index'], 'true')
        self.invoke('recover', transaction_id='update')
        self.assertEqual(self.values, installed)
        self.assertEqual(self.receipt().read_bytes(), prior_receipt)

    def test_disconnect_rollback_and_committed_cleanup(self):
        self.invoke('apply')
        installed = dict(self.values)
        self.invoke('restore', defer_commit=True)
        self.assertEqual(self.values, self.original)
        self.invoke('recover')
        self.assertEqual(self.values, installed)
        self.invoke('restore', defer_commit=True)
        self.invoke('commit')
        self.assertEqual(self.invoke('recover')['status'], 'not-pending')
        self.assertEqual(self.values, self.original)

    def test_deferred_transaction_blocks_other_installation(self):
        self.invoke('apply', defer_commit=True)
        with self.assertRaisesRegex(RuntimeError, 'Another installation'):
            resources.configuration('apply', pending=self.root / 'other/pending.json')
        self.invoke('commit')
        resources.configuration('apply', pending=self.root / 'other/pending.json')

    def test_recovery_preserves_later_user_edit(self):
        self.invoke('apply', defer_commit=True)
        self.values['auto_index'] = 'user-value'
        result = self.invoke('recover')
        self.assertEqual(result['preserved_user_settings'], ['auto_index'])
        self.assertEqual(self.values['auto_index'], 'user-value')
        self.assertEqual(self.values['auto_watch'], 'true')

    def test_legacy_receipt_adoption_requires_exact_applied_values(self):
        self.invoke('apply')
        saved = json.loads(self.receipt().read_text())
        saved.pop('schema_version')
        saved.pop('state')
        resources.atomic_json(self.receipt(), saved)
        self.writes.clear()
        self.invoke('apply')
        self.assertEqual(self.writes, [('ui_enabled', 'false')])
        resources.atomic_json(self.receipt(), saved)
        self.values['auto_watch'] = 'true'
        with self.assertRaisesRegex(RuntimeError, 'preserving user'):
            self.invoke('apply')

    def test_failed_recovery_keeps_journal_for_retry(self):
        self.invoke('apply', defer_commit=True)
        with patch.object(resources, 'config', side_effect=RuntimeError('native unavailable')):
            with self.assertRaisesRegex(RuntimeError, 'native unavailable'):
                self.invoke('recover')
        self.assertTrue(self.pending.exists())
        self.invoke('recover')
        self.assertEqual(self.values, self.original)

    def test_failed_daemon_retirement_cannot_commit_configuration(self):
        with patch.object(resources, 'stop_daemon', side_effect=RuntimeError('daemon is still active')):
            with self.assertRaisesRegex(RuntimeError, 'still active'):
                self.invoke('apply')
        self.assertTrue(self.pending.exists())
        self.assertFalse(self.receipt().exists())
        self.invoke('recover')
        self.assertEqual(self.values, self.original)

    def test_only_last_installation_restores_shared_cache_and_reinstall_is_idempotent(self):
        first = str(self.root / 'home-a')
        second = str(self.root / 'home-b')
        self.invoke('apply', owner_key=first)
        self.invoke('apply', owner_key=first)
        self.invoke('apply', owner_key=second)
        self.assertEqual(len(json.loads(self.receipt().read_text())['owners']), 2)
        self.writes.clear()
        result = self.invoke('restore', owner_key=first)
        self.assertEqual(result['owners_remaining'], 1)
        self.assertEqual(self.writes, [])
        self.assertEqual(self.values['auto_watch'], 'false')
        self.assertEqual(self.invoke('restore', owner_key=first)['status'], 'not-owned')
        self.values['auto_index'] = 'true'  # Later user revert must still win.
        result = self.invoke('restore', owner_key=second)
        self.assertEqual(result['preserved_user_settings'], ['auto_index'])
        self.assertEqual(self.values, self.original)
        self.assertFalse(self.receipt().exists())

    def test_failed_owner_release_recovers_both_leases_without_native_changes(self):
        first, second = str(self.root / 'a'), str(self.root / 'b')
        self.invoke('apply', owner_key=first)
        self.invoke('apply', owner_key=second)
        self.writes.clear()
        self.invoke('restore', owner_key=first, defer_commit=True)
        self.invoke('recover')
        self.assertEqual(len(json.loads(self.receipt().read_text())['owners']), 2)
        self.assertEqual(self.writes, [])


if __name__ == '__main__':
    unittest.main()
