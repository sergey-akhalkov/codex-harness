"""Owned native Nuphus schema, browser isolation, idle and reference regression.

No desktop/window enumeration or writes. Two loopback pages and private browser
profiles are owned by this fixture. --real is required; evidence is retained.
"""
import argparse
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import importlib.util
import json
import os
from pathlib import Path
import re
import sys
import tempfile
import threading
import time
import tomllib

import psutil

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools/code-tools'))


def fixture(idle):
    import anyio
    import nuphus_proxy
    from resources import policy
    actual = policy()
    nuphus_proxy.policy = lambda: {**actual, 'nuphus': {'idle_seconds': idle}}
    anyio.run(nuphus_proxy.main)


def main(registry, focused=False, global_config=None):
    spec = importlib.util.spec_from_file_location('owned_nuphus_probe', ROOT / 'tests/fixtures/code-tools-native/nuphus-probe.py')
    probe_module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(probe_module)
    scratch = Path(tempfile.mkdtemp(prefix='nuphus-resources-'))
    report = {'root': str(scratch), 'python': sys.executable, 'checks': [], 'source': {}}
    for name in ('lazy_stdio.py', 'nuphus_proxy.py'):
        report['source'][name] = hashlib.sha256((ROOT / 'tools/code-tools' / name).read_bytes()).hexdigest()
    clients, owned = [], []
    registration = None
    if global_config:
        if not focused:
            raise ValueError('Global registration is supported only for the focused session-preservation check')
        registration = tomllib.loads(global_config.read_text(encoding='utf-8-sig'))['mcp_servers']['nuphus']
        report['registration'] = {key: registration[key] for key in ('command', 'args', 'env') if key in registration}
        report['registration_file'] = str(global_config)
        report['registration_sha256'] = hashlib.sha256(global_config.read_bytes()).hexdigest()
    def check(value, label):
        if not value:
            raise AssertionError(label)
        report['checks'].append(label)
    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass
        def do_GET(self):
            if self.path not in ('/alpha', '/beta'):
                self.send_error(404)
                return
            name = self.path[1:]
            content = (f'<!doctype html><title>{name}</title><button id="apply" '
                       f'onclick="document.getElementById(\'state\').textContent=\'{name}-clicked\'">Apply {name}</button>'
                       f'<p id="state">{name}-before</p>').encode()
            self.send_response(200)
            self.send_header('Content-Type', 'text/html; charset=utf-8')
            self.send_header('Content-Length', str(len(content)))
            self.end_headers()
            self.wfile.write(content)
    http = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
    threading.Thread(target=http.serve_forever, daemon=True).start()
    def tool(client, name, arguments, error=False):
        result = client.call('tools/call', {'name': name, 'arguments': arguments}, timeout=45)
        text = '\n'.join(item.get('text', '') for item in result.get('content', []))
        if bool(result.get('isError')) != error:
            raise AssertionError(f'{name}: {text}')
        return text
    try:
        for index, idle in enumerate((60,) if focused else (1, 60)):
            home = scratch / f'client-{index}'
            if registration:
                command = [registration['command'], *registration.get('args', [])]
                environment = {**registration.get('env', {}),
                               'NUPHUS_MCP_BROWSER_CDP_URL': '', 'NUPHUS_MCP_ALLOW_PRIVATE_NAV': '1'}
                home = Path(environment.get('CODEX_HOME', Path.home() / '.codex'))
            else:
                command = [sys.executable, '-B', str(Path(__file__).resolve()), '--proxy-fixture', str(idle)]
                environment = {'HARNESS_CODE_TOOLS_REGISTRY': str(registry), 'CODEX_HOME': str(home),
                               'HARNESS_TOOL_RESOURCES_DIR': str(scratch / 'account'),
                               'NUPHUS_MCP_BROWSER_CDP_URL': '', 'NUPHUS_MCP_ALLOW_PRIVATE_NAV': '1'}
            profiles = home / 'harness/runtime/nuphus'
            before_profiles = set(profiles.iterdir()) if profiles.exists() else set()
            previous_cwd = Path.cwd()
            try:
                os.chdir(scratch)
                report['launch_cwd'] = str(Path.cwd())
                client = probe_module.MCP(command, environment)
            finally:
                os.chdir(previous_cwd)
            clients.append(client)
            tools = client.call('tools/list', {})['tools']
            report['tool_count'] = len(tools)
            check(len(tools) in (38, 43), 'All audited native tools exposed: ' + str(len(tools)))
            for item in tools:
                if item['name'] in ('browser_click', 'browser_type', 'browser_drag_files'):
                    schema = item['inputSchema']['properties']['ref']
                    check(schema.get('type') == 'string' and not any(key in schema for key in ('pattern', 'enum')),
                          item['name'] + ' accepts opaque string reference')
            check((set(profiles.iterdir()) if profiles.exists() else set()) == before_profiles,
                  'Initialization/catalogue creates no browser profile')
        if focused:
            client = clients[0]
            base = f'http://127.0.0.1:{http.server_port}'
            tool(client, 'browser_navigate', {'url': base + '/alpha', 'confirm': True})
            old_ref = re.search(r'@[0-9a-f]{12}:\d+', tool(client, 'browser_snapshot', {})).group()
            current_ref = re.search(r'@[0-9a-f]{12}:\d+', tool(client, 'browser_snapshot', {})).group()
            owned = psutil.Process(client.process.pid).children(recursive=True)
            report['preserved_tree'] = [{'pid': process.pid, 'created': process.create_time()} for process in owned]
            rejected = tool(client, 'browser_click', {'ref': old_ref, 'confirm': True}, error=True)
            check('fresh browser_snapshot' in rejected, 'Rejected stale reference explains how to recover')
            check(all(process.is_running() for process in owned), 'Preflight rejection preserves native/browser process identities')
            check('alpha-before' in tool(client, 'browser_evaluate', {'script': "document.getElementById('state').textContent", 'confirm': True}),
                  'Preflight rejection preserves the current page without navigation')
            tool(client, 'browser_click', {'ref': current_ref, 'confirm': True})
            check('alpha-clicked' in tool(client, 'browser_evaluate', {'script': "document.getElementById('state').textContent", 'confirm': True}),
                  'Current snapshot reference remains actionable after stale reference rejection')
            report['passed'] = True
            return
        first, second = clients
        time.sleep(1.5)
        baseline = psutil.Process(first.process.pid).children(recursive=True)
        check(all('--proxy-fixture' in process.cmdline() or process.cmdline()[0].lower().endswith('\\conhost.exe')
                  for process in baseline), 'Initial native catalogue worker retires, leaving only thin proxy infrastructure')
        control = {(process.pid, process.create_time()) for process in baseline}
        report['thin_proxy_baseline'] = [{'pid': process.pid, 'created': process.create_time(), 'command': process.cmdline()} for process in baseline]
        base = f'http://127.0.0.1:{http.server_port}'
        tool(second, 'browser_navigate', {'url': base + '/beta', 'confirm': True})
        second_snapshot = tool(second, 'browser_snapshot', {})
        second_ref = re.search(r'@[0-9a-f]{12}:\d+', second_snapshot).group()
        tool(first, 'browser_navigate', {'url': base + '/alpha', 'confirm': True})
        first_snapshot = tool(first, 'browser_snapshot', {})
        first_ref = re.search(r'@[0-9a-f]{12}:\d+', first_snapshot).group()
        check(first_ref != second_ref, 'Independent clients receive disjoint snapshot references')
        tool(first, 'browser_click', {'ref': second_ref, 'confirm': True}, error=True)
        check('beta-before' in tool(second, 'browser_evaluate', {'script': "document.getElementById('state').textContent", 'confirm': True}),
              'Foreign client reference causes no action in independent browser')
        tool(first, 'browser_navigate', {'url': base + '/alpha', 'confirm': True})
        first_snapshot = tool(first, 'browser_snapshot', {})
        first_ref = re.search(r'@[0-9a-f]{12}:\d+', first_snapshot).group()
        native = [process for process in psutil.Process(first.process.pid).children(recursive=True)
                  if (process.pid, process.create_time()) not in control]
        check(len(native) >= 2, 'Native MCP/browser owned descendants captured')
        report['idle_tree'] = [{'pid': process.pid, 'created': process.create_time(), 'command': process.cmdline()} for process in native]
        started = time.monotonic()
        deadline = started + 12  # 1s idle + two independently bounded 5s job drains.
        while time.monotonic() < deadline and any(process.is_running() for process in native):
            time.sleep(0.05)
        report['idle_stop_seconds'] = time.monotonic() - started
        report['idle_remaining'] = [process.pid for process in native if process.is_running()]
        check(not any(process.is_running() for process in native), 'Idle retirement reclaims native MCP and browser descendants')
        tool(first, 'browser_click', {'ref': first_ref, 'confirm': True}, error=True)
        tool(first, 'browser_navigate', {'url': base + '/alpha', 'confirm': True})
        fresh_snapshot = tool(first, 'browser_snapshot', {})
        fresh_ref = re.search(r'@[0-9a-f]{12}:\d+', fresh_snapshot).group()
        check(fresh_ref != first_ref, 'Fresh native @1 after idle cannot reuse expired reference')
        tool(first, 'browser_click', {'ref': first_ref, 'confirm': True}, error=True)
        tool(first, 'browser_click', {'ref': fresh_ref, 'confirm': True})
        check('alpha-clicked' in tool(first, 'browser_evaluate', {'script': "document.getElementById('state').textContent", 'confirm': True}),
              'Fresh reference still performs intended owned action')
        check('beta-before' in tool(second, 'browser_evaluate', {'script': "document.getElementById('state').textContent", 'confirm': True}),
              'Independent browser page survives other client idle/restarts')
        report['passed'] = True
    finally:
        for client in clients:
            client.close()
        http.shutdown()
        http.server_close()
        report['stderr'] = [''.join(client.errors)[-10000:] for client in clients]
        if focused:
            report['remaining_owned_pids'] = [process.pid for process in owned if process.is_running()]
            if report['remaining_owned_pids']:
                report['passed'] = False
        (scratch / 'report.json').write_text(json.dumps(report, indent=2), encoding='utf-8')
        print('Nuphus resource evidence: ' + str(scratch / 'report.json'))
        if focused and report['remaining_owned_pids']:
            raise AssertionError('Owned descendants remain after client close')


if __name__ == '__main__':
    if '--proxy-fixture' in sys.argv:
        fixture(float(sys.argv[sys.argv.index('--proxy-fixture') + 1]))
    else:
        parser = argparse.ArgumentParser(description=__doc__)
        parser.add_argument('--real', action='store_true', required=True)
        parser.add_argument('--registry', type=Path, required=True)
        parser.add_argument('--focused', action='store_true', help='Only verify rejection preserves the current owned browser session')
        parser.add_argument('--global-config', type=Path, help='Use the exact current registered launch command for the focused check')
        arguments = parser.parse_args()
        main(arguments.registry.resolve(), focused=arguments.focused, global_config=arguments.global_config)
