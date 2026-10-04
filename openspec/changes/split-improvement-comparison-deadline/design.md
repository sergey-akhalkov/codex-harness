## Context

See [proposal.md](proposal.md). The suite is one integration target with 40+ tests; several real-control tests run 60-250 s each serialized.

## Decision

Split at a stable module boundary into two targets sharing fixture code through a `common` module (or `#[path]` include), minimizing diff churn. Choose the boundary so each target's observed serialized duration stays under 1500 s (deadline minus observed admission variance margin). CI and the `ci_workflow` equality contract list exactly the resulting target set.

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

None; the boundary choice is measured, not debated.
