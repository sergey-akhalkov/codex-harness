"""Owned hooks-off consumers and compact CBM discovery; no model calls or installs."""
from __future__ import annotations

import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
from unittest.mock import patch

import anyio
from mcp import ClientSession, StdioServerParameters
from mcp.client.stdio import stdio_client

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools/lsp'))
import broker


async def cached_mcp(home, workspace):
    parameters = StdioServerParameters(command=sys.executable,
        args=['-B', str(ROOT / 'tools/lsp/server.py')], cwd=str(workspace),
        env={**os.environ, 'CODEX_HOME': str(home)})
    with anyio.fail_after(20):
        async with stdio_client(parameters) as streams:
            async with ClientSession(*streams) as session:
                await session.initialize()
                for event in ('PostToolUse', 'Stop', 'SubagentStop'):
                    for name in ('read_file', 'apply_patch', 'exec_command', 'mcp_edit'):
                        result = await session.call_tool('diagnostics_after_tool', {
                            'event': event, 'cwd': str(workspace), 'session_id': 'owned',
                            'tool_name': name, 'tool_response': {'exit_code': 7}})
                        assert not result.isError and result.content == []
                        assert result.structuredContent == {}
    assert not (home / 'harness').exists(), 'Cached MCP started a broker/journal'
    return 12


async def cbm_catalogue(report_root):
    parameters = StdioServerParameters(command=sys.executable,
        args=['-B', str(ROOT / 'tools/code-tools/cbm_proxy.py')], cwd=str(report_root),
        env={**os.environ, 'HARNESS_CODE_TOOLS_REGISTRY': str(Path.home() / '.codex/harness/code-tools.json')})
    with anyio.fail_after(35):
        async with stdio_client(parameters) as streams:
            async with ClientSession(*streams) as session:
                initialized = await session.initialize()
                assert not initialized.instructions
                tools = (await session.list_tools()).tools
                descriptions = [tool.description or '' for tool in tools]
                assert sum('Resource policy:' in value for value in descriptions) == 1
                index_tool = next(tool for tool in tools if tool.name == 'index_repository')
                assert 'repo_path' in index_tool.inputSchema['properties']
                assert 'current source state' in index_tool.description
                # The old adapter put the same policy in initialize (Codex adds
                # it to every tool) and in three descriptions. Schemas unchanged.
                contract = index_tool.description.split('\n\nResource policy:')[1]
                policy = 'Resource policy:' + contract
                after = sum(map(len, descriptions))
                before = after + (len(tools) + 2) * (len(policy) + 2)
                missing = await session.call_tool('not_a_native_tool', {})
                assert missing.isError and len(missing.model_dump_json()) < 1000
                return {'tools': len(tools), 'description_chars_before': before,
                    'description_chars_after': after, 'schema_fields_preserved': True,
                    'measurement': 'same native catalogue; old initialize-prefix and duplicate suffix rendering',
                    'unknown_tool_error': True}


def main():
    evidence = Path(tempfile.mkdtemp(prefix='harness-subscription-efficiency-'))
    home, workspace = evidence / 'codex', evidence / 'workspace'
    home.mkdir()
    workspace.mkdir()
    report = {'passed': False, 'python': sys.executable, 'cwd': str(workspace),
        'source': {name: hashlib.sha256((ROOT / name).read_bytes()).hexdigest() for name in (
            'tools/hook.ps1', 'tools/lsp/server.py', 'tools/lsp/broker.py',
            'tools/code-tools/cbm_proxy.py', 'global/hooks.json', 'global/code-tools.json')}}
    started = time.monotonic()
    try:
        assert json.loads((ROOT / 'global/hooks.json').read_text()) == {'hooks': {}}
        catalogue = json.loads((ROOT / 'global/code-tools.json').read_text())
        assert [item['id'] for item in catalogue['languages']] == ['python', 'rust']
        assert len(catalogue['retired_language_candidates']) >= 13
        scenarios = ('read', 'startup', 'unchanged', 'identical-write', 'pre-dirty',
            'untracked-create', 'failed-writer', 'rename', 'unsupported', 'oversized',
            'delete-only', 'configuration-change', 'sibling-link', 'unknown-history',
            'concurrent-root', 'stale-reply', 'pending-timeout', 'Stop', 'SubagentStop')
        with patch.object(broker, 'ensure_endpoint', side_effect=AssertionError('analysis startup')):
            for scenario in scenarios:
                assert broker.request('hook', {'event': scenario, 'cwd': str(workspace)}, timeout=0) == {}
        report['zero_request_cases'] = list(scenarios)
        original = b'value = 1\n'
        target = workspace / 'sample.py'
        target.write_bytes(original)
        for event in ('pre', 'post', 'stop'):
            for payload in ('{}', '{invalid', json.dumps({'cwd': str(workspace), 'pending': True})):
                result = subprocess.run(['pwsh', '-NoLogo', '-NoProfile', '-File',
                    str(ROOT / 'tools/hook.ps1'), '-Event', event], input=payload,
                    text=True, capture_output=True, timeout=8, cwd=workspace,
                    env={**os.environ, 'CODEX_HOME': str(home)})
                assert result.returncode == 0 and not result.stdout and not result.stderr
        assert target.read_bytes() == original
        assert not (home / 'harness').exists()
        report['silent_command_cases'] = 9
        report['silent_mcp_cases'] = anyio.run(cached_mcp, home, workspace)
        report['cbm'] = anyio.run(cbm_catalogue, evidence)
        failure_env = dict(os.environ)
        failure_env.pop('HARNESS_CODE_TOOLS_REGISTRY', None)
        failed_start = subprocess.run([sys.executable, '-B', str(ROOT / 'tools/code-tools/cbm_proxy.py')],
            cwd=workspace, env=failure_env, input='', capture_output=True, text=True, timeout=20)
        assert failed_start.returncode == 1 and not failed_start.stdout
        assert 'CBM startup failed:' in failed_start.stderr and 'Details:' in failed_start.stderr
        assert 'Traceback' not in failed_start.stderr and len(failed_start.stderr) < 1000
        detail = Path(failed_start.stderr.strip().split('Details: ', 1)[1])
        assert 'traceback' in json.loads(detail.read_text(encoding='utf-8'))
        report['cbm_startup_failure_chars'] = len(failed_start.stderr)
        # Preserve explicit project verification and the original writer status.
        writer = subprocess.run([sys.executable, '-c',
            "from pathlib import Path; Path('sample.py').write_text('value = ('); raise SystemExit(7)"],
            cwd=workspace, capture_output=True)
        assert writer.returncode == 7
        try:
            compile(target.read_text(), str(target), 'exec')
        except SyntaxError:
            pass
        else:
            raise AssertionError('Native check missed the introduced defect')
        target.write_bytes(original)
        compile(target.read_text(), str(target), 'exec')
        report['explicit_check'] = 'Python compile detects defect, accepts correction; writer exit 7 preserved'
        report['passed'] = True
    finally:
        report['elapsed_seconds'] = round(time.monotonic() - started, 3)
        (evidence / 'report.json').write_text(json.dumps(report, indent=2), encoding='utf-8')
        print(json.dumps({'passed': report['passed'], 'evidence': str(evidence / 'report.json')}))


if __name__ == '__main__':
    main()
