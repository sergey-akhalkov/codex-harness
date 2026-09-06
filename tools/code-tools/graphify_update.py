"""Explicit Graphify staging/promotion; never imported by a runtime launcher."""
from __future__ import annotations

import argparse
import copy
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tomllib
import uuid

import anyio
import dependencies as lifecycle
import graphify_proxy


def run(command, *, environment=None):
    completed = subprocess.run(list(map(str, command)), env={**(environment or os.environ), 'PYTHONDONTWRITEBYTECODE': '1'}, capture_output=True,
                               text=True, encoding='utf-8', errors='replace', timeout=240,
                               creationflags=0x08000000 if os.name == 'nt' else 0)
    if completed.returncode:
        raise RuntimeError(f'Graphify staging command failed (exit {completed.returncode}): {completed.stderr[-1800:]}')
    return completed.stdout.strip()


def record_for(user_home):
    inventory = lifecycle.discovery.Discovery(user_home, verify_records=True, processes=True).run()
    record = next(item for item in inventory['mcp'] if item['id'] == 'graphify')
    if record['status'] != 'adopted':
        raise RuntimeError(f"Graphify installation cannot be updated: {record['status']}")
    return record


async def graph_probe(python, graph):
    async with graphify_proxy.upstream(str(python), str(graph), None, None) as (session, _):
        tools = {tool.name for tool in (await session.list_tools()).tools}
        if not {'graph_stats', 'query_graph', 'get_node', 'list_prs'}.issubset(tools):
            raise RuntimeError('Candidate Graphify tool contract is incompatible')
        stats = await session.call_tool('graph_stats', {})
        query = graphify_proxy.checked_result(await session.call_tool('query_graph', {'question': 'graphify', 'token_budget': 200}))
        if query.isError or not query.content:
            raise RuntimeError('Graphify candidate query failed')
        return {'stats': ''.join(item.text for item in stats.content if item.type == 'text'), 'tools': sorted(tools)}


