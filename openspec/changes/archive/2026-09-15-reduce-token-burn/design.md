## Context

The native launcher already rewrites a first-position harness effort selector
into a native effort override, and portable defaults from
global/harness.config.toml become native -c overrides except where the
machine-local config already owns model, effort, TUI or notice keys. MCP
registrations are projected transactionally: a Rust-owned projection passes the
retired set (currently Codebase Memory) to the Python registration editor,
which removes only owned statements. CodeGraph is a first-party Rust adapter
with its own catalogue; Serena, Nuphus and Graphify attach through owned Python
proxies. See proposal.md for the measured burn evidence.

## Goals / Non-Goals

**Goals:**

- Deterministic per-model effort defaults without touching explicit user
  selections, profiles or remote routes.
- Retire Graphify through the existing owned-registration retirement path, with
  no deletion of shared packages or saved graphs.
- Bound the CodeGraph and Serena model-facing tool lists while keeping
  maintenance operations available outside model sessions.
- Make the Apps feature default off in portable defaults while machine-local
  values keep precedence.
- Compress the portable principles within the spec size bound with every
  normative rule preserved.

**Non-Goals:**

- No change to delegation visibility rules, screenshot policy or model routing.
- No compression of the portable developer_instructions block in this change.
- No kit-level requirement to remove locally installed plugins; the local
  GitHub plugin removal is a machine action recorded as verification evidence.
- No new general tool-filtering framework; filters are explicit per proxy and
  per catalogue.

## Decisions

- **Effort injection point**: extend the launcher argument policy with a
  per-model pass that runs only for session-class commands, resolves the model
  from explicit arguments first and falls back to the machine-local config,
  then the portable default. Injecting a native -c override keeps precedence
  over machine config while every explicit selection path still wins. The
  alternative of writing machine config per model switch was rejected because
  it mutates user state and cannot cover argument-level model selection.
- **Apps default**: add "features" to the portable-config keys where a
  machine-local leaf wins, and set features.apps=false in the portable file.
  Fresh machines get the lean default; a machine that explicitly enables Apps
  keeps it. Forcing the override unconditionally was rejected because it would
  make local re-enablement impossible without editing portable sources.
- **Graphify retirement**: reuse the Codebase Memory pattern: keep the name in
  the ownership/migration set, add it to the Rust retired projection, remove it
  from the dependency catalogue, and stop its runtime bootstrap during
  activation. Proxy sources and tool-resource rollback entries stay for the
  explicit local route. A hard uninstall was rejected because saved graphs and
  the shared package must remain usable.
- **CodeGraph surface**: filter the catalogue tools/list to search and detail,
  keep internal catch-up and request validation unchanged, and retarget stale
  hints to a new native CLI control command that drives the same bounded
  runtime operations for index, sync and status. Running maintenance through
  model sessions was rejected as the exact burn this change removes; deleting
  the operations entirely was rejected because deliberate indexing is a
  required capability.
- **Serena filter**: hide memory, onboarding and introspection tools in the
  proxy's tools/list responses, with an environment escape hatch for debugging.
  The alternative of filtering in Serena's own configuration was rejected
  because the kit owns the proxy and must keep the filter explicit and
  version-independent.
- **Principles compression**: rewrite the document to a tighter structure while
  mapping every requirement of the global-working-principles specification to
  retained text, remove Graphify as a live selection, and add the
  session-economy rule. Careful preservation outranks the size number: the
  bound is 24 KiB, and the measured careful result (about 23 KiB from 31.4 KiB)
  keeps every norm. Verification is the spec-mapping plus the size check; the
  git diff preserves the full prior text for review.

## Risks / Trade-offs

- [A long thread still repays history] -> The session-economy rule is advisory;
  measured rollout evidence stays the honest check, and no scheduler is added.
- [Filtered tools surprise a debugging session] -> Escape hatches and CLI
  routes are documented; filters change only tools/list, not server behavior.
- [Effort fallback misreads an exotic model string] -> Unmapped models get no
  injection and native behavior is preserved.
- [Compression loses a nuance] -> The git diff and the requirement mapping are
  reviewed together; the specification remains the authority.

## Migration Plan

1. Land code and docs; run targeted native tests.
2. Run the installer update on the machine; verify registrations, features and
   the new tool surfaces through the kit's own check paths.
3. Remove the locally installed GitHub plugin with the native CLI and record it
   as local evidence.
4. Existing sessions keep loaded catalogues until restart; state that openly.

Rollback: revert the commit and rerun the installer update; Graphify
registration can be restored by removing it from the retired projection, and
machine-local feature values are never overwritten by the kit.
