"""Opt-in installed hooks/MCP proof. Writes only owned temp fixtures and runtime journals.

--consumer reads an existing project; it never writes or analyzes its product code.
Run with the installed Serena Python. No model calls or controller connections.
"""
from __future__ import annotations

import argparse
import asyncio
import hashlib
import json
import os
from pathlib import Path
import shlex
import subprocess
import tempfile
import time
import tomllib
import uuid

from mcp import ClientSession, StdioServerParameters
from mcp.client.stdio import stdio_client


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--consumer', type=Path)
    parser.add_argument('--consumer-preserved-file', type=Path,
                        help='Existing consumer-relative file whose hash must remain unchanged')
    args = parser.parse_args()
    if bool(args.consumer) != bool(args.consumer_preserved_file):
        parser.error('--consumer and --consumer-preserved-file must be supplied together')
    preserved = None
    if args.consumer:
        consumer_root = args.consumer.resolve(strict=True)
        preserved = (consumer_root / args.consumer_preserved_file).resolve(strict=True)
        if not preserved.is_relative_to(consumer_root) or not preserved.is_file():
            parser.error('The preserved file must be an existing file inside the consumer')
    codex = Path(os.environ.get('CODEX_HOME', Path.home() / '.codex'))
    installation = json.loads((codex / 'harness/installation.json').read_text(encoding='utf-8-sig'))
    hooks = json.loads((codex / 'hooks.json').read_text(encoding='utf-8-sig'))['hooks']
    mcp = tomllib.loads((codex / 'config.toml').read_text(encoding='utf-8-sig'))['mcp_servers']['harness-lsp']
    probe = Path(tempfile.mkdtemp(prefix='harness-installed-reconciliation-')).resolve()
    workspace = probe / 'workspace'
    workspace.mkdir()
    records = []
    environment = {**os.environ, 'CODEX_HOME': str(codex), 'HARNESS_LSP_WORKSPACE_ROOTS': '[]'}

    def event(root=workspace, session=None, **values):
        return {'cwd': str(root), 'session_id': session or probe.name,
            'turn_id': 'turn', 'tool_use_id': uuid.uuid4().hex, 'tool_name': 'Bash',
            'tool_input': {}, **values}

    def hook(kind, payload):
        command = next(item['command'] for group in hooks[kind] for item in group['hooks'] if item['type'] == 'command')
        started = time.monotonic()
        result = subprocess.run(shlex.split(command, posix=False), input=json.dumps({**payload, 'hook_event_name': kind}),
            text=True, encoding='utf-8', capture_output=True, cwd=payload['cwd'], env=environment, timeout=35)
        elapsed = time.monotonic() - started
        assert result.returncode == 0, result.stderr
        output = json.loads(result.stdout)
        assert elapsed < (10 if kind == 'PreToolUse' else 30), (kind, elapsed, output)
        records.append({'event': kind, 'session': payload['session_id'], 'elapsed': round(elapsed, 3), 'output': output})
        (probe / 'progress.json').write_text(json.dumps(records, ensure_ascii=False, indent=2), encoding='utf-8')
        return output

    def diagnostic(output):
        context = output['hookSpecificOutput']['additionalContext']
        return json.loads(context[context.index('{'):])

    def rows(output, status, name):
        return [row for row in diagnostic(output)['results'] if row['file'] == name and row['status'] == status]

    source = workspace / 'index.ts'
    good, bad = 'export const value: number = 1;\n', 'export const value: number = "bad";\n'
    source.write_text(good)
    large = workspace / 'archive.json'
    large.write_text(json.dumps({'payload': 'a' * (9 * 1024 * 1024)}))
    (workspace / 'tsconfig.json').write_text('{"compilerOptions":{"strict":true},"include":["*.ts"]}')
    initial = event()
    assert hook('PreToolUse', initial) == {}
    assert hook('PostToolUse', initial) == {}  # read-only and large unchanged
    edit = event()
    assert hook('PreToolUse', edit) == {}
    source.write_text(bad)
    error = hook('PostToolUse', edit)  # deliberately no connected MCP: command fallback
    assert rows(error, 'diagnostics', 'index.ts'), error
    stop = event(tool_use_id='')
    assert hook('Stop', stop).get('decision') == 'block'
    assert hook('Stop', {**stop, 'stop_hook_active': True}) == {}
    assert hook('Stop', {**stop, 'turn_id': 'continued'}) == {}
    correction = event()
    hook('PreToolUse', correction)
    source.write_text(good)
    assert rows(hook('PostToolUse', correction), 'clean', 'index.ts')
    big_edit = event()
    hook('PreToolUse', big_edit)
    with large.open('r+b') as stream:
        stream.seek(20)
        stream.write(b'b')
    skipped = hook('PostToolUse', big_edit)
    assert rows(skipped, 'skipped', 'archive.json'), skipped
    assert diagnostic(skipped)['status'] == 'unresolved'
    assert hook('Stop', event(tool_use_id='')).get('decision') != 'block'
    assert hook('Stop', event(tool_use_id='')) == {}

    async def transport_race():
        # Installed MCP launcher plus installed command handler, one invocation.
        parameters = StdioServerParameters(command=mcp['command'], args=mcp['args'],
            env={**environment, **mcp.get('env', {})}, cwd=str(workspace))
        async with stdio_client(parameters) as (read, write):
            async with ClientSession(read, write) as client:
                await client.initialize()
                invocation = event(session=probe.name + '-race')
                await asyncio.to_thread(hook, 'PreToolUse', invocation)
                source.write_text(bad)
                native_payload = {**invocation, 'event': 'PostToolUse'}
                native, command = await asyncio.gather(client.call_tool('diagnostics_after_tool', native_payload),
                    asyncio.to_thread(hook, 'PostToolUse', invocation))
                assert not native.isError, native
                native_output = native.structuredContent
                if native_output is None:
                    native_output = json.loads(native.content[0].text)
                deliveries = [output for output in [native_output, command] if output.get('hookSpecificOutput')]
                assert len(deliveries) == 1, (native_output, command)
                assert rows(deliveries[0], 'diagnostics', 'index.ts')
                records.append({'scenario': 'native-command-race', 'deliveries': len(deliveries)})

    asyncio.run(transport_race())
    child = event(session=probe.name + '-child', transcript_path=str(probe / 'child.jsonl'))
    hook('PreToolUse', child)
    source.write_text(good)
    assert rows(hook('PostToolUse', child), 'clean', 'index.ts')
    child_stop = {**child, 'tool_use_id': '', 'agent_id': 'child', 'agent_transcript_path': child['transcript_path'],
        'transcript_path': str(probe / 'parent.jsonl')}
    assert hook('SubagentStop', child_stop).get('decision') != 'block'
    assert hook('SubagentStop', child_stop) == {}

    if args.consumer:
        consumer = args.consumer.resolve(strict=True)
        assert preserved is not None
        archive = preserved
        with archive.open('rb') as stream:
            before = hashlib.file_digest(stream, 'sha256').hexdigest()
        fresh = event(consumer, probe.name + '-consumer-fresh')
        baseline = hook('PreToolUse', fresh)
        if baseline:
            assert 'unverified' in json.dumps(baseline), baseline
            assert '8 MiB' not in json.dumps(baseline), baseline
        fresh_post = hook('PostToolUse', fresh)
        if fresh_post:
            observed = diagnostic(fresh_post)
            assert observed['status'] == 'unresolved', observed
            assert observed['results'] == [] and observed['problems'], observed
        assert hook('Stop', {**fresh, 'tool_use_id': ''}).get('decision') != 'block'
        assert hook('Stop', {**fresh, 'tool_use_id': ''}) == {}
        resumed = event(consumer, probe.name + '-consumer-resumed')
        missing = hook('PostToolUse', resumed)
        report = diagnostic(missing)
        assert report['status'] == 'unresolved' and report['coverage']['historical_gap'], report
        assert report['results'] == [], report  # no product language analysis
        first = hook('Stop', {**resumed, 'tool_use_id': ''})
        assert first.get('decision') != 'block'
        for active in [False, True, False]:
            assert hook('Stop', {**resumed, 'tool_use_id': '', 'stop_hook_active': active}) == {}
        with archive.open('rb') as stream:
            assert hashlib.file_digest(stream, 'sha256').hexdigest() == before
        records.append({'scenario': 'consumer-read-only', 'root': str(consumer), 'archive_sha256': before,
            'observed_files': report['coverage']['observed_files']})
    output = {'sourceRoot': installation['sourceRoot'], 'probe': str(probe), 'checks': records}
    (probe / 'report.json').write_text(json.dumps(output, ensure_ascii=False, indent=2), encoding='utf-8')
    print(json.dumps({'passed': True, 'checks': len(records), 'report': str(probe / 'report.json')}, indent=2))


if __name__ == '__main__':
    main()
