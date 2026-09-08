"""Opt-in global SDK acceptance; owned external files, graceful broker retirement.

Run with adopted Serena Python and --run. Reads installed registrations verbatim;
does not modify global configuration, dependencies, source projects or saved graphs.
"""
import argparse
from contextlib import asynccontextmanager
from datetime import timedelta
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import tomllib

import anyio
from mcp import ClientSession, StdioServerParameters
from mcp.client.stdio import stdio_client
import psutil

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools/lsp'))
import broker


def data(result):
    assert not result.isError, str(result)
    if result.structuredContent:
        return result.structuredContent
    return json.loads(result.content[0].text)


def state(home, name):
    endpoint = broker.read_endpoint(home / f'harness/runtime/{name}-broker')
    assert endpoint, f'Missing {name} endpoint'
    return endpoint['pid'], broker.exchange(endpoint, 'status', {}, 10)


def retire(home, name):
    script = ROOT / ('tools/lsp/broker.py' if name == 'lsp' else 'tools/code-tools/serena_broker.py')
    result = subprocess.run([sys.executable, '-B', str(script), '--retire'],
        env={**os.environ, 'CODEX_HOME': str(home)}, capture_output=True, text=True, timeout=40,
        creationflags=0x08000000 if os.name == 'nt' else 0)
    assert result.returncode == 0, result.stdout + result.stderr
    return json.loads(result.stdout)


@asynccontextmanager
async def client(record, cwd, log):
    parameters = StdioServerParameters(command=record['command'], args=record['args'],
        env={**os.environ, **record.get('env', {})}, cwd=str(cwd))
    async with stdio_client(parameters, errlog=log) as streams:
        async with ClientSession(*streams, read_timeout_seconds=timedelta(seconds=90)) as session:
            await session.initialize()
            yield session


async def idle_sample(service_pid):
    service = psutil.Process(service_pid)
    excluded = {service_pid, *(p.pid for p in service.children(recursive=True))}
    processes = [p for p in psutil.Process().children(recursive=True) if p.pid not in excluded]
    before = {(p.pid, p.create_time()): sum(p.cpu_times()[:2]) for p in processes}
    started = time.monotonic()
    await anyio.sleep(3)
    rows = []
    for process in processes:
        try:
            identity = (process.pid, process.create_time())
            memory = process.memory_info()
            rows.append({'pid': process.pid, 'private_mib': round(getattr(memory, 'private', memory.rss) / 1024**2, 2),
                         'cpu_seconds': round(sum(process.cpu_times()[:2]) - before[identity], 4)})
        except psutil.NoSuchProcess:
            pass
    return {'seconds': round(time.monotonic() - started, 3), 'thin_processes': rows,
            'service_private_mib': round(getattr(service.memory_info(), 'private', service.memory_info().rss) / 1024**2, 2)}


