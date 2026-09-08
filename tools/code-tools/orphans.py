"""Inspect or retire exact historical kit PSES trees whose original owner is gone."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import psutil


def kit_invocation(args, home):
    """Recognize the observed harness -File contract, preserving other forms.

    Script/runtime names inside -Command text are not ownership evidence. The
    adopted executable and script come from this account's lifecycle receipt;
    unknown, duplicate or missing options make retirement unavailable.
    """
    if not isinstance(args, list) or not 5 <= len(args) <= 24 or any(not isinstance(arg, str) for arg in args):
        return False
    if sum(map(len, args)) > 32768:
        return False
    try:
        receipt = home / 'harness/code-tools.json'
        with receipt.open('rb') as stream:
            raw = stream.read(2 * 1024 * 1024 + 1)
        if len(raw) > 2 * 1024 * 1024:
            return False
        registry = json.loads(raw.decode('utf-8-sig'))
        records = [record for record in registry['languages'] if record.get('id') == 'powershell']
        if len(records) != 1:
            return False
        record = records[0]
        command = record['command']
        if (record.get('status') != 'adopted' or len(command) != 5
                or [value.lower() for value in command[1:4]] != ['-nologo', '-noprofile', '-file']
                or [value.lower() for value in args[1:4]] != ['-nologo', '-noprofile', '-file']):
            return False
        executable, script = Path(command[0]), Path(command[4])
        if (not executable.is_absolute() or not script.is_absolute()
                or script.name.lower() != 'start-editorservices.ps1'
                or not Path(args[0]).is_absolute() or not Path(args[4]).is_absolute()
                or Path(args[0]).resolve() != executable.resolve()
                or Path(args[4]).resolve() != script.resolve()
                or not Path(record['paths']['executable']).is_absolute()
                or Path(record['paths']['executable']).resolve() != script.resolve()):
            return False
        options = {}
        tail = iter(args[5:])
        for option in tail:
            key = option.lower()
            if key in options:
                return False
            options[key] = True if key == '-stdio' else next(tail)
        if set(options) != {'-hostname', '-hostprofileid', '-hostversion', '-bundledmodulespath',
                            '-stdio', '-logpath', '-sessiondetailspath'}:
            return False
        if (options['-hostname'] != 'CodexHarness' or options['-hostprofileid'] != 'CodexHarness'
                or options['-hostversion'] != '1.0.0' or options['-stdio'] is not True
                or not Path(options['-bundledmodulespath']).is_absolute()
                or Path(options['-bundledmodulespath']).resolve() != script.resolve().parent):
            return False
        log, session = Path(options['-logpath']), Path(options['-sessiondetailspath'])
        if not log.is_absolute() or not session.is_absolute():
            return False
        runtime = (home / 'harness/runtime/lsp').resolve()
        relative = log.resolve().relative_to(runtime)
        return (len(relative.parts) == 3 and re.fullmatch('[0-9a-f]{64}', relative.parts[0]) is not None
                and relative.parts[1:] == ('powershell', 'pses.log')
                and session.resolve() == log.resolve().parent / 'session.json')
    except (OSError, ValueError, KeyError, TypeError, AttributeError, StopIteration):
        return False


def matching_shell(args, process_args):
    """Only the exact cmd /c wrapper, with no chaining or expansion syntax."""
    if len(args) < 3 or args[1].lower() != '/c':
        return False
    tail = ' '.join(args[2:])
    if any(character in tail for character in '&|<>%\r\n'):
        return False
    # psutil splits cmd's quoted command tail differently from the child argv.
    # Paths may contain spaces; remove only quote delimiters, not arbitrary text.
    return tail.replace('"', '').casefold() == ' '.join(process_args).replace('"', '').casefold()


def candidate(pid, home):
    process = psutil.Process(pid)
    if process.name().lower() != 'pwsh.exe':
        return None
    args = process.cmdline()
    if not kit_invocation(args, home) or Path(process.exe()).resolve() != Path(args[0]).resolve():
        return None
    shell = psutil.Process(process.ppid())
    if shell.name().lower() != 'cmd.exe' or shell.create_time() > process.create_time():
        return None
    if Path(shell.exe()).resolve() != (Path(os.environ['WINDIR']) / 'System32/cmd.exe').resolve():
        return None
    shell_args = shell.cmdline()
    if not matching_shell(shell_args, args):
        return None
    try:
        owner = psutil.Process(shell.ppid())
        if owner.create_time() <= shell.create_time():
            return None
    except psutil.NoSuchProcess:
        pass
    # Windows may give this shell its own console host as well as PSES.
    children = shell.children()
    consoles = [child for child in children if child.name().lower() == 'conhost.exe'
                and child.create_time() >= shell.create_time()
                and Path(child.exe()).resolve() == (Path(os.environ['WINDIR']) / 'System32/conhost.exe').resolve()]
    if process.children() or {child.pid for child in children} != {process.pid, *(child.pid for child in consoles)}:
        return None
    return {'pid': process.pid, 'created': process.create_time(), 'executable': process.exe(),
            'invocation_sha256': hashlib.sha256('\n'.join(args).encode()).hexdigest(),
            'shell_invocation_sha256': hashlib.sha256('\n'.join(shell_args).encode()).hexdigest(),
            'shell_pid': shell.pid, 'shell_created': shell.create_time(),
            'consoles': [(child.pid, child.create_time()) for child in consoles],
            'missing_owner': shell.ppid(), 'private_bytes': process.memory_info().private,
            'working_set_bytes': process.memory_info().rss}


def run(home, apply=False):
    found, retired, skipped = [], [], []
    for process in psutil.process_iter(['name']):
        if (process.info['name'] or '').lower() != 'pwsh.exe':
            continue
        try:
            value = candidate(process.pid, home)
            if not value:
                continue
            found.append(value)
            if apply:
                current = candidate(process.pid, home)
                keys = ('pid', 'created', 'executable', 'invocation_sha256', 'shell_invocation_sha256',
                        'shell_pid', 'shell_created', 'missing_owner', 'consoles')
                if not current or any(value[key] != current[key] for key in keys):
                    skipped.append({'pid': process.pid, 'reason': 'Ownership changed before retirement'})
                    continue
                target, shell = psutil.Process(int(value['pid'])), psutil.Process(int(value['shell_pid']))
                if target.create_time() != value['created'] or shell.create_time() != value['shell_created']:
                    continue
                target.terminate()
                target.wait(timeout=5)
                retired.append(value['pid'])
                # The shell usually exits naturally when PSES exits.
                if shell.is_running() and not shell.children():
                    shell.terminate()
                    shell.wait(timeout=5)
        except (psutil.Error, OSError) as error:
            skipped.append({'pid': process.pid, 'reason': type(error).__name__})
            continue
    return {'candidates': found, 'retired': retired, 'skipped': skipped}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--apply', action='store_true')
    parser.add_argument('--codex-home', type=Path, default=Path(os.environ.get('CODEX_HOME', Path.home() / '.codex')))
    args = parser.parse_args()
    print(json.dumps(run(args.codex_home, args.apply), indent=2))
