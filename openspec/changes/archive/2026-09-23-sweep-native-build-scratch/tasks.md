# Tasks: sweep-native-build-scratch

## 1. Scratch reclamation

- [x] 1.1 Add a guarded sweep helper to `crates/harness-core/src/native_build.rs`
  that removes only stale `hcb-`/`hcc-`/`hca-` prefixed ordinary directories
  directly under the process temp root; verify with unit tests covering stale
  removal, fresh retention, foreign-entry preservation and nested-file
  deletion
- [x] 1.2 Invoke the sweep from `prepare()` after acquiring the owned-state
  lock and before build reuse lookup; verify the call site by reading the
  changed flow and by the unit suite

## 2. Documentation

- [x] 2.1 Update `docs/rust-native.md` to document stale scratch reclamation
  beside the fresh-short-path compilation rule and the closing of retained
  verification/soak targets; verify the guide states both owners without a
  second guide

## 3. Checks

- [x] 3.1 Run `cargo fmt --all`, `cargo clippy -p harness-core --all-targets
  --locked --jobs 1 -- -D warnings` and `cargo test -p harness-core
  native_build --locked`; record results
- [x] 3.2 Run `openspec validate sweep-native-build-scratch --strict` and
  resolve findings

## 4. Delivery and verification

- [x] 4.1 Deploy the fixed manager through the explicit update lifecycle from
  this checkout; verify with `codex-harness check` that the delivered build is
  current and healthy
- [x] 4.2 Verify the installed manager reclaims seeded scratch: a stale
  prefixed directory older than the gate is removed and a fresh prefixed
  directory survives an explicit lifecycle invocation
