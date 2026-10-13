## 1. Split and verify

- [x] 1.1 Extract the shared fixtures/helpers of `improvement_comparison.rs` into a module both new targets include without duplication.
- [x] 1.2 Split the tests at the measured boundary, keeping every test name and body unchanged; both targets compile and each serialized run finishes under 1500 s on this machine.
- [x] 1.3 Update `.github/workflows/windows-installed-integration.yml` and the `ci_workflow` equality contract to the new target set; run both targets end-to-end through `codex-harness heavy` and record actual durations.

## Evidence (recorded 2026-10-13)

- Coverage parity: the 50 pre-split test bodies appear exactly once across the two targets (`improvement_comparison_a` 29, `improvement_comparison_b` 21) with 0 duplicates, 0 missing and 0 body mismatches; the 7 calibration tests moved across the boundary are byte-identical.
- Serialized end-to-end runs through `codex-harness heavy` (default account, post-fix tree): `improvement_comparison_a` finished 21 passed / 8 failed in 1294.93 s; `improvement_comparison_b` finished 19 passed / 2 failed in 1099.39 s. Both are under the 1500 s budget; the 36-test intermediate a target measured 1671.50 s, which motivated the rebalance.
- Failures are environment-rooted on this headless host (the native conversation surface cannot open; dispatch is refused before submission) and are preserved without weakening assertions: a = `owned_heavy_contention_reaches_the_native_report` + 7 mutex-poison cascades; b = `recorded_background_contention_withholds_the_unsupported_verdict` + 1 cascade. The failure set is unchanged by the split; no retry-to-green.
- `ci_workflow` equality contract: 5/5 passed. Formatting check clean. Strict OpenSpec validation re-run after this edit.
