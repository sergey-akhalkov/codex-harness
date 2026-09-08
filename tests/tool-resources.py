"""Owned CBM admission/cancellation fixtures; --real adds a disposable native index."""
from __future__ import annotations

from contextlib import closing
import hashlib
import io
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import sys
import tempfile
import threading
import time
import unittest
import uuid
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools/code-tools'))

if '--fixture' in sys.argv:
    request = json.loads(Path(sys.argv[sys.argv.index('--args-file') + 1]).read_text())
    if request.get('marker'):
        Path(request['marker']).write_text(str(os.getpid()))
    if request.get('late_writer'):
        code = ('import os,pathlib,sys,time; pathlib.Path(sys.argv[1]).write_text(str(os.getpid())); '
                'sys.stderr.write("level=info writer-ready\\n"); sys.stderr.flush(); '
                'time.sleep(0.5); sys.stderr.write("level=info late-write\\n"); sys.stderr.flush(); time.sleep(5)')
        subprocess.Popen([sys.executable, '-B', '-c', code, request['late_writer']],
                         stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=sys.stderr)
        deadline = time.monotonic() + 2
        while not Path(request['late_writer']).exists():
            if time.monotonic() > deadline:
                raise RuntimeError('Owned grandchild did not announce readiness')
            time.sleep(0.01)
    time.sleep(request.get('sleep', 0))
    print(json.dumps({'content': [{'type': 'text', 'text': json.dumps(request, ensure_ascii=False)}], 'isError': False}))
    raise SystemExit()

import cbm_proxy
import resources
import psutil


