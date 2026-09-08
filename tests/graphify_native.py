"""Real graphifyy STDIO fallback and authenticated HTTP reuse against the preserved graph."""
import anyio
from contextlib import nullcontext
import hashlib
import json
import os
from pathlib import Path
import socket
import sys
import tempfile
import time
import uuid
from typing import cast

from native_contracts import Inventory

from mcp import ClientSession, StdioServerParameters
from mcp.client.stdio import stdio_client

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools'))
from process_ownership import JobGuard


async def calls(registry: Path, environment: dict[str, str]) -> dict[str, str | int | bool]:
    process = StdioServerParameters(command=sys.executable, args=['-B', '-u', str(ROOT / 'tools/code-tools/graphify_proxy.py')],
                                   env={**environment, 'HARNESS_CODE_TOOLS_REGISTRY': str(registry), 'PYTHONDONTWRITEBYTECODE': '1'})
    async with stdio_client(process) as streams:
        async with ClientSession(*streams) as client:
            with anyio.fail_after(90):
                info = await client.initialize()
                assert info.serverInfo.name == 'harness-graphify'
                names = {tool.name for tool in (await client.list_tools()).tools}
                assert {'graph_stats', 'query_graph', 'get_node', 'list_prs'}.issubset(names)
                stats = await client.call_tool('graph_stats', {})
                stats_text = [item.text for item in stats.content if item.type == 'text']
                assert not stats.isError and any('Nodes:' in text for text in stats_text)
                query = await client.call_tool('query_graph', {'question': 'Pmac', 'token_budget': 200})
                assert not query.isError and query.content
                rejected = await client.call_tool('list_prs', {})
                rejected_text = [item.text for item in rejected.content if item.type == 'text']
                assert rejected.isError and rejected_text and 'repo' in rejected_text[0]
                return {'tool_count': len(names), 'stats': stats_text[0], 'query_succeeded': True, 'missing_repo_rejected': True}


async def main() -> None:
    source = Path(sys.argv[1])
    inventory = cast(Inventory, json.loads(source.read_text(encoding='utf-8-sig')))
    graphify = next(item for item in inventory['mcp'] if item['id'] == 'graphify')
    service = graphify.get('shared_service')
    assert service is not None, 'Selected Graphify must have a saved graph'
    graph = Path(service['graph_path'])
    before = hashlib.file_digest(graph.open('rb'), 'sha256').hexdigest()
    with nullcontext(tempfile.mkdtemp(prefix='harness-graphify-')) as folder:
        temporary = Path(folder)
        registry = temporary / 'code-tools.json'
        _ = registry.write_text(json.dumps(inventory), encoding='utf-8')
        settings = temporary / 'graphify.json'
        # A guaranteed closed port exercises fallback without depending on OpenCode.
        sock = socket.socket()
        sock.bind(('127.0.0.1', 0))
        port = cast(tuple[str, int], sock.getsockname())[1]
        sock.close()
        token = uuid.uuid4().hex + uuid.uuid4().hex
        environment = {**os.environ, 'HARNESS_GRAPHIFY_TEST_TOKEN': token}
        _ = settings.write_text(json.dumps({'graph_path': str(graph), 'endpoint': f'http://127.0.0.1:{port}/mcp',
                                        'credential_env': 'HARNESS_GRAPHIFY_TEST_TOKEN'}), encoding='utf-8')
        fallback = await calls(registry, environment)
        log_path = temporary / 'http.log'
        with log_path.open('w+', encoding='utf-8') as log:
            guard = JobGuard()
            daemon = guard.popen([graphify['paths']['python'], '-B', '-u', '-m', 'graphify.serve', '--graph', str(graph),
                '--transport', 'http', '--host', '127.0.0.1', '--port', str(port), '--path', '/mcp', '--stateless'],
                env={**os.environ, 'GRAPHIFY_API_KEY': token, 'PYTHONDONTWRITEBYTECODE': '1', 'PYTHONUTF8': '1', 'PYTHONIOENCODING': 'utf-8'},
                stdout=log, stderr=log, creationflags=0x08000000 if os.name == 'nt' else 0)
            try:
                deadline = time.monotonic() + 60
                while True:
                    if daemon.poll() is not None:
                        raise RuntimeError('Owned Graphify HTTP fixture exited during startup')
                    try:
                        with socket.create_connection(('127.0.0.1', port), timeout=0.2):
                            break
                    except OSError:
                        if time.monotonic() > deadline:
                            raise TimeoutError('Owned Graphify HTTP fixture did not start')
                        await anyio.sleep(0.2)
                shared = await calls(registry, environment)
                log.flush()
                # Successful proxy requests reached this exact authenticated fixture.
                assert b'POST /mcp' in log_path.read_bytes()
            finally:
                guard.close()
                _ = daemon.wait(timeout=5)
        after = hashlib.file_digest(graph.open('rb'), 'sha256').hexdigest()
        assert before == after, 'The selected saved graph changed during read-only acceptance'
        report = {'stdio_fallback': fallback, 'http_reuse': shared, 'graph_unchanged': True,
                  'source': {name: hashlib.sha256((ROOT / 'tools/code-tools' / name).read_bytes()).hexdigest()
                             for name in ('graphify_proxy.py', 'lazy_stdio.py')}}
        _ = (temporary / 'report.json').write_text(json.dumps(report, indent=2), encoding='utf-8')
        print(json.dumps(report, ensure_ascii=False))
        print('Graphify evidence: ' + str(temporary / 'report.json'))


if __name__ == '__main__':
    anyio.run(main)
