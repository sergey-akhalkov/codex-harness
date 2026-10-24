# Proposal

## Why

The current orchestration gives one root lead every assignment, integration and acceptance decision. Coordinating several substantial changes, including changes spanning repositories, needs delegated leads that own complete outcomes and their executor teams while the root retains overall judgment and acceptance.

## What Changes

- Introduce a fixed Director -> Lead -> Executor hierarchy through the existing installed `executors` MCP plugin. The Director may work directly or dispatch either a Lead or an Executor whenever appropriate. A Lead must receive a well-described OpenSpec change; a vague task or board card alone is not a valid Lead assignment.
- Keep the Director on the ordinary Codex route. Bind delegated Leads and Executors to `ds`, with separate role instructions and MCP capabilities despite sharing a model profile. Preserve the existing root-profile configuration meaning and prohibit silent route substitution.
- Let a Lead own the implementation and acceptance of its assigned OpenSpec change across one or several repositories. Require the change's scope, requirements, acceptance conditions and implementation tasks, including design when its workflow requires it, before dispatch. Bind specification inputs, integration worktrees and child worktrees to exact committed revisions.
- Expose role-specific tools and enforce caller, parent, root and run authority on the server. Leads may create only Executors; Executors may communicate only with their parent and cannot create agents.
- Use existing beads records for assignments and acceptance, with explicit board identity, addressed owners and separate dependencies. Accept executor work into a Lead's result, then accept the combined result at the Director.
- Configure direct-child limits independently: each Director defaults to two Leads and four Executors; each Lead defaults to six Executors. Coordinate these limits with installation-wide executor admission and per-repository worktree capacity; the shipped defaults support all sixteen Executors and two Leads in one repository.
- Preserve in-flight work, pending questions and results for explicit restoration after the controlling CLI closes; no unattended background service is introduced.
- Deliver and verify the extension through the existing global installation, update and recovery lifecycle, including real external consumption.

## Capabilities

### New Capabilities

- `director-orchestration`: Specification-backed Lead delegation with free Director choice of execution level, cross-repository workspaces and integration, shared hierarchy admission, and explicit whole-hierarchy restoration.

### Modified Capabilities

- `lead-agent-orchestration`: Distinguish the root Director from a delegated Lead, configure their routes, adapt activation and board ownership, and preserve hierarchical acceptance and recovery.
- `executor-mcp-plugin`: Add lead dispatch, role-specific tool visibility, hierarchy-aware authority, observation and control while preserving existing executor operations.

## Impact

Extend the existing Rust owners in `crates/codex-harness/src/executor_mcp.rs`, `executor_control.rs`, `executor_cli.rs` and `executor_assignment.rs`, and `crates/harness-core/src/orchestration_config.rs`, `native_launcher.rs`, `task_worktree.rs` and `task_orchestrate.rs`. Update owned role instructions, `team-lead` and `board-workflow` guidance, `global/orchestration.toml`, the existing plugin lifecycle and their native tests. These are implementation targets, not edits made by this proposal.

Existing executor calls, saved work, explicit local limits and unrelated profiles remain supported. New parent-specific limits compose with shared resource limits; installation checks expose a deliberately smaller aggregate or pool limit rather than advertising unreachable concurrency. No additional agent framework, tracker, per-role server implementation or Codex fork is planned.

## Non-goals

Unbounded agent recursion, compulsory delegation of every spec to a Lead, autonomous execution after CLI closure, distributed machines, automatic cross-repository publication, atomic multi-repository Git commits, or an OS security sandbox for unrestricted shell processes. OpenSpec's externally maintained workflow files are unchanged.
