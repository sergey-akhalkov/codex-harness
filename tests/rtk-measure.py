"""Matched native PowerShell command measurements; writes only owned TEMP fixtures.

Repeated commands are read-only Git queries. No model calls or global settings.
This measures adapter + shell + filter, not Codex's additional hook dispatcher.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import statistics
import subprocess
import tempfile
import time


def run(argv, cwd, *, env=None, data=None, timeout=30):
    started = time.perf_counter()
    result = subprocess.run(argv, cwd=cwd, env=env, input=data, capture_output=True, timeout=timeout)
    return result, time.perf_counter() - started


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--adapter', required=True, type=Path)
    parser.add_argument('--powershell', default=shutil.which('pwsh'))
    args = parser.parse_args()
    adapter = args.adapter.resolve(strict=True)
    root = Path(tempfile.mkdtemp(prefix='harness-rtk-measure-')).resolve()
    workspace = root / 'workspace'
    workspace.mkdir()
    env = {**os.environ, 'CODEX_HOME': str(root / 'codex'),
           'PATH': str(adapter.parent) + os.pathsep + os.environ['PATH']}
    report = {'passed': False, 'root': str(root), 'adapter': str(adapter),
              'adapterSha256': hashlib.sha256(adapter.read_bytes()).hexdigest(),
              'shell': args.powershell, 'cases': [], 'limits': 'bytes and elapsed time; no tokenizer/quota claim; excludes native Codex hook dispatch'}
    try:
        subprocess.run(['git', 'init', '-q', str(workspace)], check=True)
        for number in range(80):
            subprocess.run(['git', '-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid',
                            'commit', '--allow-empty', '-qm', f'receipt-{number:04d}'], cwd=workspace, check=True)
        for number in range(120):
            (workspace / f'owned-file-{number:04d}.txt').write_text(f'fixture {number}\n', encoding='utf-8')
        (workspace / 'search-fixture.txt').write_text(''.join(f'needle-{n:04d} retained source detail\n' for n in range(200)), encoding='utf-8')
        for command in ('git log -n 80', 'rg -n -H needle search-fixture.txt', 'git status --short'):
            case = {'command': command, 'pairs': []}
            report['cases'].append(case)
            for repetition in range(3):
                request = {'hook_event_name': 'PreToolUse', 'tool_name': 'Bash', 'cwd': str(workspace),
                           'tool_input': {'command': 'harness-rtk.exe exec ' + command}}
                hook, hook_seconds = run([str(adapter), 'hook'], workspace, env=env,
                                         data=json.dumps(request).encode('utf-8'))
                if hook.returncode or not hook.stdout.strip():
                    raise AssertionError(f'Eligible hook did not rewrite: {hook.stderr.decode(errors="replace")}')
                response = json.loads(hook.stdout)
                specific = response['hookSpecificOutput']
                assert specific['permissionDecision'] == 'allow'
                rewritten = specific['updatedInput']['command']
                raw, raw_seconds = run([args.powershell, '-NoLogo', '-NoProfile', '-Command', command], workspace, env=env)
                compact, compact_seconds = run([args.powershell, '-NoLogo', '-NoProfile', '-Command', rewritten], workspace, env=env)
                assert raw.returncode == compact.returncode == 0, compact.stderr.decode(errors='replace')
                assert compact.stderr == raw.stderr, 'stderr changed'
                text = compact.stdout.decode('utf-8')
                locator = re.search(r'\[rtk raw: (.+?)\]', text)
                (root / f'{len(report["cases"])}-{repetition}-raw.txt').write_bytes(raw.stdout)
                (root / f'{len(report["cases"])}-{repetition}-compact.txt').write_bytes(compact.stdout)
                if command == 'git status --short':
                    assert compact.stdout == raw.stdout, 'Already compact status should pass unchanged'
                else:
                    assert locator, 'No raw recovery locator'
                retained = Path(locator[1]).read_bytes() if locator else compact.stdout
                assert retained.replace(b'\r\n', b'\n') == raw.stdout.replace(b'\r\n', b'\n'), 'Raw recovery differs from original stdout'
                if command.startswith('git log'):
                    assert 'receipt-0079' in text, 'Latest subject was lost'
                    assert retained.count(b'commit ') == 80, 'Raw history was not complete'
                elif command.startswith('git status'):
                    assert retained.count(b'owned-file-') == 120, 'Raw status was not complete'
                else:
                    assert '200 matches' in text and retained.count(b'needle-') == 200, 'Search count or recovery was lost'
                pair = {'rawBytes': len(raw.stdout), 'compactBytes': len(compact.stdout),
                        'rawSeconds': raw_seconds, 'optimizedSeconds': hook_seconds + compact_seconds,
                        'hookSeconds': hook_seconds, 'rawLocator': locator[1] if locator else None}
                case['pairs'].append(pair)
                (root / f'{len(report["cases"])}-{repetition}-raw.txt').write_bytes(raw.stdout)
                (root / f'{len(report["cases"])}-{repetition}-compact.txt').write_bytes(compact.stdout)
            case['byteReduction'] = 1 - statistics.median(p['compactBytes'] / p['rawBytes'] for p in case['pairs'])
            case['rawMedianSeconds'] = statistics.median(p['rawSeconds'] for p in case['pairs'])
            case['optimizedMedianSeconds'] = statistics.median(p['optimizedSeconds'] for p in case['pairs'])
            increase = case['optimizedMedianSeconds'] - case['rawMedianSeconds']
            if command != 'git status --short':
                assert case['byteReduction'] >= .2, 'No useful net output reduction'
            assert not (increase > .1 and increase > .1 * case['rawMedianSeconds']), 'Material measured slowdown'
        report['passed'] = True
    finally:
        (root / 'report.json').write_text(json.dumps(report, indent=2), encoding='utf-8')
        print(json.dumps({'passed': report['passed'], 'evidence': str(root / 'report.json')}, ensure_ascii=False))


if __name__ == '__main__':
    main()
