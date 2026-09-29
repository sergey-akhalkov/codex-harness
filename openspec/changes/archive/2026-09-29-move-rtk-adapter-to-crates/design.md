## Context

`tools/rtk-adapter` is already the Cargo package `harness-rtk`, a workspace
member with its own small dependency set (`sha2`, `serde_json`), integration
tests, build identity and installation lifecycle into `harness/bin/harness-rtk.exe`.
See `proposal.md` for motivation. Two production code sites pin the source
directory: the required checkout paths in `harness-core/src/build_identity.rs`
and `SOURCE_FILES` in `harness-core/src/token_workflow_lifecycle.rs`. Roughly 20
test-fixture references across ~12 files stage the same layout in synthetic
checkouts.

## Goals / Non-Goals

**Goals:**

- One first-party Rust source root (`crates/`) with directory names matching
  package names.
- All path contracts, fixtures and current guides consistent in one change.
- Prove unchanged behavior through the real entry points (workspace build/tests,
  ownership check, one lifecycle reinstall).

**Non-Goals:**

- No package rename, no dependency changes, no behavior or CLI changes.
- No rewrite of OpenSpec archives or dated migration evidence.
- No restructure of other crates.

## Decisions

- **Target directory `crates/harness-rtk`** (package name unchanged).
  Alternative: `crates/rtk-adapter` keeps the historical directory name but
  preserves the directory/package mismatch this change removes. Alternative:
  merging as an extra bin into `codex-harness` couples the adapter's build and
  component identity to the manager's much larger dependency tree; the separate
  small crate is deliberate.
- **`git mv`, not copy/delete** — rename detection keeps `git log --follow`
  usable. The move is a single atomic commit with all path-contract updates.
- **Drop `tools/rtk-adapter` from build-identity required paths** rather than
  keeping a compatibility entry: after the move `crates` already hashes the
  crate; a stale required entry would fail on the missing path. The identity
  value changes once — the lifecycle's normal mismatch-and-rebuild path, not an
  error state.
- **`skip_specs: true`** — no current spec names the source directory; adapter
  and lifecycle behavior are unchanged, so no delta spec is invented.
- **Update fixtures in the same commit as the production lists.** The
  `SOURCE_FILES` staging exists exactly so fixtures cannot drift from the
  checkout; splitting the update across commits would turn that guard into a
  false failure.

## Risks / Trade-offs

- [A hardcoded path is missed and breaks build/tests] -> repo-wide search for
  `tools[/\]rtk-adapter` must leave only dated archives/evidence; full
  `cargo test --workspace --locked` runs as acceptance.
- [Fixture drift between production lists and staged synthetic checkouts] ->
  same-commit update; the staging tests fail loudly on drift by design.
- [Installed binaries linked from the old build identity] -> expected one-time
  rebuild/relink through the existing lifecycle; acceptance exercises one real
  reinstall outside the checkout instead of assuming it.
- [Cargo.lock staleness after the move] -> member entries are keyed by package,
  but `--locked` workspace build still verifies and catches any drift.

## Migration Plan

1. `git mv tools/rtk-adapter crates/harness-rtk`; update the workspace member in
   the root `Cargo.toml`.
2. Update `build_identity.rs` required paths and
   `token_workflow_lifecycle.rs` `SOURCE_FILES`.
3. Simplify `executable-ownership.json` `rust_source_roots` to `["crates"]` and
   its test fixture.
4. Update synthetic-checkout fixtures and `docs/token-workflow.md`.
5. Verify: workspace build/tests, `ownership-check`, residual-path search, one
   lifecycle reinstall with `harness-rtk.exe` available from the new layout.

Rollback: revert the single commit; the lifecycle rebuilds from the restored
layout on its next check.

## Open Questions

None.
