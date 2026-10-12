# Tasks

## 1. Transport repair

- [x] 1.1 Reproduce the finite-lifetime baseline with a successful native MCP initialization and open client input; record natural exit, error and elapsed time in the design.
- [x] 1.2 Add unlimited Serena connection mode with cancellable input and finite operation/cleanup budgets; native tests must cover delayed input, EOF, cancellation, retained finite mode and bounded timeout behavior.
- [x] 1.3 Generate unlimited managed Serena registrations and document the reconnect boundary; verify registration/CLI checks and relevant documentation links.
- [ ] 1.4 Extend unlimited mode to Nuphus and shared STDIO serving, preserving finite callers, operation timeouts, idle retirement, cancellation and EOF cleanup; cover these through native regressions and update registration checks.

## 2. Integration and delivery

- [ ] 2.1 Run affected native pipe, Serena broker, Nuphus and STDIO checks, including real package activation, routing and idle retirement; retain exact commands and outcomes. Serena real-package checks and installed idle-return verification have passed.
- [ ] 2.2 Deploy the combined immutable candidate through the global lifecycle, update both managed registrations, validate ownership and exercise installed Serena activation plus symbol navigation outside this checkout and a read-only Nuphus tool request. Serena deployment and external verification have passed.
- [ ] 2.3 Reconcile task/spec state and report completed checks and any one-time reconnection needed by an already closed client.
