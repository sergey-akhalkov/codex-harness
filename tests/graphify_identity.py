"""Real Graphify counterexamples: another graph, corrupt graph, explicit worktree."""
import anyio
from contextlib import asynccontextmanager
import importlib.util
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import time
import psutil

from mcp import ClientSession, StdioServerParameters, types
from mcp.client.stdio import stdio_client

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('proxy', ROOT / 'tools/code-tools/graphify_proxy.py')
proxy = importlib.util.module_from_spec(spec)
spec.loader.exec_module(proxy)


@asynccontextmanager
async def daemon(python, graph, folder):
    with socket.socket() as sock:
        sock.bind(('127.0.0.1', 0))
        port = sock.getsockname()[1]
    log_path = folder / 'http.log'
    with log_path.open('w', encoding='utf-8') as log:
        process = subprocess.Popen([python, '-B', '-u', '-m', 'graphify.serve', '--graph', str(graph),
            '--transport', 'http', '--host', '127.0.0.1', '--port', str(port), '--path', '/mcp', '--stateless'],
            env={**os.environ, 'GRAPHIFY_API_KEY': 'owned-test-only', 'PYTHONDONTWRITEBYTECODE': '1'},
            stdout=log, stderr=log, creationflags=0x08000000 if os.name == 'nt' else 0)
        try:
            deadline = time.monotonic() + 25
            while True:
                if process.poll() is not None:
                    raise RuntimeError('Owned fixture exited during startup')
                try:
                    with socket.create_connection(('127.0.0.1', port), timeout=.1):
                        break
                except OSError:
                    if time.monotonic() > deadline:
                        raise TimeoutError('Owned fixture startup')
                    await anyio.sleep(.1)
            yield f'http://127.0.0.1:{port}/mcp', process.pid, log_path
        finally:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=5)


async def main():
    inventory = json.loads(Path(sys.argv[1]).read_text(encoding='utf-8-sig'))
    python = next(i for i in inventory['mcp'] if i['id'] == 'graphify')['paths']['python']
    with tempfile.TemporaryDirectory(prefix='harness-graphify-identity-') as folder:
        temporary = Path(folder)
        graphs = {}
        for name in ('A', 'B'):
            graphs[name] = temporary / f'{name}.json'
            graphs[name].write_text(json.dumps({'directed': False, 'multigraph': False, 'graph': {},
                'nodes': [{'id': name, 'label': f'UNIQUE_GRAPH_{name}', 'community': 0}], 'links': []}), encoding='utf-8')
        async with daemon(python, graphs['B'], temporary) as (endpoint, pid, log):
            with anyio.fail_after(30):
                try:
                    proxy.http_identity(endpoint, python, str(graphs['A']))
                except ValueError:
                    pass
                else:
                    raise AssertionError('Different graph identity accepted')
                async with proxy.upstream(python, str(graphs['A']), endpoint, 'owned-test-only') as (session, identity):
                    assert identity is None, 'Different graph must use selected local fallback'
                    result = await session.call_tool('query_graph', {'question': 'UNIQUE_GRAPH_A'})
                    content = '\n'.join(item.text for item in result.content if item.type == 'text')
                    assert 'UNIQUE_GRAPH_A' in content and 'UNIQUE_GRAPH_B' not in content
                assert 'POST /mcp' not in log.read_text(encoding='utf-8'), 'Wrong endpoint must not receive credentials/query'
                async with proxy.upstream(python, str(graphs['B']), endpoint, 'owned-test-only') as (session, identity):
                    assert identity and (identity[0] == pid or psutil.Process(identity[0]).ppid() == pid)
                    await proxy.check_health(session)
        corrupt = temporary / 'corrupt.json'
        corrupt.write_text('{ broken', encoding='utf-8')
        # Upstream starts despite a corrupt default and returns an error in text
        # with isError=false. The adapter must not declare this backend healthy.
        params = StdioServerParameters(command=python,
            args=['-B', '-u', '-m', 'graphify.serve', '--graph', str(corrupt)])
        async with stdio_client(params) as streams:
            async with ClientSession(*streams) as session:
                await session.initialize()
                result = await session.call_tool('graph_stats', {})
                assert proxy.checked_result(result).isError
                try:
                    await proxy.check_health(session)
                except ValueError:
                    pass
                else:
                    raise AssertionError('Corrupt graph accepted as healthy')
        worktree = temporary / 'explicit repo кириллица'
        worktree.mkdir()
        subprocess.run(['git', 'init', '-q', str(worktree)], check=True)
        assert proxy.validate_repository('list_prs', {'repo': str(worktree)}) == str(worktree.resolve())
        try:
            proxy.validate_repository('list_prs', {'repo': str(temporary)})
        except ValueError:
            pass
        else:
            raise AssertionError('Non-worktree accepted')
        # The actual isolated server's cwd can be observed from its process and
        # worktree operation; no remote API or user's working repository needed.
        async with proxy.upstream(python, str(graphs['A']), None, None, cwd=str(worktree)) as (session, identity):
            assert identity is None
            await proxy.check_health(session)
        print(json.dumps({'same_counts_different_graph_rejected': True, 'fallback_selected_graph': True,
                          'same_graph_http_reused': True, 'corrupt_graph_rejected': True,
                          'explicit_git_worktree_validated': True}))


if __name__ == '__main__':
    anyio.run(main)
