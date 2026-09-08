"""Keep Graphify's graph identity across HTTP reuse and session-local stdio fallback."""
from __future__ import annotations

from contextlib import asynccontextmanager, nullcontext
import json
import os
from pathlib import Path
import re
import subprocess
import sys
from urllib.parse import urlparse

import anyio
import psutil
sys.path.insert(0, str(Path(__file__).resolve().parent))
from mcp import ClientSession, StdioServerParameters, types
from mcp.client.stdio import stdio_client
from mcp.client.streamable_http import streamablehttp_client
from mcp.server.lowlevel import Server
from mcp.server.stdio import stdio_server
from lazy_stdio import LazyStdio
from resources import policy

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from process_ownership import JobGuard

REPOSITORY_TOOLS = {'list_prs', 'get_pr_impact', 'triage_prs'}


def http_identity(endpoint: str, python: str, graph: str):
    """A healthy Graphify on the right port may still be serving another graph.

    Reuse only a directly inspectable local listener launched with our adopted
    interpreter/module and explicit graph. An unprovable identity falls back to
    owned stdio; neither credentials nor process arguments are logged.
    """
    parsed = urlparse(endpoint)
    port = parsed.port or (443 if parsed.scheme == 'https' else 80)
    addresses = {'127.0.0.1', '::1'} if parsed.hostname == 'localhost' else {parsed.hostname}
    owners = {c.pid for c in psutil.net_connections(kind='tcp')
              if c.status == psutil.CONN_LISTEN and c.laddr.port == port
              and c.laddr.ip in addresses and c.pid is not None}
    if len(owners) != 1:
        raise ValueError('Cannot establish a unique local Graphify listener.')
    process = psutil.Process(owners.pop())
    args = process.cmdline()
    if not args or Path(args[0]).resolve() != Path(python).resolve():
        raise ValueError('Shared listener uses a different Python installation.')
    # Require the direct module contract, with no -c/script indirection.
    module_index = args.index('-m') if '-m' in args else -1
    if module_index < 1 or args[module_index + 1:module_index + 2] != ['graphify.serve']:
        raise ValueError('Shared listener is not the expected Graphify module.')
    tail = args[module_index + 2:]
    if tail.count('--graph') != 1 or '--transport' not in tail:
        raise ValueError('Shared listener has no explicit graph/transport identity.')
    if tail[tail.index('--transport') + 1:tail.index('--transport') + 2] != ['http']:
        raise ValueError('Shared listener has a different transport.')
    selected = tail[tail.index('--graph') + 1:tail.index('--graph') + 2]
    if not selected or not Path(selected[0]).is_absolute() or Path(selected[0]).resolve() != Path(graph).resolve():
        raise ValueError('Shared listener serves a different graph.')
    return process.pid, process.create_time()


def checked_result(result):
    # graphifyy 0.9.44 catches tool exceptions and returns text with isError=false.
    if any(item.type == 'text' and item.text.startswith(('Error executing ', 'Error: ', 'Unknown tool: '))
           for item in result.content):
        return result.model_copy(update={'isError': True})
    return result


async def check_health(session):
    health = checked_result(await session.call_tool('graph_stats', {}))
    text = ''.join(item.text for item in health.content if item.type == 'text')
    if health.isError or not re.fullmatch(
            r'Nodes: \d+\nEdges: \d+\nCommunities: \d+\nEXTRACTED: \d+%\nINFERRED: \d+%\nAMBIGUOUS: \d+%\n?', text):
        raise ValueError('Selected Graphify graph did not return valid statistics.')


def selected_graph(record, settings_path):
    """Share the exact host graph selection between runtime and explicit update."""
    settings_path = Path(settings_path)
    settings = json.loads(settings_path.read_text(encoding='utf-8-sig')) if settings_path.is_file() else {}
    service = record.get('shared_service', {})
    graph = settings.get('graph_path') or service.get('graph_path')
    if not graph or not Path(graph).is_file():
        raise RuntimeError('Graphify graph is missing. Set graph_path in CODEX_HOME/harness/graphify.json.')
    return Path(graph).resolve(), settings


def configuration():
    registry_path = Path(os.environ['HARNESS_CODE_TOOLS_REGISTRY'])
    registry = json.loads(registry_path.read_text(encoding='utf-8-sig'))
    record = next(item for item in registry['mcp'] if item['id'] == 'graphify')
    graph, settings = selected_graph(record, registry_path.with_name('graphify.json'))
    service = record.get('shared_service', {})
    manifest_path = service.get('manifest')
    manifest = json.loads(Path(manifest_path).read_text(encoding='utf-8-sig')) if manifest_path and Path(manifest_path).is_file() else {}
    known = manifest.get('graphify', {}).get('configuration', {})
    same_graph = known.get('graph', {}).get('path') and Path(known['graph']['path']).resolve() == Path(graph).resolve()
    endpoint = settings.get('endpoint') or (known.get('endpoint', {}).get('url') if same_graph else None)
    credential = os.environ.get(settings.get('credential_env', 'OPENCODE_GRAPHIFY_API_KEY'))
    credential_path = settings.get('credential_file') or (str(Path(manifest_path).with_name('graphify-api-key')) if manifest_path and same_graph else None)
    if not credential and credential_path and Path(credential_path).is_file():
        credential = Path(credential_path).read_text(encoding='utf-8-sig').strip()
    if endpoint:
        parsed = urlparse(endpoint)
        if parsed.hostname not in ('127.0.0.1', 'localhost', '::1') or parsed.scheme not in ('http', 'https') or parsed.username or parsed.password:
            raise ValueError('Shared Graphify endpoint must be a credential-free local HTTP URL.')
    python = record['paths']['python']
    if not Path(python).is_file():
        raise FileNotFoundError('Registered graphifyy Python is missing.')
    return python, str(Path(graph).resolve()), endpoint, credential


