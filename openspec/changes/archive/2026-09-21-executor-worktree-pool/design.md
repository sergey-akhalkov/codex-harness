## Context

`executor spawn` currently launches sessions in a workspace and, when that
workspace is the shared checkout, passes native `--enable worktrees
--worktree` so the Codex CLI allocates a new auto-named managed tree per run;
those allocations are never auto-cleaned. `reset_for_reuse`, `Mapping` and
the audit exist in `crates/harness-core/src/task_worktree.rs`, but lane
creation, occupancy and upstream freshness are lead habits encoded in skill
text, so consuming repositories accumulated task-named trees beyond the
configured concurrency. See proposal.md for motivation.

## Goals / Non-Goals

**Goals:**

- Mechanical guarantees for executor checkout count, deterministic names,
  reuse and upstream freshness, owned by the dispatch command rather than
  prompt discipline.
- Preserve unreviewed executor work under every failure and reconciliation
  path; reset only what was merged or explicitly discarded, or what a clean
  tree can lose without data loss.
- Reuse the existing mapping, reset-for-reuse, audit and task-state
  reconciliation instead of a parallel mechanism.

**Non-Goals:**

- No new isolation mechanism for ordinary (non-executor) sessions; the
  `isolated-worktree-workflow` capability keeps its own behavior.
- No automatic deletion or adoption of legacy task-named or CLI-named
  worktrees in consuming repositories; the lead reviews and retires them.
- No cross-machine or cross-user pool sharing; the pool is local to one
  source checkout and one active lead, matching the existing single-lead
  contract.

## Decisions

### 1. The dispatch command owns the pool; skill text only describes it

`executor spawn` performs slot selection, synchronization, session binding
and mapping recording before the first model request. Count, naming, reuse
and freshness become invariants of the code path.

Alternative rejected: strengthening the `team-lead` skill instructions.
Instructions cannot guarantee behavior across sessions, and the observed
worktree growth happened exactly under instruction-level lane guidance.

### 2. Ordinary harness-owned Git worktrees as siblings of the checkout

Pool slots are created with `git worktree add --detach` into sibling
directories of the canonical source checkout and named
`<repository-name>-wt1` ... `<repository-name>-wtN`, where
`<repository-name>` is the source directory name and `N` is
`max_concurrent_executors`. The session runs with its cwd at the slot and
without native `--worktree` isolation flags.

Alternatives rejected: continuing native CLI allocation with a cleanup
wrapper (no control over names, count or slot binding, and allocation
behavior is not a stable contract we can build reuse on); placing slots
under `$CODEX_HOME` (hides them from the repository workspace and breaks
the requested sibling naming).

