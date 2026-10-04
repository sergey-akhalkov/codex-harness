## Why

Every full run of the `improvement_comparison` test target exceeds the shared heavy-command deadline (1800 s). This week alone the deadline forced manual split-and-rerun handling in at least six acceptance runs (board codex-harness-4q4.63 records the phase-1 sweep occurrence; slices 4q4.48, 4q4.51, 4q4.55, 4q4.58 and 4q4.61 each hit it). Each overrun costs a failed wrapper run, a manual remainder invocation and a repeated queue wait, and it hides per-test timing behind a truncated stream.

## Measurement

Observed problem: a single `cargo test --locked -p codex-harness --test improvement_comparison -- --test-threads=1` run cannot finish inside one heavy command (deadline 1800 s; observed runs terminate mid-suite around test 28-31 of 40+). Investigation scope: per-target compile+run cost of the comparison suite at the current base revision, on this machine, under the shared heavy admission. Measurement question: does splitting the target into two deadline-sized targets remove the overrun while preserving total coverage and CI parity? Workload: the existing full comparison suite execution linked from this change. Evidence: board codex-harness-4q4.63 sweep record and the named slice records above. Limits: one machine, one week of observed runs, shared-resource admission variance.

## What Changes

- Split `crates/codex-harness/tests/improvement_comparison.rs` into two test targets (for example `improvement_comparison_a`/`_b` or a shared module plus two thin targets) so each target's serialized run fits the 1800 s heavy deadline with margin.
- Keep every existing test exactly once; no coverage is dropped or duplicated.
- Update `.github/workflows/windows-installed-integration.yml` and the `ci_workflow` equality contract for the new target set.

## Impact

Tests and CI only; no production behavior changes. The change is the candidate treatment A of the improvement-loop comparison recorded on its Beads card; its benefit is measured on the linked workload, not claimed from the split alone.