@asynccontextmanager
async def upstream(python, graph, endpoint, credential, *, cwd=None):
    connected = False
    if endpoint and credential:
        try:
            identity = await anyio.to_thread.run_sync(http_identity, endpoint, python, graph)
            async with streamablehttp_client(endpoint, headers={'Authorization': f'Bearer {credential}'}, timeout=5) as streams:
                async with ClientSession(streams[0], streams[1]) as session:
                    with anyio.fail_after(8):
                        info = await session.initialize()
                        if info.serverInfo.name != 'graphify':
                            raise ValueError('Shared endpoint has a different server identity.')
                        await check_health(session)
                        if identity != await anyio.to_thread.run_sync(http_identity, endpoint, python, graph):
                            raise ValueError('Shared Graphify listener changed during initialization.')
                    connected = True
                    yield session, identity
                    return
        except Exception:
            if connected:
                raise
            # Do not log an HTTP exception: it can embed request authentication.
            print('Shared Graphify unavailable; using the selected graph via local stdio.', file=sys.stderr)
    parameters = StdioServerParameters(command=python, args=['-B', '-u', '-m', 'graphify.serve', '--graph', graph],
                                       env={**os.environ, 'GRAPHIFY_MAX_CONTEXTS': str(policy()['graphify']['max_contexts']), 'PYTHONUTF8': '1', 'PYTHONIOENCODING': 'utf-8', 'PYTHONDONTWRITEBYTECODE': '1'}, cwd=cwd)
    async with LazyStdio(parameters, idle_seconds=300) as session:
        with nullcontext():
            with anyio.fail_after(60):
                info = await session.initialize()
                if info.serverInfo.name != 'graphify':
                    raise ValueError('Selected process is not the expected Graphify server.')
                await check_health(session)
            yield session, None


def validate_repository(name: str, arguments: dict):
    if name not in REPOSITORY_TOOLS:
        return
    repo = arguments.get('repo')
    if not isinstance(repo, str) or not repo.strip() or not Path(repo).is_absolute() or not Path(repo).is_dir():
        raise ValueError(f'{name} requires an explicit absolute existing repo directory; the shared server cwd is not a repository selection.')
    root = subprocess.run(['git', '-C', repo, 'rev-parse', '--show-toplevel'],
                          stdin=subprocess.DEVNULL, capture_output=True, text=True, encoding='utf-8', timeout=5)
    if root.returncode or Path(root.stdout.strip()).resolve() != Path(repo).resolve():
        raise ValueError(f'{name} requires the root of an explicit Git worktree.')
    return str(Path(repo).resolve())


async def main():
    JobGuard().contain_current_process()
    python, graph, endpoint, credential = configuration()
    async with upstream(python, graph, endpoint, credential) as (remote, identity):
        server = Server('harness-graphify')

        async def verify_identity():
            if identity is not None and identity != await anyio.to_thread.run_sync(http_identity, endpoint, python, graph):
                raise ValueError('Shared Graphify listener identity changed; reconnect the tool.')

        @server.list_tools()
        async def list_tools():
            await verify_identity()
            tools = (await remote.list_tools()).tools
            for tool in tools:
                if tool.name in REPOSITORY_TOOLS:
                    tool.inputSchema.setdefault('properties', {})['repo'] = {
                        'type': 'string', 'description': 'Explicit absolute local Git worktree root. Repository tools run in an isolated process with this cwd.'}
                    tool.inputSchema['required'] = list(dict.fromkeys([*tool.inputSchema.get('required', []), 'repo']))
            return tools

        @server.call_tool()
        async def call_tool(name: str, arguments: dict):
            repo = await anyio.to_thread.run_sync(validate_repository, name, arguments)
            if repo:
                # Upstream gh --repo expects owner/name, not a path, and its
                # worktree/git helpers use cwd. Never use the HTTP daemon's cwd.
                async with upstream(python, graph, None, None, cwd=repo) as (local, _):
                    return checked_result(await local.call_tool(name, {key: value for key, value in arguments.items() if key != 'repo'}))
            await verify_identity()
            return checked_result(await remote.call_tool(name, arguments))

        @server.list_resources()
        async def list_resources():
            await verify_identity()
            return (await remote.list_resources()).resources

        @server.read_resource()
        async def read_resource(uri):
            await verify_identity()
            from mcp.server.lowlevel.helper_types import ReadResourceContents
            result = await remote.read_resource(uri)
            return [ReadResourceContents(content=item.text, mime_type=item.mimeType) for item in result.contents]

        async with stdio_server() as streams:
            await server.run(*streams, server.create_initialization_options())


if __name__ == '__main__':
    try:
        anyio.run(main)
    except Exception as error:
        # Configuration errors do not contain secrets; upstream exceptions might.
        print(f'Graphify connection failed ({type(error).__name__}); check the selected graph and installation.', file=sys.stderr)
        raise SystemExit(1)
