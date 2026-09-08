"""Bounded lazy MCP ownership; --real runs only tiny owned stdlib fixtures."""
import asyncio
from contextlib import asynccontextmanager
import json
import inspect
import os
from pathlib import Path
import subprocess
import sys
import threading
import time
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import psutil
from mcp import StdioServerParameters, types

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools/code-tools'))
import lazy_stdio
from nuphus_proxy import BrowserReferences

REAL = '--real' in sys.argv


class ReferenceTests(unittest.TestCase):
    def test_ref_is_bound_to_proxy_snapshot_and_survives_no_restart_collision(self):
        result = types.CallToolResult(content=[types.TextContent(type='text', text=json.dumps({'snapshot': '@1 [button] "Apply"\n@2 [textbox] "Value"'}))])
        first, second = BrowserReferences(), BrowserReferences()
        transformed = first.snapshot(result)
        reference = next(iter(first.references))
        self.assertIn(reference, transformed.content[0].text)
        self.assertEqual(first.arguments({'ref': reference})['ref'], '@1')
        for scope, invalid in ((first, '@1'), (second, reference)):
            with self.assertRaisesRegex(ValueError, 'expired'):
                scope.arguments({'ref': invalid})
        first.expire()
        first.snapshot(result)  # Native @1 is recycled after idle/restart.
        with self.assertRaisesRegex(ValueError, 'expired'):
            first.arguments({'ref': reference})
        self.assertEqual(len(first.references), 2)


class LazyTests(unittest.IsolatedAsyncioTestCase):
    async def asyncSetUp(self):
        self.events = []
        self.started = asyncio.Event()
        self.release = asyncio.Event()
        outer = self
        @asynccontextmanager
        async def transport(parameters):
            outer.events.append('start')
            try:
                yield (), ()
            finally:
                await asyncio.sleep(0.03)
                outer.events.append('tree-exit')
        class Session:
            def __init__(self, *args):
                pass
            async def __aenter__(self):
                return self
            async def __aexit__(self, *args):
                pass
            async def initialize(self):
                return 'initialized'
            async def call_tool(self, name, arguments):
                outer.events.append('call-' + name)
                outer.started.set()
                if name == 'blocked':
                    await outer.release.wait()
                return 'result'
        self.transport = patch.object(lazy_stdio, 'owned_stdio', transport)
        self.session = patch.object(lazy_stdio, 'ClientSession', Session)
        self.transport.start()
        self.session.start()
        self.addCleanup(self.transport.stop)
        self.addCleanup(self.session.stop)

    def lease(self, method, arguments):
        events = self.events
        class Lease:
            def __enter__(self):
                events.append('lease-enter')
            def __exit__(self, *args):
                events.append('lease-exit')
        return Lease()

    async def test_idle_restarts_only_on_next_request(self):
        async with lazy_stdio.LazyStdio(None, idle_seconds=0.03) as remote:
            await asyncio.sleep(0.01)
            self.assertEqual(self.events, [])
            await remote.initialize()
            await asyncio.sleep(0.08)
            self.assertEqual(self.events, ['start', 'tree-exit'])
            await remote.call_tool('next', {})
            self.assertEqual(self.events.count('start'), 2)

    async def test_abandoned_caller_keeps_lease_until_completion(self):
        async with lazy_stdio.LazyStdio(None, lease_for=self.lease) as remote:
            caller = asyncio.create_task(remote.call_tool('blocked', {}))
            await self.started.wait()
            caller.cancel()
            with self.assertRaises(asyncio.CancelledError):
                await caller
            self.assertNotIn('lease-exit', self.events)
            self.release.set()
            await asyncio.sleep(0.02)
            self.assertIn('lease-exit', self.events)

    async def test_rejected_input_preserves_current_worker_and_releases_lease(self):
        async def before(method, arguments):
            if arguments[0] == 'stale':
                raise lazy_stdio.RequestRejected('Take a fresh browser_snapshot')
        async with lazy_stdio.LazyStdio(None, lease_for=self.lease, before_request=before,
                                       on_idle=lambda: self.events.append('browser-exit')) as remote:
            await remote.call_tool('current', {})
            with self.assertRaisesRegex(lazy_stdio.RequestRejected, 'fresh browser_snapshot'):
                await remote.call_tool('stale', {})
            self.assertEqual(self.events.count('lease-enter'), 2)
            self.assertEqual(self.events.count('lease-exit'), 2)
            self.assertNotIn('call-stale', self.events)
            self.assertNotIn('tree-exit', self.events)
            self.assertNotIn('browser-exit', self.events)
            self.assertEqual(await remote.call_tool('continued', {}), 'result')
            self.assertEqual(self.events.count('start'), 1)

    async def test_timeout_holds_lease_until_tree_exit(self):
        async with lazy_stdio.LazyStdio(None, timeout=0.08, lease_for=self.lease) as remote:
            with self.assertRaises(TimeoutError):
                await remote.call_tool('blocked', {})
            await asyncio.sleep(0.08)
            self.assertLess(self.events.index('tree-exit'), self.events.index('lease-exit'))
            await remote.call_tool('recovered', {})
            self.assertEqual(self.events.count('start'), 2)

    async def test_owner_cancel_during_blocking_admission_releases_after_tree_exit(self):
        entered, allow = threading.Event(), threading.Event()
        events = self.events
        class Lease:
            def __enter__(self):
                entered.set()
                if not allow.wait(2):
                    raise TimeoutError('Owned admission release')
                events.append('lease-enter')
            def __exit__(self, *args):
                events.append('lease-exit')
        remote = lazy_stdio.LazyStdio(None, lease_for=lambda *args: Lease())
        await remote.__aenter__()
        caller = asyncio.create_task(remote.call_tool('blocked', {}))
        try:
            self.assertTrue(await asyncio.to_thread(entered.wait, 1))
            closing = asyncio.create_task(remote.__aexit__())
            await asyncio.sleep(0.02)
            self.assertFalse(closing.done())
            allow.set()
            await asyncio.wait_for(closing, 1)
            with self.assertRaisesRegex(RuntimeError, 'stopped'):
                await caller
            self.assertLess(events.index('tree-exit'), events.index('lease-exit'))
            self.assertNotIn('call-blocked', events)
        finally:
            allow.set()
            if not remote.task.done():
                await remote.__aexit__()


