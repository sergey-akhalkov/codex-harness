"""Install missing native MCP packages into the shared user npm package tree.

Only lifecycle calls this module. It downloads official integrity-checked package
artifacts during explicit Install, activates one previously absent package
directory, and exposes its native entry point through the global Codex registry.
No foreign package, npm configuration, public command alias or lockfile is edited.
"""
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import sys
import uuid

import anyio
from mcp import ClientSession, StdioServerParameters
from mcp.client.stdio import stdio_client

import dependencies as lifecycle
from nuphus_proxy import AUDITED_ORIGINALS


async def nuphus_probe(executable):
    environment = {**os.environ, 'NUPHUS_MCP_NO_MODEL_DOWNLOAD': '1'}
    async with stdio_client(StdioServerParameters(command=str(executable), env=environment)) as streams:
        async with ClientSession(*streams) as session:
            with anyio.fail_after(15):
                info = await session.initialize()
                tools = {tool.name for tool in (await session.list_tools()).tools}
                if len(tools) != 38 or not {'desktop_window_info', 'desktop_perceive', 'browser_snapshot', 'browser_click'}.issubset(tools):
                    raise RuntimeError('Nuphus protocol/tool contract is incompatible')
                return {'server': info.serverInfo.name, 'tool_count': len(tools), 'state': 'protocol-ready'}


def package_archive(package, version, destination):
    metadata = lifecycle.fetch_json('https://registry.npmjs.org/' + package.replace('/', '%2f') + '/' + version)
    if metadata['name'] != package or metadata['version'] != version:
        raise ValueError('Official npm metadata has a different package identity')
    raw = lifecycle.fetch(metadata['dist']['tarball'])
    lifecycle.verify_integrity(raw, metadata['dist']['integrity'])
    lifecycle.safe_extract_tar(raw, destination)
    return {'package': package, 'version': version, 'source': metadata['dist']['tarball'], 'integrity': metadata['dist']['integrity']}


def provision(identifier, user_home, state_dir, version, node=None):
    if identifier not in ('codebase-memory', 'nuphus') or not lifecycle.stable_version(version):
        raise ValueError('Select one supported MCP and a stable explicit version')
    current = lifecycle.discovery.Discovery(user_home, verify_records=True, processes=True)
    if node and Path(node).is_file():
        current.node = str(Path(node).resolve())
    record = next(item for item in current.run()['mcp'] if item['id'] == identifier)
    if record['status'] != 'missing':
        return {'id': identifier, 'state': 'reused' if record['status'] == 'adopted' else 'pending',
                'reason': 'Existing shared installation is preserved.', 'version': record.get('version')}
    node = node or current.node
    if not node or not Path(node).is_file():
        raise FileNotFoundError('The existing Node.js runtime is required; no duplicate Node is installed')
    modules = Path(user_home).resolve() / 'AppData' / 'Roaming' / 'npm' / 'node_modules'
    package = 'codebase-memory-mcp' if identifier == 'codebase-memory' else '@nuphus/nuphus-mcp'
    installation = modules / package
    if (modules / '.package-lock.json').exists():
        raise ValueError('Existing shared npm lockfile requires a manager-aware transaction; preserve it')
    state_dir = Path(state_dir).resolve()
    with lifecycle.installation_lock(state_dir, installation):
        if installation.exists() or installation.is_symlink():
            raise ValueError('Shared package appeared before provisioning; repeat discovery')
        if identifier == 'codebase-memory':
            staged = lifecycle.stage_codebase(version, state_dir)
            stage_root = Path(staged['stage'])
            candidate = Path(staged['package'])
            evidence = staged['evidence']
            sources = staged['sources']
        else:
            if version not in AUDITED_ORIGINALS:
                raise ValueError('This Nuphus version has not passed the source adapter compatibility audit')
            if os.name != 'nt' or os.environ.get('PROCESSOR_ARCHITECTURE', 'AMD64').lower() not in ('amd64', 'x64'):
                raise ValueError('Only the audited Windows x64 Nuphus binary is supported')
            stage_root = state_dir / 'staging' / ('nuphus-' + version + '-' + uuid.uuid4().hex)
            stage_root.mkdir(parents=True)
            sources = [package_archive(package, version, stage_root)]
            candidate = stage_root / 'package'
            platform_package = '@nuphus/nuphus-mcp-win32-x64'
            platform_stage = stage_root / 'platform'
            sources.append(package_archive(platform_package, version, platform_stage))
            destination = candidate / 'node_modules' / platform_package
            destination.parent.mkdir(parents=True)
            os.replace(platform_stage / 'package', destination)
            binary = destination / 'bin/nuphus-mcp.exe'
            if lifecycle.discovery.fingerprint(binary) != AUDITED_ORIGINALS[version]:
                raise ValueError('Nuphus native binary differs from the audited official executable')
            evidence = anyio.run(nuphus_probe, binary)
        # Path components may already exist, but no link may redirect this write.
        for ancestor in (installation, *installation.parents):
            if ancestor.is_symlink() or (hasattr(ancestor, 'is_junction') and ancestor.is_junction()):
                raise ValueError('Shared npm destination contains a reparse point')
        installation.parent.mkdir(parents=True, exist_ok=True)
        journal = lifecycle.activate_new_directory(candidate, installation, state_dir, identifier)
        try:
            adopted = lifecycle.discovery.npm_candidate(package, modules, node)
            if adopted['status'] != 'adopted':
                raise RuntimeError('Installed package was not rediscovered with its real native payload')
            if identifier == 'codebase-memory':
                evidence = lifecycle.mcp_probe([adopted['paths']['native_executable']], state_dir / 'probes' / ('cb-post-' + uuid.uuid4().hex[:8]))
            else:
                evidence = anyio.run(nuphus_probe, adopted['paths']['original_native_executable'])
        except BaseException:
            lifecycle.recover_transaction(journal, rollback_committed=True)
            raise
        return {'id': identifier, 'state': 'installed-unverified', 'version': version, 'record': adopted,
                'sources': sources, 'evidence': evidence, 'transaction_journal': journal,
                'verification': 'Native protocol exercised; full language/desktop/browser behavior is separate acceptance.'}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('id', choices=['codebase-memory', 'nuphus'])
    parser.add_argument('--user-home', default=str(Path.home()))
    parser.add_argument('--state-dir', required=True)
    parser.add_argument('--version', required=True)
    parser.add_argument('--node')
    parser.add_argument('--transaction-id')
    args = parser.parse_args()
    lifecycle.TRANSACTION_ID = args.transaction_id
    print(json.dumps(provision(args.id, args.user_home, args.state_dir, args.version, args.node), ensure_ascii=False, indent=2))


if __name__ == '__main__':
    main()
