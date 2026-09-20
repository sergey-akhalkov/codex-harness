## 1. Pack foundation in the adapter

- [ ] 1.1 Add the pure-Rust `sha2` dependency to `tools/rtk-adapter/Cargo.toml` and verify `cargo check -p rtk-adapter` succeeds with no new build warnings
- [ ] 1.2 Implement the `pack/` storage area (handle-named files plus atomic `index.json` with provenance, digest, totals) under the existing RTK home resolution, and verify unit-level behavior through the adapter entry points
- [ ] 1.3 Mint handles on the compressed path where `save_raw` succeeds: copy retained bytes to `pack/`, apply oldest-first retention (64 entries / 128 MiB) with orphan cleanup, and emit the pack footer line only on success, falling back to today's exact output on any pack failure; verify compact output still ends with the unchanged `[rtk raw: <path>]` locator

## 2. Recall subcommand

- [ ] 2.1 Implement `harness-rtk.exe recall <handle> [--offset N] [--limit M]` with 1-based lines, defaults (offset 1, limit 200), clamps (limit 2000, 256 KiB emitted with truncation marker), provenance header, numbered slice and next-window hint; verify a fixture recall returns the exact requested window without rerunning the source command
- [ ] 2.2 Implement fail-closed exit semantics (0 success, 2 unknown/evicted with rerun/raw remedy, 3 digest mismatch with no content, 1 usage/I/O) and verify each path with fixture archives, including a corrupted-content case

## 3. Acceptance suite

- [ ] 3.1 Extend `crates/codex-harness/tests/rtk_adapter.rs` with pack emission, no-handle bypass (disable, unsupported, filter failure, oversize, terminal stdout), exact-window, clamping, eviction and session-end-survival tests; verify `cargo test -p codex-harness --test rtk_adapter` passes
- [ ] 3.2 Add a byte-accounting fixture comparing footer overhead and bounded recall output against whole-file raw re-read, and record the measured numbers in the change evidence without token or quota claims

## 4. Documentation and delivery

- [ ] 4.1 Update `docs/token-workflow.md` and `.agents/skills/token-efficient-workflow/references/tool-results.md` to teach bounded recall before whole-file raw reads, with the NVlabs/SoL-Pi inspiration attribution and honest retention limits; verify local links and factual consistency with implemented behavior
- [ ] 4.2 Rebuild and install the adapter through the existing `--token-workflow-only` lifecycle and verify handle emission and paged recall from a consumer session outside this checkout, including `check` passing