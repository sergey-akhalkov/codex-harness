"""Actual native editor integration; disposable config homes, no model calls/packages."""
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('registration', ROOT / 'tools/code-tools/registration.py')
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class RegistrationTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        state = json.loads((Path.home() / '.codex/harness/installation.json').read_text(encoding='utf-8-sig'))
        vendor = Path(state['codexCommand']).parent / 'node_modules/@openai/codex/node_modules/@openai/codex-win32-x64/vendor'
        cls.native = next(vendor.rglob('codex.exe'))
        cls.powershell = Path(shutil.which('pwsh'))

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='harness-registration-')
        self.addCleanup(self.temp.cleanup)
        self.home = Path(self.temp.name)
        self.config = self.home / 'config.toml'
        self.original = b'# retain my comment\nmodel = "gpt-6-astra"\n[mcp_servers.foreign]\ncommand = "untouched.exe"\nargs = []\n'
        self.config.write_bytes(self.original)

    def run_mode(self, mode, preview=False):
        return module.run(self.home, ROOT, self.native, self.powershell, mode, preview)

    def test_preview_has_no_filesystem_effect(self):
        files = list(self.home.iterdir())
        result = self.run_mode('Install', True)
        self.assertEqual(6, len(result['operations']))
        self.assertEqual(files, list(self.home.iterdir()))
        self.assertEqual(self.original, self.config.read_bytes())

    def test_native_install_idempotence_and_disconnect_preserve_unrelated(self):
        self.assertEqual('connected', self.run_mode('Install')['status'])
        connected = self.config.read_bytes()
        self.assertTrue(connected.startswith(module.READINESS_STATEMENT.encode() + self.original))
        self.assertIn(b'# retain my comment', connected)
        self.assertIn(b'untouched.exe', connected)
        self.assertEqual('connected', self.run_mode('Check')['status'])
        self.assertEqual('unchanged-registration', self.run_mode('Install')['status'])
        self.assertEqual(connected, self.config.read_bytes())
        self.assertEqual('disconnected', self.run_mode('Disconnect')['status'])
        self.assertEqual(self.original, self.config.read_bytes())
        self.assertEqual(module.tomllib.loads(self.original.decode()), module.tomllib.loads(self.config.read_text()))

    def test_unmanaged_collision_is_preserved(self):
        self.config.write_bytes(self.original + b'\n[mcp_servers.serena]\ncommand = "mine.exe"\n')
        before = self.config.read_bytes()
        with self.assertRaisesRegex(ValueError, 'ownership conflict'):
            self.run_mode('Install')
        self.assertEqual(before, self.config.read_bytes())
        self.assertFalse((self.home / 'harness').exists())

    def test_owned_entry_changed_by_user_is_preserved(self):
        self.run_mode('Install')
        edited = self.config.read_bytes().replace(b'-NoLogo', b'-NoLogo-UserEdit', 1)
        self.config.write_bytes(edited)
        with self.assertRaisesRegex(ValueError, 'ownership conflict'):
            self.run_mode('Disconnect')
        self.assertEqual(edited, self.config.read_bytes())

    def test_recover_refuses_to_overwrite_concurrent_edit(self):
        pending = self.home / 'harness/code-tools-registration-pending.json'
        module.atomic(pending, module.json_bytes({'before': module.base64.b64encode(self.original).decode(),
                                                'after_hash': module.sha(b'planned'), 'previous_state': None}))
        self.config.write_bytes(self.original + b'# concurrent edit\n')
        before = self.config.read_bytes()
        with self.assertRaisesRegex(ValueError, 'Config changed'):
            self.run_mode('Recover')
        self.assertEqual(before, self.config.read_bytes())
        self.assertTrue(pending.exists())

    def test_recover_restores_known_interrupted_write(self):
        pending = self.home / 'harness/code-tools-registration-pending.json'
        changed = self.original + b'# staged state\n'
        module.atomic(pending, module.json_bytes({'before': module.base64.b64encode(self.original).decode(),
                                                'after_hash': module.sha(changed), 'previous_state': None}))
        self.config.write_bytes(changed)
        self.assertEqual('registration-recovered', self.run_mode('Recover')['status'])
        self.assertEqual(self.original, self.config.read_bytes())
        self.assertFalse(pending.exists())

    def test_deferred_commit_restores_absent_config_and_state(self):
        self.config.unlink()
        module.run(self.home, ROOT, self.native, self.powershell, 'Install', defer_commit=True)
        self.assertTrue((self.home / 'harness/code-tools-registration-pending.json').is_file())
        self.assertIn(b'mcp_servers.serena', self.config.read_bytes())
        self.run_mode('Recover')
        self.assertFalse(self.config.exists())
        self.assertFalse((self.home / 'harness/code-tools-registration.json').exists())

    def test_deferred_disconnect_restores_exact_noncanonical_state_bytes(self):
        self.run_mode('Install')
        state_path = self.home / 'harness/code-tools-registration.json'
        prior_state = b'\xef\xbb\xbf' + state_path.read_bytes().replace(b'  ', b'    ')
        # read_json deliberately expects JSON UTF-8 (without a BOM).
        prior_state = prior_state[3:] + b'  \n'
        state_path.write_bytes(prior_state)
        prior_config = self.config.read_bytes()
        module.run(self.home, ROOT, self.native, self.powershell, 'Disconnect', defer_commit=True)
        self.assertEqual(self.original, self.config.read_bytes())
        self.run_mode('Recover')
        self.assertEqual(prior_state, state_path.read_bytes())
        self.assertEqual(prior_config, self.config.read_bytes())

    def test_recover_preserves_intervening_registration_state_before_config_write(self):
        module.run(self.home, ROOT, self.native, self.powershell, 'Install', defer_commit=True)
        state_path = self.home / 'harness/code-tools-registration.json'
        changed = state_path.read_bytes() + b' \n'
        state_path.write_bytes(changed)
        config = self.config.read_bytes()
        with self.assertRaisesRegex(ValueError, 'Registration state changed'):
            self.run_mode('Recover')
        self.assertEqual(changed, state_path.read_bytes())
        self.assertEqual(config, self.config.read_bytes())

    def test_actual_native_rewrite_accepts_semantic_ownership_and_retains_foreign(self):
        self.run_mode('Install')
        process = subprocess.run([str(self.native), 'mcp', 'add', 'foreign-second', '--', 'foreign-second.exe'],
                                 env={**os.environ, 'CODEX_HOME': str(self.home)}, capture_output=True)
        self.assertEqual(0, process.returncode)
        normalized = self.config.read_bytes()
        owned = module.read_json(self.home / 'harness/code-tools-registration.json')
        self.assertNotIn(owned['block'].encode(), normalized)
        self.assertEqual('connected', self.run_mode('Check')['status'])
        self.assertEqual('unchanged-registration', self.run_mode('Install')['status'])
        self.assertEqual(normalized, self.config.read_bytes())
        self.run_mode('Disconnect')
        after = self.config.read_bytes()
        expected = module.tomllib.loads(normalized.decode())
        expected.pop(module.READINESS_KEY)
        for name in module.NAMES:
            expected['mcp_servers'].pop(name)
        self.assertEqual(expected, module.tomllib.loads(after.decode()))
        self.assertIn(b'# retain my comment', after)
        self.assertIn(b'[mcp_servers.foreign-second]\ncommand = "foreign-second.exe"', after)

    def test_interleaved_foreign_sections_and_multiline_fake_headers_survive(self):
        self.run_mode('Install')
        body = self.config.read_text()
        marker = '# END codex-harness MCP registrations\n'
        foreign = ('\n# native TUI project trust and user prose\n'
                   '[projects."D:/foreign project"]\ntrust_level = "trusted"\n'
                   'notes = """Literal header, not a table:\n[mcp_servers.serena]\n'
                   '# END codex-harness MCP registrations\ncommand = \\\"foreign prose\\\"\n"""\n')
        body = body.replace('[mcp_servers.graphify]', foreign + '\n[mcp_servers.graphify]')
        self.config.write_text(body, newline='')
        before = self.config.read_bytes()
        self.assertEqual('connected', self.run_mode('Check')['status'])
        self.run_mode('Disconnect')
        after = self.config.read_bytes()
        self.assertIn(foreign.encode(), after)
        expected = module.tomllib.loads(before.decode())
        expected.pop(module.READINESS_KEY)
        for name in module.NAMES:
            expected['mcp_servers'].pop(name)
        self.assertEqual(expected, module.tomllib.loads(after.decode()))

    def test_quoted_dotted_owned_tables_bom_and_intervening_comment_survive(self):
        self.run_mode('Install')
        body = self.config.read_text().replace('[mcp_servers.serena]', '["mcp_servers"."serena"]')
        comment = '# user note before the next registration\n'
        body = body.replace('[mcp_servers.graphify]', comment + '[mcp_servers.graphify]')
        self.config.write_bytes(b'\xef\xbb\xbf' + body.encode())
        self.run_mode('Disconnect')
        after = self.config.read_bytes()
        self.assertTrue(after.startswith(b'\xef\xbb\xbf'))
        self.assertIn(comment.encode(), after)
        self.assertEqual({'foreign'}, set(module.tomllib.loads(after.decode('utf-8-sig'))['mcp_servers']))

    def test_existing_zero_is_retained_exactly_and_nonzero_is_a_conflict(self):
        self.original = b'"mcp_optional_startup_grace_ms" = 0 # user selection\n' + self.original
        self.config.write_bytes(self.original)
        self.run_mode('Install')
        state = module.read_json(self.home / 'harness/code-tools-registration.json')
        self.assertTrue(state['connection_policy']['previous_present'])
        self.assertNotIn('prefix', state['connection_policy'])
        self.assertEqual(64, len(state['connection_policy']['prefix_hash']))
        self.run_mode('Disconnect')
        self.assertEqual(self.original, self.config.read_bytes())
        for value in ('1000', 'false', '0.0'):
            conflicting = self.original.replace(b'= 0 #', ('= ' + value + ' #').encode(), 1)
            self.config.write_bytes(conflicting)
            with self.assertRaisesRegex(ValueError, 'readiness conflict'):
                self.run_mode('Install')
            self.assertEqual(conflicting, self.config.read_bytes())

    def test_policy_change_and_removal_conflict_without_effect(self):
        self.run_mode('Install')
        installed = self.config.read_bytes()
        state = (self.home / 'harness/code-tools-registration.json').read_bytes()
        for edited in (installed.replace(module.READINESS_STATEMENT.encode(), b''),
                       installed.replace(module.READINESS_STATEMENT.encode(), b'mcp_optional_startup_grace_ms = 25\n')):
            self.config.write_bytes(edited)
            for mode in ('Check', 'Install', 'Disconnect'):
                with self.assertRaisesRegex(ValueError, 'readiness ownership conflict'):
                    self.run_mode(mode)
                self.assertEqual(edited, self.config.read_bytes())
                self.assertEqual(state, (self.home / 'harness/code-tools-registration.json').read_bytes())

    def test_legacy_connection_migration_and_recovery_preserve_prior_state(self):
        self.run_mode('Install')
        state_path = self.home / 'harness/code-tools-registration.json'
        state = module.read_json(state_path)
        state.pop('connection_policy')
        state_path.write_bytes(module.json_bytes(state))
        legacy = self.config.read_bytes().replace(module.READINESS_STATEMENT.encode(), b'')
        self.config.write_bytes(legacy)
        prior_state = state_path.read_bytes()
        self.assertEqual('degraded', self.run_mode('Check')['status'])
        module.run(self.home, ROOT, self.native, self.powershell, 'Install', defer_commit=True)
        self.assertEqual(0, module.tomllib.loads(self.config.read_text())[module.READINESS_KEY])
        self.run_mode('Recover')
        self.assertEqual(legacy, self.config.read_bytes())
        self.assertEqual(prior_state, state_path.read_bytes())
        self.run_mode('Install')
        self.assertEqual('connected', self.run_mode('Check')['status'])
        self.run_mode('Disconnect')
        self.assertEqual(self.original, self.config.read_bytes())

    def test_owned_scalar_semantic_normalization_preserves_nested_values_and_prose(self):
        self.original = (b'notes = """mcp_optional_startup_grace_ms = 123\n[mcp_servers.serena]\n"""\n' + self.original
                         + b'[profiles.foreign]\nmcp_optional_startup_grace_ms = 55\n')
        self.config.write_bytes(self.original)
        self.run_mode('Install')
        changed = self.config.read_bytes().replace(module.READINESS_STATEMENT.encode(), b'"mcp_optional_startup_grace_ms"  =  0 # native formatting\n')
        self.config.write_bytes(changed)
        self.assertEqual('connected', self.run_mode('Check')['status'])
        self.run_mode('Disconnect')
        self.assertEqual(self.original, self.config.read_bytes())

    def test_policy_user_edit_after_deferred_activation_is_retained_by_recover(self):
        module.run(self.home, ROOT, self.native, self.powershell, 'Install', defer_commit=True)
        changed = self.config.read_bytes().replace(module.READINESS_STATEMENT.encode(), b'mcp_optional_startup_grace_ms = 900\n')
        self.config.write_bytes(changed)
        with self.assertRaisesRegex(ValueError, 'Config changed'):
            self.run_mode('Recover')
        self.assertEqual(changed, self.config.read_bytes())


if __name__ == '__main__':
    unittest.main()
