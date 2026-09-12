# Global MCP and language tools

[Documentation map](README.md) · [Selected capabilities](../global/code-tools.json) ·
[Code tools specification](../openspec/specs/global-code-tools/spec.md) ·
[Resource specification](../openspec/specs/bounded-tool-resources/spec.md)


The live global MCP selection is Serena, CodeGraph, Graphify and Nuphus.
Codebase Memory is retired from the managed selection; its shared package,
indexes and native rollback commands remain installed. Start a new Codex
session to load current registrations. Existing sessions keep previously
loaded catalogues until restart.

## Selecting tools in everyday work

The [global MCP selection rules](../global/principles-of-work.md#mcp-tool-selection)
apply to parents and tool-capable children in every project. Serena is the first
choice for known-file symbols, exact references and suitable edits. CodeGraph
serves compact repository discovery and relationships. Use explicit project
checks for validation, Graphify for a
relevant selected graph, and Nuphus for authorized UI work. Connected Apps serve
their matching remote resources. Literal text and narrow line edits retain
native tools.

Availability does not establish the active project, index coverage, graph
identity, language support or an open document session. Initialize only relevant
missing context, bound queries, and use a scoped fallback for observed gaps.
This is an instruction policy, not a forced scheduler or a measured
subscription-saving claim. The archived [CodeGraph replacement](../openspec/changes/archive/2026-09-11-replace-cbm-with-codegraph/proposal.md)
records the native adapter and its accepted consumer evidence.
Use the [retrieval recipes](../.agents/skills/token-efficient-workflow/references/code-retrieval.md)
to choose small answers and preserve uncertainty; broad explore is not the
default entry point.

## Current language selection

All automatic diagnostics are disabled. The following matrix describes managed
selection, not the full capabilities of pre-existing shared packages.

| Language | Explicit selected operations | Automatic / other status |
| --- | --- | --- |
| Python | Serena 1.7.0, LSP backend basedpyright 1.39.10: symbol discovery, definitions, cross-file references, suitable symbol/body and matching-text edits; actual error diagnostics with stated uncertainty | Automatic rejected; empty diagnostics do not prove clean current analysis |
| Rust | Explicit Serena navigation using the adopted rust-analyzer; enabled in this repository's Serena project configuration | Automatic diagnostics remain disabled; use Cargo checks for acceptance |
| Other historical candidates | None selected | Retired from managed discovery, including TypeScript, JavaScript, PowerShell, Delphi, C++, C#, JSON, Markdown, TOML, XML, CMake, Bash, YAML, QML, HTML and CSS |

Python remains in dependency discovery because Serena's startup guard requires
its verified backend. Retired candidates stay outside provisioning. Shared
installations are not deleted. Native project checks remain available.

Specialized controller or CNC source formats are not a managed language set.
Extension presence does not prove a compatible language server.

## Source and local state

The kit adds MCP path registrations and one owned readiness setting to the
existing user config. The native Codex TOML editor renders these registrations
in a disposable empty config; an exact owned block is then inserted into the
live config. This avoids `codex mcp add` rewriting unrelated server formatting.
Semantic validation and host-local before/after hashes protect activation and
recovery. Native TUI may normalize that block and interleave unrelated tables.
Subsequent ownership checks compare the owned TOML tables; disconnect removes
only their statement spans and preserves foreign settings and comments.
Conflicting owned settings are preserved and reported.

Each registration explicitly supplies the active `CODEX_HOME` path. Native MCP
processes do not inherit it by default. [mcp.ps1](../tools/mcp.ps1) reads the
resolved shared installation registry and dispatches into repository source.
There are no copied server definitions in the user config. The native connection
also owns `mcp_optional_startup_grace_ms=0`: initial and resumed sessions wait for
each optional server's finite startup timeout before building the tool catalogue.
The default one-second grace produced actual missing-tool failures. An
incompatible pre-existing explicit value is preserved as a conflict; disconnect
restores prior absence or preserves a pre-existing zero.

Machine-local records live under `CODEX_HOME/harness`: `code-tools.json` holds
dependency discovery, `code-tools-registration.json` holds owned registrations,
`lsp-servers.json` records an empty retired harness selection, `dependencies/`
holds version checks and rollback/staging records, and `runtime/` holds session
state. Optional `graphify.json` declares a graph path and local connection
references. Credentials stay in existing protected storage or environment
variables.

## Resource limits and reuse

The reusable defaults live in [tool-resources.json](../global/tool-resources.json).
The kit shares compatible tool services across sessions and agents; it does not
introduce a shared Codex app-server.

| Tool | Reuse and resource contract |
| --- | --- |
| Codebase Memory | Retired from the managed selection after replacement acceptance; not registered. Shared package, existing indexes and native `cbm-index`, `cbm-catalogue`, `cbm-tool` and `mcp codebase-memory` remain the rollback/compatibility route with explicit index/refresh only. |
| CodeGraph | Live graph provider. One account-wide indexing slot, one parse worker, one resolve worker, a 2 GiB Windows Job and 25% CPU. Each indexing episode has a 600-second deadline. Native observation covers every active indexed root; clients of the same root share resources. Healthy worker retirement preserves queued refresh. Check is read-only; Install/Update stage the pinned published Windows x64 1.6.0 tree (940 files). Ordinary startup does not download, build or enable telemetry. |
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
Environment is passed through STDIN into the native process environment, never
through command lines or handoff files.

Before graph-backed CBM search or navigation, explicitly run `index_repository`
unless a successful index of the current source state is already verified.
Unknown freshness before the first search, local edits, external changes and
branch switches require refresh before the next graph query. Batch edits and
reuse a verified unchanged index across related queries. Wait for indexing to
succeed and check coverage before relying on results. A busy slot, resource
limit, cancellation or deadline does not establish freshness: use fresh Serena
or direct source and label the retained graph stale. Generated-log exclusions
are project-specific, additive changes; the kit does not delete source or
indexes.

Resource settings have an account-level receipt with a set of installation owners.
Repeated installation for the same `CODEX_HOME` is idempotent. Disconnect releases
that owner; only the last owner restores original native settings, and only where
the current value still equals the value applied by the kit. Later user edits are
preserved and reported. Changes to native watch settings gracefully retire the
existing daemon so old subscriptions do not survive the migration.

Enforcement applies to delivered kit entry points; direct native invocations can
bypass supervision. Existing MCP processes have loaded old code and must be
replaced by restarting their Codex sessions.

## Native CodeGraph provider

The first-party adapter lives in `crates/harness-core/src/codegraph_*.rs`. Native
commands:

```powershell
codex-harness.exe mcp prepare-codegraph --mode Check --codex-home <directory> --dependency-state <directory>
codex-harness.exe mcp prepare-codegraph --mode Install --codex-home <directory> --dependency-state <directory> [--package-root <directory>]
codex-harness.exe mcp apply-codegraph-registration --mode Check --codex-home <directory> [--package-root <directory>]
codex-harness.exe mcp codegraph --package-root <directory> [--project <directory>] [--broker-root <directory>]
codex-harness.exe mcp retire-codegraph
```

Check inspects identity without staging. Install/Update may explicitly acquire
and probe the pinned published package; they do not write MCP registrations.
The Rust registration command journals the provider switch and supports
Install/Update/Check/Recover/Disconnect. The existing installer coordinates it
with the other component journals. Transitional PowerShell passes the native
projection and retained tools' planned registrations into that Rust transaction;
the provider uses `startup_timeout_sec=30` / `tool_timeout_sec=660`.

The exact current directory is the default project root for a connection.
Initial/full indexing is deliberate (`codegraph_index`). An account-wide native
broker observes each connected indexed root, including clients in different
Codex homes. Source notifications coalesce into bounded counters; a fair queue
runs published finite sync operations and commits completed generations. Native
observation replaces the upstream autonomous watcher so different roots share
one indexing allowance. Queries identify the canonical root and generation;
changes arriving during an episode remain pending and require current source
until a subsequent completed refresh.

On Windows, checkpoint rotation waits up to two seconds for a short-lived
reader to release a conflicting handle, within the current operation deadline
and cancellation. Persistent sharing or permission failures remain explicit;
the saved checkpoint stays recoverable and is not presented as current.

Backend processes are reused for the selected root and drained before switching
roots. Idle backends retire after 60 seconds, and healthy replacement precedes
the process lease deadline without ending source observation. Closing the last
client stops that project's observation and cancels its active work; other roots
remain available. Reopening performs catch-up. An actual failed episode preserves
the saved checkpoint and suspends automatic retries for that root until deliberate
recovery. Ordinary queries do not make full database copies.

The managed catalogue is ten tools: `codegraph_status`, `codegraph_index`,
`codegraph_sync`, `codegraph_search`, `codegraph_callers`, `codegraph_callees`,
`codegraph_impact`, `codegraph_node`, `codegraph_explore` and
`codegraph_detail`. Defaults are five matches, depth one and 4 KiB serialized
answers, with an explicit 16 KiB maximum. Explore requires a named question and
`maxFiles` of 1 or 2; file count is not a byte budget. Per-client retained details
are at most 32
pages, 256 KiB each, 8 MiB total, 30-minute expiry; retrieving a page never
repeats the query. Graph edges are candidates; exact references and edits use
Serena or current source.

Owned project caches are `.codegraph-harness-store`,
`.codegraph-harness-active` and `.codegraph-harness-stage` beside the selected
root. Prepare/index writes an owned `OWNER` record and a cache-local
`.gitignore` containing only `*`; that ignore belongs to the cache, never to
the project's Git rules. Account-wide worker state uses a private
`LOCALAPPDATA` location. Product sources, CBM indexes and unrelated
packages stay in place for rollback.

The pinned package is CodeGraph 1.6.0 Windows x64, archive SHA-256
`cd76c3c3391f2d40abef12b142151950b6d77abc2d8429e648f89eaa90f5b68a`, verified
940-file tree. Published CodeGraph and bundled Node remain third-party; the kit
does not maintain a fork or install their development toolchains.

Accepted evidence before activation included:
real-package CLI stage/select/Check/rollback; unit identity with companion
mutation refusal; native 2 GiB allocation deny, deadline and cancel after
partial writes with the committed generation preserved; generation store
checks; small native MCP index, watch, rename, delete and reconnect; and a
full real-root comparison with zero source-oracle loss on the pack and the
locally selected large repository. Exhaustive detail recovery increases total
bytes and extra calls, so those measurements are not a weekly-quota or overall
savings claim. The managed catalogue is a stable ten-tool surface; a filtered
upstream raw catalogue can be smaller.

Activation passed through the existing recoverable lifecycle: the transitional
installer rebuilt the native manager from current source, adopted the verified
package and committed the CodeGraph registration while retiring only the owned
CBM entry. Live Check reports the connection protocol-ready; semantic config
comparison showed only the owned CodeGraph command path changing. CBM's shared
package and existing indexes stayed in place. Isolated journal tests cover
interrupted activation, Recover back to CBM's manual policy, preserved user
edits and Disconnect removing only the owned block.

Installed consumer acceptance then verified fresh non-interactive app-server
sessions in three indexed projects plus another client of the first root
(automatic add/change-burst/rename/delete, shared worker identity, last-client
retirement, offline-root stop and reopen catch-up, 49.9 seconds), a fresh
interactive TUI smoke through the installed launcher, resume and fork of a
saved thread, retained Serena/Graphify/Nuphus operations including an owned
browser effect, and one explicit exec parent plus tool-capable middle child
that each performed a real CodeGraph search with the expected canonical root.
The default probe model is Astra; the recorded run used the separately
authorized xAI/Grok route while that account quota was unavailable.

Restart boundaries: already running sessions keep their previous MCP catalogue
until restarted. After Install/Update rebuilds native source, an account broker
from the older build is not adopted by frontends from the new build; run
`codex-harness.exe mcp retire-codegraph` once from the current build before
starting new consumers. Do not continue a CBM port; retire CBM-only paths only
with further consumer-backed evidence. Disconnect retires only owned CodeGraph
workers and registrations.

## Large-repository indexing

Acceptance of graph indexing requires the locally selected large real project.
A small fixture does not replace it. Preserve that project's product sources
and do not contact controllers. Generated HTML logs are excluded.

A historical CBM full index under the delivered policy completed: 3,668
discovered files, 27.874 seconds, about 1.97 GiB peak private memory, 128,315
nodes / 219,315 edges, 0 skipped and 118 partially parsed files. Native
coverage later reported `coverage_unavailable / metadata_changed` for
representative files whose saved hash, mtime and size still matched; that
upstream signal is not relabelled clean and is not proof of missing source.

The current larger checkout of the same locally selected project fails the same
CBM memory policy. A native shared-MCP index into an owned private cache failed
in 79.53 seconds with worker `MemoryLimit`, exit 125, Job limit 2 GiB and Windows
allocation error 1455. Largest remaining inputs include maintained XML
configurations, headers and reference data rather than the previously excluded
generated HTML logs. No successful current CBM full index or refresh is claimed.

The current CodeGraph comparison indexed that same locally selected project
under the retained 2 GiB policy: 979 eligible files matched 979 SQL indexed
files; the pack checkout matched 315 eligible to 315 indexed. The large index
completed in 56.658 seconds with 714,379,264 bytes peak private memory. These
counts account for eligible maintained-source extraction; the comparison's
source oracles and refresh checks provide separate behavioral evidence.
Replacement task 3.5 is complete. Global delivery and the broader Rust task
5.4 remain open; current measurements and limits live in the
[replacement design](../openspec/changes/archive/2026-09-11-replace-cbm-with-codegraph/design.md).

Missing CBM UI JSON is treated as enabled for the audited embedded-UI binary,
regardless of SQLite settings. Coverage flags `missing` and `metadata_changed` are
upstream best-effort signals, not complete-source proof.

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
its durable commit. Disconnect retires owned shared services and removes
still-owned connections. Unrelated component journals remain untouched. A pending
combined activation requires Recover without the selector. `-CodeToolsOnly`
does not support Update; missing dependencies need the full explicit
installation lifecycle. See [installation and recovery](installation.md).

Restart native Codex sessions that loaded the previous MCP processes.

Native registration tolerates Codex reformatting or reordering TOML tables;
ownership is checked against recorded values. Install, Update, Disconnect and
Recover preserve unrelated statements and refuse semantic edits to owned
settings. Recover can retain later unrelated edits while restoring the previous
registration. The native lifecycle records ownership of
`mcp_optional_startup_grace_ms = 0`: Disconnect removes an added setting and
preserves a pre-existing zero. An explicit different value is preserved and
reported as a connection conflict.

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
installation or updates belong to Install/Update, never to a normal MCP session.
A `pending`, `failed`, `prerequisite-missing` or `installed-unverified`
dependency is unfinished work. A newer PSES release may be staged while a shared
installation used by other consumers is retained. Rust Analyzer stays with the
existing project toolchain. These holds mean not all observed updates are
applied; they are not missing language support.

The standalone native MCP health check reports `protocol-ready` after handshake
and tools/list. That status establishes protocol availability, not representative
operations or completed language acceptance.

Native MCP/hook contract notes that remain current:

- MCP registrations are path references into live source, not copied bodies.
- Native TUI trust and unprofiled app-server writers have distinct targets; see
  [installation](installation.md#shared-defaults-and-local-tui-writes).
- Child agents may be unable to resolve a parent's connected adapter in the
  tested CLI; a source-owned command fallback must preserve original results
  and never turn failed analysis into a clean report.
- Relocating the checkout and reconnecting preserves an existing dependency
  owner without changing that owner's global connections.
- No second physical Windows machine was available for original connection
  acceptance; isolated user roots and fresh checkout relocation are substitutes,
  not a clean-VM claim.

Primary contracts: [CBM 0.10.8 configuration](https://github.com/DeusData/codebase-memory-mcp/blob/v0.10.8/docs/CONFIGURATION.md),
[CodeGraph 1.6.0 Windows release](https://github.com/colbymchenry/codegraph/releases/tag/v1.6.0),
[Windows Job Objects](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects),
[Serena configuration](https://oraios.github.io/serena/02-usage/050_configuration.html),
[rust-analyzer installation](https://rust-analyzer.github.io/book/installation.html).