async def lifecycle(home, evidence):
    """A starter must close without replacing a service used by another client."""
    configuration = tomllib.loads((home / 'config.toml').read_text(encoding='utf-8-sig'))['mcp_servers']
    report = {'status': 'running', 'cwd': str(evidence), 'cases': {}}
    fixture = evidence / 'typescript'
    fixture.mkdir()
    (fixture / 'tsconfig.json').write_text('{"compilerOptions":{"strict":true,"noEmit":true},"include":["*.ts"]}')
    (fixture / 'source.ts').write_text('export const lifecycleSymbol: number = 1;\n')
    with (evidence / 'stderr.log').open('w', encoding='utf-8') as log:
        for name, registration in (('lsp', 'harness-lsp'), ('serena', 'serena')):
            row = report['cases'][name] = {}
            ready, close_first, closed = anyio.Event(), anyio.Event(), anyio.Event()

            async def operation(session):
                if name == 'lsp':
                    value = data(await session.call_tool('diagnostics', {'workspace': str(fixture),
                        'session_id': 'global-resource-lifecycle', 'file': 'source.ts'}))
                    assert value['status'] == 'clean', value
                else:
                    value = await session.call_tool('activate_project', {'project': str(ROOT)})
                    assert not value.isError, value
                    value = await session.call_tool('get_symbols_overview', {'relative_path': 'tools/code-tools/resources.py'})
                    assert not value.isError and 'policy' in str(value), value

            async def starter():
                try:
                    async with client(configuration[registration], evidence, log) as session:
                        await operation(session)
                        row['before_pid'], row['before_state'] = state(home, name)
                        ready.set()
                        await close_first.wait()
                except BaseException as error:
                    row['starter_error'] = repr(error)
                    ready.set()
                finally:
                    closed.set()

            try:
                row['pre_retire'] = retire(home, name)
                with anyio.fail_after(120):
                    async with anyio.create_task_group() as tasks:
                        tasks.start_soon(starter)
                        await ready.wait()
                        assert 'starter_error' not in row, row
                        try:
                            async with client(configuration[registration], evidence, log) as second:
                                await operation(second)
                                assert state(home, name)[0] == row['before_pid']
                                close_first.set()
                                await closed.wait()
                                owner = psutil.Process(row['before_pid']) if psutil.pid_exists(row['before_pid']) else None
                                row['original_alive_after_starter_close'] = owner is not None
                                try:
                                    await operation(second)
                                    row['second_call'] = 'succeeded'
                                    row['after_pid'], row['after_state'] = state(home, name)
                                except Exception as error:
                                    row['second_call'] = repr(error)
                                assert row.get('after_pid') == row['before_pid'], row
                                assert row['original_alive_after_starter_close'], row
                                collection = 'backends' if name == 'lsp' else 'workers'
                                before_workers = {item['pid'] for item in row['before_state'][collection]}
                                after_workers = {item['pid'] for item in row['after_state'][collection]}
                                assert before_workers <= after_workers, row
                                row['status'] = 'passed'
                        finally:
                            close_first.set()
                            await closed.wait()
            except BaseException as error:
                row['status'] = 'failed'
                row['failure'] = repr(error)
            finally:
                row['retire'] = retire(home, name)
                if row.get('status') == 'passed' and row['retire']['status'] != 'retired':
                    row['status'] = 'failed'
                    row['failure'] = 'Service did not survive until explicit retirement'
                (evidence / 'report.json').write_text(json.dumps(report, indent=2), encoding='utf-8')
    report['status'] = 'passed' if all(row['status'] == 'passed' for row in report['cases'].values()) else 'failed'
    (evidence / 'report.json').write_text(json.dumps(report, indent=2), encoding='utf-8')
    print(json.dumps(report), flush=True)
    return report['status'] == 'passed'


