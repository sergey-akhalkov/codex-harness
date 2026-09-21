## Why

When the kit checkout moves ahead of the delivered immutable build, the installed
manager currently disables source-consuming runtime (executor dispatch, task
control, Codebase Memory) until an explicit deploy runs. That turns an ordinary
source commit into a launch outage for consumers that were already working on
the delivered build, while the binary rolling-deploy semantics (running sessions
keep their build, new sessions resolve the delivered one) are already in place.

## What Changes

- Launch admission for source-consuming runtime is decided by recorded binary
  integrity only. Missing, altered or metadata-incompatible builds still refuse
  the affected runtime with their existing corrective action.
- Source staleness and an unavailable checkout become reported status, not a
  runtime disable: Check, diagnose and deploy receipts state that the checkout
  differs from the delivered build and name explicit deploy as the action that
  switches new processes to a newer build.
- The delivered build is authoritative for launches. Nothing recompiles,
  downloads or rewrites configuration at consumer startup (unchanged).
- Explicit deploy stays the only mechanism that moves new processes to a newer
  build; already running processes keep their immutable build (unchanged).
- Tests that assert the source-stale launch refusal are updated to assert the
  new contract: the launch succeeds on the integrity-verified delivered build
  while Check keeps reporting the stale relationship.

## Capabilities

### New Capabilities

- None.

### Modified Capabilities

- `linked-global-kit`: the "Source updates and checkout availability"
  requirement currently requires an explicit verified build/update before the
  affected native executable runs after source changes. It will instead require
  integrity-verified binaries for runtime admission, keep reporting stale or
  unavailable sources with the deploy action, and preserve the rolling contract
  where only new processes resolve a newly delivered build.

## Impact

- `crates/harness-core/src/build_identity.rs`: health-to-admission mapping
  (`runtime_allowed`), stale/unavailable action text.
- `crates/codex-harness/src/main.rs` and related admission helpers.
- Tests covering the stale-source gate (`mcp_cli`, `codegraph_install`,
  `native_build`, launcher preflight).
- Check/diagnose/deploy receipt wording and the installation/native guides that
  describe stale-source behavior.
