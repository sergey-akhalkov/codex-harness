# Spec Delta

## MODIFIED Requirements

### Requirement: Configurable lead and executor roles

The kit SHALL provide reusable orchestration configuration that names the lead session profile, the executor profiles available for assignment, the maximum number of concurrent executors, and the independent managed worktree pool capacity defined by `isolated-worktree-workflow`. Dispatch SHALL use exactly the configured profiles and SHALL NOT hardcode provider-specific roles, while explicit user profile selection retains its native precedence. The existing installation check SHALL validate that every configured profile is installed and that both limits are positive, reporting a concrete error without substituting another profile or silently disabling orchestration. Unrelated user configuration, credentials and profiles SHALL be preserved.

#### Scenario: Roles come from configuration
- **WHEN** the configuration names one lead profile and two executor profiles
- **THEN** executor dispatch launches native sessions with exactly those profiles and each visible conversation reports its effective provider, model and reasoning effort

#### Scenario: A configured profile is missing
- **WHEN** a named profile is absent from the target installation
- **THEN** check and dispatch report the missing profile and take no substitute model action

#### Scenario: Repository selection does not require copying kit configuration
- **WHEN** an installed executor is dispatched into a selected consuming repository
- **THEN** it uses the installed orchestration configuration and the selected repository's common pool, without requiring a copy of kit configuration inside that repository

### Requirement: Isolated and visible executor sessions

Every executor assignment SHALL run a native `codex --profile <id>` session on its own visible terminal surface - a dedicated titled tab, a pane or a window - established by the dispatch command before its first model request, and in a harness-owned pool worktree that the dispatch command selects, synchronizes and binds to that session before its first model request. A titled terminal tab in the lead's own terminal SHALL satisfy the visibility requirement; simultaneous on-screen tiling of all conversations SHALL NOT be required. Executor isolation SHALL use the selected repository's shared harness pool defined by `isolated-worktree-workflow`. The dispatch command SHALL acquire and bind through that common owner before launching a session; executor sessions and lead-side recipes MUST NOT create substitute or additional worktrees. The controller SHALL record the slot mapping with the assignment, including slot path and index, owner session identity, archived or unavailable status and synchronized base revision, and preserve it through interruption. Slot release SHALL reuse the tree: after the lead merges or explicitly discards the assignment result, the slot is reset to the merged committed base with ignored build caches kept, and the same slot serves later assignments. A slot whose state cannot be safely reset SHALL be preserved with its reason for lead review; the controller MUST NOT silently reset a dirty slot, force-remove an unmerged tree, delete a branch whose work is not preserved, or treat agents-overview hide, archive or task deletion as slot release. Deleting a pool tree is a slot-retirement action that first establishes the work is preserved and the removal is authorized. A control view SHALL attach with its cwd at the already bound pool slot instead of allocating another checkout. Ephemeral spawn_agent helpers SHALL be spawned only by the lead; executor sessions SHALL run without agent-spawning tools, so helper and nested-executor conversations cannot originate inside an executor assignment. Executors SHALL NOT write to the shared checkout. Each surface SHALL show assignment, role, actual profile/model, supported effective reasoning effort, live messages, tool activity and state. A hidden process, raw log or single chat identity masking several conversations SHALL NOT satisfy this requirement. The controller and lead SHALL NOT resize, move or arrange desktop windows - including the terminal the lead runs in - to make conversations fit the screen; missing or lost views SHALL be restored through the owning dispatch command. Automatic model-backed helpers SHALL be visible and attributable or disabled. If a required view fails or closes and no other attached view displays that conversation, the controller SHALL suspend new model dispatch for it, reconcile in-flight effects without replay, report the failure and restore visibility before continuing.

