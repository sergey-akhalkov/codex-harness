## Context

At source revision 0a1204c the shared `ControlConnection` calls tungstenite on a
blocking `TcpStream`, setting a receive timeout before every observation. The
executor pump switches to short drain waits after each event. Retained local
failure records show `executor observation failed: IO error: ... (os error 997)`;
the exact underlying Winsock trigger was not reproduced in the bounded local run.
The neighboring frontend relay already probes nonblocking reads and restores
blocking mode for bounded writes.

## Goals / Non-Goals

Preserve the existing synchronous API, authentication, frame size policy, message
ordering and failure containment. Do not retry provider requests, reconnect
silently or disable observation. Raw historical logs remain local.

## Decisions

The unchanged transport passed 128 local idle/partial-frame cycles (640 empty
reads), so that run did not reproduce 997. A separate trickling-frame regression
failed: an 80 ms observation returned a complete message after 549 ms. Both are
retained as native Rust tests, without injected OS errors.

[Microsoft's socket contract](https://learn.microsoft.com/en-us/windows/win32/winsock/sol-socket-socket-options)
states that a blocking receive timeout leaves the connection indeterminate and
it should be closed. Reusing SO_RCVTIMEO expiry as idle polling is therefore an
observed contract violation and a plausible explanation for the retained 997
failures, not a proven reproduction of their exact OS trigger.

Reuse the relay's nonblocking-read approach and wait with WSAPoll against one
elapsed observation budget. This avoids starting timed blocking receives and
avoids periodic sleep polling. Retain partial frames in tungstenite and restore
the send mode even after read errors. No dependency or general async runtime is
needed. Do not swallow 997: a pending operation is not proof of a safely reusable
buffer or socket. Other I/O errors retain their original OS code.

## Risks / Trade-offs

- Intermittent OS failure → distinguish historical evidence, real reproduction
  and narrower deterministic regression results.
- Partial frames and heartbeat writes → test retained bytes, ordered responses,
  bounded waiting and subsequent sends through the actual connection.
- Shared transport callers → run focused core tests and the executor control
  integration target before immutable lifecycle deployment.

## Migration Plan

Deploy through `codex-harness deploy`, verify the receipt and installed entrypoint
outside this checkout. Existing processes keep their loaded build; preserve
their state and use the new build for subsequent launches. Lifecycle rollback
remains available through the installation record.
