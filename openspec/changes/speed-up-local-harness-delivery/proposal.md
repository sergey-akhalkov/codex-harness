## Why

Everyday native delivery currently uses the same cold release path as public
publication: a fresh compiler target, fat LTO, one Cargo job and a second Cargo
phase. The retained build of 2026-10-22 spent 21m 03s in the first phase and
10m 58s in the second; the second phase also recompiled `harness-core` and
`codex-harness` before linking the delivery set. A local fast build with LTO
off, 16 codegen units, one Cargo invocation and two jobs completed cold in
5m 27s. With warm dependencies, rebuilding the `codex-harness` package and all
seven delivery programs took 1m 35s at two jobs and 1m 08s at four jobs. An
unchanged protected source target is already reused without Cargo.

The owner wants simple Rust fixes checked and deployed without waiting tens of
minutes. The release profile and fresh-target integrity rule remain appropriate
for publication, but they are excessive for an explicit local iteration path.
The existing source identity also invalidates a release build for any test or
fixture change even when no compiled production input changed.

## What Changes

- Add an explicit local fast delivery mode, selected as `deploy --fast`, backed
  by a maintained `delivery-fast` Cargo profile with LTO off,
  `codegen-units = 16`, stripping and release optimizations. The ordinary
  `deploy` and the public release profile keep fat LTO and fresh-target
  publication; fast mode is visible in the receipt and never silently replaces
  the release path.
- Give fast mode a persistent, installation-owned compiler target keyed by the
  toolchain, target, profile, lockfile, manifests, Cargo host configuration and
  other build-affecting inputs. Before reuse, compare the current source
  identity with the recorded build inputs; a changed compiled input with an
  untrustworthy timestamp, foreign ownership or ambiguous state SHALL force a
  scoped rebuild or a fresh target rather than trusting Cargo's mtime cache.
  Final binaries are still copied into an immutable integrity-verified build.
- Establish one source-owned delivery-program declaration usable before the
  manager is compiled, and validate it against the manager's runtime delivery
  answer. Fast mode then builds the complete delivery set in one Cargo
  invocation; legacy or unavailable metadata retains the existing two-phase
  source-owned query as a compatibility fallback.
- Record compiled-input evidence separately from the full checkout identity.
  The full identity remains publication provenance; reuse of a fast candidate
  additionally requires every compiled input and build-affecting manifest,
  toolchain and configuration input to match. Test-only changes can reuse a
  verified candidate when no compiled input changed, while a fixture compiled
  into production code still invalidates it.
- Measure fat-LTO Cargo job counts under the existing heavy-command budget
  before changing the release default. Keep every test target, ignored-test
  condition and required delivery check; evaluate test execution scheduling
  only through its own measured, coverage-preserving route.
- Document the fast/local versus release/publication choice in the installation
  and deployment guides and in the existing `cargo-fast` and `harness-deploy`
  skills. Thin LTO remains unadopted unless a new matched measurement reverses
  the prior slower-and-larger result.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `rust-build-efficiency`: fast local compiler profile, guarded persistent
  target reuse, one-invocation delivery selection, compiled-input identity and
  measured concurrency/test feedback.
- `rust-native-harness`: profile-aware native candidate identity, handoff,
  finalization and installed delivery while the fat-LTO release path and
  source/freshness guarantees remain authoritative.
- `linked-global-kit`: explicit fast local deployment, truthful receipts and
  preservation of ordinary release deployment and recovery.

## Impact

Affected owners include `crates/harness-core/src/native_build.rs`,
`native_handoff.rs`, `build_identity.rs`, `core_install.rs`,
`crates/codex-harness/src/install_cli.rs`, the root `Cargo.toml`, source-owned
delivery metadata, deployment documentation and the existing global skills.
The change adds no new background service or first-party scripting runtime.
Public release publication, installation ownership, immutable build integrity,
runtime verification and recovery remain required.