class ResourceTests(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory(prefix='tool-resources-')
        self.root = Path(self.scratch.name)
        self.directory = patch.object(resources, 'account_directory', return_value=self.root)
        self.directory.start()
        self.evidence = patch.object(cbm_proxy, 'account_directory', return_value=self.root)
        self.evidence.start()
        self.command = [sys.executable, '-B', str(Path(__file__).resolve()), '--fixture']
        self.limits = {**resources.policy()['codebase_memory'], 'deadline_seconds': 5, 'admission_seconds': 0.2,
                       'job_memory_mb': 96}

    def tearDown(self):
        self.evidence.stop()
        self.directory.stop()
        self.scratch.cleanup()

    def invoke(self, arguments, cancelled=None, limits=None):
        return cbm_proxy.index(arguments, cancelled, executable=self.command, limits=limits or self.limits)

    def test_unicode_stdio_and_result(self):
        result = self.invoke({'repo_path': 'путь с пробелом'})
        self.assertFalse(result.isError, result)
        self.assertIn('путь с пробелом', result.content[0].text)

    def test_cancelled_request_never_spawns(self):
        cancelled = threading.Event()
        cancelled.set()
        with patch.object(cbm_proxy.JobGuard, 'popen') as launch:
            result = self.invoke({}, cancelled)
        self.assertTrue(result.isError)
        launch.assert_not_called()

    def test_output_bounds_include_completed_response(self):
        response = self.root / 'response.json'
        response.write_bytes(b'x' * 1025)
        with (self.root / 'stdout').open('w+b') as stdout, (self.root / 'stderr').open('w+b') as stderr:
            with self.assertRaisesRegex(RuntimeError, 'bounded capture'):
                cbm_proxy.capture_sizes(stdout, stderr, response, 1024)
        stream = io.BytesIO(b'level=info progress\n' * 10000)
        self.assertLessEqual(len(cbm_proxy.progress_excerpt(stream, 200)), 200)
        self.assertLess(stream.tell(), 1000, 'Excerpt must stop reading once its budget is full')

    def test_configuration_drift_blocks_each_native_operation_without_reapplying(self):
        cache = self.root / 'native-cache'
        cache.mkdir()
        database = cache / '_config.db'
        with closing(sqlite3.connect(database)) as connection, connection:
            connection.execute('CREATE TABLE config (key TEXT PRIMARY KEY, value TEXT)')
            connection.executemany('INSERT INTO config VALUES (?, ?)',
                                   resources.policy()['codebase_memory']['configuration'].items())
        receipt = self.root / 'cbm-configuration.json'
        receipt.write_text(json.dumps({'cache': str(cache.resolve())}))
        receipt_before = receipt.read_bytes()
        with patch.dict(os.environ, {'CBM_CACHE_DIR': str(cache)}):
            for name in ('index_repository', 'search_graph', 'delete_project'):
                with self.subTest(name=name):
                    with closing(sqlite3.connect(database)) as connection, connection:
                        connection.execute("UPDATE config SET value='false' WHERE key='auto_watch'")
                    with patch.object(cbm_proxy, 'command_for', return_value=(['owned-native'], self.root / 'unused')):
                        with patch.object(cbm_proxy.JobGuard, 'popen', side_effect=RuntimeError('owned launch sentinel')) as launch:
                            first = cbm_proxy.run_tool(name, {}, executable='owned-native.exe', limits=self.limits)
                            self.assertIn('owned launch sentinel', first.content[0].text)
                            launch.assert_called_once()
                        with closing(sqlite3.connect(database)) as connection, connection:
                            connection.execute("UPDATE config SET value='true' WHERE key='auto_watch'")
                        drifted = database.read_bytes()
                        with patch.object(cbm_proxy.JobGuard, 'popen', side_effect=RuntimeError('unexpected launch reached')) as launch:
                            second = cbm_proxy.run_tool(name, {}, executable='owned-native.exe', limits=self.limits)
                        self.assertTrue(second.isError)
                        self.assertIn('resource policy is not active', second.content[0].text)
                        launch.assert_not_called()
                        self.assertEqual(database.read_bytes(), drifted)
                        self.assertEqual(receipt.read_bytes(), receipt_before)

    def test_completed_parent_reclaims_late_writer_before_reading_output(self):
        marker = self.root / 'grandchild.pid'
        processes = []
        capture_sizes = cbm_proxy.capture_sizes
        original_spawn = cbm_proxy.JobGuard.popen
        inspected = []
        def spawn(guard, *args, **kwargs):
            process = original_spawn(guard, *args, **kwargs)
            processes.append(process)
            return process
        def capture(*args):
            if processes and processes[0].poll() is not None and marker.exists():
                self.assertFalse(psutil.pid_exists(int(marker.read_text())),
                                 'Late writer must exit before completed output is inspected')
                inspected.append(True)
            return capture_sizes(*args)
        with patch.object(cbm_proxy.JobGuard, 'popen', spawn), patch.object(cbm_proxy, 'capture_sizes', capture):
            result = self.invoke({'late_writer': str(marker)})
        self.assertFalse(result.isError, result)
        self.assertTrue(inspected, 'Completed-parent path must actually be exercised')
        self.assertFalse(psutil.pid_exists(int(marker.read_text())))

    def test_timeout_reclaims_worker(self):
        marker = self.root / 'worker.pid'
        result = self.invoke({'marker': str(marker), 'sleep': 30}, limits={**self.limits, 'deadline_seconds': 1.5})
        self.assertTrue(result.isError)
        self.assertIn('deadline', result.content[0].text)
        self.assertTrue(marker.exists(), 'Fixture must actually start before the deadline')
        self.assertFalse(psutil.pid_exists(int(marker.read_text())))

    def test_single_account_admission(self):
        marker = self.root / 'first.pid'
        completed = []
        thread = threading.Thread(target=lambda: completed.append(self.invoke({'marker': str(marker), 'sleep': 2})))
        thread.start()
        until = time.monotonic() + 4
        while not marker.exists() and time.monotonic() < until:
            time.sleep(0.05)
        rejected = self.invoke({'marker': str(self.root / 'second.pid')})
        thread.join(7)
        self.assertFalse(thread.is_alive())
        self.assertTrue(rejected.isError)
        self.assertIn('busy', rejected.content[0].text)
        self.assertFalse((self.root / 'second.pid').exists())
        self.assertFalse(completed[0].isError)

    def test_cancellation_reclaims_worker(self):
        cancelled = threading.Event()
        timer = threading.Timer(1.5, cancelled.set)
        marker = self.root / 'cancelled.pid'
        timer.start()
        try:
            result = self.invoke({'marker': str(marker), 'sleep': 30}, cancelled)
        finally:
            timer.cancel()
        self.assertTrue(result.isError)
        self.assertIn('cancelled', result.content[0].text)
        if marker.exists():
            self.assertFalse(psutil.pid_exists(int(marker.read_text())))

    def test_ignore_is_additive(self):
        (self.root / '.cbmignore').write_text('existing/\n')
        resources.add_ignore(self.root, ['logs/'])
        resources.add_ignore(self.root, ['logs/'])
        self.assertEqual((self.root / '.cbmignore').read_text().count('logs/'), 1)
        self.assertTrue((self.root / '.cbmignore').read_text().startswith('existing/\n'))


def native_catalogue(root):
    """Force a miss only in an owned account directory; preserve global cache."""
    account = root / 'catalogue-account'
    account.mkdir()
    processes = []
    guard_type = cbm_proxy.JobGuard
    class TrackingGuard(guard_type):
        def popen(self, *args, **kwargs):
            process = super().popen(*args, **kwargs)
            processes.append(psutil.Process(process.pid))
            return process
        def close(self):
            for process in tuple(processes):
                try:
                    processes.extend(process.children(recursive=True))
                except psutil.Error:
                    pass
            super().close()
    with patch.object(resources, 'account_directory', return_value=account), \
            patch.object(cbm_proxy, 'account_directory', return_value=account), \
            patch.object(cbm_proxy, 'JobGuard', TrackingGuard):
        cbm_proxy.configuration('check')  # The production main startup contract.
        if (account / 'cbm-catalogue.json').exists():
            raise AssertionError('Cold catalogue fixture must begin without a cache')
        listed = cbm_proxy.catalogue()
        if len(listed) != 15 or not processes:
            raise AssertionError('Cold native catalogue did not expose all15 audited tools through a child')
        with patch.object(TrackingGuard, 'popen', side_effect=AssertionError('Warm catalogue started another process')):
            warm = cbm_proxy.catalogue()
        if [tool.model_dump() for tool in listed] != [tool.model_dump() for tool in warm]:
            raise AssertionError('Cached native catalogue changed tool contracts')
    remaining = [process.pid for process in processes if process.is_running()]
    if remaining:
        raise AssertionError('Catalogue startup left owned descendants: ' + repr(remaining))
    return {'tool_count': len(listed), 'cache_miss_started_native': True, 'cache_hit_started_native': False,
            'remaining_owned_pids': remaining}


def native_acceptance():
    name = 'harness-resource-' + uuid.uuid4().hex[:12]
    with tempfile.TemporaryDirectory(prefix='cbm-native-resource-') as temporary:
        root = Path(temporary)
        (root / 'main.py').write_text('def alpha():\n    return 42\n\ndef beta():\n    return alpha()\n')
        try:
            baseline = cbm_proxy.index({'repo_path': str(root), 'name': name, 'mode': 'fast'})
            if baseline.isError:
                raise AssertionError(baseline.model_dump_json())
            cache = Path(os.environ.get('CBM_CACHE_DIR', Path.home() / '.cache/codebase-memory-mcp'))
            database = cache / (name + '.db')
            before = hashlib.sha256(database.read_bytes()).hexdigest()
            (root / 'main.py').write_text('def replacement():\n    return 43\n')
            failed = cbm_proxy.index({'repo_path': str(root), 'name': name, 'mode': 'fast'},
                limits={**resources.policy()['codebase_memory'], 'deadline_seconds': 0.05})
            if not failed.isError or hashlib.sha256(database.read_bytes()).hexdigest() != before:
                raise AssertionError('Failed refresh changed the committed native index')
            query = cbm_proxy.run_tool('search_graph', {'project': name, 'name_pattern': 'alpha'})
            if query.isError or 'alpha' not in query.model_dump_json():
                raise AssertionError('Previously committed symbol is not readable after failed refresh')
            return {'native_index': 'passed', 'failed_refresh_preserved_database': True,
                    'old_symbol_readable': True, 'project': name}
        finally:
            cleanup = cbm_proxy.run_tool('delete_project', {'project': name})
            if cleanup.isError:
                raise AssertionError('Owned native test graph cleanup failed: ' + cleanup.model_dump_json())


if __name__ == '__main__':
    if '--real' in sys.argv:
        evidence = Path(tempfile.mkdtemp(prefix='cbm-resource-evidence-'))
        report = {'python': sys.executable, 'evidence': str(evidence), 'passed': False,
                  'policy': resources.policy()['codebase_memory'],
                  'source': {name: hashlib.sha256((ROOT / 'tools/code-tools' / name).read_bytes()).hexdigest()
                             for name in ('cbm_proxy.py', 'resources.py')}}
        try:
            report['catalogue'] = native_catalogue(evidence)
            # Keep actual account-wide index admission; redirect only diagnostic
            # output so the full-project acceptance receipt is not overwritten.
            with patch.object(cbm_proxy, 'account_directory', return_value=evidence):
                report['index'] = native_acceptance()
            report['passed'] = True
        finally:
            (evidence / 'report.json').write_text(json.dumps(report, indent=2), encoding='utf-8')
            print('CBM resource evidence: ' + str(evidence / 'report.json'))
    else:
        unittest.main()
