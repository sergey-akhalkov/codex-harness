## Context

See [proposal.md](proposal.md). The suite is one integration target with 40+ tests; several real-control tests run 60-250 s each serialized.

## Decision

Split at a stable module boundary into two targets sharing fixture code through a `common` module (or `#[path]` include), minimizing diff churn. Choose the boundary so each target's observed serialized duration stays under 1500 s (deadline minus observed admission variance margin). CI and the `ci_workflow` equality contract list exactly the resulting target set.

## Measured boundary (recorded 2026-10-13)

The intermediate 36-test a target finished in 1671.50 s, over the 1500 s budget, so the 7 calibration tests moved across the boundary (bodies byte-identical). The rebalanced targets then measured 1294.93 s (a) and 1099.39 s (b), serialized through `codex-harness heavy`. These are retained failed-run measurements: a passed 21 tests and failed 8, while b passed 19 and failed 2, including mutex-poison cascades that did not exercise their assertions. They do not establish the duration or acceptance of complete green targets.

A complete-green attempt on 2026-10-13 (readiness fix 465691e, same heavy account) was terminated by the 1800 s command deadline after 28 of the 29 a tests passed, the last in flight; all seven former poison cascades executed for real and passed. The failed-run numbers above therefore understate the green cost: under that load the complete green a run did not finish inside the 1800 s command deadline (test time alone exceeded 1769 s), so it cannot meet the 1500 s budget. Re-derive the boundary from per-target green timings; if the two-target green total stays >=3000 s, <1500 s per target is infeasible for two targets and the split shape needs a lead decision.

No measured run of the pre-split single target is retained from this session; that baseline remains open and no history is reconstructed.

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

- Split boundary hides a cross-module shared-state dependency -> both targets still run in CI and the full local set; the workload oracle requires the complete set green.
- CI target list drift -> the equality contract test fails on any mismatch.

## Migration Plan

1. Extract shared fixtures into a module included by both targets.
2. Move tests across the boundary; keep names identical.
3. Update the workflow and equality contract; run both targets end-to-end under heavy.

## Open Questions

None; the boundary choice is measured, not debated. The 2026-10-13 green attempt falsified the recorded boundary's margin, so the boundary is being re-derived from per-target green timings; see Measured boundary.
