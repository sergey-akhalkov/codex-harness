## Why

`executor watch` can return success when the native turn has completed but the
host has not yet saved its exit status and final message. A lead then receives
an incomplete result and must read the same receipt again before reviewing the
work or releasing its slot. This race was observed during actual executor runs.

## What Changes

- Keep watching a completion observation that still awaits host finalization.
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
