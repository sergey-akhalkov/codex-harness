## Why

The current delegation policy prefers Grok and sends unavailable or unfinished worker work back to GPT; the available Z.AI subscription has no assigned senior responsibility. Large tasks need productive use of all three subscriptions, patient independent execution, and automatic continuation even when the GPT leader cannot obtain another response.

## What Changes

- Keep GPT as the normal lead; make Z.AI the preferred senior executor for substantial text and code work, and Grok the preferred executor for visual work and suitable routine assignments. Select the explicit model and its supported reasoning effort at dispatch, without requiring separate named agent files. Remove redundant supplied presets with a documented migration; preserve native GPT availability and user-owned configuration.
- Show every active conversation simultaneously in a separate visible window or pane, including leaders, executors and any model-backed helpers. Show its assignment, provider/model, effort, live messages/tool activity and state; leadership changes must remain explicit. A switchable chat list or hidden background transcript alone does not meet this requirement.
- Delegate complete, independently verifiable outcomes, including correction of defects in the assigned work. Avoid duplicate investigation, speculative races, repeated status prompts and takeover based solely on elapsed time.
- Add a small Rust task controller that can react to provider failures without calling the failed leader. Preserve partial results and single ownership when handing work to a capable available subscription.
- When GPT quota is exhausted, let Z.AI temporarily lead the already agreed task and coordinate the remaining executors. Return leadership to GPT at a safe boundary after recovery; retain difficult unresolved questions without blocking independent accepted work.
- Distinguish quota exhaustion, temporary throttling, authentication/model failures, transport failures and incomplete worker output. Remember unavailable routes and reset information instead of probing them on every assignment.
- Pace new assignments using available account limits, reset times and observed consumption; include leader coordination, worker effort and rework in comparisons. Preserve acceptance and measure elapsed time as well as GPT use.
- Deliver through the supported global Windows Codex lifecycle, with resumable local state, explicit stop behavior, owned failure tests and real external-consumer acceptance. No new purchases, paid fallback, recursive worker trees or independent general-purpose orchestration platform.

## Capabilities

### New Capabilities

- `subscription-task-orchestration`: Durable task ownership, capability-aware recovery, temporary leadership, quota-aware dispatch and continuation across provider exhaustion and application restarts.

### Modified Capabilities

- `agent-delegation`: Three-subscription responsibilities, patient outcome ownership, economical handoffs, explicit auxiliary use and global acceptance.
- `subscription-model-routing`: Exact named Z.AI and Grok execution plus visible task-level reassignment while retaining explicit model selection.
- `subscription-efficiency`: Replace Grok-only priority with capability-aware use of Z.AI and Grok, account for quota windows, and verify GPT conservation without lowering quality or hiding latency/rework.

## Impact

Affected owners include `global/harness.config.toml`, retirement of redundant supplied agent definitions, subscription lifecycle and launcher code in the Rust workspace, native conversation presentation, the existing delegation/routing checks, and the owning delegation, subscription and token-workflow guides. Reuse the installed Codex protocol and pinned OpenCodex routes; exact runtime control and simultaneous-view contracts must be verified before dependent implementation. Repository `.agents/skills` are skills, not expendable agent presets.

Coordinate with `adapt-workflow-through-decomposition`: retain its workstream inputs, resource ownership, failure-preserving decomposition and verified integration requirements. This change supersedes conflicting Grok-only routing and parent-takeover rules, not that change's scope or acceptance. Preserve the independent proxy crash-recovery limits and the ongoing Rust migration. Implementation tasks in this proposal remain open until performed and verified.
