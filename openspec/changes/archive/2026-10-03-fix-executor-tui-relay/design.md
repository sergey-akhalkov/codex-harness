## Context

See proposal.md. The host control connection and native app-server keep working while a separate authenticated WebSocket relay serves the TUI. The relay currently performs a blocking read of each side once per loop with a 50 ms timeout. Native Codex 0.159.3 is installed; there is no controlled 0.157.1 comparison yet.

## Goals / Non-Goals

Keep the existing relay and its warning semantics, eliminate the throughput cap, and verify native frontend consumption. Preserve running assignments and their process ownership; do not restart a model to repair presentation or change provider settings.

## Decisions

Read each socket only when data is available, preserving partially read frames in the existing WebSocket object. Retain bounded blocking writes, alternating both directions fairly. Wait only when neither side supplies traffic. Merely shortening the blocking timeout retains an arbitrary throughput cap; a new asynchronous runtime or transport owner is unnecessary for this bounded relay.

Authenticate and connect within the existing write/connect bound rather than the idle poll interval. Test server bursts with idle frontend, client bursts with idle server, heartbeat traffic and reconnection against the real relay, then exercise the installed native CLI. A deterministic burst is the minimal counterexample; no live model request is necessary.

## Risks / Trade-offs

- A slow destination can still reach the bounded write timeout; failed sessions must close and allow a new connection without replaying requests.
- Updating binaries cannot replace code inside an already running host. Preserve current work and observe recovered windows; new hosts use the delivered fix. Any recovery requiring interruption must be reported explicitly.

## Migration Plan

Run native checks, deploy an immutable build, and verify the installed manager outside the source checkout. Existing installation rollback retains the previous build. Keep active hosts intact.
