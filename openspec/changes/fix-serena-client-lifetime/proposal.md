# Proposal

## Why

The managed Serena client is configured to expire after 24 hours even while its Codex conversation remains open. Later activation calls in that conversation fail immediately with `Transport closed`, while a fresh connection can activate the same project.

## What Changes

- Keep managed Serena STDIO connections available for the lifetime of their client, independently of request deadlines and worker idle retirement.
- Preserve bounded worker resources, process ownership, per-client project routing and non-replayed edits.
- Cover the expiry failure and continued requests with native protocol regression checks, and deliver the fix through the global installation lifecycle.
- Document how an already closed connection adopts the repair.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `bounded-tool-resources`: separate the lifetime of a connected Serena client from finite requests and idle worker retention.

## Impact

Serena client transport, managed registration, native transport tests and the existing code-tools operation guide. No new dependency, project migration or increase to worker resource allowances is required.
