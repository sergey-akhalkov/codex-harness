## MODIFIED Requirements

### Requirement: Isolated and visible executor sessions

Every executor assignment SHALL run a native `codex --profile <id>` session on its own visible terminal surface - a dedicated titled tab, a pane or a window - established by the dispatch command before its first model request, and in a harness-owned pool worktree that the dispatch command selects, synchronizes and binds to that session before its first model request. A titled terminal tab in the lead's own terminal SHALL satisfy the visibility requirement; simultaneous on-screen tiling of all conversations SHALL NOT be required. Executor isolation SHALL use the executor worktree pool of the source checkout; the dispatch command SHALL be its sole allocator, and an executor session, the lead, or the `isolated-worktree-workflow` skill MUST NOT create a substitute or additional worktree for an executor assignment. The controller SHALL record the slot mapping with the assignment, including slot path and index, owner session identity, archived or unavailable status and synchronized base revision, and preserve it through interruption. Slot release SHALL reuse the tree: after the lead merges or explicitly discards the assignment result, the slot is reset to the merged committed base with ignored build caches kept, and the same slot serves later assignments. A slot whose state cannot be safely reset SHALL be preserved with its reason for lead review; the controller MUST NOT silently reset a dirty slot, force-remove an unmerged tree, delete a branch whose work is not preserved, or treat agents-overview hide, archive or task deletion as slot release. Deleting a pool tree is a slot-retirement action that first establishes the work is preserved and the removal is authorized. A control view SHALL attach with its cwd at the already bound pool slot instead of allocating another checkout. Ephemeral spawn_agent helpers SHALL remain in the parent executor slot. Executors SHALL NOT write to the shared checkout. Each surface SHALL show assignment, role, actual profile/model, supported effective reasoning effort, live messages, tool activity and state. A hidden process, raw log or single chat identity masking several conversations SHALL NOT satisfy this requirement. The controller and lead SHALL NOT resize, move or arrange desktop windows - including the terminal the lead runs in - to make conversations fit the screen; missing or lost views SHALL be restored through the owning dispatch command. Automatic model-backed helpers SHALL be visible and attributable or disabled. If a required view fails or closes and no other attached view displays that conversation, the controller SHALL suspend new model dispatch for it, reconcile in-flight effects without replay, report the failure and restore visibility before continuing.

