"""Transactional MCP source references and native startup readiness policy."""
from __future__ import annotations

import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import tomllib

NAMES = ('serena', 'codebase-memory', 'graphify', 'nuphus', 'harness-lsp')
MARKERS = {'# BEGIN codex-harness MCP registrations', '# END codex-harness MCP registrations'}
READINESS_KEY = 'mcp_optional_startup_grace_ms'
READINESS_STATEMENT = READINESS_KEY + ' = 0\n'


def bytes_at(path):
    return path.read_bytes() if path.is_file() else b''


def sha(value):
    return hashlib.sha256(value).hexdigest()


def atomic(path, data):
    path.parent.mkdir(parents=True, exist_ok=True)
    descriptor, temporary = tempfile.mkstemp(prefix='.' + path.name, dir=path.parent)
    try:
        with os.fdopen(descriptor, 'wb') as stream:
            stream.write(data)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, path)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)


def json_bytes(value):
    return (json.dumps(value, ensure_ascii=False, indent=2) + '\n').encode('utf-8')


def read_json(path):
    return json.loads(bytes_at(path)) if path.is_file() else None


def assert_plain(path):
    for item in (path, *path.parents):
        if item.is_symlink() or (hasattr(item, 'is_junction') and item.is_junction()):
            raise ValueError(f'Preserving reparse path; connection needs an ordinary host file: {item}')


def registration(source, powershell, name, home):
    return {'command': str(powershell), 'args': ['-NoLogo', '-NoProfile', '-File', str(source / 'tools/mcp.ps1'), '-Server', name],
            'env': {'CODEX_HOME': str(home)}}


def statements(text):
    """Yield complete TOML statement spans, preserving comments and whitespace.

    The document is validated by tomllib first. Only lexical boundaries are
    needed here; quoted/multiline values and arrays can contain header-like text.
    """
    start = index = int(text.startswith('\ufeff'))
    quote = None
    multiline = False
    square = curly = 0
    comment = False
    while index < len(text):
        char = text[index]
        if comment:
            if char not in '\r\n':
                index += 1
                continue
            comment = False
        if quote:
            if quote == '"' and char == '\\':
                index += 2
                continue
            if multiline and text.startswith(quote * 3, index):
                while index < len(text) and text[index] == quote:
                    index += 1
                quote = None
                multiline = False
                continue
            if not multiline and char == quote:
                quote = None
            index += 1
            continue
        if char == '#':
            comment = True
        elif char in ('"', "'"):
            quote = char
            multiline = text.startswith(char * 3, index)
            if multiline:
                index += 3
                continue
        elif char == '[':
            square += 1
        elif char == ']':
            square -= 1
        elif char == '{':
            curly += 1
        elif char == '}':
            curly -= 1
        if char == '\n' and not square and not curly:
            yield start, index + 1
            start = index + 1
        index += 1
    if start < len(text):
        yield start, len(text)


def assignment_key(statement):
    quote = None
    index = 0
    while index < len(statement):
        char = statement[index]
        if quote:
            if quote == '"' and char == '\\':
                index += 2
                continue
            if char == quote:
                quote = None
        elif char in ('"', "'"):
            quote = char
        elif char == '=':
            return statement[:index]
        index += 1
    raise ValueError('Cannot isolate the TOML assignment key; preserving configuration.')


def single_key_path(parsed):
    path = []
    while isinstance(parsed, dict) and len(parsed) == 1:
        key, parsed = next(iter(parsed.items()))
        path.append(key)
    return tuple(path)


def readiness_span(before):
    """Find the root scalar statement, excluding similarly named nested keys."""
    text = before.decode('utf-8')
    table = ()
    for start, end in statements(text):
        statement = text[start:end]
        stripped = statement.strip()
        if not stripped or stripped.startswith('#'):
            continue
        if stripped.startswith('['):
            table = single_key_path(tomllib.loads(stripped))
        elif not table and single_key_path(tomllib.loads(assignment_key(statement) + '= 0')) == (READINESS_KEY,):
            return start, end
    raise ValueError('Cannot isolate the native MCP readiness setting; preserving configuration.')


