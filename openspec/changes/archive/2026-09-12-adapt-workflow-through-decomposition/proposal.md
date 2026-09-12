## Why

An agent can spend many expensive edit/run cycles discovering a different small problem on each attempt, while its overall method remains inefficient. The accepted direction is to reassess that method during work, prefer independently verifiable decomposition, and optimize repeated operations without reducing the agreed outcome or correctness.

## What Changes

- Make workflow reassessment part of planning and execution at consequential slow operations, unexpected failures and recurring classes of difficulty; a new error does not by itself justify another expensive full cycle.
- Prefer extracting a smaller problem with its own meaningful check before optimizing the remaining cycle. Preserve the original failure condition, real dependencies and complete acceptance.
- Define useful workstreams by their result, inputs, dependencies, resource ownership, verification and integration consumer. Delegate when separate context or independent execution reduces total work; retain direct execution for tightly coupled or small tasks.
- Reuse expensive preparation, sessions and verified results when their relevant conditions still hold; automate recurring mechanics in their existing owner and restore owned state after exploration.
- Require the parent workflow to consume a verified workstream result before that supporting work is treated as delivered. Continue independent work while a dependency is being resolved.
- Deliver the behavior through the existing global principles, pack-owned skills and delegation guidance. Check fresh external loading, actual task behavior, correctness and complete elapsed cost, including preparation, handoff and rework.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `global-working-principles`: Adaptive workflow reassessment, decomposition before local optimization, bounded reuse and evidence of useful behavior outside this checkout.
- `agent-delegation`: Independently verifiable workstream assignments, ownership of mutable runtime resources and completion through verified integration.

## Impact

- Implementation owners: `global/principles-of-work.md`, the existing `project-verification` and `token-efficient-workflow` skills, `docs/agent-delegation.md`, and their concise owning decision/operation records.
- Reuse the existing `reproduce-regression`, `isolated-worktree` and project-memory workflows within their current scopes; reuse native execution and current installation links.
- OpenSpec skills, schemas, templates and configuration remain externally maintained. This change creates no new orchestrator, mandatory worker, background reflection service, model route or dependency.
- Delivered scope remains the supported Windows Codex installation. Fresh sessions receive updated sources through the existing lifecycle; current sessions are not presumed to reload instructions.
- Acceptance examples use synthetic inputs. A real external development task is selected locally without publishing consumer identities, machine paths or operational data. Other active changes and their outstanding acceptance remain independent.
