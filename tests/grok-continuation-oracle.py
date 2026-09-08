"""Validate visible native continuation evidence; never decode reasoning state."""
import argparse
import json
from pathlib import Path
import re


def output_text(value):
    if isinstance(value, str):
        return value
    if isinstance(value, list):
        return '\n'.join(part.get('text', '') for part in value
                         if isinstance(part, dict) and part.get('type') in
                         ('text', 'input_text', 'output_text'))
    return ''


def verify(rows, marker, require_clean=False):
    if not marker:
        raise ValueError('Empty expected marker')
    models, meta, calls, pending, yielded, waits, errors = set(), [], {}, set(), [], [], []
    observed = False
    final = None
    for row in rows:
        if not isinstance(row, dict) or not isinstance(row.get('payload'), dict):
            raise ValueError('Malformed native row')
        payload = row['payload']
        kind = row.get('type')
        if kind == 'session_meta':
            meta.append(payload.get('id'))
        if kind == 'turn_context':
            models.add(payload.get('model'))
        if kind == 'response_item':
            item_type = payload.get('type')
            if item_type in ('function_call', 'custom_tool_call'):
                call_id = payload.get('call_id')
                if not call_id or call_id in calls:
                    raise ValueError('Missing or duplicate call identity')
                name = payload.get('name')
                if not isinstance(name, str):
                    raise ValueError('Malformed tool name')
                name = name.removeprefix('functions.')
                if payload.get('namespace') not in (None, 'functions'):
                    name = 'foreign.' + name
                cell = None
                if name == 'wait':
                    args = json.loads(payload.get('arguments', '{}'))
                    if not isinstance(args, dict):
                        raise ValueError('Malformed wait arguments')
                    cell = args.get('cell_id')
                    if cell not in pending:
                        raise ValueError('Wait does not match a pending cell')
                    waits.append(cell)
                calls[call_id] = (name, cell)
            if item_type in ('function_call_output', 'custom_tool_call_output'):
                text = output_text(payload.get('output'))
                name, cell = calls.get(payload.get('call_id'), ('', None))
                running = re.match(r'^Script running with cell ID ([^\s]+)', text)
                if 'failed to parse function arguments:' in text:
                    errors.append('tool_argument_parse_error')
                if running:
                    handle = running.group(1)
                    if name == 'exec':
                        if handle in pending:
                            raise ValueError('Duplicate pending handle')
                        yielded.append(handle)
                        pending.add(handle)
                    elif name != 'wait' or handle != cell:
                        raise ValueError('Running output lacks matching call')
                elif name == 'wait' and re.match(r'^Script (completed|failed)\b', text):
                    pending.remove(cell)
                    observed = observed or marker in text
        if kind == 'event_msg' and payload.get('type') == 'task_complete':
            final = payload.get('last_agent_message')
            if pending or not isinstance(final, str) or not final.strip():
                raise ValueError('Premature or empty final')
            if not observed or marker not in final:
                raise ValueError('Final lacks an actually retrieved marker')
    if len(meta) != 1 or not isinstance(meta[0], str) or not meta[0]:
        raise ValueError('Missing or conflicting session identity')
    if models != {'xai/grok-4.6'}:
        raise ValueError('Unexpected or missing model identity')
    if not yielded or pending or not final:
        raise ValueError('Missing completed delayed operation or final')
    if require_clean and errors:
        raise ValueError('Tool argument errors remain')
    return dict(model='xai/grok-4.6', thread_id=meta[0], yielded=yielded,
                waits=waits, errors=errors, final=final)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--rollout', type=Path, required=True)
    parser.add_argument('--marker', required=True)
    parser.add_argument('--require-clean', action='store_true')
    args = parser.parse_args()
    try:
        if not args.rollout.is_absolute() or args.rollout.stat().st_size > 16 * 1024 * 1024:
            raise ValueError('Require an absolute rollout no larger than 16 MiB')
        with args.rollout.open(encoding='utf-8') as stream:
            rows = [json.loads(line) for line in stream if line.strip()]
        print(json.dumps(verify(rows, args.marker, args.require_clean), ensure_ascii=True))
    except (ValueError, OSError, TypeError, KeyError) as error:
        print(json.dumps({'status': 'failed', 'error': type(error).__name__}))
        return 1
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