def stage(user_home, state_dir, version, *, codex_home=None):
    if not re.fullmatch(r'\d+\.\d+\.\d+', version):
        raise ValueError('Only a concrete stable Graphify version can be staged')
    record = record_for(user_home)
    installation = Path(record['installation_root'])
    settings_path = (Path(codex_home) / 'harness' if codex_home else Path(state_dir).resolve().parent) / 'graphify.json'
    graph, _ = graphify_proxy.selected_graph(record, settings_path)
    settings_before = lifecycle.discovery.fingerprint(settings_path) if settings_path.is_file() else None
    graph_before = lifecycle.discovery.fingerprint(graph)
    expected = anyio.run(graph_probe, record['paths']['python'], graph)
    service = record.get('shared_service', {})
    manifest_path = Path(service['manifest']) if service.get('manifest') else None
    protected_manifest = str(manifest_path.resolve()) if manifest_path and manifest_path.is_file() else None
    workstation_graph = None
    if protected_manifest:
        manifest = json.loads(manifest_path.read_text(encoding='utf-8-sig'))
        workstation_graph = Path(manifest['graphify']['configuration']['graph']['path']).resolve()
        if not workstation_graph.is_file():
            raise FileNotFoundError('The shared workstation graph is missing; preserve the adopted package')
    # A kit override selects another graph without transferring ownership of
    # the workstation's data. Both consumers must remain compatible.
    graph_checks = [{'graph': str(graph), 'sha256': graph_before, 'evidence': expected}]
    if workstation_graph and workstation_graph != graph:
        graph_checks.append({'graph': str(workstation_graph),
            'sha256': lifecycle.discovery.fingerprint(workstation_graph),
            'evidence': anyio.run(graph_probe, record['paths']['python'], workstation_graph)})
    original_receipt = (installation / 'uv-receipt.toml').read_text(encoding='utf-8')
    parsed = tomllib.loads(original_receipt)
    requirements = parsed['tool']['requirements']
    if len(requirements) != 1 or requirements[0]['name'] != 'graphifyy' or requirements[0].get('extras') != ['mcp']:
        raise ValueError('Unknown UV receipt requirements; preserve the shared environment')
    old_specifier = requirements[0].get('specifier')
    if old_specifier != '==' + record['version']:
        raise ValueError('Shared UV receipt version differs from adopted distribution')
    receipt, changed = re.subn(r'(specifier\s*=\s*")' + re.escape(old_specifier) + '"',
                               lambda match: match[1] + '==' + version + '"', original_receipt)
    if changed != 1:
        raise ValueError('Cannot update the exact UV requirement without changing other receipt fields')
    uv = shutil.which('uv')
    if not uv:
        raise FileNotFoundError('Existing UV manager is required for Graphify update')
    stage_root = Path(state_dir).resolve() / 'staging' / ('graphify-' + version + '-' + uuid.uuid4().hex)
    stage_root.mkdir(parents=True)
    candidate = stage_root / 'tools' / 'graphifyy'
    environment = {**os.environ, 'UV_TOOL_DIR': str(stage_root / 'tools'), 'UV_TOOL_BIN_DIR': str(stage_root / 'bin'),
                   'UV_NO_CONFIG': '1', 'UV_DEFAULT_INDEX': 'https://pypi.org/simple', 'PYTHONDONTWRITEBYTECODE': '1'}
    run([uv, 'tool', 'install', f'graphifyy[mcp]=={version}', '--python', record['paths']['python'], '--no-python-downloads'], environment=environment)
    python = candidate / 'Scripts' / 'python.exe'
    base = run([python, '-B', '-c', 'import sys; print(sys._base_executable)'])
    # UV's supported relocatable venv mode regenerates activation and console
    # entry points. Existing public tool wrappers keep their original absolute
    # destination, which remains the same after directory promotion.
    run([uv, 'venv', '--allow-existing', '--relocatable', '--python', base, '--no-python-downloads', candidate], environment=environment)
    run([uv, 'pip', 'install', '--python', python, '--reinstall', '--offline', f'graphifyy[mcp]=={version}'], environment=environment)
    # A package update must preserve the adopted interpreter launchers. UV's
    # relocatable mode otherwise changes their Windows binary identity, which
    # the existing OpenCode workstation correctly checks independently.
    for name in ('python.exe', 'pythonw.exe'):
        shutil.copy2(installation / 'Scripts' / name, candidate / 'Scripts' / name)
    old_entries = installation / 'Lib' / 'site-packages' / f"graphifyy-{record['version']}.dist-info" / 'entry_points.txt'
    new_entries = candidate / 'Lib' / 'site-packages' / f'graphifyy-{version}.dist-info' / 'entry_points.txt'
    if old_entries.read_text().strip() != new_entries.read_text().strip():
        raise ValueError('Graphify console entry points changed; existing public wrappers require a separate migration')
    (candidate / 'uv-receipt.toml').write_text(receipt, encoding='utf-8')
    for check in graph_checks:
        evidence = anyio.run(graph_probe, python, Path(check['graph']))
        if evidence != check['evidence'] or lifecycle.discovery.fingerprint(check['graph']) != check['sha256']:
            raise RuntimeError('Graphify candidate changed a selected graph or its query/statistics contract')
    # A move before promotion proves the generated entry point is relocatable.
    relocated = stage_root / 'candidate'
    os.replace(candidate, relocated)
    version_read = run([relocated / 'Scripts' / 'graphify.exe', '--version'])
    if version not in version_read:
        raise RuntimeError('Relocated Graphify console entry point is broken')
    stage_manifest = {'schema_version': 1, 'id': 'graphify', 'version': version, 'old_version': record['version'],
        'installation': str(installation), 'candidate': str(relocated), 'prior_identity': lifecycle.tree_identity(installation),
        'candidate_identity': lifecycle.tree_identity(relocated), 'graph': str(graph), 'graph_sha256': graph_before,
        'protected_manifest': protected_manifest, 'workstation_graph': str(workstation_graph) if workstation_graph else None,
        'graph_checks': graph_checks, 'evidence': expected,
        'settings_path': str(settings_path.resolve()), 'settings_sha256': settings_before,
        'wrapper_fingerprints': {entry['install-path']: lifecycle.discovery.fingerprint(entry['install-path']) for entry in parsed['tool']['entrypoints']},
        'python_sha256': lifecycle.discovery.fingerprint(record['paths']['python']),
        'source': f'https://pypi.org/project/graphifyy/{version}/', 'state': 'staged-compatible'}
    lifecycle.atomic_json(stage_root / 'stage.json', stage_manifest)
    return {**stage_manifest, 'stage_manifest': str(stage_root / 'stage.json')}


