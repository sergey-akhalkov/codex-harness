"""Historical PSES ownership proof using owned receipts and process doubles.

No native server is started and no real process is terminated, even when the
retirement branch is exercised. Actual inspection remains an explicit CLI call.
"""
import copy
import json
import os
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools/code-tools'))
import orphans


class Process:
    def __init__(self, pid, name, executable, args, parent, created):
        self.pid, self.label, self.executable, self.args = pid, name, str(executable), args
        self.parent, self.created = parent, created
        self.descendants, self.terminated = [], False
        self.info = {'name': name}

    def name(self): return self.label
    def exe(self): return self.executable
    def cmdline(self): return self.args.copy()
    def ppid(self): return self.parent
    def create_time(self): return self.created
    def children(self): return self.descendants.copy()
    def memory_info(self): return SimpleNamespace(private=1024, rss=512)
    def terminate(self): self.terminated = True
    def wait(self, timeout): return 0
    def is_running(self): return not self.terminated


class OwnershipTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix='orphans-proof-')
        self.addCleanup(self.directory.cleanup)
        self.home = Path(self.directory.name)
        self.script = self.home / 'adopted/PowerShellEditorServices/Start-EditorServices.ps1'
        self.executable = self.home / 'adopted/pwsh.exe'
        self.folder = self.home / ('harness/runtime/lsp/' + 'a' * 64 + '/powershell')
        self.folder.mkdir(parents=True)
        self.command = [str(self.executable), '-NoLogo', '-NoProfile', '-File', str(self.script)]
        self.args = self.command + ['-HostName', 'CodexHarness', '-HostProfileId', 'CodexHarness',
                    '-HostVersion', '1.0.0', '-BundledModulesPath', str(self.script.parent), '-Stdio',
                    '-LogPath', str(self.folder / 'pses.log'), '-SessionDetailsPath', str(self.folder / 'session.json')]
        self.receipt = {'languages': [{'id': 'powershell', 'status': 'adopted', 'command': self.command,
                                      'paths': {'executable': str(self.script)}}]}
        self.save_receipt()
        self.windows = self.home / 'Windows'
        environment = patch.dict(os.environ, {'WINDIR': str(self.windows)})
        environment.start()
        self.addCleanup(environment.stop)
        self.child = Process(12, 'pwsh.exe', self.executable, self.args, 11, 102)
        self.shell = Process(11, 'cmd.exe', self.windows / 'System32/cmd.exe',
                             [str(self.windows / 'System32/cmd.exe'), '/c', ' '.join(self.args)], 10, 101)
        self.shell.descendants = [self.child]
        self.processes = {11: self.shell, 12: self.child}
        def process(pid):
            if pid not in self.processes:
                raise orphans.psutil.NoSuchProcess(pid)
            return self.processes[pid]
        provider = patch.object(orphans.psutil, 'Process', process)
        provider.start()
        self.addCleanup(provider.stop)

    def save_receipt(self):
        (self.home / 'harness/code-tools.json').write_text(json.dumps(self.receipt), encoding='utf-8')

    def test_exact_adopted_file_contract_is_an_orphan_candidate(self):
        value = orphans.candidate(12, self.home)
        self.assertEqual(value['pid'], 12)
        self.assertEqual(value['missing_owner'], 10)
        self.assertEqual(value['private_bytes'], 1024)
        self.assertIn('shell_invocation_sha256', value)

    def test_command_text_containing_both_old_substrings_is_preserved(self):
        self.child.args = [str(self.executable), '-Command',
            "Write-Output 'Start-EditorServices.ps1'; Write-Output '" + str(self.home / 'harness/runtime/') + "'; Start-Sleep 300"]
        self.assertIsNone(orphans.candidate(12, self.home))

    def test_wrong_script_runtime_host_and_extra_options_are_preserved(self):
        cases = []
        def replace(old, new):
            args = self.args.copy()
            args[args.index(old)] = new
            cases.append(args)
        replace(str(self.script), str(self.home / 'foreign/Start-EditorServices.ps1'))
        replace(str(self.script.parent), '.')
        replace('CodexHarness', 'Serena')
        replace(str(self.folder / 'pses.log'), str(self.home / 'harness/runtime-extra/pses.log'))
        replace(str(self.folder / 'session.json'), str(self.folder.parent / 'session.json'))
        replace(str(self.folder / 'pses.log'), str(self.home / 'harness/runtime/lsp/unreviewed/powershell/pses.log'))
        cases.extend([self.args + ['-HostName', 'CodexHarness'], self.args + ['-Unknown', 'value'], self.args[:-1]])
        for args in cases:
            with self.subTest(args=args):
                self.assertFalse(orphans.kit_invocation(args, self.home))

    def test_unavailable_or_changed_adoption_receipt_preserves_process(self):
        self.receipt['languages'][0]['command'][4] = str(self.home / 'new/Start-EditorServices.ps1')
        self.save_receipt()
        self.assertIsNone(orphans.candidate(12, self.home))
        (self.home / 'harness/code-tools.json').write_text('{', encoding='utf-8')
        self.assertIsNone(orphans.candidate(12, self.home))
        (self.home / 'harness/code-tools.json').unlink()
        self.assertIsNone(orphans.candidate(12, self.home))

    def test_live_original_owner_or_extra_children_are_preserved(self):
        owner = Process(10, 'python.exe', self.home / 'python.exe', [], 1, 100)
        self.processes[10] = owner
        self.assertIsNone(orphans.candidate(12, self.home))
        owner.created = 103  # PID has been reused since the wrapper was born.
        self.assertIsNotNone(orphans.candidate(12, self.home))
        self.shell.descendants.append(owner)
        self.assertIsNone(orphans.candidate(12, self.home))
        self.shell.descendants = [self.child]
        self.child.descendants = [owner]
        self.assertIsNone(orphans.candidate(12, self.home))

    def test_interactive_or_chained_shell_is_preserved(self):
        for tail in (['/k', ' '.join(self.args)], ['/c', ' '.join(self.args) + ' & echo unrelated'],
                     ['/c', 'echo unrelated']):
            self.shell.args = [self.shell.executable] + tail
            self.assertIsNone(orphans.candidate(12, self.home))

    def test_read_only_inventory_and_recheck_do_not_terminate_unproven_process(self):
        with patch.object(orphans.psutil, 'process_iter', return_value=[self.child]):
            result = orphans.run(self.home)
            self.assertEqual(len(result['candidates']), 1)
            self.assertEqual(result['retired'], [])
            self.assertFalse(self.child.terminated)
            original = orphans.candidate(12, self.home)
            changed = copy.deepcopy(original)
            changed['invocation_sha256'] = 'changed'
            with patch.object(orphans, 'candidate', side_effect=[original, changed]):
                result = orphans.run(self.home, apply=True)
            self.assertEqual(result['retired'], [])
            self.assertEqual(len(result['skipped']), 1)
            self.assertFalse(self.child.terminated)


if __name__ == '__main__':
    unittest.main(verbosity=2)
