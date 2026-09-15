## Why

Installed Codex CLI still starts without a ready CodeGraph MCP even after earlier
repairs. The live registration points at a source-linked native manager that
refuses ordinary runtime as soon as checkout source changes, so Codex closes the
handshake and omits the server. Serena, Graphify and Nuphus remain available
because they do not share that gate.

## What Changes

- Keep CodeGraph MCP ready on every Codex CLI start through the installed
  registration, including interactive, non-interactive, resumed/forked and
  tool-capable child sessions.
- Register a runtime-admitted native command for CodeGraph. A manager that is
  only valid for integrity-checked management MUST NOT be the Codex MCP
  command.
- Make Check report owned CodeGraph as degraded when its command cannot serve
  the MCP handshake, instead of calling a source-stale registration connected.
- Preserve unrelated MCP registrations, user edits, published CodeGraph/Node
  and existing indexes. Ordinary startup still MUST NOT download packages,
  rebuild from source or enable telemetry.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `global-code-tools`: owned CodeGraph must start with the rest of the managed
  MCP set, and Check must not treat a runtime-disabled manager as connected.

## Impact

Native CodeGraph registration, the transitional installer that writes that
registration, Check, and native MCP/runtime-identity tests. No new dependency,
package version, resource allowance or change to Serena/Graphify/Nuphus
launchers. Private runtime evidence stays local.
