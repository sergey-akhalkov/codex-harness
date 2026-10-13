## Why

Every full run of the `improvement_comparison` test target exceeds the shared heavy-command deadline (1800 s). This week alone the deadline forced manual split-and-rerun handling in at least six acceptance runs (board codex-harness-4q4.63 records the phase-1 sweep occurrence; slices 4q4.48, 4q4.51, 4q4.55, 4q4.58 and 4q4.61 each hit it). Each overrun costs a failed wrapper run, a manual remainder invocation and a repeated queue wait, and it hides per-test timing behind a truncated stream.

## Measurement

Observed problem: a single `cargo test --locked -p codex-harness --test improvement_comparison -- --test-threads=1` run cannot finish inside one heavy command (deadline 1800 s; observed runs terminate mid-suite around test 28-31 of 40+). Investigation scope: per-target compile+run cost of the comparison suite at the current base revision, on this machine, under the shared heavy admission. Measurement question: does splitting the target into deadline-sized targets remove the overrun while preserving total coverage and CI parity? Workload: the existing full comparison suite execution linked from this change. Evidence: board codex-harness-4q4.63 sweep record and the named slice records above. Limits: one machine, one week of observed runs, shared-resource admission variance. The target count was re-derived once from measured green timings (design.md, Measured boundary): the normative requirement is "more than one" target, and the current design uses three.

## What Changes

- Split `crates/codex-harness/tests/improvement_comparison.rs` into three test targets (`improvement_comparison_a`/`_b`/`_c` sharing one common fixture module) so each target's serialized run fits the 1800 s heavy deadline with margin.
- Keep every existing test exactly once; no coverage is dropped or duplicated.
- Update `.github/workflows/windows-installed-integration.yml` and the `ci_workflow` equality contract for the new target set.

## Impact

The packaging changes tests and CI. Exercising the full coverage also required separately recorded corrections to existing window readiness, terminal placement and blocked-comparison handling; their runtime delivery remains with the existing harness owners. The change is the candidate treatment A of the improvement-loop comparison recorded on its Beads card. The complete native runs verify the deadline and coverage requirements; the broader paired workload comparison remains open and is not claimed from the split alone.
