"""Resource policy, cross-process admission and reversible native configuration."""
from __future__ import annotations

import argparse
from contextlib import closing, contextmanager
import json
import os
from pathlib import Path
import subprocess
import sqlite3
import time

SOURCE = Path(__file__).resolve().parents[2]


def policy():
    value = json.loads((SOURCE / 'global/tool-resources.json').read_text(encoding='utf-8'))
    if value.get('schema_version') != 1:
        raise ValueError('Unsupported resource policy')
    return value


def account_directory():
    # Independent of CODEX_HOME: multiple installations still share one slot.
    path = Path(os.environ.get('HARNESS_TOOL_RESOURCES_DIR',
        Path(os.environ.get('LOCALAPPDATA', Path.home() / '.cache')) / 'codex-tool-resources'))
    path.mkdir(parents=True, exist_ok=True)
    return path


@contextmanager
def admission(name, timeout=2):
    """The kernel releases ownership on crash; never unlink a lock with waiters."""
    stream = (account_directory() / (name + '.lock')).open('a+b')
    held = False
    deadline = time.monotonic() + timeout
    try:
        while not held:
            try:
                stream.seek(0)
                if os.name == 'nt':
                    import msvcrt
                    msvcrt.locking(stream.fileno(), msvcrt.LK_NBLCK, 1)
                else:
                    import fcntl
                    fcntl.flock(stream, fcntl.LOCK_EX | fcntl.LOCK_NB)
                held = True
            except OSError:
                if time.monotonic() >= deadline:
                    raise TimeoutError(f'{name} is busy; no additional worker was started')
                time.sleep(0.05)
        yield
    finally:
        if held:
            stream.seek(0)
            if os.name == 'nt':
                import msvcrt
                msvcrt.locking(stream.fileno(), msvcrt.LK_UNLCK, 1)
            else:
                import fcntl
                fcntl.flock(stream, fcntl.LOCK_UN)
        stream.close()


def native(registry=None):
    registry = Path(registry or os.environ['HARNESS_CODE_TOOLS_REGISTRY'])
    inventory = json.loads(registry.read_text(encoding='utf-8-sig'))
    return next(item for item in inventory['mcp'] if item['id'] == 'codebase-memory')['paths']['native_executable']


def config(executable, key, value=None):
    args = [executable, 'config', 'get', key] if value is None else [executable, 'config', 'set', key, value]
    result = subprocess.run(args, stdin=subprocess.DEVNULL, capture_output=True, text=True, encoding='utf-8', timeout=60,
                            creationflags=getattr(subprocess, 'CREATE_NO_WINDOW', 0))
    if result.returncode:
        raise RuntimeError(f'Native configuration {key} failed: {result.stderr[-1000:]}')
    return result.stdout.strip()


def atomic_json(path, value):
    temporary = path.with_name(path.name + f'.{os.getpid()}.tmp')
    temporary.write_text(json.dumps(value, indent=2, ensure_ascii=False), encoding='utf-8')
    os.replace(temporary, path)


def stop_daemon(executable):
    """Native 0.10.x coordinated retirement; never terminate arbitrary PIDs."""
    result = subprocess.run([executable, 'daemon', 'stop'], stdin=subprocess.DEVNULL,
        capture_output=True, text=True, encoding='utf-8', timeout=60,
        creationflags=getattr(subprocess, 'CREATE_NO_WINDOW', 0))
    if result.returncode:
        raise RuntimeError(f'Native CBM daemon retirement failed: {result.stderr[-1000:]}')


def read_configuration():
    """Read the installed 0.10.x settings schema without launching a daemon client."""
    root = Path(os.environ.get('CBM_CACHE_DIR', Path.home() / '.cache/codebase-memory-mcp')).resolve()
    values = {'auto_index': 'false', 'auto_watch': 'true', 'ui_enabled': 'false'}
    if (root / '_config.db').exists():
        with closing(sqlite3.connect((root / '_config.db').as_uri() + '?mode=ro', uri=True, timeout=2)) as connection:
            rows = dict(connection.execute('SELECT key, value FROM config'))
        values.update({key: rows[key] for key in values if key in rows})
    if (root / 'config.json').exists():
        ui = json.loads((root / 'config.json').read_text(encoding='utf-8-sig'))
        values['ui_enabled'] = 'true' if ui.get('ui_enabled', False) else 'false'
    return values