#### Scenario: Two executors work in parallel
- **WHEN** the lead dispatches two independent assignments to different configured profiles
- **THEN** both executor sessions run concurrently on separate titled terminal surfaces (for example tabs of the lead's terminal), each bound before its first model request to a distinct slot of the executor worktree pool, with no shared-checkout writes and no worktree created beyond the pool

#### Scenario: Same-terminal tabs satisfy visibility
- **WHEN** executor spawn runs inside the lead's Windows terminal and opens each assignment as a titled tab of that terminal
- **THEN** the visibility requirement is satisfied without opening separate windows and without resizing, moving or arranging any desktop window

#### Scenario: Desktop window rearrangement is not used for visibility
- **WHEN** a lead considers making conversation windows fit one screen before or after dispatch
- **THEN** it does not resize, move or tile desktop windows - including its own terminal - and relies on the dispatch command's titled surfaces instead

#### Scenario: A remote control view attaches to a managed worktree
- **WHEN** an executor is dispatched through the verified remote control path
- **THEN** its session cwd is the already bound pool slot before the first model request, the visible surface attaches without allocating a second checkout, and the shared checkout is unmodified

#### Scenario: An executor window is lost
- **WHEN** a required executor view fails or closes while its session is active
- **THEN** new model dispatch for that conversation is suspended, in-flight effects are reconciled without replay, and visibility is restored before continuation

#### Scenario: An interrupted worktree is recovered
- **WHEN** a controller interruption occurs while an executor has partial work in its slot worktree
- **THEN** the recorded slot mapping and partial changes survive reconciliation and the same slot is reused instead of being recreated

#### Scenario: A clean managed worktree is retired natively
- **WHEN** the lead has merged or explicitly discarded an assignment whose slot worktree contains no unreviewed work
- **THEN** the slot is reset to the merged committed base with ignored build caches kept and the next dispatch binds the same slot path without creating a new tree

#### Scenario: A dirty managed worktree cannot be force-removed
- **WHEN** slot release is due but the slot still holds local, untracked or unmerged work that the lead has not merged or explicitly discarded
- **THEN** the slot and its mapping are preserved, the limitation is reported with its cause, and the tree is not reset, force-removed or counted as free

#### Scenario: Ordinary Git worktrees are not executor isolation
- **WHEN** an executor assignment needs an isolated checkout
- **THEN** the session is bound to a pool slot allocated by the dispatch command, and neither the executor nor the lead creates a task-named or otherwise ad-hoc substitute worktree for that session

#### Scenario: Overview archive does not retire a worktree
- **WHEN** an executor task is hidden, archived or deleted in the agents overview while its slot worktree still exists
- **THEN** the slot mapping remains until merge or explicit discard, and overview task deletion is not treated as slot release

### Requirement: Lead acceptance and merge

The lead SHALL review each completed assignment against its accepted requirements and applicable checks before acceptance, merge accepted branches itself, and return concrete defects with their acceptance conditions to the original executor when correction is within its scope. A rejected assignment SHALL retain its partial work and slot worktree until corrected or explicitly discarded. Acceptance decisions and merges SHALL be recorded in task state and reflected on the board; the lead SHALL NOT claim completion for integrated work whose checks have not passed.

#### Scenario: Accepted work is merged
- **WHEN** an executor branch passes the applicable checks and meets its requirements
- **THEN** the lead merges it, the slot is reset to the merged base for later reuse when its state is safely resettable, otherwise preserved with an explicit limitation, and the board and task records show the accepted outcome

#### Scenario: A defect returns to the executor
- **WHEN** review finds a concrete defect that the original executor can correct within its scope
- **THEN** the lead returns the finding and acceptance condition to that executor instead of rewriting the work, and the partial result is preserved

## ADDED Requirements

### Requirement: Executor worktree pool with deterministic slots and upstream freshness

For each source checkout used by an activated lead, executor dispatch SHALL maintain a fixed pool of ordinary Git worktrees sized to the configured maximum concurrent executor count, created as sibling directories of the source checkout and named `<repository-name>-wt1` through `<repository-name>-wtN` in stable slot order. The dispatch command SHALL be the sole pool allocator: it selects a free slot, creating it only when its pool position does not yet exist, and MUST NOT register or create executor worktrees beyond the configured pool size; when every slot is occupied by a live executor session, dispatch SHALL abort with a concrete cause instead of allocating another tree. Before binding a session to a slot, the dispatch command SHALL synchronize the slot with upstream by fetching the configured remote and resetting the slot to the resolved base while removing untracked files and keeping ignored build caches; synchronization SHALL complete before the first model request. The default base SHALL be the upstream default branch, and an explicit base override SHALL be supported for assignments that must start from another revision. Failed upstream fetch, an occupied dirty slot, a free slot holding unreviewed changes, or a missing or unusable slot SHALL abort dispatch with the concrete cause before any model request, instead of degrading to a stale base, an extra tree or silent data loss. Slot occupancy SHALL be reconciled with executor session liveness, and the worktree inventory check SHALL enforce the pool invariant instead of warning: harness dispatch never creates beyond the pool, reports preserved slots with their reasons, and reports non-pool trees as foreign or legacy items for lead review rather than silently absorbing them.

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
- **WHEN** a slot is selected for dispatch and the upstream default branch has advanced beyond the slot's previous base
- **THEN** the slot reaches the fetched upstream base before the first model request, and no obligation on the executor to synchronize the checkout is needed for freshness

#### Scenario: Upstream fetch failure blocks dispatch
- **WHEN** the configured remote cannot be fetched during slot synchronization
- **THEN** dispatch aborts with the fetch failure before any model request, the slot keeps its prior state, and no executor session starts from a stale base through that path

#### Scenario: An explicit base override is honored
- **WHEN** an assignment must start from a revision other than the upstream default branch
- **THEN** dispatch names that revision, the slot is reset to it after the upstream fetch, and the recorded slot mapping carries that base

#### Scenario: A free slot with unreviewed work is preserved
- **WHEN** a slot's executor session has ended but the slot still holds unmerged or unreviewed changes
- **THEN** dispatch does not select or reset that slot, reports it with its reason, and the lead must merge or explicitly discard the work before the slot re-enters the pool
