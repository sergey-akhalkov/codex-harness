# lead-agent-orchestration Specification

## Purpose

Run asynchronous development with one configured lead session that specifies,
assigns, unblocks, accepts and merges work, and executor sessions that
implement complete outcomes in isolated worktrees and visible terminal
surfaces with durable recovery.

## Requirements

### Requirement: Configurable lead and executor roles

The kit SHALL provide reusable orchestration configuration that names the lead session profile, the executor profiles available for assignment, and the maximum number of concurrent executors. Dispatch SHALL use exactly the configured profiles and SHALL NOT hardcode provider-specific roles, while explicit user profile selection retains its native precedence. The existing installation check SHALL validate that every configured profile is installed and that the concurrency limit is positive, reporting a concrete error without substituting another profile or silently disabling orchestration. Unrelated user configuration, credentials and profiles SHALL be preserved.

#### Scenario: Roles come from configuration
- **WHEN** the configuration names one lead profile and two executor profiles
- **THEN** executor dispatch launches native sessions with exactly those profiles and each visible conversation reports its effective provider, model and reasoning effort

#### Scenario: A configured profile is missing
- **WHEN** a named profile is absent from the target installation
- **THEN** check and dispatch report the missing profile and take no substitute model action

### Requirement: Explicit lead activation through the team-lead skill

The kit SHALL provide a `team-lead` skill, delivered through its installation lifecycle, that activates the lead role in an ordinary Codex session of a consuming project. The role SHALL be entered explicitly by skill invocation or by a user request that clearly asks for orchestrated asynchronous development; an ordinary session SHALL NOT spawn executors or create orchestration state without that activation. The skill SHALL own the lead workflow instructions: role configuration discovery, board setup and inspection, specification creation, executor briefs through harness commands, steering, acceptance, merge and explicit stop. Global instructions SHALL point to the skill without duplicating its workflow. Leaving the role or stopping orchestration SHALL remain explicit and preserve partial work.

#### Scenario: A lead session is activated
- **WHEN** the user invokes the `team-lead` skill with a stage goal in a consuming project
- **THEN** the session reads the validated role configuration, prepares the board records and dispatches executors through harness commands, while sessions without activation behave as before

#### Scenario: An ordinary session is not hijacked
- **WHEN** a user asks for a small direct task in a session where the role was never activated
- **THEN** the request is served directly, with no executors, board records or orchestration state created

#### Scenario: The skill is selected from a clear request
- **WHEN** the user asks for orchestrated asynchronous development without naming the skill
- **THEN** the agent selects the `team-lead` skill based on its description before spawning anything, instead of improvising an orchestration workflow

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

### Requirement: Board-based asynchronous assignment and feedback

Lead and executors SHALL coordinate routine assignments and feedback through the consuming project's task board using its non-interactive CLI: development stages as epics, specifications as features, executor feedback as feedback tasks, and lead-initiated improvements as tasks, or as OpenSpec changes when they alter accepted requirements. The board SHALL remain consuming-project state; the Rust controller SHALL NOT parse or duplicate it, because lead and executors read and update the board themselves. The kit SHALL deliver board-tool availability and workflow guidance through its installation lifecycle, verified from a fresh external session. Public kit sources SHALL contain only synthetic examples. When the board is unavailable, the orchestrator SHALL report the limitation explicitly and continue only work whose acceptance does not depend on the board.

#### Scenario: An executor escalates a blocker
- **WHEN** an executor cannot proceed because of a missing dependency or an ambiguous requirement
- **THEN** it records a feedback task with the concrete blocker on the project board, the lead processes that task, and the resolution is visible to the assignment without duplicating the record in controller state

#### Scenario: A fresh session discovers the workflow
- **WHEN** an ordinary Codex session starts in a consuming project after kit installation
- **THEN** board commands and the lead/executor workflow guidance are available without copying kit sources or performing manual setup

#### Scenario: The board is unavailable
- **WHEN** the board tool or its project state is missing or broken
- **THEN** orchestration reports the limitation and affected assignments explicitly instead of silently losing acceptance records or inventing a replacement protocol

### Requirement: Lead steering and executor escalation without waste

