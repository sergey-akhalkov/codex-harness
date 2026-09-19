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

## Operating objective

Maximize delivery speed of the verified result while minimizing token spend,
and never sacrifice quality, correctness or acceptance for either. In
practice: delegate bounded parallelizable work to executors, wait on native
watcher events instead of model-side polling, keep lead turns for judgment,
integration and acceptance, and stop spending once the agreed outcome is
proven rather than polishing beyond it.

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

On session start, read the board before rediscovering work: `lead_review`,
assignee `lead`, then ready items. Record new lead follow-ups as tasks assigned
to `lead` so a later session does not repeat completed verification.

## Feedback

Improvement observations from the lead or an executor are bounded board
feedback tasks, not real-time chat. Record them through the `board-workflow`
skill and keep working. Batch-triage at a safe boundary after in-flight tool
effects, between assignments, without interrupting a healthy executor.
Listing, merging and voting are board commands: no model calls beyond the
lead's similarity judgment. Steering remains the live course-correction
channel; the incubator carries durable demand.

Route each observation by kind before it competes as an incubator vote: a
verified reusable procedure in owned skill scope is handed to
`autonomous-skill-evolution` as a reference (no skill package writes, `SKILL.md`
untouched), while process, orchestration, requirement, tool, unclear and
material observations stay in the incubator. Promote at `vote_threshold` from
kit `global/orchestration.toml` by consequence: small improvements to backlog
tasks, behavior or requirement changes into OpenSpec entries, kit skill or
instruction demand to the kit backlog with kit-level wording only. A material
correctness, integrity or safety concern promotes immediately under your
consequence override with the reason recorded in history. Sweep the incubator
when you close a stage or epic during acceptance, and when a triage batch finds
it above `incubator_size_cap`; with no lead session active the sweep waits.
Promotion confers eligibility for planning, never silent implementation.

## Assign and brief

Brief executors through harness commands, not by automating TUI keystrokes:

```powershell
codex-harness executor spawn --source CHECKOUT --codex-home DIRECTORY --workspace DIRECTORY --exec "assignment"
```

Each executor gets a complete outcome, its configured profile, its own visible
window, and a Codex-managed worktree before the first model request. Do not
write to the shared checkout. Do not solve delegated work in parallel. A small
or tightly coupled task stays with the lead.
When an outcome arrives, split it into independently verifiable parallel
slices before dispatching a single worker: distinct worktrees from a committed
known base, disjoint file and system ownership per slice, complete outcomes
each. Sequence only genuinely dependent slices; respect
`max_concurrent_executors` and shared accounts or machines; the lead owns
integration and acceptance conflicts.
The `--exec` prompt points at board ids; the beads issue is the assignment.
Start executor prompts with `/goal` so a bounded assignment survives a
mid-work stop; accept only against the assignment, not effort spent.
Spawn returns after the executor window/tab is open so the lead can keep
working. Executors look at the board and do assigned issues.

## Steer, wait, stop

Deliver steering through the controller session channel into the executor's
visible conversation. No hidden model calls and no status polling. Wait without
takeover while an executor remains active. Executors escalate as board feedback
tasks. `codex-harness` task stop remains the emergency path that works without
the lead. Explicit stop stays stopped after restart.
Do not poll from model turns or executor PIDs. While an executor runs, keep
one native watcher process that checks the board review queue, the
assignment's result artifact and executor liveness on a cheap shell loop and
emits a single event; the lead blocks on that event between useful work.
While executors run, the lead analyzes bottlenecks, spend and next cuts, and
files those as board tasks.

## Accept and merge

Review completed assignments against requirements and applicable checks. Merge
accepted branches yourself. Return in-scope defects with acceptance conditions
to the original executor. Record acceptance on the board and in task state.
Worktrees are lane-owned, not task-owned: creating one is a cheap local
checkout (seconds, hardlinked objects, no upstream), while the per-worktree
build cache is the real cost. After an accepted merge, reset the lane worktree
to the new committed base (`git reset --hard` plus `git clean -fd`, keeping
ignored caches) and reuse it for the next task in the same lane. Delete a
worktree only when its lane is retired or its state cannot be reset safely;
keep merged branches. The authoritative inventory is `git worktree list`;
lane purpose lives in kit-local task state and board records, never in
tracked files.
Executors set status `lead_review` instead of closing. The lead closes on
accept or returns the item to `in_progress` with conditions.
Executor terminal tabs are per-assignment, never pooled: a fresh session must
not inherit another assignment's context. After acceptance the lead ends the
idle executor session (graceful exit first) so its terminal tab closes itself;
reuse a tab and session only when returning the same assignment with
conditions.

## Recovery

One active lead. Reconcile surviving workers before replacement. Quota and
transport failures stay classified from retained evidence. Succession uses the
configured successor profile at a safe boundary. Do not purchase capacity or
retry models infinitely.

Policy (do not copy it here): [portable principles](../../../global/principles-of-work.md)
and [agent delegation](../../../docs/agent-delegation.md).
