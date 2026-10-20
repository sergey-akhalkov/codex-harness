# Proposal

## Why

A transient model-capacity error can end a Codex turn and stall an unfinished goal until the user enters `/goal resume`. Codex already has request retries, but its overload handling can bypass them; recovery should preserve the existing conversation and work without requiring the user to supervise temporary service failures.

## What Changes

- Add bounded automatic recovery for an active goal whose latest turn fails specifically with native `serverOverloaded`.
- Resume the existing goal in the same session after exponential delays, with five recovery attempts per consecutive failure episode and an explicit exhausted state.
- Preserve model/provider, credentials, budget, usage accounting, conversation, tool effects and running work. Expose waiting, cancellation and recovery in the existing terminal.
- Make user control authoritative: pause, interruption, replacement, clearing, new input and session closure invalidate pending recovery. Serialize recovery with user control; a status read followed by an unchecked update is insufficient.
- Deliver the behavior through the existing global harness lifecycle and ordinary `codex`/`codex resume` entry points, preserving the calling terminal. Reuse owned process/control transport primitives without enabling the window-management and quota-handoff controller.

## Capabilities

### New Capabilities

- `goal-recovery`: Recover native goals from transient model overload with bounded attempts, preserved state, authoritative cancellation, visible outcomes and global lifecycle integration.

### Modified Capabilities

None. Existing launch availability, terminal ownership, profile precedence and installation requirements remain constraints rather than being relaxed.

## Impact

The change affects the native launcher/session owner, local app-server control routing, installation/check/rollback, native integration tests and the operating guide. Existing `harness-core` process and control primitives are reusable foundations. No upstream Codex fork, additional model, subscription change, global watcher service or new dependency is proposed.

Discovery on installed Codex 0.163.0 demonstrated same-session recovery through the native API and exposed a stale-resume race after a real TUI pause. Implementation must address that race before automatic recovery is enabled. This proposal does not implement or activate the feature.
