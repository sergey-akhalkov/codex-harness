"""Real graphifyy STDIO fallback and authenticated HTTP reuse against the preserved graph."""
import anyio
import hashlib
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import time
import uuid

from mcp import ClientSession, StdioServerParameters
from mcp.client.stdio import stdio_client

ROOT = Path(__file__).resolve().parents[1]


async def calls(registry, environment):
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
                assert not stats.isError and any('Nodes:' in item.text for item in stats.content)
                query = await client.call_tool('query_graph', {'question': 'Pmac', 'token_budget': 200})
                assert not query.isError and query.content
                rejected = await client.call_tool('list_prs', {})
                assert rejected.isError and 'repo' in rejected.content[0].text
                return {'tool_count': len(names), 'stats': stats.content[0].text, 'query_succeeded': True, 'missing_repo_rejected': True}


async def main():
    source = Path(sys.argv[1])
    inventory = json.loads(source.read_text(encoding='utf-8-sig'))
    graphify = next(item for item in inventory['mcp'] if item['id'] == 'graphify')
    graph = Path(graphify['shared_service']['graph_path'])
    before = hashlib.file_digest(graph.open('rb'), 'sha256').hexdigest()
    with tempfile.TemporaryDirectory(prefix='harness-graphify-') as folder:
        temporary = Path(folder)
        registry = temporary / 'code-tools.json'
        registry.write_text(json.dumps(inventory), encoding='utf-8')
        settings = temporary / 'graphify.json'
        # A guaranteed closed port exercises fallback without depending on OpenCode.
        sock = socket.socket()
        sock.bind(('127.0.0.1', 0))
        port = sock.getsockname()[1]
        sock.close()
        token = uuid.uuid4().hex + uuid.uuid4().hex
        environment = {**os.environ, 'HARNESS_GRAPHIFY_TEST_TOKEN': token}
        settings.write_text(json.dumps({'graph_path': str(graph), 'endpoint': f'http://127.0.0.1:{port}/mcp',
                                        'credential_env': 'HARNESS_GRAPHIFY_TEST_TOKEN'}), encoding='utf-8')
        fallback = await calls(registry, environment)
        log_path = temporary / 'http.log'
        with log_path.open('w+', encoding='utf-8') as log:
            daemon = subprocess.Popen([graphify['paths']['python'], '-B', '-u', '-m', 'graphify.serve', '--graph', str(graph),
                '--transport', 'http', '--host', '127.0.0.1', '--port', str(port), '--path', '/mcp', '--stateless'],
                env={**os.environ, 'GRAPHIFY_API_KEY': token, 'PYTHONDONTWRITEBYTECODE': '1'},
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
                assert 'POST /mcp' in log_path.read_text(encoding='utf-8')
            finally:
                daemon.terminate()
                try:
                    daemon.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    daemon.kill()
                    daemon.wait(timeout=5)
        after = hashlib.file_digest(graph.open('rb'), 'sha256').hexdigest()
        assert before == after, 'The selected saved graph changed during read-only acceptance'
        print(json.dumps({'stdio_fallback': fallback, 'http_reuse': shared, 'graph_unchanged': True}, ensure_ascii=False))


if __name__ == '__main__':
    anyio.run(main)
