# Global MCP and language tools

[Documentation map](README.md) · [Selected capabilities](../global/code-tools.json) · [OpenSpec tasks](../openspec/changes/archive/2026-09-06-connect-global-mcp-lsp/tasks.md)

Status: globally connected on the current Windows host. All four MCPs passed
real calls in new native consumers and a subagent. The fourteen required
languages and available HTML/CSS passed native automatic error/clearance checks.
All 45 change tasks are complete. The [acceptance report](code-tools-verification.md)
maps the requirements to actual checks and records update holds and environment
limits. Start a new Codex session to load the global registrations.

The user selected four MCPs (Serena, Codebase Memory, graphifyy and Nuphus),
plus Rust, TypeScript, JavaScript, PowerShell, Python, Delphi, C++, C#, JSON,
Markdown, TOML, XML, CMake and Bash. YAML, QML, HTML and CSS are conditional
on ready compatible support. The [filesystem inventory](language-inventory.md)
records why these additional formats were selected; it does not add every
observed extension to the required language set.

## Source and local state

The kit adds MCP path registrations and one owned readiness setting to the
existing user config. The existing
native Codex TOML editor renders these registrations in a disposable empty
config; an exact owned block is then inserted into the live config. This avoids
`codex mcp add` rewriting unrelated server formatting. Semantic validation and
host-local before/after hashes protect activation and recovery. Native TUI may
normalize that block and interleave unrelated tables. Subsequent ownership
checks compare the owned TOML tables; disconnect removes only their statement
spans and preserves foreign settings and comments. Conflicting owned settings
are preserved and reported.

Each registration explicitly supplies the active `CODEX_HOME` path. Native MCP
processes do not inherit it by default. [mcp.ps1](../tools/mcp.ps1) reads the
resolved shared installation registry and dispatches into repository source.
There are no copied server definitions in the user config. The native connection
also owns `mcp_optional_startup_grace_ms=0`: initial and resumed sessions wait for
each optional server's finite startup timeout before building the tool catalogue.
The default one-second grace produced actual missing-tool failures during
acceptance. An incompatible pre-existing explicit value is preserved as a
conflict; disconnect restores prior absence or preserves a pre-existing zero.
This scalar belongs to connection metadata, with no duplicated source body.
The fifth endpoint,
`harness-lsp`, supplies the automatic diagnostics missing from the four upstream
MCPs.

The user's `hooks.json` and `harness/bin/hook.ps1` are direct source links managed
by the kit's core link transaction. The global hook source uses PowerShell
`-EncodedCommand` solely to pass the small Codex-home/path bootstrap without
expansion by an outer shell. The UTF-16LE payload decodes to this command, with
`pre`, `post` or `stop` as its last argument:

```powershell
$r = if ($env:CODEX_HOME) { $env:CODEX_HOME } else { Join-Path ([Environment]::GetFolderPath('UserProfile')) '.codex' }
& (Join-Path $r 'harness/bin/hook.ps1') -Event pre
```

Native `/hooks` review establishes trust for the exact hook definitions.
Changing definitions invalidates that trust. No trust hash is manufactured by
the installer and no bypass flag is part of the integration.

Seven handlers cover PreToolUse, PostToolUse, Stop and SubagentStop. Pre records
the baseline before the edit independently of MCP startup. Post calls the
existing lazy adapter. If the native consumer cannot connect that hook to its
MCP manager, a bounded command worker uses the same journal and backend; atomic
claims prevent duplicate analysis. This route is required for the tested native
subagent manager. Stop reconciles pending changes with finite bounds. Pending,
unavailable and stale results are explicit; they are never presented as clean.

Workspace and child identity come from native hook fields. The linked launcher
also captures explicit `--add-dir` roots relative to the effective `--cd`.
Actual native tool `workdir` values establish additional pre-edit baselines.
Diagnostics run only for observed changes and affected files, preserve the
original tool result, and never edit source or install packages.

Machine-local records live under `CODEX_HOME/harness`: `code-tools.json` holds
dependency discovery, `code-tools-registration.json` holds owned registrations,
`lsp-servers.json` selects resolved language commands, `dependencies/` holds
version checks and rollback/staging records, and `runtime/` holds session state.
Optional `graphify.json` declares a graph path and local connection references.
Credentials stay in existing protected storage or environment variables.

## Explicit lifecycle

```powershell
./install.ps1 -Mode Install -WhatIf
./install.ps1 -Mode Install
./install.ps1 -Mode Update -WhatIf
./install.ps1 -Mode Update
./install.ps1 -Mode Check
./install.ps1 -Mode Recover
./install.ps1 -Mode Disconnect
```

Preview performs read-only discovery and official metadata requests. Package
installation or updates belong to Install/Update, never to a normal MCP session
or diagnostic request. A `pending`, `failed`, `prerequisite-missing` or
`installed-unverified` dependency is unfinished work. A newer PSES release is
staged and compatible but its shared installation is retained while other
consumers use it. Rust Analyzer stays with the existing project toolchain.
These holds are recorded by the [dependency report](evidence/tool-dependencies.md).
Preview and the next explicit Update reconsider the observed versions.

The standalone native MCP health check reports `protocol-ready` after handshake
and tools/list. That status establishes protocol availability, not representative
operations or completed language acceptance. Real tests and the final acceptance
matrix are required for stronger claims.

## Operation evidence

- [Native MCP/hooks contract](evidence/code-tools-native.md): real CLI and
  unprofiled app-server calls, native TUI trust, live linked source and automatic
  model-visible hook feedback.
- [Serena](evidence/mcp-serena.md): existing installation, no runtime provisioning,
  two real project roots, semantic edit, diagnostics and clearance.
- [Graphify](evidence/mcp-graphify.md): preserved existing graph, STDIO fallback,
  authenticated HTTP reuse and an actual explicit-repository query.
- [Codebase Memory](evidence/mcp-codebase.md): real independent indexing/query
  with private Windows rendezvous and cache.
- [Nuphus](evidence/mcp-nuphus.md): owned desktop/browser targets and offline OCR.
- [Language matrix](evidence/lsp-languages.md): actual navigation, native
  diagnostics and the limitations of each selected server.
- [Dependency lifecycle](evidence/tool-dependencies.md) and
  [combined recovery](evidence/code-tools-activation.md).
- [Native Serena edit](evidence/lsp-native-mcp.md), including UTF-8 hook input,
  automatic feedback and preservation of original MCP results.
- [Fresh source relocation](evidence/full-source-move.md), including explicit
  reuse of a dependency owner without changing that owner's global connections.

The task list and acceptance report record the completed scope. The change was
archived on 2026-09-06 after synchronizing its requirements into the main specs.