def prepare_manifest(stage_manifest):
    """Prepare only the expected module identity change, with the original DACL.

    Source and prepared manifest bodies never appear in command lines, stdout,
    source files or the dependency journal. Empty files acquire the source DACL
    before receiving any bytes.
    """
    stage_path = Path(stage_manifest)
    staged = json.loads(stage_path.read_text(encoding='utf-8'))
    if not staged.get('protected_manifest'):
        auxiliary_path = stage_path.with_name('auxiliary-files.json')
        lifecycle.atomic_json(auxiliary_path, [])
        return {'auxiliary_files': str(auxiliary_path), 'manifest_sddl': None}
    source = Path(staged['protected_manifest'])
    original = source.read_bytes()
    before = json.loads(original.decode('utf-8-sig'))
    after = copy.deepcopy(before)
    config = after['graphify']['configuration']
    if config['module']['packageVersion'] != staged['old_version'] or Path(config['graph']['path']).resolve() != Path(staged.get('workstation_graph') or staged['graph']).resolve():
        raise ValueError('Workstation graph/module identity changed since staging')
    installed_module = Path(config['module']['source']['path'])
    if lifecycle.discovery.fingerprint(installed_module) != config['module']['source']['sha256']:
        raise ValueError('Existing workstation module hash differs; preserve its configuration')
    relative = installed_module.relative_to(Path(staged['installation']))
    candidate_module = Path(staged['candidate']) / relative
    config['module']['packageVersion'] = staged['version']
    config['module']['source']['length'] = candidate_module.stat().st_size
    config['module']['source']['sha256'] = lifecycle.discovery.fingerprint(candidate_module)
    before_path = stage_path.with_name('workstation-before.json')
    after_path = stage_path.with_name('workstation-after.json')
    pwsh = shutil.which('pwsh')
    if not pwsh:
        raise FileNotFoundError('PowerShell is required to preserve the protected manifest DACL')
    sddl = None
    for target, data in ((before_path, original), (after_path, json.dumps(after, indent=2).encode('utf-8'))):
        with target.open('xb'):
            pass
        literal = lambda value: "'" + str(value).replace("'", "''") + "'"
        script = '$ErrorActionPreference="Stop"; $acl=Get-Acl -LiteralPath ' + literal(source)
        script += '; Set-Acl -LiteralPath ' + literal(target) + ' -AclObject $acl; (Get-Acl -LiteralPath ' + literal(target) + ').Sddl'
        observed = run([pwsh, '-NoLogo', '-NoProfile', '-NonInteractive', '-Command', script])
        if sddl is not None and sddl != observed:
            raise ValueError('Prepared manifest ACLs differ')
        sddl = observed
        target.write_bytes(data)
    descriptor = [{'path': str(source), 'before_path': str(before_path), 'after_path': str(after_path)}]
    auxiliary_path = stage_path.with_name('auxiliary-files.json')
    lifecycle.atomic_json(auxiliary_path, descriptor)
    return {'auxiliary_files': str(auxiliary_path), 'manifest_sddl': sddl}


