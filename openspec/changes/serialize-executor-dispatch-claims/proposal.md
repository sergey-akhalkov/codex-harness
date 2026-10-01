## Why

Concurrent executor dispatches have been observed claiming the same pool slot
and replacing its pending assignment receipt before the first host reads it.
Both callers can report acceptance while only one assignment survives. The
current claim path can reclaim a clean slot whose owner has no live host yet,
including the interval while that owner's dispatch is still preparing startup.

## What Changes

- Preserve exclusive ownership from slot selection through the verified host
  handoff, including the interval before a host lease exists.
- Prevent a competing dispatch or recovery path from replacing an in-flight
  claim, its assignment, or its receipt.
- Retain bounded failure and recovery through the existing pool/process owners,
  with independent executor conversations continuing concurrently after startup.
- Verify competing native dispatches and interrupted startup using owned,
  model-free processes and retained assignments.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `lead-agent-orchestration`: make pool ownership continuous across dispatch
  preparation and the live-host handoff.

## Impact

The existing owners are `crates/harness-core/src/task_worktree.rs` and
`crates/codex-harness/src/executor_cli.rs`, their Rust checks and the pool guidance
in `docs/agent-delegation.md`. No additional pool, tracker, model call or dependency
is required. This is a proposed real workload for the sequential improvement
experiment; its planning does not establish an implementation or measured benefit.