def configuration(mode, registry=None, pending=None, defer_commit=False, transaction_id=None, owner_key=None):
    """Compare-and-restore ownership, with a write-ahead inverse for each activation.

    The permanent receipt describes the first migration. The separate pending
    journal describes only this transaction, including a failed Update/Disconnect.
    A completed receipt never treats a user revert as an interrupted migration.
    """
    expected = policy()['codebase_memory']['configuration']
    with admission('cbm-configuration', 10):
        receipt = account_directory() / 'cbm-configuration.json'
        pending = Path(pending) if pending else account_directory() / 'cbm-configuration-pending.json'
        installation = os.path.normcase(str(Path(owner_key or pending.parent).resolve()))
        owner = account_directory() / 'cbm-configuration-owner.json'
        if owner.exists():
            active = json.loads(owner.read_text(encoding='utf-8'))
            if active.get('pending') != str(pending.resolve()):
                raise RuntimeError('Another installation has a pending resource transaction; recover its activation first')
        saved = json.loads(receipt.read_text(encoding='utf-8')) if receipt.exists() else None
        current = read_configuration()
        cache = str(Path(os.environ.get('CBM_CACHE_DIR', Path.home() / '.cache/codebase-memory-mcp')).resolve())
        if saved and saved['cache'] != cache:
            raise RuntimeError('CBM cache identity differs from the resource migration receipt')
        if mode in ('recover', 'commit'):
            if not pending.exists():
                return {'status': 'not-pending'}
            record = json.loads(pending.read_text(encoding='utf-8'))
            if (record.get('schema_version') != 1 or record.get('cache') != cache
                    or record.get('receipt') != str(receipt.resolve())
                    or (transaction_id and record.get('transaction_id') != transaction_id)):
                raise RuntimeError('Resource transaction identity changed; preserving pending state')
            if mode == 'commit':
                if record['state'] != 'complete':
                    raise RuntimeError('Resource transaction is incomplete; run Recover')
                owner.unlink(missing_ok=True)
                pending.unlink()
                return {'status': 'committed'}
            if saved not in (record['receipt_before'], record['receipt_after']):
                raise RuntimeError('Resource receipt changed after transaction; preserving pending state')
            preserved = []
            for key in reversed(record['intents']):
                # A command may have committed before its client died. Its
                # recorded intent is enough to undo precisely that write.
                observed = read_configuration()[key]
                if observed == record['after'][key]:
                    config(record['native'], key, record['before'][key])
                    if read_configuration()[key] != record['before'][key]:
                        raise RuntimeError(f'Native configuration recovery did not persist {key}')
                elif observed != record['before'][key]:
                    preserved.append(key)
            if record['intents']:
                stop_daemon(record['native'])
            if record['receipt_before'] is None:
                receipt.unlink(missing_ok=True)
            else:
                atomic_json(receipt, record['receipt_before'])
            owner.unlink(missing_ok=True)
            pending.unlink()
            return {'status': 'recovered', 'preserved_user_settings': preserved}
        if mode == 'check':
            if pending.exists():
                raise RuntimeError('Resource configuration transaction needs Recover')
            if current != expected:
                raise RuntimeError('CBM resource policy is not active; run resources.py apply explicitly')
            return {'status': 'active', 'configuration': current}
        if pending.exists():
            raise RuntimeError('Interrupted resource configuration; run Recover before another mutation')
        if mode not in ('apply', 'restore'):
            raise ValueError(f'Unsupported configuration operation: {mode}')
        executable = native(registry) if mode == 'apply' else saved['native'] if saved else None
        preserved = []
        owners = set(saved.get('owners', [])) if saved else set()
        if mode == 'apply':
            if saved and current != saved['applied']:
                raise RuntimeError('Native CBM settings changed after migration; preserving user changes')
            after = dict(expected)
            owners.add(installation)
            receipt_after = {'schema_version': 2, 'state': 'complete', 'cache': cache,
                'native': executable, 'before': saved['before'] if saved else current, 'applied': after,
                'owners': sorted(owners)}
        else:
            after = dict(current)
            if owners and installation not in owners:
                return {'status': 'not-owned', 'owners_remaining': len(owners)}
            owners.discard(installation)
            if saved and not owners:
                for key, value in saved['before'].items():
                    if current[key] == saved['applied'][key]:
                        after[key] = value
                    else:
                        preserved.append(key)
            receipt_after = {**saved, 'owners': sorted(owners)} if owners else None
        record = {'schema_version': 1, 'state': 'prepared', 'transaction_id': transaction_id,
            'cache': cache, 'native': executable, 'receipt': str(receipt.resolve()),
            'receipt_before': saved, 'receipt_after': receipt_after,
            'before': current, 'after': after, 'intents': []}
        pending.parent.mkdir(parents=True, exist_ok=True)
        atomic_json(pending, record)
        atomic_json(owner, {'pending': str(pending.resolve())})
        for key, value in after.items():
            # Persist UI opt-out even when native's current effective default
            # is false; UI asset discovery can otherwise enable it later.
            force_ui = mode == 'apply' and key == 'ui_enabled' and (not saved or saved.get('schema_version') != 2)
            if current[key] != value or force_ui:
                if read_configuration()[key] != current[key]:
                    raise RuntimeError(f'Native configuration {key} changed concurrently; preserving it')
                record['intents'].append(key)
                atomic_json(pending, record)
                config(executable, key, value)
                if read_configuration()[key] != value:
                    raise RuntimeError(f'Native configuration did not persist {key}')
        if record['intents']:
            stop_daemon(executable)
        if receipt_after is None:
            receipt.unlink(missing_ok=True)
        else:
            atomic_json(receipt, receipt_after)
        record['state'] = 'complete'
        atomic_json(pending, record)
        if not defer_commit:
            owner.unlink(missing_ok=True)
            pending.unlink()
        return {'status': 'applied' if mode == 'apply' else 'restored',
            'restart_required': False, 'daemon_retired': bool(record['intents']), 'receipt': str(receipt),
            'preserved_user_settings': preserved, 'owners_remaining': len(owners)}


