# Design

## Context

See proposal.md. The CLI constructs one absolute deadline from the managed registration's `--connection-seconds 86400`. The Serena proxy uses it for input, request budgets and output, so expiry closes STDIO. A live baseline with a 90-second limit completed initialization, kept client input open and exited after 90.24 seconds with `Serena proxy deadline expired` and exit code 2. A 2-second cold-start attempt failed before initialization and is not the expiry reproducer.

The installed Nuphus entry point reproduces the same failure with an eight-second limit: initialization succeeds at 0.17 seconds, client input remains open, and the proxy naturally exits with code 2 after 8.17 seconds (`MCP connection deadline expired`; the input worker reports a deadline and confirms its join).

## Goals / Non-Goals

Keep open managed Serena and Nuphus connections usable across long conversations and returns after idle periods. Preserve existing finite operation budgets, bounded cleanup, broker/worker lifecycle and explicit finite-lifetime probes. This repair does not add a client reconnection service.

## Decisions

- Use an explicit unlimited connection mode (`--connection-seconds 0`) for managed Serena and Nuphus registrations. Preserve positive limits for deliberately bounded callers and fixtures. Do not substitute a larger arbitrary maximum or an idle-session timeout: both eventually break an open conversation when its user returns.
- Reuse the cancellable pipe and broker owners. Add the smallest cancellation-aware indefinite input wait needed by Serena, while retaining finite request, write and cleanup deadlines. Do not recreate the pipe after a timed-out read, which can lose partially consumed bytes, and do not generalize all process deadlines unnecessarily.
- Keep the existing 240-second request cap, finite response writes and cleanup, fixed project routing, worker pool limits, idle retirement and uncertain-mutation protections. Unlimited transport age does not grant unlimited execution.
- Extend the shared STDIO serving owner for Nuphus using the existing cancellable indefinite pipe read. Preserve its finite request/session deadlines, lazy worker startup and retirement, browser isolation and desktop admission. Other bounded STDIO callers retain their existing API behavior where practical.
- Use native protocol/pipe regressions with owned synthetic endpoints for fast deterministic expiry, cancellation and EOF coverage, then verify real installed activation and symbol navigation outside the kit checkout.

## Risks / Trade-offs

- Indefinite input must still exit on EOF and cancellation: exercise both through actual pipe endpoints and verify cleanup.
- A change to shared pipe mechanics can affect other users: retain the bounded API behavior and run its existing native regression suite.
- Already loaded proxies keep their executable and startup options: deployment updates the managed registration, and the affected client must reconnect once. Do not interrupt unrelated active conversations to adopt the change.

## Migration Plan

Run focused baseline/candidate checks, install the immutable candidate through the normal lifecycle, update both managed registrations and validate installation ownership. Exercise installed Serena from an external project and Nuphus through a read-only native tool request. Preserve the lifecycle recovery receipt and report the one-time reconnection boundary.

## Verification and existing limitations

The native CLI comparison exercises a two-second finite connection and an unlimited connection that receives requests after three seconds, then exits on client EOF. Pipe regressions verify delayed data, EOF, cancellation and unchanged bounded I/O. The focused CLI, registration and pipe suites pass. With an explicit adopted registry, `cargo test --locked -p codex-harness --test serena_stdio --jobs 2 -- --ignored --test-threads=1` passes all five real-package checks, including semantic edit/readback, independent roots and transport recovery without replaying edits.

An installed external-project check initializes and activates Serena, reads symbols, keeps the same client open for 310 seconds, observes the original idle worker exit, and reads symbols again without reactivation. The route is preserved and client EOF exits with code 0. This exercises worker retirement and return; it is not a 24-hour soak test.

Nuphus CLI regressions compare a three-second finite connection with an unlimited connection serving requests after four seconds. Shared STDIO checks cover delayed input, EOF, external cancellation and active-request reclamation; session checks retain request expiry. The affected `mcp_stdio`, `nuphus_stdio`, `lazy_stdio`, `mcp_cli` and `mcp_registration` suites pass (10, 6, 10, 5 and 20 tests respectively), as do the seven preparation/session checks. The optional real Nuphus package test passes through browser navigation and cleanup; an earlier attempt reached an Edge loopback-page load error whose cause was not established. That browser observation is not evidence of a transport lifetime failure.

The full repository gates are not clean: controlled base/candidate checks retain the same formatting differences in `serena_acceptance.rs` and `serena_configuration.rs`, Clippy errors in `task_worktree.rs`, five orchestration tests tied to the configured executor profile and a token-audit cache test failure. These are outside the changed files and are not counted as passing gates. Changed Rust files pass their formatting check; focused regression acceptance remains separate from these existing failures. An explicit Serena diagnostic query returned an empty object, which does not establish fresh, complete analysis.
