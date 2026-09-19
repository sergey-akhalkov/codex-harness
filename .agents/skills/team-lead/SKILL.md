---
name: team-lead
description: Activate orchestrated asynchronous development with one lead session that specifies, assigns, unblocks, accepts and merges work while isolated executor sessions implement complete outcomes. Use when the user invokes team-lead or clearly asks for orchestrated asynchronous development, a team-lead workflow, or lead/executor dispatch. Do not use for a small direct task.
---

# Team lead

Enter this role only when the user invokes this skill or clearly asks for
orchestrated asynchronous development. An ordinary session without that
activation must not spawn executors, create board records, or write
orchestration state. Leaving the role or stopping orchestration is explicit
and preserves partial work.

## Discover roles

Read kit `global/orchestration.toml` (lead profile, successor lead, executor
profiles, max concurrent executors). Installation check already rejects missing
profiles and non-positive limits. Dispatch uses exactly those profiles.
Explicit user `codex --profile <id>` keeps native precedence. Do not substitute
another model.

## Board setup

Use the `board-workflow` skill. Init with
`bd init --skip-agents --non-interactive --quiet`. Stages are epics,
specifications are features, executor feedback is a `task` labeled `feedback`.
If `bd` is missing or broken, report `board unavailable` and continue only work
whose acceptance does not depend on the board.

## Assign and brief

Brief executors through harness commands, not by automating TUI keystrokes:

```powershell
codex-harness executor spawn --source CHECKOUT --codex-home DIRECTORY --workspace DIRECTORY --exec "assignment"
```

Each executor gets a complete outcome, its configured profile, its own visible
window, and a Codex-managed worktree before the first model request. Do not
write to the shared checkout. Do not solve delegated work in parallel. A small
or tightly coupled task stays with the lead.

## Steer, wait, stop

Deliver steering through the controller session channel into the executor's
visible conversation. No hidden model calls and no status polling. Wait without
takeover while an executor remains active. Executors escalate as board feedback
tasks. `codex-harness` task stop remains the emergency path that works without
the lead. Explicit stop stays stopped after restart.

## Accept and merge

Review completed assignments against requirements and applicable checks. Merge
accepted branches yourself. Return in-scope defects with acceptance conditions
to the original executor. Record acceptance on the board and in task state.
Retire a clean managed worktree through native confirmed deletion when eligible;
otherwise preserve it and report the limit.

## Recovery

One active lead. Reconcile surviving workers before replacement. Quota and
transport failures stay classified from retained evidence. Succession uses the
configured successor profile at a safe boundary. Do not purchase capacity or
retry models infinitely.

Policy (do not copy it here): [portable principles](../../../global/principles-of-work.md)
and [agent delegation](../../../docs/agent-delegation.md).
