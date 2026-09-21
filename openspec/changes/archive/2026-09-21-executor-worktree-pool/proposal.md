## Why

Executor isolation currently delegates worktree allocation to the native Codex
CLI (`--enable worktrees --worktree`): each dispatch creates a new
auto-named tree, allocations are never auto-cleaned, and the kit's
lane-reuse guidance lives only in skill text. Consuming repositories
therefore accumulate task-named and CLI-named worktrees while concurrency
stays configured and small, and an executor can start from a stale base
because upstream synchronization is a prompt obligation rather than a
mechanism. The count, names, reuse and freshness of executor checkouts need
to be guarantees of the dispatch path, not habits of the lead.

## What Changes

- Executor isolation moves from per-run Codex-managed allocation to a
  harness-owned pool of ordinary Git worktrees with one slot per configured
  concurrent executor (`max_concurrent_executors`), created next to the
  source checkout and named `<repo-name>-wt1`, `<repo-name>-wt2`, ... up to
  the pool size. **BREAKING** for the documented behavior that executor
  isolation must use Codex-managed worktrees and must never use an ordinary
  Git worktree.
- `codex-harness executor spawn` becomes the sole pool owner: it selects a
  free slot, synchronizes it with upstream (`git fetch` plus reset to the
  resolved base and `git clean -fd`, keeping ignored build caches) before
  the first model request, binds the executor session to that slot, and
  records the slot mapping (path, owner session, base revision).
- The default synchronization base is the upstream default branch; an
  explicit base override remains available for assignments that must start
 from another revision.
- Dispatch is fail-closed: no free slot, failed upstream fetch, an occupied
 slot that is dirty, or a missing/unusable slot aborts dispatch with the
  concrete cause instead of allocating an extra worktree.
- Slot release after accepted merge reuses the existing reset-for-reuse
  semantics (reset to the merged base, keep ignored caches); a slot with
  unaccepted or unmergeable work is preserved with its reason and never
  silently reset.
- The worktree inventory check becomes an enforced invariant of the pool
  instead of a warning: harness dispatch never registers more than the
  configured pool slots for a source checkout.
- The `team-lead` skill instructions and agent-delegation documentation are
  updated to slot-based lanes: executors never create additional worktrees,
  and upstream synchronization is performed by the dispatch mechanism, not
  by executor prompt discipline.

## Capabilities

### New Capabilities

- None.

### Modified Capabilities

- `lead-agent-orchestration`: executor isolation requirements change from
  Codex-managed per-run allocation to a harness-owned fixed worktree pool
  with deterministic slot names, mechanical upstream synchronization before
  the first model request, fail-closed allocation, recorded slot ownership,
  reset-for-reuse after merge, and an enforced pool-size invariant replacing
  the warning-level worktree limit.
- `isolated-worktree-workflow`: the allocator boundary wording is updated so
  executor isolation is allocated by the orchestration harness pool, not by
  this workflow; the prohibition on this workflow substituting an isolation
  checkout for an executor session is unchanged.

## Impact

- `crates/harness-core/src/task_worktree.rs`: pool slot naming/selection,
 occupancy, upstream synchronization and base resolution; existing mapping
  and reset-for-reuse logic is reused; test-only allocation is replaced by
  the production pool.
- `crates/codex-harness/src/executor_cli.rs`: spawn flow selects and syncs a
  pool slot, launches the executor in it, and stops passing native
  `--worktree` isolation flags for pooled dispatch.
- `global/orchestration.toml` consumers: pool size derives from
  `max_concurrent_executors`; `worktree_limit` changes from a warning
  threshold to the enforced pool invariant (or is superseded by it).
- `global` skill text and `docs/agent-delegation.md`: lane model changes to
  fixed slots; legacy task-named and CLI-named executor worktrees in
  consuming repositories are reviewed and retired by the lead, not deleted
  automatically.
- Native Rust tests cover slot count, naming, reuse after reset, dirty-slot
  preservation, fetch failure and base override; installed-launcher
  integration verifies reuse through the real entry point on a real
  repository.
