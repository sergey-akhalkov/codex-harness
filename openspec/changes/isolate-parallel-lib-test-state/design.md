## Context

See [proposal.md](proposal.md). The lib suite is large (900+ tests); serializing everything would hide the defect rather than fix it.

## Decision

Reproduce first: run the recorded failing filter in parallel until a failure recurs, capture the failing pair, then isolate the demonstrated shared state at its owner (the test or helper that leaks it), not by adding global mutexes. Keep the fix minimal and per-owner; a suite-wide serialization is rejected as hiding the mechanism.

## Experiment selection

Method: paired-implementations
Claim: local-operation
Outcome: workload acceptance wall time
Rationale: the workload is a real defect fix whose acceptance is real suite execution, so measuring its completion time under both harnesses requires implementing it independently in each arm
Controls: identical frozen task inputs and oracle, one attempt per arm, serialized arms, nuisance plan frozen before results
Projection: none claimed for the harness arms, the comparison measures whether candidate A changes workload completion time
Baseline: the unsplit baseline harness at the same base revision
Stopping: one predeclared matched pair, a failed oracle fails the arm without retry

## Risks

- Load-sensitive reproduction may not recur during investigation -> the change then stays open with recorded limits; it must not be closed by an unreproduced assumption.
- Acceptance flakiness misread as a fix -> the oracle requires repeated green parallel runs, not a single pass.

## Open Questions

None at planning time; the investigation owns identification of the concrete shared state.
