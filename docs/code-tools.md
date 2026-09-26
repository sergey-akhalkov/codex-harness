# Global MCP and language tools

[Documentation map](README.md) · [Selected capabilities](../global/code-tools.json) ·
[Code tools specification](../openspec/specs/global-code-tools/spec.md) ·
[Resource specification](../openspec/specs/bounded-tool-resources/spec.md)


The live global MCP selection is Serena and Nuphus. Codebase Memory, Graphify
and CodeGraph are retired and their first-party code is removed; leftover
shared packages, saved graphs and indexes stay on the host as inert residue
for manual inspection or deletion.
Start a new Codex session to load current registrations. Existing sessions keep
previously loaded catalogues until restart.

## Selecting tools in everyday work

The [global MCP selection rules](../global/principles-of-work.md#mcp-tool-selection)
apply to parents and tool-capable children in every project. Serena is the
primary code surface: file structure through `get_symbols_overview`, bounded
bodies through `find_symbol`, exact relationships through
`find_referencing_symbols`/`find_implementations`/`find_declaration`, and code
changes through symbol operations (`replace_symbol_body`,
`insert_before_symbol`/`insert_after_symbol`, `rename_symbol`,
`safe_delete_symbol`) instead of line surgery; `replace_in_files` serves
matching narrow multi-file text edits. Use explicit project checks for
validation and Nuphus for authorized UI work. Connected Apps serve their
matching remote resources when enabled locally; the portable default disables
the Apps feature. Literal text and narrow line edits retain native tools. The
managed Serena connection excludes memory, onboarding and
configuration-introspection tools plus `search_for_pattern` through Serena's
own `excluded_tools` setting in the generated worker home, and that home
replaces Serena's stock connection prompt, which named the excluded
`initial_instructions` tool. The worker therefore advertises exactly the
accepted selection, and initialize and project-activation guidance name no
excluded tool. Scoped native `rg` owns literal text and regex search because it
is faster, complete and shell-owned. Debugging the full catalogue means running
the adopted console directly with an unrestricted context
(`serena start-mcp-server --context desktop-app --project <PROJECT>`) instead of
the managed proxy.

Desktop window identity uses list, title, bounds and state. Screenshots are for
genuine visual questions about owned non-text UI, not to prove Codex conversation
visibility. Desktop/window captures without a path become native image blocks;
caller-supplied owned paths stay path-only and must not include image bytes.

Availability does not establish the active project, language support or an
open document session. Initialize only relevant
missing context, bound queries, and use a scoped fallback for observed gaps.
This is an instruction policy, not a forced scheduler or a measured
subscription-saving claim. The measured-use history that led to the CodeGraph
retirement lives in the archived [CodeGraph replacement](../openspec/changes/archive/2026-09-11-replace-cbm-with-codegraph/proposal.md)
and the 2026-09 retirement change.
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
its verified backend; BasedPyright registry updates are held
(`held-backend-staging`) until the native dependency staging path lands, so
ordinary upstream releases do not block installs. Retired candidates stay
outside provisioning. Shared installations are not deleted. Native project
checks remain available.

Specialized controller or CNC source formats are not a managed language set.
Extension presence does not prove a compatible language server.

## Source and local state

Optional workflow selection keeps the existing broker/activation route for
cross-project Serena queries. The qualified 1.7.0 LSP `query_project` path needs
a separate Project Server, whose loaded language managers are outside the kit's
three-worker/idle-expiry lifecycle. Two owned native consumers returned distinct
values for the same Rust symbol name under the current broker and completed
their owned process cleanup; no additional Project Server is selected.

Graphify 0.9.55 was the first graph tool retired after measured use collapsed
to a handful of calls; its first-party proxy and update sources were removed
with that retirement.

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
processes do not inherit it by default. The native `codex-harness mcp` entry
reads the resolved shared installation registry and serves the selected tool
through its first-party adapter.
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
state. A retired `graphify.json` may remain from earlier installations and is
no longer managed. Credentials stay in existing protected storage or
environment variables.

## Resource limits and reuse

The reusable defaults live in [tool-resources.json](../global/tool-resources.json).
The kit shares compatible tool services across sessions and agents; it does not
introduce a shared Codex app-server.

| Tool | Reuse and resource contract |
| --- | --- |
| harness-lsp | Retired; no managed registration or backend. Cached hook callbacks are silent compatibility guards. |
| Serena | One authenticated local broker per `CODEX_HOME`, with at most three project workers, 300-second idle expiry, a 4 GiB Windows Job and 25% CPU per worker. Each worker retains a fixed project; matching project/mode/configuration requests share serialized access. Clients retain their own project selection and conversation state. A worker whose language-server manager failed during project initialization is replaced and the same semantic call is retried once. The generated worker home excludes memory, onboarding, introspection and text-search tools through Serena's own `excluded_tools` and supplies the managed connection prompt, so the worker's catalogue and guidance are forwarded unchanged; native Git records stay authoritative and scoped `rg` owns literal text. |
| Nuphus | Native tools and the session's owned browser start lazily. Browser operations use a private browser profile and verified endpoint; session/snapshot references expire after navigation or browser retirement. Foreign or expired references require a fresh snapshot. Desktop operations share account-wide admission. Desktop or window screenshots without a destination path return a native image content block; a caller-supplied owned path remains path-only. Image bytes, base64 and nested JSON text are not model-visible. Conversation visibility is not a Nuphus screenshot task. |

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

Resource settings have an account-level receipt with a set of installation owners.
Repeated installation for the same `CODEX_HOME` is idempotent. Disconnect releases
that owner; only the last owner restores original native settings, and only where
the current value still equals the value applied by the kit. Later user edits are
preserved and reported. Changes to native watch settings gracefully retire the
existing daemon so old subscriptions do not survive the migration.

Enforcement applies to delivered kit entry points; direct native invocations can
bypass supervision. Existing MCP processes have loaded old code and must be
replaced by restarting their Codex sessions.

## Retired graph integrations

CodeGraph, Codebase Memory and Graphify are fully retired: no managed
selection, registration, first-party adapter or CLI route remains in the kit.
Update removes any owned registration recorded by an earlier version while
preserving unrelated settings; shared packages, saved graphs, caches and
account/broker state stay on the host as inert residue. Delete them manually
when no longer needed; the kit neither reads nor re-registers them. The
measured-use and breakage history that justified the retirement lives in the
archived [CodeGraph replacement](../openspec/changes/archive/2026-09-11-replace-cbm-with-codegraph/proposal.md)
and the 2026-09 retirement change.

## Explicit lifecycle

On an existing installation, reconcile only code-tool connections and resource
policy without package changes, core links or subscription routing:

```powershell
& <build>\codex-harness.exe install    --code-tools-only --source . --codex-home <CODEX_HOME> --user-home <USER_HOME> --preview
& <build>\codex-harness.exe install    --code-tools-only --source . --codex-home <CODEX_HOME> --user-home <USER_HOME>
& <build>\codex-harness.exe check      --code-tools-only --source . --codex-home <CODEX_HOME> --user-home <USER_HOME>
& <build>\codex-harness.exe recover    --code-tools-only --source . --codex-home <CODEX_HOME> --user-home <USER_HOME>
& <build>\codex-harness.exe disconnect --code-tools-only --source . --codex-home <CODEX_HOME> --user-home <USER_HOME>
```

Install adopts discovered dependencies and commits registration, registry and
resource changes through the scoped activation transaction. Check reports
connection/dependency health and resource drift; it does not install packages.
Recover rolls back an interrupted scoped transaction or completes cleanup after
its durable commit. Disconnect retires owned shared services and removes
still-owned connections. Unrelated component journals remain untouched. A pending
combined activation requires Recover without the selector.
Missing dependencies need the full explicit
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
& <build>\codex-harness.exe install    --core-only --source . --build <build> --codex-home <CODEX_HOME> --user-home <USER_HOME> --preview
& <build>\codex-harness.exe install    --core-only --source . --build <build> --codex-home <CODEX_HOME> --user-home <USER_HOME>
& <build>\codex-harness.exe update     --core-only --source . --build <build> --codex-home <CODEX_HOME> --user-home <USER_HOME>
& <build>\codex-harness.exe check      --core-only --codex-home <CODEX_HOME> --user-home <USER_HOME>
& <build>\codex-harness.exe recover    --core-only --codex-home <CODEX_HOME> --user-home <USER_HOME>
& <build>\codex-harness.exe disconnect --core-only --codex-home <CODEX_HOME> --user-home <USER_HOME>
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
  [installation](installation.md#what-is-connected).
- Child agents may be unable to resolve a parent's connected adapter in the
  tested CLI; a source-owned command fallback must preserve original results
  and never turn failed analysis into a clean report.
- Relocating the checkout and reconnecting preserves an existing dependency
  owner without changing that owner's global connections.
- No second physical Windows machine was available for original connection
  acceptance; isolated user roots and fresh checkout relocation are substitutes,
  not a clean-VM claim.

Primary contracts: [Windows Job Objects](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects),
[Serena configuration](https://oraios.github.io/serena/02-usage/050_configuration.html),
[rust-analyzer installation](https://rust-analyzer.github.io/book/installation.html).