def readiness_policy(parsed, state, mode):
    prior = state.get('connection_policy')
    actual = parsed.get(READINESS_KEY)
    if prior is not None:
        if (prior.get('key') != READINESS_KEY or type(prior.get('value')) is not int or prior['value'] != 0
                or type(prior.get('previous_present')) is not bool):
            raise ValueError('Unknown native MCP readiness ownership; preserving configuration.')
        if type(actual) is not int or actual != 0:
            raise ValueError(f'Native MCP readiness ownership conflict: {READINESS_KEY} changed; preserve the current setting and reconcile the recorded connection before retrying.')
    if mode == 'Disconnect':
        return None, ([] if prior is None else [{'name': READINESS_KEY, 'action': 'restore', 'target': 0 if prior['previous_present'] else None}])
    if READINESS_KEY in parsed and (type(actual) is not int or actual != 0):
        raise ValueError(f'Native MCP readiness conflict: explicit {READINESS_KEY} differs from 0. Preserve it; set it to 0 deliberately or keep this connection inactive.')
    policy = prior or {'key': READINESS_KEY, 'value': 0, 'previous_present': READINESS_KEY in parsed}
    return policy, ([] if prior is not None else [{'name': READINESS_KEY, 'action': 'register', 'target': 0}])


def without_readiness(before, state):
    prior = state.get('connection_policy')
    if prior is None or prior['previous_present']:
        return before
    parsed = tomllib.loads(before.decode('utf-8-sig'))
    if type(parsed.get(READINESS_KEY)) is not int or parsed[READINESS_KEY] != 0:
        raise ValueError('Native MCP readiness ownership conflict; preserving configuration.')
    start, end = readiness_span(before)
    text = before.decode('utf-8')
    after = (text[:start] + text[end:]).encode('utf-8')
    expected = dict(parsed)
    expected.pop(READINESS_KEY)
    if tomllib.loads(after.decode('utf-8-sig')) != expected:
        raise ValueError('Cannot remove native MCP readiness without changing unrelated settings.')
    return after


def without_owned(before, state):
    """Remove only semantically owned statements, never a stale marker range.

    Native TUI writes can interleave foreign sections inside the old BEGIN/END
    markers. TOML statements identify our entries even after that normalization.
    """
    owned = state['registrations']
    parsed = tomllib.loads(before.decode('utf-8-sig')) if before else {}
    existing = parsed.get('mcp_servers', {})
    for name, expected in owned.items():
        if name not in NAMES or existing.get(name) != expected:
            raise ValueError(f'MCP name/ownership conflict: {name}; preserving current registration.')
    if not owned:
        return before
    block = state.get('block', '').encode('utf-8')
    if block and before.count(block) == 1:
        candidate = before.replace(block, b'', 1)
        try:
            remaining = tomllib.loads(candidate.decode('utf-8-sig')) if candidate else {}
        except tomllib.TOMLDecodeError:
            remaining = None
        if remaining is not None and {key: value for key, value in parsed.items() if key != 'mcp_servers'} == {key: value for key, value in remaining.items() if key != 'mcp_servers'} and remaining.get('mcp_servers', {}) == {name: value for name, value in existing.items() if name not in owned}:
            return candidate
    text = before.decode('utf-8')
    table = ()
    removed = []
    for start, end in statements(text):
        statement = text[start:end]
        stripped = statement.strip()
        if stripped in MARKERS:
            removed.append((start, end))
        elif not stripped or stripped.startswith('#'):
            continue
        elif stripped.startswith('['):
            table = single_key_path(tomllib.loads(stripped))
            if len(table) >= 2 and table[0] == 'mcp_servers' and table[1] in owned:
                removed.append((start, end))
        else:
            path = table + single_key_path(tomllib.loads(assignment_key(statement) + '= 0'))
            if len(path) >= 2 and path[0] == 'mcp_servers' and path[1] in owned:
                removed.append((start, end))
    for start, end in reversed(removed):
        text = text[:start] + text[end:]
    after = text.encode('utf-8')
    remaining = tomllib.loads(text.lstrip('\ufeff')) if text else {}
    old_servers = parsed.pop('mcp_servers', {})
    new_servers = remaining.pop('mcp_servers', {})
    if parsed != remaining or new_servers != {name: value for name, value in old_servers.items() if name not in owned}:
        raise ValueError('Cannot isolate owned MCP statements without changing unrelated settings; preserving configuration.')
    return after


