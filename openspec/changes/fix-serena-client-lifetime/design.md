# Design

## Context

See proposal.md. The CLI constructs one absolute deadline from the managed registration's `--connection-seconds 86400`. The Serena proxy uses it for input, request budgets and output, so expiry closes STDIO. A live baseline with a 90-second limit completed initialization, kept client input open and exited after 90.24 seconds with `Serena proxy deadline expired` and exit code 2. A 2-second cold-start attempt failed before initialization and is not the expiry reproducer.

## Goals / Non-Goals

Keep open managed Serena connections usable across long conversations and returns after idle periods. Preserve existing finite operation budgets, bounded cleanup, broker/worker lifecycle and explicit finite-lifetime probes. This repair does not change Nuphus transport behavior or add a client reconnection service.

## Decisions

- Use an explicit unlimited connection mode (`--connection-seconds 0`) for managed Serena registrations. Preserve positive limits for deliberately bounded callers and fixtures. Do not substitute a larger arbitrary maximum or an idle-session timeout: both eventually break an open conversation when its user returns.
- Reuse the cancellable pipe and broker owners. Add the smallest cancellation-aware indefinite input wait needed by Serena, while retaining finite request, write and cleanup deadlines. Do not recreate the pipe after a timed-out read, which can lose partially consumed bytes, and do not generalize all process deadlines unnecessarily.
- Keep the existing 240-second request cap, finite response writes and cleanup, fixed project routing, worker pool limits, idle retirement and uncertain-mutation protections. Unlimited transport age does not grant unlimited execution.
- Use native protocol/pipe regressions with owned synthetic endpoints for fast deterministic expiry, cancellation and EOF coverage, then verify real installed activation and symbol navigation outside the kit checkout.

## Risks / Trade-offs

- Indefinite input must still exit on EOF and cancellation: exercise both through actual pipe endpoints and verify cleanup.
- A change to shared pipe mechanics can affect other users: retain the bounded API behavior and run its existing native regression suite.
- Already loaded proxies keep their executable and startup options: deployment updates the managed registration, and the affected client must reconnect once. Do not interrupt unrelated active conversations to adopt the change.

## Migration Plan

Run focused baseline/candidate checks, install the immutable candidate through the normal lifecycle, update the Serena registration and validate installation ownership. Exercise the installed entry point from an external project. Preserve the lifecycle recovery receipt and report the one-time reconnection boundary.
