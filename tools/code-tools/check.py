"""Check actual MCP handshakes and tool availability without claiming language acceptance."""
import argparse
import json
import os
from pathlib import Path
import sys
from typing import TypeAlias

import anyio
from mcp import ClientSession, StdioServerParameters
from mcp.client.stdio import stdio_client


class Arguments(argparse.Namespace):
    registry: Path = Path()
    codex_home: Path = Path()


CheckResult: TypeAlias = dict[str, str | int | None]


def failure_reason(error: BaseException) -> str:
    if isinstance(error, BaseExceptionGroup):
        return '; '.join(failure_reason(item) for item in error.exceptions)
    return f'{type(error).__name__}: {error}'


async def check_one(name: str, registry: Path, environment: dict[str, str]) -> CheckResult:
    try:
        # Some adopted MCPs keep a shared daemon alive after the stdio client
        # exits. Its working directory must remain valid; Check must neither
        # kill that consumer nor mask its result with Windows cleanup errors.
        workspace = Path(environment['CODEX_HOME']) / 'harness' / 'verification' / 'mcp-check' / name
        workspace.mkdir(parents=True, exist_ok=True)
        state = json.loads((Path(environment['CODEX_HOME']) / 'harness/code-tools-registration.json').read_text(encoding='utf-8-sig'))
        target = state['registrations'][name]
        parameters = StdioServerParameters(command=target['command'], args=target['args'],
            env={**environment, **target.get('env', {}), 'HARNESS_CODE_TOOLS_REGISTRY': str(registry), 'PYTHONDONTWRITEBYTECODE': '1'}, cwd=str(workspace))
        with anyio.fail_after(75):
            async with stdio_client(parameters) as streams:
                async with ClientSession(*streams) as session:
                    initialized = await session.initialize()
                    tools = (await session.list_tools()).tools
                    if not tools:
                        raise ValueError('Server exposes no tools')
                    return {'id': name, 'status': 'protocol-ready', 'server': initialized.serverInfo.name,
                            'version': initialized.serverInfo.version, 'tool_count': len(tools),
                            'language_acceptance': 'not-established-by-handshake'}
    except Exception as error:
        return {'id': name, 'status': 'failed', 'reason': failure_reason(error)}


async def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    _ = parser.add_argument('--registry', type=Path, required=True)
    _ = parser.add_argument('--codex-home', type=Path, required=True)
    args = Arguments()
    _ = parser.parse_args(namespace=args)
    # Explicit CODEX_HOME remains necessary even when the native MCP host filters it.
    environment = {**os.environ, 'CODEX_HOME': str(args.codex_home)}
    results: list[CheckResult] = []

    async def collect(name: str) -> None:
        results.append(await check_one(name, args.registry, environment))

    # Verify only the accepted global selection, sequentially.
    state = json.loads((args.codex_home / 'harness/code-tools-registration.json').read_text(encoding='utf-8-sig'))
    for name in state['registrations']:
        await collect(name)
    print(json.dumps({'status': 'protocol-ready' if all(r['status'] == 'protocol-ready' for r in results) else 'degraded', 'servers': results}))


if __name__ == '__main__':
    anyio.run(main)
