## Context

See `proposal.md` for motivation. Current state that shapes the approach:

- `global/code-tools.json` no longer lists Graphify, and launch/registration
  paths already refuse it. The remaining native code in `crates/harness-core`
  (discovery, plan, fetch, apply) is therefore unreachable from the CLI in
  production, but it is still compiled, maintained and covered by tests that
  construct Graphify fixtures.
- The graphify-named first-party sources are deleted in the in-flight
  migrate-harness-to-rust 8.2 batch (staged, not yet committed); HEAD still
  contains them until that batch lands.
- `codegraph_registration.rs` carries a name-based retirement guard: retired
  names are dropped from the desired registration set, so an update removes an
  owned registration recorded by a pre-retirement version instead of
  re-registering it. `codegraph_integration.rs` projects the same retired set.
- `subscription_lifecycle.rs` treats `bootstrap-graphify-pending.json` as an
  unfinished-operation blocker.
- Transitional Python/PowerShell sources (`tools/code-tools/*.py`,
  `tools/activation.psm1`, `tools/code-tools.psm1`) still contain Graphify
  branches; migrate-harness-to-rust tasks 8.2/9.2 delete that layer wholesale.

## Goals / Non-Goals

**Goals:**

- Remove every Graphify-specific behavior and claim from the maintained native
  surface, current specifications, documentation and evidence.
- Preserve correct update behavior for hosts whose recorded registration state
  still contains a pre-retirement Graphify registration.
- Leave a verifiable end state: a recorded reference sweep with an explicit,
  justified allowlist.

**Non-Goals:**

- Migrating other retired names (Codebase Memory, harness-lsp) beyond the code
  lines they share with Graphify handling.
- Editing OpenSpec archives, the dated `rust-migration-map.json` inventory,
  decision records or git history - they are historical records.
- Touching host-local data (the shared `graphifyy` package, saved graphs,
  user-held `graphify.json`); these stay outside the repository by design.
- Duplicating the transitional-layer removal owned by migrate-harness-to-rust
  8.2/9.2.

## Decisions

**D1. Delete the unreachable dependency-lifecycle surface instead of keeping
rollback parity.** The catalogue has no Graphify entry, so no spec reaches the
uv verification branch, identity mapping, npm exclusion or manifest read;
keeping them means maintaining dead code and test fixtures forever. Removed:
the `graphify_manifest` request/discovery plumbing, the uv verification and
shared-service manifest read, plan/fetch dispatch arms, the apply note, and the
`--graphify-manifest` CLI flag with its `PROGRAMDATA/OpenCodeWorkstation`
default. The flag removal is an accepted CLI break: no documented consumer
exists and it served only the retired tool. Alternative (keep parity for a
possible rollback) is rejected because the retirement deliberately deleted the
adapter sources, so parity has no consumer.

**D2. Keep the name-based retirement guard for owned registrations.** Dropping
`graphify` from the registration skip lists would make an update re-register a
Graphify entry recorded in pre-retirement host state, resurrecting a broken
server; dropping it from `all_names`/`refuse_conflicts` would stop refusing
name/ownership conflicts on an unowned same-name table. Both are correctness
regressions for hosts that have not updated since the retirement, in exchange
for removing three short lists. The guard is kept, covered by the existing
`codegraph_install`/`codegraph_consumers` assertions, and documented in the
retirement evidence with an explicit removal condition: it may be dropped once
no supported host state can predate the 2026-09-15 retirement (revisit at the
next major lifecycle change). Alternative (allowlisting desired names) is
rejected because it would drop foreign user registrations that the retention
path exists to preserve.

**D3. Drop the `bootstrap-graphify-pending.json` restart blocker.** Graphify
bootstrap can no longer start, so the marker can only be a stale file from a
pre-retirement version; keeping it in `unfinished_restart_blockers` would block
restart-policy changes indefinitely. The entry is removed; removal of a stale
file itself stays with the existing transitional activation cleanup
(`tools/activation.psm1`) until that layer is retired.

**D4. Make the specifications describe the current system.** Remove the
obsolete "Graphify lifecycle and repository selection" requirement, keep the
retirement behavior requirement (reworded to the reduced rollback surface:
shared package and saved state, no first-party adapter sources), drop the
named retired candidates from the discovery and reuse requirements, and
generalize the optional-candidate evaluation requirement because the Graphify
evaluation is complete. Specs must not carry a candidate that cannot be
selected.

**D5. Reconcile documentation and evidence, keep historical records.**
Documentation loses stale prerequisite/status text and the rollback claim that
contradicts the deleted proxy/update sources, but keeps a minimal retired-tool
note so users who still hold local Graphify state can understand its status.
`legacy-mcp-retirement.json` records the final action and the guard condition;
`rust-requirement-checks.json` updates its note and row. Archives, the dated
inventory and decision records are untouched.

**D6. Split ownership with migrate-harness-to-rust.** Transitional
`tools/`-layer references are not edited here; that layer's removal is already
owned by tasks 8.2/9.2. This change records the residue list and its owner in
the retirement evidence and re-runs the reference sweep after the cutover, so
the allowlist shrinks to historical records plus the D2 guard.

**D7. Verification.** Affected native Cargo suites, `codex-harness
ownership-check --source`, and the `dependencies` CLI help are checked
directly. The reference sweep is a recorded `git grep -i graphify` whose every
remaining hit must fall into: OpenSpec archives, the dated inventory, decision
records, retirement evidence, the D2 guard and its tests, the minimal
documentation note, and - until the cutover - the transitional layer with its
owning change named.

## Risks / Trade-offs

- [A host still carries a pre-retirement Graphify registration] → D2 keeps the
  guard until the recorded removal condition is met; guard behavior stays
  covered by `codegraph_install`/`codegraph_consumers` tests.
- [Test coverage lost when Graphify fixtures are removed] → re-point fixtures
  to retained ids and keep normalized-name and rejection coverage; delete only
  feature-specific tests (the manifest-secret case), not the generic ones.
- [Docs/evidence drift after the sweep] → the sweep is part of the tasks and
  its result is recorded, so later drift is visible as a new reference.
- [The change cannot close before migrate-harness-to-rust 9.2 lands] → the
  residue is explicit in the retirement record with its owning change; the
  final sweep task re-runs at that point. This is deliberate: duplicating the
  transitional removal would waste work and risk conflicts with an active
  migration.

## Migration Plan

1. Land (or confirm landed) the in-flight 8.2 deletion batch that removes the
   graphify-named sources; this change does not re-delete them.
2. Remove the D1 surface and update tests; keep the D2 guard; apply D3.
3. Update specifications (this change's deltas), documentation and evidence.
4. Run the D7 checks and record the sweep result in the retirement evidence.
5. After the migrate-harness-to-rust cutover, re-run the sweep, update the
   residue list in the retirement evidence, and confirm only historical records
   and the D2 guard remain.

Rollback: revert the change's commits and re-run the affected Cargo suites; the
retirement guard (D2) is unchanged throughout, so host registration state is
never put at risk by a rollback.
