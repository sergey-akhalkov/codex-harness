## Context

See [proposal.md](proposal.md). The comparison coverage grew beyond one command's deadline; several real-control tests take 60-250 s each serialized. The accepted candidate packaging contains 50 tests across three targets.

## Decision

Split at a stable module boundary into three deadline-sized targets (`improvement_comparison_a`, `_b`, `_c`) sharing fixture code through one `common` module, minimizing diff churn. Choose the boundary so each target's observed serialized duration stays under 1500 s (deadline minus observed admission variance margin). CI and the `ci_workflow` equality contract list exactly the resulting target set. The normative requirement is "more than one" target; the count moved from two to three after the earlier A boundary exceeded the command deadline (see Measured boundary).

## Measured boundary (recorded 2026-10-13)

The intermediate 36-test a target finished in 1671.50 s, over the 1500 s budget, so the 7 calibration tests moved across the boundary (bodies byte-identical). The rebalanced targets then measured 1294.93 s (a) and 1099.39 s (b), serialized through `codex-harness heavy`. These are retained failed-run measurements: a passed 21 tests and failed 8, while b passed 19 and failed 2, including mutex-poison cascades that did not exercise their assertions. They do not establish the duration or acceptance of complete green targets.

A complete-green attempt on 2026-10-13 (readiness fix 465691e, same heavy account) was terminated by the 1800 s command deadline after 28 of the 29 a tests passed, the last in flight; all seven former poison cascades executed for real and passed. The failed-run numbers above therefore understate the green cost: under that load the complete green a run did not finish inside the 1800 s command deadline (test time alone exceeded 1769 s), so it cannot meet the 1500 s budget. Re-derive the boundary from per-target green timings; if the two-target green total stays >=3000 s, <1500 s per target is infeasible for two targets and the split shape needs a lead decision.

No measured run of the pre-split single target is retained from this session; that baseline remains open and no history is reconstructed.

Boundary re-derivation (2026-10-13, lead decision): the old A boundary exceeded its deadline, so three targets were selected to provide measured headroom without another diagnostic full run. No complete two-target green total was measured. The suite was re-split into `improvement_comparison_a` (16 tests), `improvement_comparison_b` (21) and `improvement_comparison_c` (13 moved tests) with byte-identical bodies; coverage parity re-verified (50 = 16+21+13; 0 duplicates, 0 missing, 0 extra, 0 body mismatches) and the workflow plus `ci_workflow` equality contract list all three. Complete green runs recorded 2026-10-13 on candidate 730b599 (default heavy account, serialized): a finished 16 passed in 895.07 s, c 13 passed in 933.31 s and b 21 passed in 1201.51 s; every whole target stayed under the 1500 s budget.

## Experiment selection

Method: paired-implementations
Claim: local-operation
Outcome: workload acceptance wall time
Rationale: the per-command deadline overrun manifests deterministically whenever the workload agent verifies against the full comparison coverage, so a paired same-task implementation measures the effect on real agent work
Controls: unchanged frozen task inputs and oracle, one measured attempt per arm, serialized arms, nuisance plan frozen before results
Projection: removes one forced split plus one repeated setup and queue wait per full-suite verification
Baseline: the unsplit target at the same base revision
Stopping: one predeclared matched pair, failures retained without retry

## Risks

- Split boundary hides a cross-module shared-state dependency -> all three targets still run in CI and the full local set; the workload oracle requires the complete set green.
- CI target list drift -> the equality contract test fails on any mismatch.

## Migration Plan

1. Extract shared fixtures into a module included by all three targets.
2. Move tests across the boundary; keep names identical.
3. Update the workflow and equality contract; run all three targets end-to-end under heavy.

## Open Questions

The target boundary is verified by the three complete green runs. The predeclared paired-implementation experiment above remains open: these native checks do not establish an end-to-end agent-work cost comparison against the unsplit baseline. Failed historical runs remain evidence of their actual exercised scope.
