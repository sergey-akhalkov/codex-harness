"""Native CBM queries with explicitly refreshed, OS-bounded indexing."""
from __future__ import annotations

import asyncio
from contextlib import nullcontext
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
import time
import psutil

import anyio
from mcp import types
from mcp.server.lowlevel import Server
from mcp.server.stdio import stdio_server

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from process_ownership import JobGuard
from resources import account_directory, admission, atomic_json, configuration, native, policy

FRESHNESS = ('Resource policy: automatic indexing/watching is disabled. Graphs reflect the last '
             'successful explicit index_repository call. Before graph-backed search or navigation, run '
             'index_repository unless a successful index of the current source state is already verified. '
             'Unknown freshness, source edits, external changes or a branch switch require refresh before '
             'the next graph query; batch edits and reuse a verified unchanged index across related queries. '
             'Wait for indexing to succeed. If refresh is busy or fails, use fresh Serena/LSP or direct '
             'source and treat the retained graph as stale; check coverage before relying on results. '
             'Indexing has one account-wide slot, a 2 GiB memory cap, 25% CPU and a 600-second deadline.')

# v0.10.8's ordinary CLI delegates to an account daemon (its main.c header is
# outdated). Only this audited worker entrypoint performs indexing in our job.
AUDITED_WORKER = 'b4b403b1d7c4def3785f148b93f345ce8427858f4f5489ce28580c4387a336a6'


def command_for(executable, name, arguments, root, limits):
    if isinstance(executable, list):  # Owned subprocess fixtures.
        return [*executable, 'cli', '--json', name, '--args-file', str(root / 'arguments.json')], root / 'stdout'
    if name != 'index_repository':
        return [executable, 'cli', '--json', name, '--args-file', str(root / 'arguments.json')], root / 'stdout'
    if hashlib.sha256(Path(executable).read_bytes()).hexdigest() != AUDITED_WORKER:
        raise RuntimeError('Installed CBM worker build is unreviewed; refusing unbounded daemon indexing')
    response = root / 'response.json'
    return [executable, 'cli', '--index-worker', '--index-worker-build', AUDITED_WORKER,
            name, json.dumps(arguments), '--response-out', str(response),
            '--index-worker-memory-budget-bytes', str(limits['internal_memory_mb'] * 1024**2)], response


def failure(message):
    return types.CallToolResult(isError=True, content=[types.TextContent(type='text', text=message)])


def check_request(cancelled, deadline):
    if cancelled and cancelled.is_set():
        raise RuntimeError('Request cancelled; owned workers reclaimed')
    if time.monotonic() >= deadline:
        raise TimeoutError('Request deadline exceeded; owned workers reclaimed')


def capture_sizes(stdout, stderr, response, maximum):
    sizes = [os.fstat(stream.fileno()).st_size for stream in (stdout, stderr)]
    if response.exists():
        sizes.append(response.stat().st_size)
    if max(sizes) > maximum:
        raise RuntimeError('Native output exceeded its bounded capture size')


def progress_excerpt(stream, maximum=16000):
    stream.seek(0)
    parts = []
    remaining = maximum
    while remaining:
        line = stream.readline(65536)
        if not line:
            break
        if (b'level=' in line and b'parallel.extract.progress' not in line
                and b'parallel.extract.file.' not in line):
            part = line.decode('utf-8', errors='replace')[:remaining]
            parts.append(part)
            remaining -= len(part)
    return ''.join(parts)


def index(arguments, cancelled=None, *, executable=None, limits=None):
    return run_tool('index_repository', arguments, cancelled, executable=executable, limits=limits)


