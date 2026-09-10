## Why

The current Codebase Memory full index of the locally selected large repository exceeds the accepted 2 GiB process-tree limit. An isolated CodeGraph 1.6.0 trial fits that limit, but its broad context responses can exceed 20 KiB even with a two-file request; replacement must preserve useful coverage and control the information delivered to agents.

## What Changes

- **BREAKING**: replace the kit-managed `codebase-memory` MCP selection with `colbymchenry/codegraph`, after the acceptance below passes. Preserve unrelated installations, existing indexes and rollback. This proposal does not activate the replacement.
- Deliver the selected published Windows package through the existing global dependency, registration and recovery lifecycle. Do not maintain a CodeGraph or CBM fork, install development toolchains, or raise the 2 GiB limit to make acceptance pass.
- Implement all new first-party CodeGraph integration and executable acceptance checks in Rust, including MCP adaptation, process supervision, resource/response limits and provider lifecycle logic. The published CodeGraph implementation and bundled Node remain external dependencies; existing transitional lifecycle entry points may dispatch to the native implementation.
- Enable bounded automatic incremental refresh and connect-time catch-up in every indexed project with an open Codex CLI session, including several different projects open simultaneously. Keep one account-wide heavy-indexing slot, finite operation deadlines and owned process trees; the first project must not monopolize service. A healthy long-running session must not need periodic manual sync to keep watching. Stop observation after the last session for that project closes, with automatic catch-up on reopening. Initial/full rebuilds remain deliberate operations; unsolicited telemetry and an independent detached upstream daemon stay disabled.
- Reuse one project's index, watcher and backend across its clients, including different Codex homes. Reuse suitable published CodeGraph and existing native broker capabilities before adding coordination; avoid a heavy resident process per CLI session and measure actual process/memory behavior with multiple projects. Resource scheduling may serialize expensive work without disabling observation in other open projects.
- Prefer Serena for known-file symbols, exact references and edits. Use CodeGraph for compact repository relationships and cross-file discovery when that avoids several reads. Keep literal/configuration/document retrieval on scoped native tools.
- Enforce response budgets before results reach model context, with small defaults, explicit incompleteness and bounded access to retained details. A `maxFiles` setting alone is insufficient; broad `explore`, whole-file reads and exhaustive traversal are not defaults.
- Compare task results, request/response bytes, setup, follow-ups and latency on the pack and the locally selected large repository. Distinguish byte counts from tokenizer measurements and actual subscription usage.
- Update reusable instructions now with the measured selection policy, conditionally describing CodeGraph until cutover. Reconcile overlapping active work during implementation without closing unfulfilled requirements.

## Capabilities

### New Capabilities

None; extend the existing code-tool, resource and retrieval contracts.

### Modified Capabilities

- `global-code-tools`: managed CodeGraph replacement, accepted operations, workspace identity and recoverable global delivery.
- `bounded-tool-resources`: CodeGraph indexing, refresh, storage and process limits with preserved committed indexes.
- `token-efficient-agent-workflow`: task-based Serena/CodeGraph selection, enforced result budgets and comparative acceptance.

## Impact

Affected owners include `global/code-tools.json`, `global/tool-resources.json`, native dependency/registration/MCP code, the transitional code-tool entry points, `global/principles-of-work.md`, relevant skills and maintained code-tool/token documentation. Global instructions are linked from this checkout, so new sessions receive instruction edits without changing the current MCP registration.

This change owns the first-party Rust CodeGraph adapter and its provider acceptance. `migrate-harness-to-rust` tasks 5.3–5.4 reuse the existing generic foundation and integrate this same adapter/evidence; they must not continue a separate CBM port. The [ownership and order map](../migrate-harness-to-rust/design.md#graph-provider-ownership-and-order) preserves large-repository, recovery and global-delivery requirements without a circular dependency: this replacement may activate through the current lifecycle before full native installer cutover. `improve-installed-tool-workflows` retains its comparative workflow acceptance. The shared public-pack cleanup must not be reverted, and local consumer names, paths and raw source responses must stay outside Git.
