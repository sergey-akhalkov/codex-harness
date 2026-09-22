## MODIFIED Requirements

### Requirement: Executor worktree pool with deterministic slots and upstream freshness

For each source checkout used by an activated lead, executor dispatch SHALL maintain a fixed pool of ordinary Git worktrees sized to the configured maximum concurrent executor count, created as sibling directories of the source checkout and named `<repository-name>-wt1` through `<repository-name>-wtN` in stable slot order. The dispatch command SHALL be the sole pool allocator: it selects a free slot, creating it only when its pool position does not yet exist, and MUST NOT register or create executor worktrees beyond the configured pool size; when every slot is occupied by a live executor session, dispatch SHALL abort with a concrete cause instead of allocating another tree. Before binding a session to a slot, the dispatch command SHALL synchronize the slot with upstream by fetching the configured remote and resetting the slot to the resolved base while removing untracked files and keeping ignored build caches; synchronization SHALL complete before the first model request. The default base SHALL be the upstream default branch, and an explicit base override SHALL be supported for assignments that must start from another revision. The lead SHALL fix a committed snapshot before dispatch: an assignment that depends on source-checkout state SHALL name, through the explicit base override, a revision that contains every assignment input - changes committed in the source checkout before dispatch (a local commit; pushing remains a separate authorized step) or verified committed HEAD - while unrelated dirty work SHALL NOT be committed just to form a base, and an assignment whose required inputs cannot be committed within authorization SHALL NOT be dispatched from a stale base. Executor briefs SHALL name the synchronized base revision; an executor SHALL verify that its slot HEAD equals that base before substantive edits and SHALL stop and report a mismatch instead of repairing synchronization or creating a substitute tree. Copying files into a live executor slot SHALL NOT be accepted as synchronization: changed tracked inputs SHALL travel as a new commit and a redispatch, and reusing the owner id SHALL rebind and resynchronize the same slot. Failed upstream fetch, an occupied dirty slot, a free slot holding unreviewed changes, or a missing or unusable slot SHALL abort dispatch with the concrete cause before any model request, instead of degrading to a stale base, an extra tree or silent data loss. Slot occupancy SHALL be reconciled with executor session liveness, and the worktree inventory check SHALL enforce the pool invariant instead of warning: harness dispatch never creates beyond the pool, reports preserved slots with their reasons, and reports non-pool trees as foreign or legacy items for lead review rather than silently absorbing them.

#### Scenario: Pool size matches configured concurrency
- **WHEN** the maximum concurrent executor count is configured as two and executor dispatch first runs for a repository checkout
- **THEN** dispatch uses exactly the sibling slots named `<repository-name>-wt1` and `<repository-name>-wt2` and never creates a third executor tree for that checkout

#### Scenario: Dispatch beyond the pool is refused
- **WHEN** every pool slot is occupied by a live executor session and the lead dispatches another assignment
- **THEN** dispatch aborts naming the occupied slots, no new worktree is registered, and the lead can wait or stop a running assignment instead

#### Scenario: A slot is reused instead of recreated
- **WHEN** an earlier assignment was merged or explicitly discarded and a new assignment is dispatched into the same lane
- **THEN** the next dispatch binds the same slot path after reset, and `git worktree list` shows no additional executor tree for that dispatch

#### Scenario: Upstream freshness is mechanical, not prompt-side
- **WHEN** a slot is selected for dispatch and the resolved base is beyond the slot's previous base
- **THEN** the slot reaches that base before the first model request, and no obligation on the executor to synchronize the checkout is needed for freshness

#### Scenario: Dependent lead work is committed before dispatch
- **WHEN** an assignment depends on uncommitted changes in the source checkout
- **THEN** the lead commits those changes locally before dispatch and names the new revision as the explicit base, and no files are copied into the live executor slot

#### Scenario: Commits are not authorized
- **WHEN** the user has not authorized commits and a slice depends on uncommitted source state
- **THEN** the lead keeps that slice or asks for snapshot-commit authorization instead of dispatching it from a stale base or transferring files into a live slot

#### Scenario: The executor verifies the named base
- **WHEN** an executor begins work and its slot HEAD differs from the base named in its brief
- **THEN** it stops dependent edits and reports the mismatch, and the lead corrects the slot by redispatching with the same owner id

#### Scenario: Upstream fetch failure blocks dispatch
- **WHEN** the configured remote cannot be fetched during slot synchronization
- **THEN** dispatch aborts with the fetch failure before any model request, the slot keeps its prior state, and no executor session starts from a stale base through that path

#### Scenario: An explicit base override is honored
- **WHEN** an assignment must start from a revision other than the upstream default branch
- **THEN** dispatch names that revision, the slot is reset to it after the upstream fetch, and the recorded slot mapping carries that base

#### Scenario: A free slot with unreviewed work is preserved
- **WHEN** a slot's executor session has ended but the slot still holds unmerged or unreviewed changes
- **THEN** dispatch does not select or reset that slot, reports it with its reason, and the lead must merge or explicitly discard the work before the slot re-enters the pool