def run_tool(name, arguments, cancelled=None, *, executable=None, limits=None):
    limits = limits or policy()['codebase_memory']
    executable = executable or native()
    started = time.monotonic()
    deadline = started + (limits['deadline_seconds'] if name == 'index_repository' else 60)
    result = None
    process = None
    evidence = {'started_at': time.time(), 'operation': name, 'policy': limits,
                'repository': arguments.get('repo_path'), 'status': 'failed'}
    try:
        check_request(cancelled, deadline)
        with admission('cbm-index', min(limits['admission_seconds'], limits['deadline_seconds'])) if name == 'index_repository' else nullcontext():
            check_request(cancelled, deadline)
            if not isinstance(executable, list):  # Only explicit owned fixtures bypass native policy.
                # Another client may edit settings after this proxy starts or
                # while indexing waits for admission. Refuse drift without writes.
                configuration('check')
            with tempfile.TemporaryDirectory(prefix='harness-cbm-') as temporary, JobGuard(memory_limit_bytes=limits['job_memory_mb'] * 1024**2,
                          cpu_rate_percent=limits['cpu_percent']) as guard:
                if not guard.enabled:
                    raise RuntimeError('Enforced indexing is currently supported on Windows only')
                with nullcontext():
                    root = Path(temporary)
                    arguments_path = root / 'arguments.json'
                    arguments_path.write_text(json.dumps(arguments), encoding='utf-8')
                    env = {**os.environ, 'CBM_WORKERS': str(limits['workers']),
                           'CBM_MEM_BUDGET_MB': str(limits['internal_memory_mb']), 'CBM_LOG_LEVEL': 'info'}
                    if 'retain_total_mb' in limits:
                        env['CBM_RETAIN_TOTAL_MB'] = str(limits['retain_total_mb'])
                        env['CBM_RETAIN_PER_FILE_MB'] = str(limits.get('retain_per_file_mb', 1))
                    with (root / 'stdout').open('w+b') as stdout, (root / 'stderr').open('w+b') as stderr:
                        command, response = command_for(executable, name, arguments, root, limits)
                        check_request(cancelled, deadline)
                        process = guard.popen(command, env=env,
                                              stdin=subprocess.DEVNULL, stdout=stdout, stderr=stderr,
                                              creationflags=subprocess.CREATE_NO_WINDOW)
                        evidence['pid'] = process.pid
                        observed_cpu = {}
                        measured_at = 0
                        while process.poll() is None:
                            check_request(cancelled, deadline)
                            capture_sizes(stdout, stderr, response, limits['output_limit_mb'] * 1024**2)
                            if name == 'index_repository' and time.monotonic() - measured_at >= 0.5:
                                measured_at = time.monotonic()
                                private = working_set = 0
                                try:
                                    parent = psutil.Process(process.pid)
                                    for item in [parent, *parent.children(recursive=True)]:
                                        try:
                                            usage = item.memory_info()
                                            private += getattr(usage, 'private', usage.rss)
                                            working_set += usage.rss
                                            observed_cpu[(item.pid, item.create_time())] = sum(item.cpu_times()[:2])
                                        except psutil.Error:
                                            pass
                                except psutil.Error:
                                    pass
                                evidence['sampled_peak_private_bytes'] = max(private, evidence.get('sampled_peak_private_bytes', 0))
                                evidence['sampled_peak_working_set_bytes'] = max(working_set, evidence.get('sampled_peak_working_set_bytes', 0))
                                evidence['sampled_cpu_seconds'] = round(sum(observed_cpu.values()), 3)
                            time.sleep(0.1)
                        evidence.update(guard.snapshot())
                        evidence['exit_code'] = process.returncode
                        # A completed parent can leave a writer grandchild. Reclaim
                        # the entire job before inspecting its now-immutable files.
                        guard.close()
                        capture_sizes(stdout, stderr, response, limits['output_limit_mb'] * 1024**2)
                        evidence['diagnostic_progress'] = progress_excerpt(stderr)
                        stderr.seek(max(0, os.fstat(stderr.fileno()).st_size - 4000))
                        diagnostic = stderr.read(4000).decode('utf-8', errors='replace')
                        evidence['diagnostic_tail'] = diagnostic
                        if process.returncode:
                            raise RuntimeError(f'Bounded {name} failed (exit {process.returncode}); no automatic retry')
                        with response.open('rb') as result_stream:
                            payload = result_stream.read(limits['output_limit_mb'] * 1024**2 + 1)
                        if len(payload) > limits['output_limit_mb'] * 1024**2:
                            raise RuntimeError('Native response exceeded its bounded capture size')
                        result = types.CallToolResult.model_validate_json(payload)
                        evidence['status'] = 'failed' if result.isError else 'complete'
    except Exception as error:
        evidence['reason'] = str(error)
        detail = account_directory() / f'cbm-failure-{time.time_ns()}.json'
        atomic_json(detail, evidence)
        cause = str(error)
        if len(cause) > 600:
            cause = cause[:600] + '…'
        result = failure(f'{name}: {cause}. Details: {detail}')
    finally:
        if process is not None:
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                evidence['status'] = 'failed'
                result = failure('Owned worker did not finish cleanup within its deadline')
        evidence['elapsed_seconds'] = round(time.monotonic() - started, 3)
        if name == 'index_repository':
            atomic_json(account_directory() / 'cbm-last-index.json', evidence)
    return result