The lead SHALL deliver steering to an executor through the controller's verified session channel, and the delivered input SHALL appear in that executor's visible conversation. Waiting and routine event handling SHALL require no model calls, and no repeated status requests SHALL be sent to active executors. Steering SHALL add relevant facts, resolve a request, or correct an established mistake. Executor reports and escalations SHALL reach the lead as board feedback tasks with bounded context rather than unbounded transcript copies.

#### Scenario: A mid-assignment correction arrives
- **WHEN** the lead sends a concrete correction while an executor is working
- **THEN** the input appears in that executor's conversation, the executor continues in the same session and worktree, and no duplicate session is started

#### Scenario: A healthy executor is waiting
- **WHEN** an executor remains active beyond several observation waits without a demonstrated failure
- **THEN** waiting generates no model status requests and the executor's ownership is preserved

### Requirement: Utilization checkpoints and observable occupancy

The lead workflow SHALL check executor utilization at session start, at stage or epic planning, after each dispatch decision, and after each acceptance or slot release. At each checkpoint the lead SHALL either dispatch the next worthwhile, capability-sized slice to available executor capacity or record the concrete reason the capacity stays idle. Lead progress reporting SHALL state executor occupancy against the configured concurrency limit and the recorded reason for every idle executor or free slot, derived from board and executor-pool records without window polling or model status requests to active executors. When a slot is released after acceptance or explicit discard and a worthwhile dispatchable slice exists, the lead SHALL backfill that capacity before starting unrelated implementation work itself. Sessions without lead activation SHALL remain unchanged.

#### Scenario: Progress reports occupancy
- **WHEN** the lead reports progress while orchestration is active
- **THEN** the report states how many configured executor slots are busy and the recorded reason for every idle executor or free slot, derived from board and pool records

#### Scenario: A released slot is backfilled
- **WHEN** the lead merges accepted work and releases a slot while a worthwhile, dispatchable slice exists within a configured executor profile's capability
- **THEN** the lead dispatches that slice into the freed capacity before starting unrelated implementation work itself

#### Scenario: An idle reason is refreshed
- **WHEN** the circumstance behind a recorded idle reason changes, such as a dependency resolving, a quota window resetting, or a preserved slot returning to the pool
- **THEN** the next utilization checkpoint re-evaluates the capacity and either dispatches a worthwhile slice or records the updated reason

#### Scenario: An ordinary session is unchanged
- **WHEN** a session without lead activation works a small direct task
- **THEN** no executor is spawned, no orchestration state is created, and no utilization reporting is required

### Requirement: Lead acceptance and merge

The lead SHALL review each completed assignment against its accepted requirements and applicable checks before acceptance, merge accepted branches itself, and return concrete defects with their acceptance conditions to the original executor when correction is within its scope. A rejected assignment SHALL retain its partial work and slot worktree until corrected or explicitly discarded. Acceptance decisions and merges SHALL be recorded in task state and reflected on the board; the lead SHALL NOT claim completion for integrated work whose checks have not passed.

#### Scenario: Accepted work is merged
- **WHEN** an executor branch passes the applicable checks and meets its requirements
- **THEN** the lead merges it, the slot is reset to the merged base for later reuse when its state is safely resettable, otherwise preserved with an explicit limitation, and the board and task records show the accepted outcome

#### Scenario: A defect returns to the executor
- **WHEN** review finds a concrete defect that the original executor can correct within its scope
- **THEN** the lead returns the finding and acceptance condition to that executor instead of rewriting the work, and the partial result is preserved

### Requirement: Durable authorized task state

A recoverable task SHALL retain its workspace and worktree identity, accepted objective, constraints and authorization, current decisions, active lead, assignments and owners, visible partial results, acceptance state and next useful action. State SHALL be saved before dispatch and ownership transitions and reconciled after interruption. Provider credentials, raw private evidence and task state SHALL remain in host-private storage. Reassignment SHALL NOT expand authorization, change a read-only/exploration task into implementation, drop unfinished requirements or transfer opaque reasoning state between providers.

#### Scenario: Quota fails after a partial edit
- **WHEN** an executor has changed files in its worktree but cannot complete the next request
- **THEN** recovery retains the changes and check status and gives the replacement enough visible context to continue without recreating completed work

#### Scenario: Exploration leadership changes
- **WHEN** a read-only planning task transfers to another lead profile
- **THEN** the successor inherits the planning boundary and cannot treat the transfer as permission to implement

