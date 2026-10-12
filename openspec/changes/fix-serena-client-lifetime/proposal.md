# Proposal

## Why

The managed Serena and Nuphus clients are configured to expire after 24 hours even while their Codex conversation remains open. Later calls in that conversation fail immediately with `Transport closed`, while a fresh connection can serve requests. Serena expiry is reproduced through the installed entry point; Nuphus uses the same absolute-lifetime policy and is included in the repair.

## What Changes

- Keep managed Serena and Nuphus STDIO connections available for the lifetime of their client, independently of request deadlines and worker idle retirement.
- Preserve bounded worker resources, process ownership, per-client project routing and non-replayed edits.
- Cover the expiry failure and continued requests with native protocol regression checks, and deliver the fix through the global installation lifecycle.
- Document how an already closed connection adopts the repair.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `bounded-tool-resources`: separate connected MCP client lifetimes from finite requests and idle worker retention.

## Impact

Serena and Nuphus client transports, shared STDIO serving, managed registrations, native transport tests and the existing code-tools operation guide. No new dependency, project migration or increase to worker resource allowances is required.
