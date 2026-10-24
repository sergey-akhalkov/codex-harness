# Proposal

## Why

`executor_wait` currently ends after at most 55 seconds, so a lead with no independent work repeatedly calls the model and MCP just to receive unchanged timeout state. Keep the installed MCP workflow while allowing one quiet wait of up to 1800 seconds, with immediate delivery of results and questions, including results produced before waiting begins.

## What Changes

- Make the ordinary wait last up to 1800 seconds without intermediate timeout responses to the model; return earlier for an actionable question, terminal result or observation failure.
- **BREAKING:** an omitted `run` waits across the calling lead's eligible runs instead of rejecting several matches. Explicit `run` continues to select one verified run. An omitted timeout becomes 1800 seconds instead of 30 seconds.
- Inspect retained current state before blocking. Preserve results across late observation, several completions and reconnection; distinguish newly received results from those already acknowledged without accepting work or releasing slots.
- **BREAKING:** replace the repeated run card in timeout responses with a compact wait receipt. Keep full status, result evidence and errors accessible through their existing owners.
- Align the wait limit, harness request deadline and effective Codex plugin client timeout. Preserve cancellation, live communication, native conversations and the existing installation lifecycle.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `executor-mcp-plugin`: long waits across owned runs, immediate observation of existing outcomes, retry-safe result acknowledgement, compact responses and installed delivery.

## Impact

Implementation owners are `crates/codex-harness/src/executor_mcp.rs`, `crates/harness-core/src/mcp_session.rs`, and, only where required for responsive cancellation and communication, `crates/harness-core/src/mcp_stdio.rs`. Plugin configuration lives in `.agents/plugins/executors/.mcp.json`; native coverage starts in `crates/codex-harness/tests/executor_mcp.rs` and the existing MCP session/stdio tests. Owned lead guidance and `docs/agent-delegation.md` must teach the new default and acknowledgement behavior.

The first implementation task must establish the supported timeout mechanism for the installed plugin route; ordinary MCP configuration documentation alone does not establish that capability. No new service, event journal, polling agent, model route or legacy transport is proposed. Fewer model-visible wait exchanges are the acceptance measure; token or subscription savings require separate actual measurements and are not promised.
