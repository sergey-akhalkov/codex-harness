# Tasks

## 1. Exact process reconciliation

- [x] 1.1 Add a read-only exact pid-and-creation-time liveness check and test
  exact, reused and invalid identities through the native process service.
- [x] 1.2 Reconcile an ended app-server lease under the existing receipt lock,
  re-read the endpoint before native use, and add MCP regressions showing a
  same-thread resume succeeds while a live or unverifiable old process is
  refused without effects.
- [x] 1.3 Document same-thread app-server recovery and its fail-closed boundary
  in the executor delegation guide; verify links and public source hygiene.

## 2. Integrated verification

- [x] 2.1 Run formatting, focused compiler checks and the complete
  `executor_mcp` integration target with truthful results.
- [x] 2.2 Deliver the corrected build through the normal installation lifecycle,
  then reproduce against an owned live same-thread reconnect and verify
  `executor_status` returns the preserved run instead of the stale-lease error.

## Workflow follow-up

- Archive the change after the project's review and delivery requirements are satisfied.
