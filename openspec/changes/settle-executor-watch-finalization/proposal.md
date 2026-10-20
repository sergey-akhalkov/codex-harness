## Why

The historical `executor watch` defect returned success before the host saved
its exit status and final message. Current source already contains the fix
(`e2395f5`) and the regression
`watch_waits_for_host_finalization_after_turn_completion`. This change now owns
reconciliation and remaining acceptance, not a new implementation from an
assumed failing current HEAD. Source presence does not prove installed behavior.

## Evidence and hypothesis

The inspectable evidence is `crates/codex-harness/src/executor_cli.rs`
(`completion_awaiting_host`, `watch`, `settled_watch_outcome`) and
`crates/codex-harness/tests/executor_observation.rs`. Hypothesis: correlating
success with retained host finalization eliminates premature successful returns
in the controlled transition without losing timeout, failure or reply semantics.
The primary metric is premature-success count; the required change is a failing
historical control to zero such successes under identical transitions. No
productivity or frequency estimate is inferred from this regression.

## What Changes

- Reuse and verify the existing wait for host finalization; implement only a demonstrated remaining gap.
- Return successful completion only with the current run's finalized outcome
  and retained result; report an actual output defect through the existing
  lifecycle contract.
- Preserve bounded waiting, original failures, reply holds, addressing,
  text/JSON output and the rule that timeout does not stop the executor.
- Verify the race through the real watch entry point with a controlled receipt
  transition and an independent check outside the implementation's write scope.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `lead-agent-orchestration`: clarify completion readiness between the native
  turn ending and the host persisting its final outcome.

## Impact

The implementation belongs to the existing observation/watch path in
`crates/codex-harness/src/executor_cli.rs`, with native coverage in
`crates/codex-harness/tests/executor_observation.rs` and operating guidance in
`docs/agent-delegation.md`. It adds no model requests, dispatcher, dependency or
public command. This real defect is also the specified workload for a separate
improvement experiment; fixing it does not by itself prove an efficiency gain
or authorize adoption of that experiment's treatment.
