## Why

Installed Codex CLI starts without Serena or Nuphus. Both registrations point at
the source-linked native manager, which refuses source-consuming MCP as soon as
the checkout differs from the recorded build, so Codex closes initialize and
omits those servers. CodeGraph already uses serving admission for this case;
Serena's broker path is serving-admitted, but the stdio frontends are not.

## What Changes

- Keep owned Serena and Nuphus ready on every Codex CLI start through the
  installed registrations, including after later checkout edits whose recorded
  binaries still match.
- Admit `mcp serena` and `mcp nuphus` through serving admission. Leave
  `mcp codebase-memory` gated on a healthy source match.
- Preserve unrelated MCP registrations, user edits, audited Nuphus/Serena
  packages and ordinary no-download startup. Native adapter changes still need
  explicit Install/Update and a session restart.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `global-code-tools`: owned Serena and Nuphus must complete MCP initialize
  after later source edits the same way CodeGraph already does.

## Impact

Native MCP command admission, the existing source-stale runtime test, and the
CodeGraph serving-admission notes. No new dependency, package version or
resource allowance. Private runtime evidence stays local.
