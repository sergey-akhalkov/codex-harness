# Fix MCP broker anchor growth

## Why

After the latest delivery, the registered Serena and CodeGraph MCP servers fail
before the MCP handshake: their account/location anchor records accumulated one
entry per build generation, crossed the fixed 4096-byte read bound, and every
`codex-harness mcp serena` / `mcp codegraph` startup now exits with
“… record exceeds its bound”. Nuphus is unaffected because it has no such
anchor. Repeated deliveries must not make the managed MCP surface unavailable.

## What Changes

- Accept still-bounded but grown Serena/CodeGraph location records instead of
  rejecting them at startup, so an installation broken by historical growth
  starts again.
- Reclaim broker locations whose generation has no live broker: a new
  generation reuses a free existing location instead of always preparing a new
  one, and dead entries are pruned from the record under its existing
  account-wide admission lock.
- Preserve live older-generation brokers and the joining behavior for a
  generation that already matches its own record entry.
- Add regression tests that reproduce the exact post-delivery failure (grown
  anchor → startup failure) and assert the MCP entrypoints complete
  `initialize` and `tools/list` after growth, in the standard native checks and
  in the adopted-package acceptance.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `rust-native-harness`: broker generation records stay usable and bounded
  across deliveries, and MCP startup completes after historical growth.

## Impact

`harness-core` broker-state generation selection, the Serena broker and
CodeGraph account location records, their unit tests, and the CodeGraph/Serena
MCP integration checks. No registration, package, protocol or lifecycle-command
changes; Nuphus is untouched.
