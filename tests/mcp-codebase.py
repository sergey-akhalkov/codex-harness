"""Actual Codebase Memory indexing/query isolation in two same-named worktrees."""
import anyio
import json
import os
from pathlib import Path
import sys
import tempfile
import psutil
from contextlib import contextmanager

from mcp import ClientSession, StdioServerParameters
from mcp.client.stdio import stdio_client


def payload(result):
    assert not result.isError, str(result)
    if result.structuredContent:
        return result.structuredContent
    text = '\n'.join(c.text for c in result.content if c.type == 'text')
    try:
        return json.loads(text)
    except ValueError:
        return text


@contextmanager
def owned_processes(processes):
    try:
        yield
    finally:
        for process in reversed(list(processes.values())):
            try:
                # psutil guards against PID reuse with its creation-time identity.
                process.terminate()
            except psutil.NoSuchProcess:
                pass
        _, alive = psutil.wait_procs(list(processes.values()), timeout=3)
        for process in alive:
            process.kill()
        psutil.wait_procs(alive, timeout=3)


async def main():
    inventory = json.loads(Path(sys.argv[1]).read_text(encoding='utf-8-sig'))
    record = next(item for item in inventory['mcp'] if item['id'] == 'codebase-memory')
    # Codebase verifies every ancestor of its private IPC/cache path. The host's
    # shared TEMP has a sandbox-user mutation ACL, so use the private kit state.
    probe_parent = Path(os.environ.get('CODEX_HOME', str(Path.home() / '.codex'))) / 'harness' / 'verification'
    probe_parent.mkdir(parents=True, exist_ok=True)
    processes = {}
    with tempfile.TemporaryDirectory(prefix='codebase-isolation-', dir=probe_parent) as folder, owned_processes(processes):
        root = Path(folder)
        environment = {**os.environ}
        for variable in ('HOME', 'USERPROFILE', 'APPDATA', 'LOCALAPPDATA', 'XDG_CACHE_HOME', 'XDG_CONFIG_HOME', 'XDG_DATA_HOME'):
            directory = root / variable.lower()
            directory.mkdir(exist_ok=True)
            environment[variable] = str(directory)
        # Windows daemon rendezvous ignores HOME. Use the official override so
        # these disposable indexes cannot reach an existing user's daemon.
        for variable, name in (('CBM_RUNTIME_DIR', 'ipc'), ('CBM_CACHE_DIR', 'cache')):
            directory = root / name
            directory.mkdir()
            environment[variable] = str(directory)
        projects = []
        for tag in ('alpha', 'beta'):
            project = root / tag / 'same name кириллица'
            project.mkdir(parents=True)
            (project / 'sample.py').write_text(f'def shared_symbol():\n    return {tag!r}\n\ndef only_{tag}():\n    return shared_symbol()\n', encoding='utf-8')
            projects.append((tag, project))
        parameters = StdioServerParameters(command=record['paths']['native_executable'], args=[], env=environment, cwd=str(root))
        async with stdio_client(parameters) as streams:
            async with ClientSession(*streams) as client:
                with anyio.fail_after(60):
                    await client.initialize()
                    processes.update({p.pid: p for p in psutil.Process().children(recursive=True)})
                    tools = {tool.name: tool for tool in (await client.list_tools()).tools}
                    names = []
                    for tag, project in projects:
                        indexed = payload(await client.call_tool('index_repository', {'repo_path': str(project)}))
                        listed = payload(await client.call_tool('list_projects', {}))
                        items = listed.get('projects', []) if isinstance(listed, dict) else listed
                        possible = [p.get('name', p.get('project')) if isinstance(p, dict) else p for p in items]
                        created = [name for name in possible if name not in names]
                        assert len(created) == 1, 'Same-named roots must retain distinct project identities: ' + str(listed)
                        names.extend(created)
                    for (tag, project), name in zip(projects, names):
                        query = payload(await client.call_tool('query_graph', {'project': name, 'query': 'MATCH (n:Function) RETURN n.name'}))
                        encoded = json.dumps(query)
                        assert 'shared_symbol' in encoded and f'only_{tag}' in encoded
                        other = 'beta' if tag == 'alpha' else 'alpha'
                        assert f'only_{other}' not in encoded, 'Query leaked the adjacent project'
                    assert len(names) == 2 and names[0] != names[1]
        # A fresh backend must discover both saved indexes without another index call.
        async with stdio_client(parameters) as streams:
            async with ClientSession(*streams) as client:
                with anyio.fail_after(30):
                    await client.initialize()
                    processes.update({p.pid: p for p in psutil.Process().children(recursive=True)})
                    listed = payload(await client.call_tool('list_projects', {}))
                    assert all(name in json.dumps(listed) for name in names)
        print(json.dumps({'two_same_named_roots': True, 'structured_queries_isolated': True,
                          'indexes_survive_backend_restart': True, 'tool_count': len(tools)}, ensure_ascii=False))


if __name__ == '__main__':
    anyio.run(main)
