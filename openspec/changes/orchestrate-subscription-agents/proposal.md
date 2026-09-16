## Why

Asynchronous development needs a durable team-lead pattern: one lead session
creates and accepts specifications, assigns complete outcomes to executor
sessions, removes blockers, and merges accepted work while executors implement
independently. OpenCodex and its proxy routing are retired; native Codex
profiles such as `codex --profile zai` and `codex --profile xai` are the
dispatch surface. This change is refocused from quota-failover leadership to
that everyday lead/executor workflow, preserving the verified native control
foundation and retiring obsolete routing assumptions.

## What Changes

- Add kit-owned orchestration configuration naming the lead profile, executor profiles and the maximum concurrent executor count. The existing installation check validates it: a missing profile or invalid limit is an explicit error, never a silent substitute.
- Spawn every executor as a native `codex --profile <id>` session in its own visible terminal window and its own Git worktree before its first model request. The controller owns worktree creation, mapping and retirement; executors never write to the shared checkout.
- Coordinate assignments asynchronously through the consuming project's task board (`beads` / `bd` CLI): stages as epics, specifications as features, executor feedback as feedback tasks, and lead-initiated improvements as tasks or OpenSpec changes. The Rust controller stays board-agnostic; the kit delivers board availability and workflow guidance through its lifecycle.
- Deliver lead steering through the verified controller session channel into the executor's visible conversation, with no hidden model calls and no status polling. Executors escalate blockers as board feedback tasks.
- Make the lead accept completed assignments against their requirements and checks, return concrete defects to the original executor, and merge accepted branches itself.
- Activate the lead role explicitly through a kit-delivered `team-lead` skill that owns the lead workflow instructions (role discovery, board setup, briefs, steering, acceptance, merge, explicit stop). Global instructions only point to the skill; ordinary sessions spawn no executors.
- Keep the verified recovery core: durable host-private task state, cause-aware failure classification, single active lead, reassignment within the configured executor pool, and lead succession at safe boundaries without requiring a response from the failed lead.
- Deliver through the supported global Windows lifecycle with resumable state, explicit stop behavior, owned failure tests and one real external consuming development task; public artifacts contain only synthetic examples.

## Capabilities

### New Capabilities

- `lead-agent-orchestration`: Explicitly activated team-lead dispatch, isolated visible executor sessions, board-based assignment and feedback, lead acceptance and merge, durable ownership, cause-aware recovery and lead succession across restarts.

### Modified Capabilities

- `agent-delegation`: Team-lead responsibilities, configurable role routing and patient outcome ownership replace fixed three-subscription assignments; delegation guidance follows orchestration configuration.
- `subscription-model-routing`: Exact profile-backed model/effort assignment is selected through orchestration configuration with visible effective identities and verified heterogeneous dispatch.
- `subscription-efficiency`: Alignment only - provider preference follows orchestration configuration instead of fixed provider roles; quota pacing and token-benefit evidence move to the follow-up change.

## Impact

Affected owners include `global/harness.config.toml` role configuration, the
Rust `task_*` controller modules in `crates/harness-core`, native conversation
views and worktree management, the `beads` dependency assessment and lifecycle
integration, existing delegation/routing/subscription checks, and the owning
delegation, subscription, installation and decision records. The verified
app-server control contract, Rust control connection, implemented lead-handoff
seed and completed preset migration remain the foundation.

Feedback-cadence budgets, instruction-refresh succession through
`codex resume`, account-limit pacing and matched token-benefit comparisons are
deliberately excluded and owned by a separate follow-up change.
`autonomous-skill-evolution` continues to own skill and instruction evolution;
this change only orchestrates sessions that consume them. Private consuming
projects, paths and evidence stay outside shared sources.