def promote(user_home, state_dir, stage_manifest, auxiliary_files):
    staged = json.loads(Path(stage_manifest).read_text(encoding='utf-8'))
    installation = Path(staged['installation'])
    candidate = Path(staged['candidate'])
    state_dir = Path(state_dir).resolve()
    if not lifecycle.discovery.contained(candidate, state_dir) or staged['id'] != 'graphify':
        raise ValueError('Foreign Graphify staging identity')
    with lifecycle.installation_lock(state_dir, installation):
        record = record_for(user_home)
        consumers = record['active_consumers']
        if consumers['state'] != 'observed' or consumers['processes']:
            raise RuntimeError('Active or uninspectable Graphify consumers prevent replacement')
        if Path(record['installation_root']).resolve() != installation.resolve():
            raise ValueError('Adopted Graphify installation changed')
        observed_manifest = record.get('shared_service', {}).get('manifest')
        observed_manifest = str(Path(observed_manifest).resolve()) if observed_manifest and Path(observed_manifest).is_file() else None
        if 'settings_path' in staged and observed_manifest != staged.get('protected_manifest'):
            raise ValueError('The shared workstation manifest changed since staging')
        if lifecycle.tree_identity(installation) != staged['prior_identity'] or lifecycle.tree_identity(candidate) != staged['candidate_identity']:
            raise ValueError('Graphify installation or staged candidate changed since acceptance')
        for path, fingerprint in staged['wrapper_fingerprints'].items():
            if lifecycle.discovery.fingerprint(path) != fingerprint:
                raise ValueError('Graphify public wrapper changed since staging')
        graph = Path(staged['graph'])
        graph_checks = staged.get('graph_checks') or [{'graph': str(graph), 'sha256': staged['graph_sha256'], 'evidence': staged['evidence']}]
        if staged.get('settings_path'):
            settings_path = Path(staged['settings_path'])
            settings_now = lifecycle.discovery.fingerprint(settings_path) if settings_path.is_file() else None
            if settings_now != staged.get('settings_sha256'):
                raise ValueError('Host Graphify settings changed since staging; preserve the current selection')
        for check in graph_checks:
            if lifecycle.discovery.fingerprint(check['graph']) != check['sha256']:
                raise ValueError('Graph changed since candidate validation; repeat staging on the current graph')
        if staged.get('protected_manifest') and Path(staged['protected_manifest']).exists() and not auxiliary_files:
            raise ValueError('Protected workstation module identity must participate in the update transaction')
        for item in auxiliary_files:
            if Path(item['path']).resolve() != Path(staged['protected_manifest']).resolve():
                raise ValueError('Graphify update may only change the discovered workstation manifest')
            original = json.loads(Path(item['before_path']).read_text(encoding='utf-8-sig'))
            prepared = json.loads(Path(item['after_path']).read_text(encoding='utf-8-sig'))
            expected = copy.deepcopy(original)
            module = expected['graphify']['configuration']['module']
            relative = Path(module['source']['path']).relative_to(installation)
            module['packageVersion'] = staged['version']
            module['source']['length'] = (candidate / relative).stat().st_size
            module['source']['sha256'] = lifecycle.discovery.fingerprint(candidate / relative)
            if prepared != expected:
                raise ValueError('Prepared workstation manifest changes fields beyond the selected module identity')
        backup = state_dir / 'rollback' / ('graphify-' + staged['old_version'] + '-' + uuid.uuid4().hex)
        journal = lifecycle.transaction_path(state_dir, 'graphify')

        def validate(active):
            if lifecycle.discovery.fingerprint(active / 'Scripts' / 'python.exe') != staged['python_sha256']:
                raise ValueError('Graphify package promotion changed the adopted Python launcher')
            for check in graph_checks:
                if anyio.run(graph_probe, active / 'Scripts' / 'python.exe', Path(check['graph'])) != check['evidence']:
                    raise RuntimeError('Promoted Graphify failed actual graph compatibility checks')
            for path, fingerprint in staged['wrapper_fingerprints'].items():
                if lifecycle.discovery.fingerprint(path) != fingerprint:
                    raise ValueError('Shared public wrapper was unexpectedly modified')
                if Path(path).name == 'graphify.exe' and staged['version'] not in run([path, '--version']):
                    raise ValueError('Existing public Graphify wrapper does not load the updated package')
            if any(lifecycle.discovery.fingerprint(check['graph']) != check['sha256'] for check in graph_checks):
                raise RuntimeError('Saved graph changed during promotion validation')
            return staged['evidence']

        evidence = lifecycle.atomic_promote(candidate, installation, backup, validate,
                                             journal_path=journal, auxiliary_files=auxiliary_files)
        return {'id': 'graphify', 'state': 'updated', 'version': staged['version'], 'rollback': str(backup),
                'transaction_journal': str(journal), 'graph_unchanged': True, 'public_wrappers_unchanged': True, 'evidence': evidence}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('operation', choices=['stage', 'prepare-manifest', 'promote'])
    parser.add_argument('--user-home', default=str(Path.home()))
    parser.add_argument('--state-dir', required=True)
    parser.add_argument('--codex-home', help='Host configuration root when dependency state is stored elsewhere')
    parser.add_argument('--version')
    parser.add_argument('--stage-manifest')
    parser.add_argument('--auxiliary-files', help='Machine-local JSON containing only path/before_path/after_path')
    parser.add_argument('--transaction-id')
    args = parser.parse_args()
    lifecycle.TRANSACTION_ID = args.transaction_id
    if args.operation == 'stage':
        result = stage(args.user_home, args.state_dir, args.version, codex_home=args.codex_home)
    elif args.operation == 'prepare-manifest':
        result = prepare_manifest(args.stage_manifest)
    else:
        result = promote(args.user_home, args.state_dir, args.stage_manifest,
            json.loads(Path(args.auxiliary_files).read_text(encoding='utf-8-sig')) if args.auxiliary_files else [])
    print(json.dumps(result, ensure_ascii=False, indent=2))


if __name__ == '__main__':
    main()
