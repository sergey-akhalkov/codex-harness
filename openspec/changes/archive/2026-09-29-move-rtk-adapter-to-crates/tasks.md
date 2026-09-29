## 1. Move and workspace wiring

- [x] 1.1 `git mv tools/rtk-adapter crates/harness-rtk` and update the workspace
  member in the root `Cargo.toml`; verify `cargo build --locked -p harness-rtk`
  succeeds from the new path and `tools/` no longer exists.
- [x] 1.2 Update the required checkout paths in
  `crates/harness-core/src/build_identity.rs` (drop the `tools/rtk-adapter`
  entry) and `SOURCE_FILES` in
  `crates/harness-core/src/token_workflow_lifecycle.rs`; verify the owning
  module tests pass (`cargo test --locked -p harness-core build_identity` and
  the token-workflow lifecycle tests).

## 2. Ownership boundary and fixtures

- [x] 2.1 Simplify `rust_source_roots` in `docs/evidence/executable-ownership.json`
  to `["crates"]` and update `crates/codex-harness/tests/executable_ownership.rs`;
  verify `cargo test --locked -p codex-harness --test executable_ownership` and
  `codex-harness ownership-check` pass.
- [x] 2.2 Update all synthetic-checkout fixtures staging `tools/rtk-adapter`
  across `crates/codex-harness/tests/*` and `crates/harness-core/src/*`; verify
  the affected test targets pass (at minimum `native_build`, `heavy_command`,
  `mcp_cli`, `installation_state`, `native_launcher`, `migration_baseline`,
  `executor_succession`, `manager_delivery`, `task_control_launch`).

## 3. Guides and residual references

- [x] 3.1 Update the `tools/rtk-adapter` reference in `docs/token-workflow.md`
  to `crates/harness-rtk`; verify the guide's build-identity description still
  matches the code.
- [x] 3.2 Run a repo-wide search for `tools[/\]rtk-adapter` excluding
  `target*`; verify only OpenSpec archives and `docs/evidence/rust-migration*`
  historical records remain.

## 4. Acceptance

- [x] 4.1 Full workspace checks from the new layout:
  `cargo build --workspace --locked`, `cargo clippy --workspace --all-targets
  --locked -- -D warnings`, `cargo test --workspace --locked -- --test-threads=1`;
  verify clean results with no path-related failures.
- [x] 4.2 Exercise the installation lifecycle once outside the checkout through
  the `codex-harness` entry point so the changed source identity triggers the
  expected rebuild/relink; verify `harness/bin/harness-rtk.exe` resolves from
  the new layout and one real `harness-rtk.exe` exec/recall round trip works.
