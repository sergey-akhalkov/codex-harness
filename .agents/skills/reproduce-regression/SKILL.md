---
name: reproduce-regression
description: Reproduce and reduce CLI, MCP, or subprocess regressions with controlled identities and failure-preserving checks. Use for a reported crash, hang, protocol failure, or intermittent process defect; routine verification without a concrete regression does not need this workflow.
---

# Reproduce a regression

Keep the original defect observable while making the case easier to diagnose and retain as a regression check.

## Pin the case

Record the original trigger and inputs, expected behavior and its contract source, observed failure, relevant environment/configuration, source revision and dirty inputs, runtime/build versions and actual entrypoint. Include generated assets, transport, timing or seed when they affect the failure. State allowed effects and a bounded attempt/elapsed-time budget before execution.

Identify the failing baseline, candidate and any known-good version or independent reference separately. Compare with controlled equivalent inputs only where the reference shares the contract. If a reference is missing, unavailable or incompatible, say so and use a stated specification/invariant with explicit limits; do not invent a known-good oracle or label the defect newly introduced without evidence.

## Reproduce, then reduce

Use the project's native test runtime and actual CLI/MCP/process entrypoint in owned isolated state. Establish a recognizable failure condition before editing or reducing. Preserve input, identity, invocation, output and outcome for each attempt, including non-reproductions and blocked setup. If the original failure cannot be reproduced, retain the attempted case and precise limitation; do not claim a fix.

Reduce one relevant dimension at a time, rerunning each proposed reduction against the original condition. Keep the last verified reproducer independently recoverable. A smaller case with another error, altered assertion, version drift or lost timing conditions is rejected. Read [reduction and attempt records](references/reduction.md) for wrong-failure and intermittent examples. Use differential tests or invariant perturbations only when a concrete hypothesis warrants them; no mandatory fuzzing or extra orchestration.

## Verify the result

Add an executable regression assertion to the project's existing checks when possible. Show the original failure on the pinned failing baseline and the intended behavior on the candidate, retaining applicable reference evidence. Preserve protocol/output, exit and timing assertions that define the contract; passing because the trigger stopped running is not a fix. Complete required project acceptance beyond the focused regression.

For process fixtures, read [process ownership and optional helper](references/process-fixtures.md). Prefer native helpers that already provide bounded execution, concurrent stream capture, observable readiness and owned cleanup. The linked Windows helper is optional when those capabilities are missing; do not add a framework merely to reproduce a defect.

Return the reproducer/check location, trigger and identities, oracle and reference limits, accepted/rejected reductions, all attempt outcomes, candidate checks and unresolved work. Report intermittent frequency with its denominator and conditions; a few passing retries do not establish a deterministic fix. Timeout, forced cleanup, infrastructure failure and assertion failure remain distinct from natural termination.