def catalogue():
    executable = native()
    identity = hashlib.sha256(Path(executable).read_bytes()).hexdigest()
    if identity != AUDITED_WORKER:
        raise RuntimeError('Installed CBM build has not passed the bounded-worker audit; update the kit adapter before using it')
    cache = account_directory() / 'cbm-catalogue.json'
    with admission('cbm-catalogue', 20):
        saved = json.loads(cache.read_text(encoding='utf-8')) if cache.exists() else {}
        if saved.get('identity') != identity:
            messages = [
                {'jsonrpc': '2.0', 'id': 1, 'method': 'initialize', 'params': {
                    'protocolVersion': '2024-11-05', 'capabilities': {},
                    'clientInfo': {'name': 'harness-catalogue', 'version': '1'}}},
                {'jsonrpc': '2.0', 'method': 'notifications/initialized'},
                {'jsonrpc': '2.0', 'id': 2, 'method': 'tools/list', 'params': {}}]
            with JobGuard(memory_limit_bytes=policy()['codebase_memory']['job_memory_mb'] * 1024**2,
                          cpu_rate_percent=policy()['codebase_memory']['cpu_percent']) as guard:
                process = guard.popen([executable], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                    stderr=subprocess.PIPE, text=True, encoding='utf-8',
                    env={**os.environ, 'CBM_LOG_LEVEL': 'warn'}, creationflags=subprocess.CREATE_NO_WINDOW)
                try:
                    output, _ = process.communicate(''.join(json.dumps(item) + '\n' for item in messages), timeout=20)
                finally:
                    if process.poll() is None:
                        guard.close()
                        process.wait(timeout=5)
            responses = [json.loads(line) for line in output.splitlines() if line.strip()]
            listed = next((item.get('result', {}).get('tools') for item in responses if item.get('id') == 2), None)
            if process.returncode or not listed:
                raise RuntimeError('Cannot obtain the installed native CBM tool catalogue')
            saved = {'identity': identity, 'tools': listed}
            atomic_json(cache, saved)
        return [types.Tool.model_validate(item) for item in saved['tools']]


async def main():
    await anyio.to_thread.run_sync(configuration, 'check')
    tools = await anyio.to_thread.run_sync(catalogue)
    with nullcontext():
        # Codex prepends server instructions to every discovered tool. Put the
        # refresh contract on its owning operation instead of multiplying it.
        server = Server('harness-codebase-memory')

        @server.list_tools()
        async def list_tools():
            selected = [tool.model_copy(deep=True) for tool in tools]
            for tool in selected:
                if tool.name == 'index_repository':
                    tool.description = (tool.description or '') + '\n\n' + FRESHNESS
            return selected

        @server.call_tool()
        async def call_tool(name, arguments):
            if name not in {tool.name for tool in tools}:
                return failure('Unknown native tool')
            cancelled = threading.Event()
            try:
                return await asyncio.to_thread(run_tool, name, arguments, cancelled)
            except asyncio.CancelledError:
                cancelled.set()
                raise

        async with stdio_server() as streams:
            await server.run(*streams, server.create_initialization_options())


if __name__ == '__main__':
    try:
        anyio.run(main)
    except Exception as error:
        import traceback
        detail = account_directory() / f'cbm-startup-{time.time_ns()}.json'
        atomic_json(detail, {'reason': str(error), 'traceback': traceback.format_exc()})
        print(f'CBM startup failed: {type(error).__name__}: {str(error)[:600]}. Details: {detail}', file=sys.stderr)
        raise SystemExit(1)
