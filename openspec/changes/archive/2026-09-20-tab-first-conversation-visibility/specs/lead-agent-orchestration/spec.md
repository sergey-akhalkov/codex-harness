## MODIFIED Requirements

### Requirement: Isolated and visible executor sessions

Every executor assignment SHALL run a native `codex --profile <id>` session on its own visible terminal surface - a dedicated titled tab, a pane or a window - established by the dispatch command before its first model request, and in a Codex-managed Git worktree created from the task's base revision and bound to that session before its first model request. A titled terminal tab in the lead's own terminal SHALL satisfy the visibility requirement; simultaneous on-screen tiling of all conversations SHALL NOT be required. Executor isolation SHALL use that Codex-managed checkout and SHALL NOT substitute an ordinary Git worktree from `isolated-worktree-workflow` or a second harness-owned tree. The controller SHALL record the native worktree mapping with the assignment, including native owner identity (owning thread, archived or unavailable status, path and source revision), preserve it through interruption, and retire the checkout only after the lead merges or explicitly discards the result. Retirement SHALL use the installed CLI's confirmed deletion of a clean managed worktree when that operation applies: the checkout is a managed worktree of the current repository, is not the current checkout or a path alias of it, and contains no local, untracked or ignored changes. When native deletion does not apply or is unavailable, the controller SHALL preserve the checkout and report the limitation; it MUST NOT force-remove a dirty or unmerged worktree, delete a branch whose work is not preserved, or treat agents-overview hide, archive or task deletion as worktree retirement. CLI allocations remain not auto-cleaned. A control view that cannot pass native `--worktree` SHALL attach to the already bound managed checkout instead of allocating another one. Ephemeral spawn_agent helpers SHALL remain in the parent executor checkout and MUST NOT receive a second `--worktree`. Executors SHALL NOT write to the shared checkout. Each surface SHALL show assignment, role, actual profile/model, supported effective reasoning effort, live messages, tool activity and state. A hidden process, raw log or single chat identity masking several conversations SHALL NOT satisfy this requirement. The controller and lead SHALL NOT resize, move or arrange desktop windows - including the terminal the lead runs in - to make conversations fit the screen; missing or lost views SHALL be restored through the owning dispatch command. Automatic model-backed helpers SHALL be visible and attributable or disabled. If a required view fails or closes and no other attached view displays that conversation, the controller SHALL suspend new model dispatch for it, reconcile in-flight effects without replay, report the failure and restore visibility before continuing.

#### Scenario: Two executors work in parallel
- **WHEN** the lead dispatches two independent assignments to different configured profiles
- **THEN** both executor sessions run concurrently on separate titled terminal surfaces (for example tabs of the lead's terminal) and in separate Codex-managed worktrees with distinct live identities and no shared-checkout writes

#### Scenario: Same-terminal tabs satisfy visibility
- **WHEN** executor spawn runs inside the lead's Windows terminal and opens each assignment as a titled tab of that terminal
- **THEN** the visibility requirement is satisfied without opening separate windows and without resizing, moving or arranging any desktop window

#### Scenario: Desktop window rearrangement is not used for visibility
- **WHEN** a lead considers making conversation windows fit one screen before or after dispatch
- **THEN** it does not resize, move or tile desktop windows - including its own terminal - and relies on the dispatch command's titled surfaces instead

#### Scenario: A remote control view attaches to a managed worktree
- **WHEN** an executor is dispatched through the verified remote control path
- **THEN** its session cwd is the already bound Codex-managed worktree before the first model request, the visible surface attaches without allocating a second checkout, and the shared checkout is unmodified

#### Scenario: An executor window is lost
- **WHEN** a required executor view fails or closes while its session is active
- **THEN** new model dispatch for that conversation is suspended, in-flight effects are reconciled without replay, and visibility is restored before continuation

#### Scenario: An interrupted worktree is recovered
- **WHEN** a controller interruption occurs while an executor has partial work in its worktree
- **THEN** the recorded worktree mapping and partial changes survive reconciliation and are reused instead of being recreated

#### Scenario: A clean managed worktree is retired natively
- **WHEN** the lead has merged or explicitly discarded an assignment whose Codex-managed worktree has no local, untracked or ignored changes, is not the current checkout, and belongs to the current repository
- **THEN** retirement uses the CLI confirmed-deletion path for that managed worktree and does not allocate a second harness tree

#### Scenario: A dirty managed worktree cannot be force-removed
- **WHEN** retirement is due but the managed worktree still has local, untracked or ignored changes, or native confirmed deletion is unavailable
- **THEN** the checkout and its mapping are preserved, the limitation is reported, and the tree is not force-removed

#### Scenario: Ordinary Git worktrees are not executor isolation
- **WHEN** an executor assignment needs an isolated checkout
- **THEN** the session is bound to a Codex-managed worktree rather than an ordinary Git worktree created by `isolated-worktree-workflow`

#### Scenario: Overview archive does not retire a worktree
- **WHEN** an executor task is hidden, archived or deleted in the agents overview while its managed worktree still exists
- **THEN** the worktree mapping remains until merge or explicit discard retirement, and overview task deletion is not treated as checkout cleanup

### Requirement: Globally verified orchestration

Delivery SHALL include installation, update, check, recovery and disconnection through the supported global kit lifecycle. Fresh ordinary Codex sessions outside this checkout SHALL exercise configured lead/executor dispatch, board workflow, titled terminal surfaces for every active conversation, worktree isolation, steering, recovery and merge through the real entry point, with actual subscribed tool work on a locally selected real external development task whose applicable acceptance passes and whose results are consumed. Failure acceptance SHALL use owned targets and deterministic quota/transport faults rather than exhausting accounts. Public records SHALL contain no consumer-specific data; a toy fixture alone SHALL NOT establish complete delivery.

#### Scenario: A real consuming task completes
- **WHEN** the globally installed workflow completes a locally selected external development task
- **THEN** its actual result and applicable checks demonstrate integrated use, evidence names the tested identities privately, and public records contain no consumer-specific data

#### Scenario: Recovery is accepted without exhausting a subscription
- **WHEN** an owned test injects a realistic quota failure at the lead and worker boundaries
- **THEN** the installed control path performs the expected succession and preserves work, while separate bounded live requests verify the actual configured profile bindings
