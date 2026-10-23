# Proposal

## Why

Executor delegation currently combines a useful worktree pool and profile routing with separate terminal surfaces and harness-specific conversation plumbing. The owner wants to assign work, inspect distinct executor conversations and exchange results from one Codex CLI interface, while keeping configured providers, isolated worktrees and the CLI's privilege level.

## What Changes

- Deliver one Codex plugin exposing executor dispatch, addressed communication, observation, stop and recovery through MCP. Reuse the existing Rust pool, assignment, profile and installation owners.
- Use native Codex threads, queues, turns, events and history for conversation execution. Make executor conversations accessible through the lead's native `/agents` interface in the same terminal; questions and results reach the lead without opening the picker. Registration as native `/subagents` children is not required.
- Preserve configured executor profiles, including different providers and per-profile settings, without silent fallback. Preserve ordinary or elevated execution identity and prevent lower-privilege clients from controlling an elevated runtime.
- Preserve exact assignment/base/slot ownership, both directions of messaging, correlated replies, partial work, urgent isolated stop and recovery. Completion remains separate from acceptance, merge and slot release.
- Replace dedicated executor terminal management and redundant message relays with supported native facilities. First verify the actual MCP-to-native-UI path; protocol admission alone is insufficient.
- Switch owned instructions, skills, generated briefs, help and recovery hints to the installed MCP tools. Remove mentions and examples of the migrated CLI operations from active agent context, without migration tables or required legacy appendices. Keep existing command/package compatibility in implementation and regression coverage.
- Deliver and verify the plugin through the existing global lifecycle in a fresh consuming project. This change does not convert unrelated harness capabilities or create a general plugin platform.

## Capabilities

### New Capabilities

- `executor-mcp-plugin`: One installed executor interface using native Codex conversations, verified caller identity, configured profiles, inherited privileges, native presentation and context cutover.

### Modified Capabilities

- `agent-delegation`: Replace CLI-oriented guidance and dedicated terminal assumptions with the MCP workflow; retain assignment, model, cache-loss, preservation and acceptance guarantees.
- `lead-agent-orchestration`: Consolidate superseded terminal and message-transport requirements into the plugin contract while preserving shared-pool, board, ownership and recovery behavior.

## Impact

Affected owners include executor dispatch/control/assignment and role context in `crates/codex-harness`, pool/profile/MCP/installation support in `crates/harness-core`, repository-owned skills and global role instructions, installation and delegation documentation, and their existing integration tests. Use supported Codex plugin packaging and app-server contracts; no Codex source fork, instruction-only enforcement, new task tracker, separate messaging plugin or per-executor terminal requirement is planned.

The first implementation slice must prove native UI attachment, originating-lead identity, automatic bidirectional communication and ordinary/elevated operation on the supported installed Codex version. Current experiments establish only parts of that path. Broad migration and removal of working internals depend on this proof; an unavailable native capability is an explicit blocker, not permission to substitute the rejected multi-window or file-relay workflow.
