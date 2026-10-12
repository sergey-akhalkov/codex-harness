# Proposal

## Why

Isolated tasks currently have competing instructions for disposable Git worktrees, the harness executor pool, and Codex-managed worktrees. A single reusable pool must serve every task that needs file isolation, with a hard configured directory limit and preservation of unfinished work across interruption and reuse.

## What Changes

- Make the harness the sole allocator of managed task isolation, for ordinary isolated tasks, experiments, reviews and executors. Reuse the existing pool implementation rather than introduce another registry or service.
- Separate acquiring an isolated checkout from launching an executor. Keep existing executor commands as consumers of the common lifecycle.
- Identify one pool per Git repository across its linked worktrees. Create stable slots lazily and never exceed the configured pool capacity, including when all slots contain suspended or unreviewed work.
- Separate `worktree_pool_size` from `max_concurrent_executors` in orchestration configuration, preserving existing installations through an explicit compatibility path.
- Preserve exact committed bases, task ownership, interruption recovery, accepted-result release and warm build caches. A locally available explicit base does not require a remote fetch.
- **BREAKING**: retire the standalone `isolated-worktree` skill and its disposable-worktree recipe after the common task entry point is delivered. Replace conflicting guidance with the shared pool contract through the global installation lifecycle.
- Keep ordinary Git as the physical worktree mechanism. Do not add a Codex MCP backend: the inspected tool contract does not establish the lifecycle controls needed to replace the pool.

## Capabilities

### New Capabilities

None. Extend the existing isolation capability instead of creating a parallel worktree specification.

### Modified Capabilities

- `isolated-worktree-workflow`: own the common bounded pool, task acquisition, preservation, reuse and globally delivered replacement for the standalone skill.
- `lead-agent-orchestration`: consume the common pool, separate executor concurrency from directory capacity, and retain existing dispatch, visibility and recovery guarantees.
- `agent-delegation`: align checkout ownership and terminology with the shared harness pool while preserving delegation acceptance requirements.

## Impact

Affected owners include `crates/harness-core/src/task_worktree.rs`, `crates/harness-core/src/orchestration_config.rs`, the CLI and executor adapters under `crates/codex-harness/src/`, `global/orchestration.toml`, `.agents/skills/isolated-worktree/`, `.agents/skills/team-lead/SKILL.md`, installation delivery and their native checks. Update `docs/agent-delegation.md`, `docs/rust-native.md` and the existing decision record without adding another operational manual.

Preserve existing worktree paths, partial work, executor command names, model selection, terminal visibility and heavy-command limits. Coordinate the existing `serialize-executor-dispatch-claims` change: startup exclusivity remains required, and its implementation or acceptance is not implied by this proposal. Delivery includes a real installed use path in an independent repository; no model-backed probe is required merely to test pool mechanics.
