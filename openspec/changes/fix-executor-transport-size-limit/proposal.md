## Why

The managed executor transports imposed a one-mebibyte WebSocket record bound on
both the host's control connection and the frontend-facing warning relay. Real
conversation state exceeds it: an exact-session resume of 1,149,210 bytes was
rejected with `Space limit exceeded: Message too long` before the assignment was
submitted, the owned child tree was terminated, and the native frontend surfaced
the dead backend as an endless `connection lost. attempting to reconnect` loop.
Executors with real session histories could not start at all. Both hops are
loopback connections between processes this host owns, so a harness-side size
cap protects nothing while rejecting legitimate records.

## What Changes

- Remove the harness-side message, frame and write-buffer size caps from the
  control transport in `harness-core/src/task_control.rs` and the warning relay
  in `crates/codex-harness/src/executor_warning.rs`; no record is refused for
  size on either hop.
- Remove the oversized-final-message-read mitigation and its output-defect
  scenario: a full-thread read now either completes and records the thread's own
  final assistant message, or fails for its own reason and fails the run.
- Keep every other transport bound (connect, read poll, write and shutdown
  timeouts) unchanged.
- Extend the hosted executor-control and relay suites with above-one-mebibyte
  resume, thread-read, forwarding and diagnostic-delivery regressions.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `lead-agent-orchestration`: the oversized final-message-read requirement is
  replaced by a no-harness-side-size-cap requirement for managed executor
  transports.

## Impact

Primary owners are the control transport, the warning relay and the control
host's final-message path, with their Rust tests. The canned endpoint fixture's
own sanity bound rises to 16 MiB so the doubles can carry real-sized records;
that bound is test infrastructure, not a product contract. Documentation and the
decision record must stop describing the removed limit and its fallback.