Collision policy: if a slot path exists but is not a registered worktree of
this source checkout (an unrelated directory or another repository's tree),
dispatch aborts with that cause instead of adopting or replacing it.

### 3. Slot state machine with review disposition, not filesystem guessing

```
free(clean) --dispatch--> synchronizing --> occupied(live session)
     ^                                        |
     |                                        | session ends
     |                                        v
     +-- reset to merged base <-- released(merged|discarded recorded)
                                    awaiting-review(dirty, unreviewed)
                                          |
                                          | lead merges/discards + records
                                          v
                                    released (as above)
```

- A slot is occupied only while its bound executor session is live;
  liveness comes from existing task-runtime reconciliation, not from lock
  files, which go stale after crashes.
- A dirty slot without a live session is awaiting review: dispatch never
  selects or resets it and reports it with its reason. The lead must merge
  or explicitly discard before the slot re-enters the pool.
- Reset destroys data only when the slot is clean, or its disposition is
  recorded as merged or explicitly discarded. `reset_for_reuse` keeps its
  current unresettable-state preservation semantics.
- Executor branches are ordinary refs in the shared object database; slot
  reset never deletes branches, and branch cleanup stays a lead action with
  the existing preservation rule.

### 4. Upstream synchronization is fail-closed and precedes every dispatch

Selected slot: `git fetch <remote>`; resolve base (upstream default branch
by default, explicit override for assignment-specific bases); `git reset
--hard <base>`; `git clean -fd` (ignored build caches stay warm); verify
HEAD and a clean status. Fetch failure aborts dispatch with the cause and
leaves the slot untouched - a stale base is never a silent fallback, because
working from stale source is the failure this change exists to prevent.

The default-branch and remote names are resolved from the repository
(`remote show` / `symbolic-ref` of the remote HEAD), not hardcoded. The
explicit override accepts a revision accepted by `git rev-parse` after the
fetch.

### 5. Explicit slot release and discard for the lead

The harness exposes an explicit slot release path (record disposition
`merged` or `discarded` with reason, then reset or preserve per
`reset_for_reuse`). This keeps pool bookkeeping consistent when the lead
accepts work outside a dispatch and gives rejected work a safe, recorded
way back into the pool. Exact CLI spelling is an implementation choice
within the existing `executor` command family.

### 6. CLI surface stays compatible; pooled dispatch becomes the default

`--source` remains the repository checkout. `--workspace` stops being the
isolation mechanism: when omitted, spawn derives the slot from `--source`.
When supplied, it must be the source checkout or a pool slot; ad-hoc
task-named worktree paths are refused with a migration hint, because
silently accepting them would recreate unbounded lane creation. Native
`--worktree` flags leave the pooled exec path; the worktrees feature flag
no longer gates executor isolation.

### 7. Audit enforces the pool; `worktree_limit` is superseded

The inventory check classifies registered trees into pool slots of this
source and foreign/legacy trees. Dispatch allocates only within the pool
and refuses rather than warns when no free slot exists; foreign and legacy
trees are reported for lead review and never absorbed or deleted. The
`worktree_limit` warning path is removed from dispatch; the field remains
accepted in `orchestration.toml` for compatibility and is documented as
superseded by the pool size.

## Risks / Trade-offs

- [Reset could destroy unreviewed work] -> Only clean or dispositioned
  slots are resettable; dirty slots without live sessions are preserved and
  reported; `reset_for_reuse` already refuses unresettable state.
- [Two harness processes race on one pool] -> Slot claims are recorded in
  kit-local task state with a compare-and-set style update; the existing
  single-active-lead and reconciliation contract bounds concurrent writers,
  and a lost race surfaces as an occupied-slot refusal, not corruption.
- [Same repository name checked out twice in one parent directory] ->
  Detected by the collision policy; dispatch refuses with the concrete path
  conflict instead of guessing.
- [Fetch adds latency and network dependence to dispatch] -> Accepted by
  requirement: freshness before work. Fail-closed keeps the failure visible
  instead of spreading stale-base rework.
- [Legacy consumer trees confuse the inventory at first run] -> Classified
  as foreign/legacy and reported; migration is an explicit lead review, and
  nothing is deleted automatically.

## Migration Plan

1. Implement the pool in `harness-core` and the `executor` CLI behind the
   existing command names; extend native tests first around slot naming,
   cap, reuse, dirty preservation, fetch failure and base override.
2. Update the `team-lead` skill text and `docs/agent-delegation.md` from
   task-named lanes to fixed slots, remove the executor-side sync
   obligation, and document the explicit release path and superseded
   `worktree_limit`.
3. Rebuild and update the installed launcher through the installation
   lifecycle; verify the stale-build probe still passes.
4. In each consuming repository, the lead reviews legacy task-named and
   CLI-named worktrees, merges accepted work, removes retired trees with
   authorized `git worktree remove`, and prunes prunable entries; the first
   pooled dispatch then creates the sibling slots.
5. Verify through the real entry point on a real repository: repeated
   dispatches reuse the same slot paths, `git worktree list` shows the
   source plus exactly the pool slots, and private evidence stays local.

Rollback: revert the installed launcher and kit skill links through the
installation lifecycle; pool slots are ordinary Git worktrees and can be
removed manually after their work is preserved, leaving no harness-specific
state in the repository.