@unittest.skipUnless(REAL, '--real is required for owned process fixtures')
class NativeTests(unittest.IsolatedAsyncioTestCase):
    async def test_native_idle_and_parent_exit_reclaim_descendants(self):
        code = 'import json,os,subprocess,sys\n' + inspect.getsource(fixture) + '\nfixture()'
        parameters = StdioServerParameters(command=sys.executable, args=['-B', '-c', code])
        guard_type = lazy_stdio.JobGuard
        with patch.object(lazy_stdio, 'JobGuard', lambda: guard_type(memory_limit_bytes=96 * 1024 * 1024)):
            await self.exercise_native(parameters)

    async def exercise_native(self, parameters):
        async with lazy_stdio.LazyStdio(parameters, idle_seconds=0.15, timeout=5) as remote:
            first = await remote.call_tool('identity', {})
            first_info = json.loads(first.content[0].text)
            first_tree = [psutil.Process(value) for value in first_info.values()]
            await asyncio.sleep(0.5)
            self.assertFalse(any(process.is_running() for process in first_tree))
            await remote.list_tools()
            second = await remote.call_tool('exit', {})
            second_info = json.loads(second.content[0].text)
            self.assertNotEqual(first_info['parent'], second_info['parent'])
            second_tree = []
            for value in second_info.values():
                try:
                    second_tree.append(psutil.Process(value))
                except psutil.NoSuchProcess:
                    pass
            await asyncio.sleep(0.5)
            self.assertFalse(any(process.is_running() for process in second_tree))


def fixture():
    child = subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(15)'],
                             stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    for raw in sys.stdin:
        message = json.loads(raw)
        if 'id' not in message:
            continue
        method = message['method']
        if method == 'initialize':
            result = {'protocolVersion': '2024-11-05', 'capabilities': {'tools': {}},
                      'serverInfo': {'name': 'owned-stdio-fixture', 'version': '1'}}
        elif method == 'tools/call':
            result = {'content': [{'type': 'text', 'text': json.dumps({'parent': os.getpid(), 'child': child.pid})}]}
        else:
            result = {'tools': [{'name': name, 'inputSchema': {'type': 'object', 'properties': {}}}
                                for name in ('identity', 'exit')]}
        print(json.dumps({'jsonrpc': '2.0', 'id': message['id'], 'result': result}), flush=True)
        if method == 'tools/call' and message['params']['name'] == 'exit':
            os._exit(0)


if __name__ == '__main__':
    if '--fixture' in sys.argv:
        fixture()
    else:
        unittest.main(argv=[argument for argument in sys.argv if argument != '--real'], verbosity=2)