def add_ignore(root, patterns):
    root = Path(root).resolve(strict=True)
    destination = root / '.cbmignore'
    before = destination.read_bytes() if destination.exists() else b''
    text = before.decode('utf-8-sig')
    added = [item for item in patterns if item not in text.splitlines()]
    if added:
        if text and not text.endswith('\n'):
            text += '\n'
        text += '# Generated inputs excluded from code intelligence; source files remain on disk.\n'
        text += '\n'.join(added) + '\n'
        destination.write_text(text, encoding='utf-8', newline='')
    return {'path': str(destination), 'added': added}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('mode', choices=['apply', 'check', 'restore', 'recover', 'commit', 'ignore'])
    parser.add_argument('--registry')
    parser.add_argument('--root')
    parser.add_argument('--pattern', action='append', default=[])
    parser.add_argument('--pending')
    parser.add_argument('--state-dir')
    parser.add_argument('--cache-dir')
    parser.add_argument('--transaction-id')
    parser.add_argument('--owner', help='Canonical CODEX_HOME owning this shared configuration lease')
    parser.add_argument('--defer-commit', action='store_true')
    args = parser.parse_args()
    if args.state_dir:
        os.environ['HARNESS_TOOL_RESOURCES_DIR'] = args.state_dir
    if args.cache_dir:
        os.environ['CBM_CACHE_DIR'] = args.cache_dir
    print(json.dumps(add_ignore(args.root, args.pattern) if args.mode == 'ignore'
                     else configuration(args.mode, args.registry, args.pending, args.defer_commit, args.transaction_id, args.owner)))
