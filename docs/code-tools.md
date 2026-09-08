# Global MCP and language tools

[Documentation map](README.md) · [Selected capabilities](../global/code-tools.json) · [OpenSpec tasks](../openspec/changes/archive/2026-09-06-connect-global-mcp-lsp/tasks.md)

Current selection, 2026-09-08: **ordinary diagnostic and Stop hooks remain off**;
the accepted RTK exception is globally active. Cached diagnostic handlers remain
inert. The separate `harness-lsp` registration
and its managed backend provisioning are retired. Shared language installations
remain intact. Explicit Serena Python navigation and suitable edits are retained;
Rust was additionally selected for harness development on 2026-09-08, using the
existing rust-analyzer and this repository's Serena language configuration;
its diagnostic API has documented freshness limits. Automatic
LSP has no selected scope. See the
[confirmed decision](project-decisions.md#расход-подписки-и-условная-автоматизация),
[suspension evidence](evidence/hooks-suspension.md) and
[implementation evidence](evidence/subscription-efficiency.md).
The automatic-delivery checks below describe the earlier installation.
The separately authorized [RTK exception](../openspec/changes/archive/2026-09-08-optimize-agent-token-workflow/proposal.md)
has its own acceptance and does not restore automatic language diagnostics.

Historical connection acceptance: all four MCPs passed
real calls in new native consumers and a subagent. The fourteen required
languages and available HTML/CSS passed native automatic error/clearance checks.
All 45 original connection tasks are complete. The [acceptance report](code-tools-verification.md)
maps the requirements to actual checks and records update holds and environment
limits. Start a new Codex session to load the global registrations.

The earlier resource policy activated five MCP endpoints. The current selection
has four protocol-ready endpoints and zero harness diagnostic backends. The
historical full pmac index completed in 27.874 seconds within
the enforced 2 GiB limit; shared-service lifecycle acceptance also passed. Measurements, failures
and acceptance status have one home in the [resource evidence](evidence/tool-resources.md).

The original selection included four MCPs (Serena, Codebase Memory, graphifyy and Nuphus),
plus Rust, TypeScript, JavaScript, PowerShell, Python, Delphi, C++, C#, JSON,
Markdown, TOML, XML, CMake and Bash. YAML, QML, HTML and CSS are conditional
on ready compatible support. These are now evaluation candidates, not mandatory
installations. The [filesystem inventory](language-inventory.md)
records why these additional formats were selected; it does not add every
observed extension to the required language set.

## Selecting tools in everyday work

The [global MCP selection rules](../global/principles-of-work.md#mcp-tool-selection)
apply to parents and tool-capable children in every project. Codebase Memory is
preferred for repository relationships, Serena for suitable symbol operations,
explicit project checks for validation, Graphify for a relevant selected graph,
and Nuphus for authorized UI work. Connected Apps serve their matching remote
resources. Literal text and narrow line edits retain native tools.

Availability does not establish the active project, index coverage, graph identity,
language support or an open document session. Initialize only relevant missing
context, bound queries, and use a scoped fallback for observed gaps. The
[live acceptance report](evidence/mcp-tool-selection.md) records parent and child
calls, qualitative usefulness, global loading and rollback. This is an instruction
policy, not a forced tool scheduler or a measured subscription-saving claim.

## Current language selection

All automatic diagnostics are disabled. The following matrix describes managed
selection, not the full capabilities of pre-existing shared packages.

| Language | Explicit selected operations | Automatic / other status |
| --- | --- | --- |
| Python | Serena 1.7.0, LSP backend basedpyright 1.39.10: symbol discovery, definitions, cross-file references, suitable symbol/body and matching-text edits; actual error diagnostics with stated uncertainty | Automatic rejected; creation uses native tools; empty diagnostics do not prove clean current analysis |
| Rust | Explicit Serena navigation using the adopted rust-analyzer; enabled in codex-harness project configuration | Automatic diagnostics remain disabled; use Cargo checks for acceptance |
| TypeScript | None selected | Retired |
| JavaScript | None selected | Retired |
| PowerShell | None selected | Retired |
| Delphi | None selected | Retired; historical dialect results are not a current support claim |
| C++ | None selected | Retired |
| C# | None selected | Retired |
| JSON | None selected | Retired |
| Markdown | None selected | Retired; use explicit link checks |
| TOML | None selected | Retired |
| XML | None selected | Retired, including Qt XML `.ts` |
| CMake | None selected | Retired |
| Bash | None selected | Retired |
| YAML | None selected | Retired candidate |
| QML | None selected | Retired candidate |
| HTML | None selected | Retired candidate |
| CSS | None selected | Retired candidate |

The [installed Serena qualification](evidence/serena-efficiency.md) uses two owned
Python projects, real edits/references and the live discovery registry. Python
remains in dependency discovery because Serena's startup guard requires its
verified backend. The other seventeen candidates stay outside provisioning.
This does not remove shared installations or prevent native project checks.

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
The former fifth endpoint, `harness-lsp`, and its automatic delivery stack are
retired. Hook definitions are empty, the command returns silently, and cached
MCP/broker callbacks return before analysis. Core links preserve this disabled
selection on update and relocation. Historical trigger, Stop and transport
behavior is retained in the [reconciliation evidence](evidence/diagnostic-reconciliation.md);
it is not an installation instruction or an active capability.
Machine-local records live under `CODEX_HOME/harness`: `code-tools.json` holds
dependency discovery, `code-tools-registration.json` holds owned registrations,
`lsp-servers.json` records an empty retired harness selection, `dependencies/` holds
version checks and rollback/staging records, and `runtime/` holds session state.
Optional `graphify.json` declares a graph path and local connection references.
Credentials stay in existing protected storage or environment variables.

## Resource limits and reuse

The reusable defaults live in [tool-resources.json](../global/tool-resources.json).
The kit shares compatible tool services across sessions and agents; it does not
introduce a shared Codex app-server.

| Tool | Reuse and resource contract |
| --- | --- |
| Codebase Memory | Explicit index/refresh only: automatic indexing, watching and graph UI are disabled. One account-wide index slot, including across different `CODEX_HOME` values; two native workers, a 1 GiB advisory target, a 2 GiB Windows Job memory limit, 25% CPU rate and a 600-second deadline. Queries use bounded native calls without a permanent per-session native frontend. |
| harness-lsp | Retired; no managed registration or backend. Cached hook callbacks are silent compatibility guards. |
| Serena | One authenticated local broker per `CODEX_HOME`, with at most three project workers and 300-second idle expiry. Each worker retains a fixed project; matching project/mode/configuration requests share serialized access. Clients retain their own project selection and conversation state. |
| Nuphus | Native tools and the session's owned browser start lazily. Browser operations use a private browser profile and verified endpoint; session/snapshot references expire after navigation or browser retirement. Foreign or expired references require a fresh snapshot. Desktop operations share account-wide admission. |
| Graphify | Reuses the explicitly configured authenticated HTTP endpoint when available; that foreign service's lifetime belongs to its operator. The kit owns and reclaims only its lazy STDIO fallback. Always select the intended saved graph/project. |

Windows ownership guards reclaim owned descendant processes on owner exit or
crash, including language servers and private browsers. Pool eviction closes the
previous owned tree before admitting a replacement. Startup locks prevent duplicate
broker launches; retirement drains admitted work. Unfinished activation journals
block new shared-service work until Recover resolves the transaction.

On Windows, shared brokers start through built-in local WMI as the same verified
user, independently of the first client's process Job. They immediately establish
their own child ownership and a finite startup watchdog. Closing the starter does
not close a broker used by another client. No scheduled task or installed Windows
service is added; an unavailable WMI launcher produces an explicit startup error.

Before graph-backed Codebase Memory search or navigation, explicitly run
`index_repository` unless a successful index of the current source state is already
verified. Unknown freshness before the first search, local edits, external changes
and branch switches require refresh before the next graph query. Batch edits and
reuse a verified unchanged index across related queries. Wait for indexing to
succeed and check coverage before relying on results. A busy slot, resource limit,
cancellation or deadline does not establish freshness: use fresh Serena/LSP or
direct source and label the retained graph stale. Generated-log exclusions are
project-specific, additive changes; the kit does not delete source or indexes.

Resource settings have an account-level receipt with a set of installation owners.
Repeated installation for the same `CODEX_HOME` is idempotent. Disconnect releases
that owner; only the last owner restores original native settings, and only where
the current value still equals the value applied by the kit. Later user edits are
preserved and reported. Changes to native watch settings gracefully retire the
existing daemon so old subscriptions do not survive the migration.

## Explicit lifecycle

On an existing installation, reconcile only code-tool connections and resource
policy without package changes, core links or subscription routing:

```powershell
pwsh -NoProfile -File ./install.ps1 -CodeToolsOnly -Mode Install -WhatIf
pwsh -NoProfile -File ./install.ps1 -CodeToolsOnly -Mode Install
pwsh -NoProfile -File ./install.ps1 -CodeToolsOnly -Mode Check
pwsh -NoProfile -File ./install.ps1 -CodeToolsOnly -Mode Recover
pwsh -NoProfile -File ./install.ps1 -CodeToolsOnly -Mode Disconnect
```

Install adopts discovered dependencies and commits registration, registry and
resource changes through the scoped activation transaction. Check reports
connection/dependency health and resource drift; it does not install packages.
Recover rolls back an interrupted scoped transaction or completes cleanup after
its durable commit. Disconnect retires owned shared services and removes still-owned
connections, releasing the resource owner as described above. Unrelated component
journals remain untouched. A pending combined activation requires Recover without
the selector. `-CodeToolsOnly` does not support Update; missing dependencies need
the full explicit installation lifecycle. See [installation and recovery](installation.md).

Restart native Codex sessions that loaded the previous MCP processes. Installation
retires managed brokers, but already-running native consumers retain their original
MCP children and catalogue until their session is restarted. Reuse does not remove
this migration boundary.

The full kit lifecycle, including explicit dependency maintenance, remains:

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

- [Resource limits and reuse](evidence/tool-resources.md): current resource
  incident, process ownership, scoped lifecycle and remaining acceptance work.
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

The original task list and acceptance report record the completed connection scope. That change was
archived on 2026-09-06 after synchronizing its requirements into the main specs.
