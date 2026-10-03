## Why

Managed executor observations have failed mid-turn with Windows I/O error 997,
causing the harness to terminate the owned child tree and require continuation.
Ordinary idle periods and incomplete control messages must not end an assignment.

## What Changes

- Reproduce the control transport failure using owned local sockets and retain a regression.
- Correct bounded observation without replaying requests, changing model bindings or hiding genuine disconnects.
- Verify the executor caller and deliver the corrected immutable global build.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `agent-delegation`: observation waits preserve a running conversation across idle and partial-message intervals.

## Impact

The shared Rust control transport, its native tests and installed executor observation.
No provider requests or changes to existing executor worktrees are needed for reproduction.
