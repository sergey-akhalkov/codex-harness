# Proposal: committed-dispatch-snapshot

## Why

A lead dispatched executors while the source checkout still held uncommitted
assignment inputs: the slots synchronized to the fetched upstream base, and
the lead then copied the missing files into live slots after launch. The
executor worktrees were never current copies of the main worktree, and the
post-launch transfer left a race between the executor's reads and the lead's
writes. The user confirmed the correction: the lead commits the main
worktree before handing out a task, and the executor slot starts from exactly
that revision.

## What Changes

- Before each dispatch the lead fixes a committed snapshot of the source
  checkout: assignment-relevant changes are committed locally (pushing stays
  a separate authorized step) and named through `--base`, or committed HEAD
  is verified to contain every assignment input and named instead.
- The fetched upstream default branch remains the base only for assignments
  with no dependency on local lead state; unrelated dirty work is never
  committed just to form a base.
- Copying files into a live executor slot is not synchronization: changed
  tracked inputs travel as a new commit and a redispatch with the same owner
  id, which rebinds and resynchronizes the same slot.
- Executor briefs name the exact base revision; executors verify their slot
  HEAD equals it before substantive edits and stop with a report on mismatch
  instead of repairing synchronization or creating a substitute tree.
- When the user has not authorized commits, slices that depend on uncommitted
  state stay in the lead or wait for authorization; they are not dispatched
  from a stale base.

## Non-Goals

- No launcher change: `executor spawn --base`, the upstream fetch, the
  fail-closed reset and pool binding already implement the mechanical
  synchronization this change disciplines.
- No push policy: a local snapshot commit does not imply pushing.
- No change to pool sizing, slot reuse or terminal targeting.

## Capabilities

### Modified Capabilities

- `lead-agent-orchestration`: the worktree-pool requirement gains
  committed-snapshot dispatch discipline, executor base verification and the
  no-live-copy rule.
- `agent-delegation`: workstream briefs name the synchronized base revision,
  and executors verify it before substantive edits.
- `global-working-principles`: the portable lead-handoff rule requires a
  committed snapshot before dispatch.

## Impact

- `.agents/skills/team-lead/SKILL.md` (live-linked skill),
  `docs/agent-delegation.md`, `global/principles-of-work.md` (live global
  instructions) and `docs/project-decisions.md` carry the confirmed rule.
