# Proposal

## Why

An executor run becomes permanently unusable when its lead conversation is
resumed on a new app-server process. The lead thread, executor thread, run and
pool ownership are still valid, but the receipt records the old process, so
`executor_status` and every dependent operation refuse before observation or
recovery.

## What Changes

- Rebind a run's recorded app-server process when the same lead thread has a
  new live registered endpoint and the previously recorded process has
  provably ended.
- Preserve the run, lead and executor threads, slot, checkout, base and profile;
  record the process change in the existing run receipt.
- Continue to refuse when the old process is still live or its end cannot be
  proved, preventing two runtimes from controlling one run.
- Cover observation and subsequent native operations after the reconnect
  through the existing executor MCP entry point.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `executor-mcp-plugin`: permit a verified same-thread app-server rebind while
  retaining exact run, lease and stale-authority checks.

## Impact

Affects the executor MCP receipt validation in
`crates/codex-harness/src/executor_mcp.rs`, exact process liveness in
`crates/codex-harness/src/executor_control.rs` and
`crates/harness-core/src/process_service.rs`, focused MCP regression checks,
and executor recovery guidance.
