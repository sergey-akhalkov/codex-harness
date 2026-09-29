## Why

`tools/` is a vestige of the pre-Rust script era and now contains exactly one
occupant: the `harness-rtk` crate, which is already a Cargo workspace member and
architecturally identical to the crates under `crates/`. Keeping a second
first-party Rust source root costs concrete exceptions — a separate
`rust_source_roots` entry, an extra required path in build identity, a workspace
member outside `crates/` — and a directory/package name mismatch
(`tools/rtk-adapter` vs package `harness-rtk`).

## What Changes

- Move `tools/rtk-adapter` to `crates/harness-rtk`; the package name
  `harness-rtk`, binary name `harness-rtk.exe`, commands, hook contract and
  runtime behavior stay unchanged. The directory name then matches the package
  name, as for every other crate.
- Remove the now-empty `tools/` directory from the checkout.
- Update the hardcoded source-path contracts:
  - workspace member in the root `Cargo.toml`;
  - required checkout paths in `harness-core/src/build_identity.rs`
    (`tools/rtk-adapter` entry drops; `crates` already covers the new location);
  - `SOURCE_FILES` in `harness-core/src/token_workflow_lifecycle.rs`.
- Simplify `docs/evidence/executable-ownership.json` `rust_source_roots` to
  `["crates"]` and its test fixture accordingly.
- Update synthetic-checkout fixtures that stage `tools/rtk-adapter` in tests.
- Update the single current-guide reference (`docs/token-workflow.md`).
- Accepted one-time effect: the token-workflow component's source identity
  changes once, so its lifecycle rebuilds/relinks the installed binaries.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

None. No spec-level behavior changes: existing specs describe adapter and
lifecycle behavior, not the source directory. This is a pure refactor, so
`.openspec.yaml` sets `skip_specs: true`.

## Impact

- Root `Cargo.toml` (workspace member) and path-derived `Cargo.lock` entries.
- `crates/harness-core/src/build_identity.rs`,
  `crates/harness-core/src/token_workflow_lifecycle.rs`.
- `docs/evidence/executable-ownership.json` and
  `crates/codex-harness/tests/executable_ownership.rs`.
- Synthetic-checkout fixtures in `crates/codex-harness/tests/*` and
  `crates/harness-core/src/*` that stage `tools/rtk-adapter` (about 20
  occurrences across ~12 files).
- `docs/token-workflow.md` (one reference).
- Not touched: OpenSpec archives and `docs/evidence/rust-migration.md` — dated
  historical records of past states.
- Installed token-workflow component rebuilds once through its existing
  lifecycle; user-facing binaries and hooks keep working names.