### Requirement: Single ownership and cause-aware recovery

The runtime SHALL distinguish quota exhaustion, temporary throttling, authentication/model rejection, transport failure and incomplete output from retained provider evidence, preserving original cause, scope and uncertainty. Confirmed unavailability SHALL exclude the affected route for a bounded, cause-appropriate period instead of probing it on every assignment. Reassignment SHALL select a capable configured executor and preserve partial work. When no connected profile can serve the next required work, the task SHALL retain its checkpoint, report the reason and any known recovery condition, and suspend without purchases, repeated model probes or infinite retries. One active lead SHALL own decisions and acceptance for a task, and workers SHALL NOT create recursive agent trees.

#### Scenario: A worker fails after partial work
- **WHEN** an executor receives a confirmed quota or model refusal after changing files
- **THEN** the runtime reassigns the remaining work to a capable configured executor with the preserved partial result and original cause, without racing the failed attempt

#### Scenario: All suitable capacity is exhausted
- **WHEN** no connected profile can serve the next required work
- **THEN** the task retains its checkpoint, reports the reason, and resumes only when capacity or the user changes that condition

#### Scenario: An empty completion is not a provider failure
- **WHEN** an executor ends with an intermediate response and partial artifacts but no provider rejection
- **THEN** recovery reconciles its visible events and artifacts instead of marking any provider unavailable

### Requirement: Lead succession at safe boundaries

The installed task runtime SHALL observe assignment outcomes and perform already authorized lead recovery without requiring a successful response from the unavailable lead. On a confirmed lead failure, the controller SHALL establish exactly one successor lead from the configured capable profiles, with current decisions, assignments and acceptance intact, in its own identified visible conversation, and only at a safe boundary where in-flight executor effects have been reconciled - never in the middle of an executor tool call. A configured preferred lead SHALL regain leadership at a safe decision boundary after verified recovery, without interrupting a healthy executor or duplicating completed work. Problems beyond available capability SHALL remain explicit dependencies rather than being guessed away. Instruction-refresh succession is owned by the follow-up change and SHALL NOT be claimed by this capability. Unsupported control contracts SHALL produce an explicit capability limitation before activation.

#### Scenario: The lead cannot issue a fallback instruction
- **WHEN** an active lead receives a confirmed quota-exhaustion response before it can request recovery
- **THEN** the runtime initiates succession from the configured capable profiles without another response from the failed lead and preserves active executor work

#### Scenario: The preferred lead returns during healthy work
- **WHEN** the preferred lead profile has recovered while an executor is still productively working
- **THEN** leadership returns at a safe boundary with current decisions and evidence, and the executor retains its assignment

#### Scenario: No capable lead is available
- **WHEN** every configured lead profile is unavailable but an executor can finish an existing assignment
- **THEN** that result is delivered, hard unresolved decisions remain pending, and the task does not claim full completion before required acceptance

### Requirement: Restart recovery and explicit stop

After an unexpected controller or client interruption, the runtime SHALL reconcile saved task, process and worktree state and restore required visible conversation views before resuming active authorized work. It SHALL reconnect or recover surviving assignments instead of starting duplicate copies. Explicit user stop, task cancellation or integration disconnection SHALL disable automatic dispatch for the affected task and SHALL NOT be undone by a quota reset or process restart. Stop SHALL preserve recoverable partial work and prevent orphaned owned writers. Losing one client SHALL NOT suspend another task whose required conversations remain visible.

#### Scenario: The controller restarts with surviving workers
- **WHEN** the controller restarts after failure and executors may still be active
- **THEN** ownership, liveness and worktrees are reconciled before any reassignment and only the still-active authorized task resumes

#### Scenario: The user stops a suspended task
- **WHEN** the user explicitly stops a suspended task and capacity later recovers
- **THEN** it remains stopped until the user resumes it, with its partial work available

### Requirement: Globally verified orchestration

Delivery SHALL include installation, update, check, recovery and disconnection through the supported global kit lifecycle. Fresh ordinary Codex sessions outside this checkout SHALL exercise configured lead/executor dispatch, board workflow, titled terminal surfaces for every active conversation, worktree isolation, steering, recovery and merge through the real entry point, with actual subscribed tool work on a locally selected real external development task whose applicable acceptance passes and whose results are consumed. Failure acceptance SHALL use owned targets and deterministic quota/transport faults rather than exhausting accounts. Public records SHALL contain no consumer-specific data; a toy fixture alone SHALL NOT establish complete delivery.