#### Scenario: Two executors work in parallel
- **WHEN** the lead dispatches two independent assignments to different configured profiles
- **THEN** both executor sessions run concurrently on separate titled terminal surfaces (for example tabs of the lead's terminal), each bound before its first model request to a distinct slot of the shared worktree pool, with no shared-checkout writes and no worktree created beyond the pool

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
- **THEN** the session is bound to a shared pool slot acquired by the dispatch command, and neither the executor nor the lead creates a task-named or otherwise ad-hoc substitute worktree for that session

#### Scenario: Overview archive does not retire a worktree
- **WHEN** an executor task is hidden, archived or deleted in the agents overview while its slot worktree still exists
- **THEN** the slot mapping remains until merge or explicit discard, and overview task deletion is not treated as slot release

### Requirement: Executor worktree pool with deterministic slots and upstream freshness

Executor dispatch SHALL consume the selected repository's shared harness pool under `isolated-worktree-workflow`; ordinary isolated tasks and executors SHALL count toward the same independently configured capacity. The existing `executor spawn`, `resume`, `restart`, `pool` and `release` entry points SHALL remain supported as adapters to the common lifecycle, with executor concurrency enforced separately. An explicit repository selector SHALL allow installed dispatch to target a consuming repository while preserving the existing `--source` meaning and default target when that selector is omitted. Dispatch SHALL select, synchronize and bind a free slot before its first model request, or report the actual capacity, concurrency, ownership or preparation failure without creating an extra checkout. The default executor base SHALL remain the resolved upstream default branch for compatibility. An explicit locally available base SHALL not require a fetch; a base requiring remote resolution or requested refresh SHALL fail closed if that operation fails. The lead SHALL fix a committed snapshot before dispatch: an assignment that depends on source-checkout state SHALL name a revision containing every assignment input, using authorized local commits or verified committed HEAD without committing unrelated dirty work. An assignment whose required inputs cannot be committed within authorization SHALL remain with the lead rather than use a stale base. Executor briefs SHALL name the exact synchronized base, and executors SHALL verify it before substantive edits and report any mismatch without repairing synchronization or allocating a substitute tree. Copying files into a live slot SHALL NOT count as synchronization: changed assignment inputs SHALL use a new committed base after acceptance or explicit discard and release of the previous task binding. Repeating acquisition or continuation for a still-held task SHALL preserve its HEAD and partial work; owner reuse alone SHALL NOT authorize reset. Pool inventory SHALL report all task types and preserved reasons, and report non-pool worktrees as foreign or legacy rather than silently absorbing them.

#### Scenario: Pool size is independent of configured concurrency
- **WHEN** pool capacity is four, maximum concurrent executors is two and dispatch first runs for a repository
- **THEN** it allocates only within the four stable pool positions, starts at most two executors and shares remaining capacity with ordinary isolated tasks

#### Scenario: Pool size matches configured concurrency
- **WHEN** a legacy configuration has maximum concurrent executors two and no explicit pool capacity
- **THEN** migration preserves capacity two as the explicit pool value, so the two existing stable slot positions remain the limit until that independent setting is deliberately changed

#### Scenario: Dispatch beyond the pool is refused
- **WHEN** every pool slot is held by a live, suspended, reserved or unreviewed task and the lead dispatches another assignment
- **THEN** dispatch aborts naming the occupied slots, no new worktree is registered, and the lead can wait or complete the acceptance and release of a held task instead; stopping a run alone does not free its slot

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
- **THEN** it stops dependent edits and reports the mismatch, and the lead preserves the mismatched checkout, resolves the cause and performs an explicitly authorized release or correction before redispatch rather than treating owner reuse as reset permission

#### Scenario: Upstream fetch failure blocks dispatch
- **WHEN** dispatch requires an upstream refresh or remote base resolution and the configured remote cannot be fetched
- **THEN** dispatch aborts with the fetch failure before any model request, the slot keeps its prior state, and no executor session starts from a stale base through that path

#### Scenario: An explicit base override is honored
- **WHEN** an assignment must start from a revision other than the upstream default branch
- **THEN** dispatch names that revision, the slot is synchronized to its exact commit without requiring a fetch when that commit is already available locally, and the recorded slot mapping carries that base

#### Scenario: A free slot with unreviewed work is preserved
- **WHEN** a slot's executor session has ended but the slot still holds unmerged or unreviewed changes
- **THEN** dispatch does not select or reset that slot, reports it with its reason, and the lead must merge or explicitly discard the work before the slot re-enters the pool

#### Scenario: Ordinary isolation consumes the last slot
- **WHEN** all shared pool slots are held by ordinary experiments and reviews while executor concurrency is available
- **THEN** executor dispatch reports pool exhaustion without starting a model or allocating another worktree

#### Scenario: Existing executor command remains compatible
- **WHEN** a caller uses an existing executor command without a repository selector
- **THEN** its previous target selection, configured profile, visible terminal behavior and default upstream-base behavior remain available while its allocation uses the common pool

### Requirement: Concise titles and four executor slots

New executor terminal titles SHALL begin with `CEx` and retain their profile and assignment owner. The shipped orchestration configuration SHALL permit four concurrent executor runs and independently default the shared worktree pool capacity to four without changing the aggregate heavy-command allowance or explicit executor profile.

#### Scenario: Executor terminal identity
- **WHEN** dispatch or continuation opens an executor terminal
- **THEN** its title uses `CEx ({profile}) - {owner}` and remains distinct from another owner's terminal

#### Scenario: Configured four-slot pool is full
- **WHEN** all four configured shared slots contain live, suspended, reserved or unreviewed tasks of any supported type and a fifth independent owner requests dispatch
- **THEN** the existing pool reports capacity exhaustion without creating another slot or replacing held work
