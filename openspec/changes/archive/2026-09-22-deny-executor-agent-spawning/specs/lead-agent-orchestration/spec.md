## MODIFIED Requirements

### Requirement: Isolated and visible executor sessions

Every executor assignment SHALL run a native `codex --profile <id>` session on its own visible terminal surface - a dedicated titled tab, a pane or a window - established by the dispatch command before its first model request, and in a harness-owned pool worktree that the dispatch command selects, synchronizes and binds to that session before its first model request. A titled terminal tab in the lead's own terminal SHALL satisfy the visibility requirement; simultaneous on-screen tiling of all conversations SHALL NOT be required. Executor isolation SHALL use the executor worktree pool of the source checkout; the dispatch command SHALL be its sole allocator, and an executor session, the lead, or the `isolated-worktree-workflow` skill MUST NOT create a substitute or additional worktree for an executor assignment. The controller SHALL record the slot mapping with the assignment, including slot path and index, owner session identity, archived or unavailable status and synchronized base revision, and preserve it through interruption. Slot release SHALL reuse the tree: after the lead merges or explicitly discards the assignment result, the slot is reset to the merged committed base with ignored build caches kept, and the same slot serves later assignments. A slot whose state cannot be safely reset SHALL be preserved with its reason for lead review; the controller MUST NOT silently reset a dirty slot, force-remove an unmerged tree, delete a branch whose work is not preserved, or treat agents-overview hide, archive or task deletion as slot release. Deleting a pool tree is a slot-retirement action that first establishes the work is preserved and the removal is authorized. A control view SHALL attach with its cwd at the already bound pool slot instead of allocating another checkout. Ephemeral spawn_agent helpers SHALL be spawned only by the lead; executor sessions SHALL run without agent-spawning tools, so helper and nested-executor conversations cannot originate inside an executor assignment. Executors SHALL NOT write to the shared checkout. Each surface SHALL show assignment, role, actual profile/model, supported effective reasoning effort, live messages, tool activity and state. A hidden process, raw log or single chat identity masking several conversations SHALL NOT satisfy this requirement. The controller and lead SHALL NOT resize, move or arrange desktop windows - including the terminal the lead runs in - to make conversations fit the screen; missing or lost views SHALL be restored through the owning dispatch command. Automatic model-backed helpers SHALL be visible and attributable or disabled. If a required view fails or closes and no other attached view displays that conversation, the controller SHALL suspend new model dispatch for it, reconcile in-flight effects without replay, report the failure and restore visibility before continuing.

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

