## Why

The historical claim-to-host gap could replace a pending assignment while both
callers reported acceptance. Current source already has startup reservations,
atomic admission and a shared task pool; `6610736` and the native reservation
regression are inspectable evidence of work already done. This change owns
verification and any demonstrated remaining gap, not reimplementation of the
old allocator. Zero checked tasks does not establish absent implementation.

## Evidence and hypothesis

Inspect `claim_slot_for` and its startup-reservation regression in
`crates/harness-core/src/task_worktree.rs`, and `acquire_pool_slot` in
`crates/codex-harness/src/executor_cli.rs`. Hypothesis: continuous ownership
prevents lost/overwritten assignments and false acceptance under competing
startup. The controlled baseline must demonstrate that loss; the candidate must
have zero identity violations while preserving capacity and recovery. This is
a correctness claim, not an assumed throughput gain.

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