async def run(home, evidence):
    configuration = tomllib.loads((home / 'config.toml').read_text(encoding='utf-8-sig'))['mcp_servers']
    registry_path = home / 'harness/code-tools.json'
    registry = json.loads(registry_path.read_text(encoding='utf-8-sig'))
    report = {'home': str(home), 'cwd': str(evidence), 'checks': {}, 'registration_sha256':
              hashlib.sha256((home / 'config.toml').read_bytes()).hexdigest()}
    def save():
        (evidence / 'report.json').write_text(json.dumps(report, indent=2), encoding='utf-8')
    fixture = evidence / 'typescript'
    fixture.mkdir()
    (fixture / 'tsconfig.json').write_text('{"compilerOptions":{"strict":true,"noEmit":true},"include":["*.ts"]}')
    source = fixture / 'source.ts'
    source.write_text("export const externalAcceptance: number = 'wrong';\n")
    with (evidence / 'stderr.log').open('w', encoding='utf-8') as log:
        try:
            report['retired_before'] = {name: retire(home, name) for name in ('lsp', 'serena')}
            async with client(configuration['harness-lsp'], evidence, log) as first, client(configuration['harness-lsp'], evidence, log) as second:
                async def diagnostic(session, identity):
                    return data(await session.call_tool('diagnostics', {'workspace': str(fixture),
                        'session_id': identity, 'file': 'source.ts'}))
                bad = await diagnostic(first, 'global-resource-first')
                assert any(row.get('code') == 2322 for row in bad['diagnostics']), bad
                service_pid, initial = state(home, 'lsp')
                selected = [row for row in initial['backends'] if row['key'][0] == os.path.normcase(str(fixture))]
                assert len(selected) == 1, selected
                pid = selected[0]['pid']
                assert (await diagnostic(second, 'global-resource-second'))['status'] == 'diagnostics'
                second_pid, current = state(home, 'lsp')
                assert second_pid == service_pid
                assert [row['pid'] for row in current['backends'] if row['key'][0] == os.path.normcase(str(fixture))] == [pid]
                source.write_text('export const externalAcceptance: number = 1;\n')
                assert (await diagnostic(second, 'global-resource-second'))['status'] == 'clean'
                report['checks']['lsp'] = {'error': 2322, 'fixed': 'clean', 'two_clients_backend_pid': pid,
                    'service_pid': service_pid, 'idle': await idle_sample(service_pid)}
            report['lsp_cleanup'] = retire(home, 'lsp')
            save()
            async with client(configuration['serena'], evidence, log) as first, client(configuration['serena'], evidence, log) as second:
                for session in (first, second):
                    result = await session.call_tool('activate_project', {'project': str(ROOT)})
                    assert not result.isError, result
                    result = await session.call_tool('get_symbols_overview', {'relative_path': 'tools/code-tools/resources.py', 'depth': 0})
                    assert not result.isError and 'configuration' in str(result) and 'policy' in str(result), result
                service_pid, status = state(home, 'serena')
                workers = [row for row in status['workers'] if row['project'] and Path(row['project']).resolve() == ROOT]
                assert len(workers) == 1, status
                report['checks']['serena'] = {'project': str(ROOT), 'symbols': ['configuration', 'policy'],
                    'two_clients_worker_pid': workers[0]['pid'], 'service_pid': service_pid,
                    'client_count': status['clients'], 'idle': await idle_sample(service_pid)}
            report['serena_cleanup'] = retire(home, 'serena')
            save()
            record = next(row for row in registry['mcp'] if row['id'] == 'graphify')
            graph = Path(record['shared_service']['graph_path'])
            graph_project = graph.parent.parent
            before = hashlib.sha256(graph.read_bytes()).hexdigest()
            async with client(configuration['graphify'], evidence, log) as session:
                stats = await session.call_tool('graph_stats', {'project_path': str(graph_project)})
                assert not stats.isError and 'Nodes:' in str(stats), stats
                query = await session.call_tool('query_graph', {'project_path': str(graph_project), 'question': 'Pmac', 'token_budget': 200})
                assert not query.isError and query.content, query
                report['checks']['graphify'] = {'project_path': str(graph_project), 'graph': str(graph),
                    'stats': str(stats), 'query': str(query)}
            assert hashlib.sha256(graph.read_bytes()).hexdigest() == before
            report['checks']['graphify']['graph_unchanged'] = True
            report['status'] = 'passed'
        except BaseException as error:
            report['status'] = 'failed'
            report['failure'] = repr(error)
            raise
        finally:
            for name in ('lsp', 'serena'):
                try:
                    report[name + '_final_retire'] = retire(home, name)
                except Exception as error:
                    report[name + '_cleanup_failure'] = repr(error)
            save()
    print(json.dumps(report, ensure_ascii=False), flush=True)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--run', action='store_true', required=True)
    parser.add_argument('--lifecycle-only', action='store_true')
    parser.add_argument('--home', type=Path, default=Path(os.environ.get('CODEX_HOME', Path.home() / '.codex')))
    arguments = parser.parse_args()
    evidence = Path(tempfile.mkdtemp(prefix='harness-global-resources-'))
    print('Evidence: ' + str(evidence / 'report.json'), flush=True)
    if arguments.lifecycle_only:
        sys.exit(0 if anyio.run(lifecycle, arguments.home, evidence) else 1)
    anyio.run(run, arguments.home, evidence)
