## Why

Codebase Memory 0.10.8 exhausted Windows commit while updating `pmac-emulator`: a worker logged 5,844 MiB RSS despite a 1,010 MiB internal budget. Its full rebuild processed thousands of generated HTML logs. Separately, 23 PowerShell language servers survived their original owners. Multiple Codex sessions amplify unnecessary tool startup and language-server duplication on this 16 GiB host.

## What Changes

- Bound Codebase Memory indexing memory, CPU, concurrency and execution time using enforced process ownership; retain existing graph queries and report stale/failed indexing honestly.
- Stop uncontrolled automatic indexing and filter verified generated inputs through explicit project ignore rules, including the affected `pmac-emulator` log directory.
- Ensure owned Windows tool trees terminate with their owner, including abnormal exits; clean only verified historical orphan trees.
- Reuse retained language services across compatible sessions with configuration isolation, bounded caches and idle shutdown. The accepted subscription-efficiency selection controls which services remain connected; no hook or diagnostic broker is mandatory.
- Reuse Serena by canonical project and compatible configuration, preserve Graphify's shared endpoint, and retain isolated lazy browser state for Nuphus where sharing would mix clients.
- Remove unnecessary persistent launcher processes while preserving MCP protocol and lifecycle behavior.
- Deliver these policies globally through the kit installation/update/rollback lifecycle and verify resource behavior from outside the harness.
- Keep separate Codex CLI applications. A shared Codex app-server is explicitly excluded by the user's 2026-09-07 decision.

## Capabilities

### New Capabilities
- `bounded-tool-resources`: Enforced resource limits, compatible tool reuse, bounded lifecycle, global delivery and measured acceptance for local MCP/LSP tooling.

### Modified Capabilities
None. Resource and reuse guarantees apply to the retained global-code-tools selection. `reduce-subscription-waste` supersedes unconditional automatic diagnostics and Stop delivery; this change must not restore retired operations.

## Impact

Reusable implementation in `tools/code-tools/`, `tools/lsp/`, a Windows process ownership helper, native registration and installation support, `global/` resource policy, regression checks and documentation. Runtime metadata remains outside tracked sources. Authorized consumer configuration includes `D:/mekha/mtronics/pmac-emulator/.cbmignore`; product sources, controller access, unrelated processes and the subscription proxy are outside the mutation scope.
