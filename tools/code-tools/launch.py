"""Repository-owned MCP entry points; consume resolved shared packages without downloads."""
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import sys
import subprocess
import runpy
from typing import TypedDict, cast

SOURCE_ROOT = Path(__file__).resolve().parents[2]


class Dependency(TypedDict):
    id: str
    status: str
    paths: dict[str, str]


class Inventory(TypedDict):
    mcp: list[Dependency]


class ServerDefinition(TypedDict):
    id: str
    arguments: list[str]


class Catalogue(TypedDict):
    mcp: list[ServerDefinition]


class Arguments(argparse.Namespace):
    server: str = ''
    registry: str = ''


def resolve_launch(server: str, inventory: Inventory, catalogue: Catalogue) -> tuple[list[str], dict[str, str]]:
    if server == 'harness-lsp':
        raise RuntimeError('harness-lsp is retired; use suitable Serena tools and project-native checks')
    records = {record['id']: record for record in inventory['mcp']}
    record = records['serena' if server == 'harness-lsp' else server]
    if record.get('status') in ('missing', 'broken', 'ambiguous') or (record.get('status') == 'modified' and server != 'nuphus'):
        raise RuntimeError(f"{server}: dependency is {record['status']}; run install.ps1 -Mode Check")
    paths = record['paths']
    environment = {**os.environ, 'PYTHONUTF8': '1', 'PYTHONIOENCODING': 'utf-8', 'PYTHONDONTWRITEBYTECODE': '1'}
    if server == 'harness-lsp':
        command = [paths['python'], '-u', str(SOURCE_ROOT / 'tools/lsp/server.py')]
    elif server == 'serena':
        # The guarded entry point refuses runtime package provisioning.
        command = [paths['python'], '-u', str(SOURCE_ROOT / 'tools/code-tools/serena_proxy.py')]
        definition = next(item for item in catalogue['mcp'] if item['id'] == server)
        command.extend(definition['arguments'])
    elif server == 'codebase-memory':
        # npm's bin.js downloads on a cache miss. Never execute that wrapper here.
        command = [records['serena']['paths']['python'], '-u', str(SOURCE_ROOT / 'tools/code-tools/cbm_proxy.py')]
    elif server == 'nuphus':
        # The proxy audits and uses the preserved official binary; the local patched
        # wrapper/binary stay untouched. Schema compatibility lives in our source.
        command = [records['serena']['paths']['python'], '-u', str(SOURCE_ROOT / 'tools/code-tools/nuphus_proxy.py')]
        environment['NUPHUS_MCP_CONFIRM_WRITE'] = '0'
    elif server == 'graphify':
        command = [records['serena']['paths']['python'], '-u', str(SOURCE_ROOT / 'tools/code-tools/graphify_proxy.py')]
    else:
        raise ValueError('Unknown repository MCP')
    if not Path(command[0]).is_file():
        raise FileNotFoundError(f"{server}: resolved executable is missing")
    # Missing source is a broken installation, never a reason to download or search PATH.
    if server in ('serena', 'harness-lsp', 'graphify', 'nuphus', 'codebase-memory') and not Path(command[2]).is_file():
        raise FileNotFoundError(f"{server}: repository entry point is missing")
    return command, environment


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    _ = parser.add_argument('server', choices=['serena', 'codebase-memory', 'graphify', 'nuphus', 'harness-lsp'])
    _ = parser.add_argument('--registry', required=True)
    args = Arguments()
    _ = parser.parse_args(namespace=args)
    # The installer discovery record and source catalogue own these JSON schemas.
    inventory = cast(Inventory, json.loads(Path(args.registry).read_text(encoding='utf-8-sig')))
    catalogue = cast(Catalogue, json.loads((SOURCE_ROOT / 'global/code-tools.json').read_text(encoding='utf-8-sig')))
    command, environment = resolve_launch(args.server, inventory, catalogue)
    environment['HARNESS_CODE_TOOLS_REGISTRY'] = str(Path(args.registry).resolve())
    # Native registration uses this same adopted interpreter. Retain its open
    # Windows stdio handles and module environment without another Python layer.
    if Path(command[0]).resolve() == Path(sys.executable).resolve():
        os.environ.update(environment)
        sys.argv = command[2:]
        sys.path.insert(0, str(Path(command[2]).parent))
        _ = runpy.run_path(command[2], run_name='__main__')
        return
    # Compatibility consumers may invoke this through a different interpreter.
    raise SystemExit(subprocess.call(command, env=environment))


if __name__ == '__main__':
    try:
        main()
    except Exception as error:
        print(f'Harness MCP startup failed: {error}', file=sys.stderr)
        raise SystemExit(1)