#### Scenario: A real consuming task completes
- **WHEN** the globally installed workflow completes a locally selected external development task
- **THEN** its actual result and applicable checks demonstrate integrated use, evidence names the tested identities privately, and public records contain no consumer-specific data

#### Scenario: Recovery is accepted without exhausting a subscription
- **WHEN** an owned test injects a realistic quota failure at the lead and worker boundaries
- **THEN** the installed control path performs the expected succession and preserves work, while separate bounded live requests verify the actual configured profile bindings

### Requirement: Launcher capability preflight and stale-build handling

Before the first executor dispatch in an activated lead session, the `team-lead` skill SHALL verify that the installed launcher implements executor dispatch by running the kit's executor usage probe (`codex-harness executor --help`) and requiring its success. When the probe reports an unsupported or unknown command, the session SHALL classify the failure as an installed launcher older than the kit instructions, report that blocker together with the kit's rebuild-and-update remedy and the registered kit source location, and continue only work whose acceptance does not depend on executors. The session SHALL NOT treat the failure as proof that executors are unavailable, substitute another profile, or bypass harness dispatch with raw native exec, TUI automation or in-session helper agents, because those paths drop the visible-conversation, steering, board and recovery contract. The manager's unknown-command diagnostic SHALL name the attempted command and state that kit instructions referencing it indicate an installed build older than the kit source, with the kit lifecycle as the remedy. An installed immutable launcher SHALL identify its recorded source identity through its own version output when that record is present.

#### Scenario: A consumer lead meets a stale launcher
- **WHEN** an activated lead session probes `codex-harness executor --help` and the installed launcher answers with an unsupported-command error
- **THEN** the session reports a stale launcher with the rebuild-and-update remedy and the kit source location, performs no executor dispatch, no profile substitution and no raw native-exec bypass, and continues only acceptance-independent work

#### Scenario: A capable launcher passes the preflight
- **WHEN** the probe prints the executor usage
- **THEN** executor dispatch proceeds through the harness command with the configured profile and the ordinary visibility, board and recovery contract

#### Scenario: The manager explains an unknown command
- **WHEN** the manager receives a command it does not implement
- **THEN** its diagnostic names that command, points at `--help`, and explains that kit instructions referencing the command mean the installed build is older than the kit source and must be rebuilt and updated through the kit lifecycle

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

### Requirement: Deterministic executor terminal targeting

When the lead runs inside Windows Terminal, executor dispatch SHALL open the
executor tab in the lead's own terminal window, and MUST NOT address the
terminal's most-recently-used or focused window. Dispatch SHALL identify the
lead's window through the console parenting exposed to the calling process,
hold it foreground only as long as the terminal needs to resolve the tab there,
and restore the user's previous foreground window and selected tab once the
titled tab is observably open; the user's deliberate window switches SHALL be
left untouched. When the lead's window cannot be addressed (another virtual
desktop or a blocked activation), dispatch SHALL target the stable
per-checkout window name derived from the source checkout, which the terminal
creates on first use, and MUST NOT create the tab in an unrelated focused
window; an explicit window name requested by configuration or arguments SHALL
be targeted exactly. Activation escalation MUST NOT send synthetic input into
any conversation surface.

#### Scenario: Another project holds the user's focus
- **WHEN** the lead running in one terminal window dispatches an executor while the user works in another window
- **THEN** the executor tab opens in the lead's window, no new terminal window is created, the user's window keeps its foreground position and its selected tab

#### Scenario: The lead window cannot be addressed
- **WHEN** the lead's window is on another virtual desktop or its activation is blocked
- **THEN** the tab opens in the per-checkout harness window instead of the user's focused window, and the user's foreground window is still restored

#### Scenario: An explicit window name is requested
- **WHEN** dispatch receives an explicit terminal window name
- **THEN** the tab opens in exactly that window, and the terminal creates it first when it does not exist

#### Scenario: The terminal activates late
- **WHEN** the terminal summons the receiving window after the launcher exits or activates it again for the created tab
- **THEN** dispatch keeps undoing those activations until they go quiet and leaves the user in their previous foreground window