def inspect(home, source, powershell, mode):
    config = home / 'config.toml'
    state_path = home / 'harness/code-tools-registration.json'
    assert_plain(config)
    assert_plain(state_path)
    before = bytes_at(config)
    parsed = tomllib.loads(before.decode('utf-8-sig')) if before else {}
    existing = parsed.get('mcp_servers', {})
    state = read_json(state_path) or {'schema_version': 1, 'registrations': {}}
    if state.get('schema_version') != 1:
        raise ValueError('Unknown registration state schema.')
    policy, operations = readiness_policy(parsed, state, mode)
    desired = {}
    for name in NAMES:
        old = state['registrations'].get(name)
        actual = existing.get(name)
        target = registration(source, powershell, name, home) if mode != 'Disconnect' else None
        # A same-named unmanaged registration is a conflict, even if it looks similar.
        if actual is not None and (old is None or actual != old):
            raise ValueError(f'MCP name/ownership conflict: {name}; preserving current registration.')
        if target is not None:
            desired[name] = target
        if actual != target:
            operations.append({'name': name, 'action': 'remove' if target is None else 'register', 'target': target})
    return before, state, desired, policy, operations


def recover(home, preview=False):
    pending_path = home / 'harness/code-tools-registration-pending.json'
    assert_plain(pending_path)
    pending = read_json(pending_path)
    if not pending:
        return {'status': 'no-pending-registration'}
    config, state = home / 'config.toml', home / 'harness/code-tools-registration.json'
    assert_plain(config)
    assert_plain(state)
    before = base64.b64decode(pending['before'], validate=True)
    current = bytes_at(config)
    if sha(current) not in (sha(before), pending['after_hash']):
        raise ValueError('Config changed after the interrupted registration. Preserving concurrent changes; inspect the local pending record.')
    previous_bytes = (base64.b64decode(pending['previous_state_bytes'], validate=True)
                      if pending.get('previous_state_bytes') is not None else b'')
    if 'after_state_hash' in pending and sha(bytes_at(state)) not in (sha(previous_bytes), pending['after_state_hash']):
        raise ValueError('Registration state changed after interruption; preserving concurrent changes.')
    if preview:
        return {'status': 'preview-recovery', 'config': str(config)}
    if pending.get('config_existed', True) is False:
        config.unlink(missing_ok=True)
    elif current != before:
        atomic(config, before)
    if pending.get('previous_state_bytes') is not None:
        atomic(state, base64.b64decode(pending['previous_state_bytes'], validate=True))
    elif pending['previous_state'] is None:
        state.unlink(missing_ok=True)
    else:
        atomic(state, json_bytes(pending['previous_state']))
    pending_path.unlink()
    return {'status': 'registration-recovered'}


