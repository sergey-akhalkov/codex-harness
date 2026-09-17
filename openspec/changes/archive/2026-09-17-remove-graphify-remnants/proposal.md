## Why

Graphify was retired from the managed MCP selection on 2026-09-15, and its
first-party adapters and transitional tests are being deleted, but the working
tree still carries live-looking remnants: a normative requirement ("Graphify
lifecycle and repository selection") that contradicts the retirement,
dead-but-maintained dependency branches and a `--graphify-manifest` CLI flag,
a private consumer manifest path in first-party code, and documentation that
promises a rollback route whose sources no longer exist. These leftovers
mislead users and future changes, and they keep maintenance surface on code
paths that can no longer run because the dependency catalogue no longer
contains Graphify.

## What Changes

- Remove Graphify-specific behavior from the native dependency lifecycle:
  discovery branches (`graphifyy` identity, `graphify-mcp` / `graphify.serve`
  verification, the unrelated `@dreamtree-org/graphify` npm exclusion, and the
  shared-service manifest read), plan/fetch dispatch arms, the apply note, and
  the `--graphify-manifest` CLI flag with its
  `PROGRAMDATA/OpenCodeWorkstation/manifest.json` default (**BREAKING** for
  that flag only; it served the retired tool).
- Re-point test fixtures that used Graphify as a sample id; delete the
  manifest-secret test together with the removed feature.
- Keep one bounded retirement guard: the owned-registration skip/conflict
  lists and retired projections that let Update remove a Graphify registration
  recorded by pre-retirement versions, together with their tests. Drop the
  `bootstrap-graphify-pending.json` restart blocker, because a retired tool can
  no longer be mid-bootstrap and a stale marker must not block restart-policy
  changes. Record the guard's removal condition in the retirement evidence.
- Update current specifications: remove the obsolete "Graphify lifecycle and
  repository selection" requirement, replace "Serena, Graphify, Nuphus"
  wording, and generalize the discovery, command-name-collision and
  optional-candidate evaluation scenarios so no retired candidate is named.
  Keep the retirement requirement, with rollback-surface wording reconciled to
  the deleted first-party adapters.
- Update documentation and evidence: fix stale prerequisite and status text,
  reconcile the `code-tools.md` retirement text with the removed proxy/update
  sources, and update the retirement record and requirement map.
- Transitional `tools/code-tools/*.py` and `tools/activation.psm1` references
  disappear with migrate-harness-to-rust tasks 8.2 and 9.2; this change
  verifies their absence after the cutover instead of editing files owned by
  that removal. It also assumes the in-flight 8.2 batch that deletes the
  graphify-named sources is committed, and verifies that HEAD no longer
  carries graphify-named files.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `global-code-tools`: remove the "Graphify lifecycle and repository
  selection" requirement and update retained-server and retirement statements.
- `tool-dependency-lifecycle`: generalize the discovery and command-name
  collision scenarios that still name `graphifyy`.
- `bounded-tool-resources`: remove Graphify from the safe-reuse requirement.
- `mcp-tool-selection`: generalize the optional-candidate evaluation
  requirement; the Graphify evaluation is complete and must not remain a named
  obligation.

## Impact

- `crates/harness-core`: `dependency_discovery.rs`, `dependency_plan.rs`,
  `dependency_fetch.rs`, `dependency_apply.rs`, `codegraph_integration.rs`,
  `subscription_lifecycle.rs` (one blocker entry), `codegraph_registration.rs`
  (guard retained and documented).
- `crates/codex-harness`: `dependency_cli.rs`, `dependency_apply_cli.rs`, and
  tests (`dependency_discovery.rs`, `dependency_plan.rs`,
  `dependency_npm_install.rs`, `installed_tool_workflows.rs`;
  `codegraph_install.rs` and `codegraph_consumers.rs` keep their retirement
  assertions as guard coverage).
- `docs/`: `installation.md`, `rust-native.md`, `code-tools.md`;
  `docs/evidence/legacy-mcp-retirement.json`,
  `docs/evidence/rust-requirement-checks.json`.
- Not changed: `global/code-tools.json` (already without Graphify); OpenSpec
  archives, the dated `rust-migration-map.json` inventory and decision records
  (historical); host-local shared packages and saved graphs (outside the
  repository); git history.
