# Delivered-build launch authority notes

## Verification (tasks 4.1-4.2)

Delivered builds: `5f159c85141507fc` (cargo-run bootstrap after the six-binary
set could not be finalized by the older installed manager), then `2da05d5581cff522`
through the normal lifecycle once the new manager owned the flow.

- Deliberate source-ahead probe (4.2): after a one-line source edit, `check
  --build` exits 1 with `status: source-stale`, `runtime_allowed: true` and the
  action naming continued launches on the delivered build plus explicit deploy;
  the gated `executor pool` command runs on the same stale source. Probe edit
  reverted afterwards.
- Launcher classification: `native_launcher` keeps harness overrides when the
  recorded binaries verify (`stale_source_keeps_delivered_overrides_instead_of_degrading`);
  missing source, malformed shared config and damaged companion binaries keep
  the fail-open upstream fallback with its notice
  (`missing_stale_source_and_shared_toml_fail_open_to_verified_upstream`).
- Build reuse/selection: a candidate is reused or delivered only for an exact
  healthy source match, so a stale candidate can never silently stand in for
  newer source (`cli_build_reuse_source_staleness_integrity_and_failed_update`).
- Suites: build_identity unit tests, native_launcher integration (11/11),
  native_build (including `real_four_binary_producer...`), mcp_cli (4/4),
  codegraph_install (20/20), migration_baseline with
  `HARNESS_ACCEPTANCE_POWERSHELL` set to the owner-designated pwsh (1/1),
  `cargo clippy --workspace --all-targets --locked -- -D warnings` clean,
  `cargo fmt --all -- --check` clean, harness-source-check with no finding in
  files touched by this change, ownership-check clean.

## Unrelated failures observed in full serial workspace runs

- `native_launcher::run_reaps_detached_session_helper...`: pre-existing;
  reproduced on pristine main before this change.
- migration_baseline/tui desktop-pwsh tests: need
  `HARNESS_ACCEPTANCE_POWERSHELL` on machines without the MSI desktop install;
  pass with the documented variable.
- codegraph_mcp live-lease tests: interfere with a concurrently running
  CodeGraph worker; pass in isolation.