def run(home, source, native, powershell, mode, preview=False, defer_commit=False):
    pending_path = home / 'harness/code-tools-registration-pending.json'
    if mode == 'Recover':
        return recover(home, preview)
    if pending_path.exists():
        raise ValueError('Interrupted MCP registration: run install.ps1 -Mode Recover.')
    config = home / 'config.toml'
    state_path = home / 'harness/code-tools-registration.json'
    before, state, desired, policy, operations = inspect(home, source, powershell, mode)
    if mode == 'Check':
        return {'status': 'connected' if not operations else 'degraded', 'callable': None, 'operations': operations,
                'note': 'This checks registrations; real MCP calls are separate acceptance evidence.'}
    untouched = without_readiness(without_owned(before, state), state) if operations else before
    if preview:
        return {'status': 'preview-registration', 'operations': operations, 'mutated': False}
    if not operations:
        return {'status': 'unchanged-registration'}
    state_path.parent.mkdir(parents=True, exist_ok=True)
    # Codex mcp add rewrites the whole mcp_servers table, including foreign entries.
    # Let its structural editor render only our path registrations in an empty stage.
    # The exact owned block is replaced; all unrelated live bytes remain untouched.
    with tempfile.TemporaryDirectory(prefix='registration-', dir=state_path.parent) as temporary:
        stage = Path(temporary)
        (stage / 'config.toml').write_bytes(b'')
        environment = {**os.environ, 'CODEX_HOME': str(stage)}
        for name, target in desired.items():
            arguments = ['mcp', 'add', name, '--env', 'CODEX_HOME=' + target['env']['CODEX_HOME'], '--', target['command'], *target['args']]
            process = subprocess.run([str(native), *arguments], env=environment, cwd=stage, capture_output=True, timeout=30)
            if process.returncode:
                # Never emit editor stdout/stderr: it may contain local settings.
                raise RuntimeError(f'Native MCP editor failed for {name}, exit {process.returncode}. Live configuration unchanged.')
        rendered = (stage / 'config.toml').read_bytes()
        block = (b'\n# BEGIN codex-harness MCP registrations\n' + rendered +
                 b'# END codex-harness MCP registrations\n') if desired else b''
        after = untouched + block
        if policy is not None and not policy['previous_present']:
            offset = 3 if after.startswith(b'\xef\xbb\xbf') else 0
            after = after[:offset] + READINESS_STATEMENT.encode() + after[offset:]
        old_structure = tomllib.loads(before.decode('utf-8-sig')) if before else {}
        new_structure = tomllib.loads(after.decode('utf-8-sig'))
        old_servers = old_structure.pop('mcp_servers', {})
        new_servers = new_structure.pop('mcp_servers', {})
        if policy is not None or state.get('connection_policy') is not None:
            old_structure.pop(READINESS_KEY, None)
            expected_readiness = 0 if policy is not None or state['connection_policy']['previous_present'] else None
            if new_structure.pop(READINESS_KEY, None) != expected_readiness:
                raise RuntimeError('Unexpected native MCP readiness result; live configuration unchanged.')
        old_unmanaged = {name: value for name, value in old_servers.items() if name not in NAMES}
        new_unmanaged = {name: value for name, value in new_servers.items() if name not in NAMES}
        if old_structure != new_structure or old_unmanaged != new_unmanaged:
            raise RuntimeError('Native editor changed unrelated settings; live configuration unchanged.')
        for name in NAMES:
            if new_servers.get(name) != desired.get(name):
                raise RuntimeError(f'Native editor produced an unexpected registration for {name}.')
    if bytes_at(config) != before:
        raise RuntimeError('Config changed during preparation; preserving concurrent changes.')
    previous_state = read_json(state_path)
    if policy is not None:
        start, end = readiness_span(after)
        text = after.decode('utf-8')
        policy = {**policy, 'prefix_hash': sha(text[:end].encode('utf-8')),
                  'prefix_length': len(text[:end].encode('utf-8')), 'statement': text[start:end]}
        policy.pop('prefix', None)  # Upgrade the short-lived literal-prefix metadata.
    after_state = (b'' if mode == 'Disconnect' else json_bytes({'schema_version': 1, 'source_root': str(source),
                    'registrations': desired, 'block': block.decode('utf-8'), 'connection_policy': policy}))
    pending = {'schema_version': 1, 'before': base64.b64encode(before).decode('ascii'), 'after_hash': sha(after),
               'previous_state': previous_state,
               'previous_state_bytes': base64.b64encode(state_path.read_bytes()).decode('ascii') if state_path.is_file() else None,
               'after_state_hash': sha(after_state), 'config_existed': config.is_file()}
    atomic(pending_path, json_bytes(pending))
    try:
        if bytes_at(config) != before:
            raise RuntimeError('Config changed before activation; preserving concurrent changes.')
        atomic(config, after)
        if mode == 'Disconnect':
            state_path.unlink(missing_ok=True)
        else:
            atomic(state_path, after_state)
        if not defer_commit:
            pending_path.unlink()
    except Exception:
        recover(home)
        raise
    return {'status': 'disconnected' if mode == 'Disconnect' else 'connected', 'servers': list(desired), 'callable': None}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--codex-home', required=True, type=Path)
    parser.add_argument('--source-root', default=str(Path(__file__).resolve().parents[2]), type=Path)
    parser.add_argument('--native-codex', required=True, type=Path)
    parser.add_argument('--powershell', required=True, type=Path)
    parser.add_argument('--mode', choices=['Install', 'Update', 'Check', 'Disconnect', 'Recover'], default='Install')
    parser.add_argument('--preview', action='store_true')
    parser.add_argument('--defer-commit', action='store_true')
    args = parser.parse_args()
    print(json.dumps(run(args.codex_home.absolute(), args.source_root.resolve(), args.native_codex, args.powershell, args.mode, args.preview, args.defer_commit)))


if __name__ == '__main__':
    main()
