# lead-agent-orchestration Specification

## Purpose

Run asynchronous development with one configured lead session that specifies,
assigns, unblocks, accepts and merges work, and executor sessions that
implement complete outcomes in isolated worktrees and visible terminal
surfaces with durable recovery.

## Requirements

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

### Requirement: Lead activation through the team-lead skill

The kit SHALL provide a `team-lead` skill, delivered through its installation
lifecycle, that activates the lead role in an ordinary Codex session of a
consuming project. The role SHALL be entered by skill invocation, by a user
request that clearly asks for orchestrated asynchronous development, or by a
user request to use executors or lead/executor dispatch for the current work -
including a user question about why executors are unused; such requests SHALL
NOT require the skill name. The main session SHALL also be able to enter the
role on its own judgement, without a user request, when the user's task
decomposes into genuinely parallel, independently verifiable implementation
slices whose orchestration cost the task repays; it SHALL state that
activation and its basis before spawning anything. That autonomous decision
SHALL belong to the main session alone: an executor session or ephemeral
helper agent SHALL NOT activate the role or originate further agents or
executors, and SHALL report such a need to the lead instead. A session that
has not entered the role SHALL NOT spawn executors or create orchestration
state; small direct tasks, read-only work and corrections smaller than their
own delegation overhead SHALL stay direct without orchestration. While the
role is active, the lead SHALL minimize its own token spend and the time to
the accepted result: it keeps for itself judgment, decomposition,
integration, acceptance and work that genuinely exceeds executor capability,
delegates the remaining parallelizable implementation work, and creates no
manufactured slices or delegation-count targets. The skill SHALL own the lead
workflow instructions: role configuration discovery, board setup and
inspection, specification creation, executor briefs through the installed MCP interface,
steering, acceptance, merge and explicit stop. Global instructions SHALL
point to the skill without duplicating its workflow. Leaving the role or
stopping orchestration SHALL remain explicit and preserve partial work.

#### Scenario: A lead session is activated
- **WHEN** the user invokes the `team-lead` skill with a stage goal in a consuming project
- **THEN** the session reads the validated role configuration, prepares the board records and dispatches executors through the installed MCP interface, while sessions without activation behave as before

#### Scenario: A user executor request activates the role
- **WHEN** the user asks to use executors for the current work, or asks why executors are unused, without naming the `team-lead` skill
- **THEN** the session activates the lead role for that work through the skill and dispatches executor-suitable slices instead of reporting a rule conflict

#### Scenario: The main session activates orchestration on its own judgement
- **WHEN** the user requests an implementation outcome that decomposes into parallel, independently verifiable slices, executors are configured and available, and no executor use was requested
- **THEN** the main session enters the role through the skill, states the activation and its basis, and dispatches the parallel slices instead of implementing them alone

#### Scenario: An ordinary session is not hijacked
- **WHEN** a user asks for a small direct task, a read-only answer, or a correction smaller than its own brief, and the role was never activated
- **THEN** the request is served directly, with no executors, board records or orchestration state created

#### Scenario: A trivial correction stays with the lead
- **WHEN** the only remaining change is smaller than the brief, board record and review its delegation would require
- **THEN** the active lead makes the correction itself without creating a task, executor assignment or idle-capacity note

#### Scenario: An executor cannot widen orchestration
- **WHEN** an executor assignment would benefit from another agent or executor
- **THEN** it reports the need to the lead, which owns the further dispatch, instead of activating the role or spawning anything

#### Scenario: The skill is selected from a clear request
- **WHEN** the user asks for orchestrated asynchronous development without naming the skill
- **THEN** the agent selects the `team-lead` skill based on its description before spawning anything, instead of improvising an orchestration workflow

### Requirement: Isolated and visible executor sessions

Every executor assignment SHALL run in a distinct native Codex conversation with its configured effective profile and a harness-owned pool worktree selected, synchronized and bound before its first model request. The same CLI's native /agents interface SHALL provide conversation visibility under `executor-mcp-plugin`; separate terminal surfaces SHALL NOT be required. Executor isolation SHALL use the selected repository's shared harness pool defined by `isolated-worktree-workflow`. The MCP dispatch SHALL acquire and bind through that common owner before launching a session; executor sessions and lead-side recipes MUST NOT create substitute or additional worktrees. The controller SHALL record the slot mapping with the assignment, including slot path and index, owner session identity, archived or unavailable status and synchronized base revision, and preserve it through interruption. Slot release SHALL reuse the tree: after the lead merges or explicitly discards the assignment result, the slot is reset to the merged committed base with ignored build caches kept, and the same slot serves later assignments. A slot whose state cannot be safely reset SHALL be preserved with its reason for lead review; the controller MUST NOT silently reset a dirty slot, force-remove an unmerged tree, delete a branch whose work is not preserved, or treat agents-overview hide, archive or task deletion as slot release. Deleting a pool tree is a slot-retirement action that first establishes the work is preserved and the removal is authorized. A native conversation view SHALL expose the already bound pool checkout instead of allocating another checkout. Ephemeral spawn_agent helpers SHALL be spawned only by the lead; executor sessions SHALL run without agent-spawning tools, so helper and nested-executor conversations cannot originate inside an executor assignment. Executors SHALL NOT write to the shared checkout. Each native conversation SHALL expose assignment, role, actual profile/model, supported effective reasoning effort, live messages, tool activity and state. A hidden process, raw log or single chat identity masking several conversations SHALL NOT satisfy this requirement. The controller and lead SHALL NOT resize, move or arrange desktop windows - including the terminal the lead runs in - to make conversations fit the screen; missing or lost native access SHALL be restored through the owning runtime connection. Automatic model-backed helpers SHALL be visible and attributable or disabled. Ordinary conversation navigation or closing an individual view SHALL NOT establish visibility loss. If the controlling CLI surface or required native access is lost, the controller SHALL suspend new dependent model dispatch, reconcile in-flight effects without replay, report the failure and restore access before continuing.

#### Scenario: Two executors work in parallel
- **WHEN** the lead dispatches two independent assignments to different configured profiles
- **THEN** both executor conversations run concurrently and are accessible through /agents in the same CLI, each bound before its first model request to a distinct slot of the shared worktree pool, with no shared-checkout writes and no worktree created beyond the pool

#### Scenario: Same-terminal tabs satisfy visibility
- **WHEN** MCP dispatch creates distinct titled native conversations reachable from the lead's CLI
- **THEN** the visibility requirement is satisfied without new terminal tabs, panes or windows and without resizing, moving or arranging any desktop window

#### Scenario: Desktop window rearrangement is not used for visibility
- **WHEN** a lead considers making conversation windows fit one screen before or after dispatch
- **THEN** it does not resize, move or tile desktop windows - including its own terminal - and uses the same CLI's native conversation picker

#### Scenario: A remote control view attaches to a managed worktree
- **WHEN** an executor is dispatched through the verified MCP/native runtime path
- **THEN** its session cwd is the already bound pool slot before the first model request, the native view opens without allocating a second checkout, and the shared checkout is unmodified

#### Scenario: An executor window is lost
- **WHEN** the controlling CLI surface or its required native runtime connection is lost while an executor is active
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
- **THEN** the session is bound to a shared pool slot acquired by the MCP dispatch, and neither the executor nor the lead creates a task-named or otherwise ad-hoc substitute worktree for that session

#### Scenario: Overview archive does not retire a worktree
- **WHEN** an executor task is hidden, archived or deleted in the agents overview while its slot worktree still exists
- **THEN** the slot mapping remains until merge or explicit discard, and overview task deletion is not treated as slot release

### Requirement: Board-based asynchronous assignment and feedback

Lead and executors SHALL coordinate routine assignments and feedback through the consuming project's task board using its non-interactive CLI: development stages as epics, specifications as features, executor feedback as feedback tasks, and lead-initiated improvements as tasks, or as OpenSpec changes when they alter accepted requirements. The board SHALL remain consuming-project state; the Rust controller SHALL NOT parse or duplicate it, because lead and executors read and update the board themselves. The kit SHALL deliver board-tool availability and workflow guidance through its installation lifecycle, verified from a fresh external session. Public kit sources SHALL contain only synthetic examples. When the board is unavailable, the orchestrator SHALL report the limitation explicitly and continue only work whose acceptance does not depend on the board. Exceptional questions and escalations SHALL be able to reach the originating lead through the authenticated MCP communication tool while durable blockers, decisions, progress and results remain on the board; messaging SHALL NOT replace the board or introduce a parallel task record.

#### Scenario: An executor escalates a blocker
- **WHEN** an executor cannot proceed because of a missing dependency or an ambiguous requirement
- **THEN** it records the concrete blocker in the existing assignment or appropriate feedback task, can ask its own lead through the authenticated MCP communication tool, and records the resulting durable decision on the board without duplicating task state in the controller

#### Scenario: A fresh session discovers the workflow
- **WHEN** an ordinary Codex session starts in a consuming project after kit installation
- **THEN** board commands and the lead/executor workflow guidance are available without copying kit sources or performing manual setup

#### Scenario: The board is unavailable
- **WHEN** the board tool or its project state is missing or broken
- **THEN** orchestration reports the limitation and affected assignments explicitly instead of silently losing acceptance records or inventing a replacement protocol

### Requirement: Delegation-first utilization without reporting ritual

The lead workflow SHALL dispatch substantial eligible investigation, writing or code to available configured executor capacity before starting that work itself, and SHALL backfill a slot released after acceptance or explicit discard with the next worthwhile dispatchable slice before starting unrelated implementation work itself. Capacity that stays idle SHALL have a nameable cause - no worthwhile slice now, remaining slices dependent on unresolved work, configured pacing, a preserved or blocked slot, or unavailable dispatch with its reported cause - which board and executor-pool state already show: no utilization checkpoint, per-report occupancy statement or kept-current reason note SHALL be required, and occupancy SHALL be read from those records without window polling or model status requests to active executors. A stage or result summary that would otherwise mislead about capacity that sat unused SHALL name the cause in a clause. Sessions without lead activation SHALL remain unchanged.

#### Scenario: Substantial eligible work is dispatched, not solved by the lead
- **WHEN** the lead identifies substantial implementable investigation, writing or code within a configured executor profile's capability while capacity is available
- **THEN** the lead dispatches that work instead of starting it itself, retaining decomposition, integration and acceptance

#### Scenario: A released slot is backfilled
- **WHEN** the lead merges accepted work and releases a slot while a worthwhile, dispatchable slice exists within a configured executor profile's capability
- **THEN** the lead dispatches that slice into the freed capacity before starting unrelated implementation work itself

#### Scenario: Idle capacity needs no note or checkpoint
- **WHEN** configured capacity stays idle, whether the cause is an absent worthwhile slice, an unresolved dependency, configured pacing or a preserved slot
- **THEN** board and executor-pool state already show that cause, no checkpoint, reason note or occupancy statement is required to keep it current, and a summary that would otherwise mislead names the cause in a clause

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

Delivery SHALL include installation, update, check, recovery and disconnection through the supported global kit lifecycle. Fresh ordinary Codex sessions outside this checkout SHALL exercise configured lead/executor dispatch, board workflow, distinct native conversations accessible through /agents in one CLI, worktree isolation, steering, recovery and merge through the real entry point, with actual subscribed tool work on a locally selected real external development task whose applicable acceptance passes and whose results are consumed. Failure acceptance SHALL use owned targets and deterministic quota/transport faults rather than exhausting accounts. Public records SHALL contain no consumer-specific data; a toy fixture alone SHALL NOT establish complete delivery.

#### Scenario: A real consuming task completes
- **WHEN** the globally installed workflow completes a locally selected external development task
- **THEN** its actual result and applicable checks demonstrate integrated use, evidence names the tested identities privately, and public records contain no consumer-specific data

#### Scenario: Recovery is accepted without exhausting a subscription
- **WHEN** an owned test injects a realistic quota failure at the lead and worker boundaries
- **THEN** the installed control path performs the expected succession and preserves work, while separate bounded live requests verify the actual configured profile bindings

### Requirement: Launcher capability preflight and stale-build handling

Before first executor dispatch, the installed MCP interface SHALL verify its own required tool/native-runtime capabilities and recorded installation identity. Missing, incompatible or stale components SHALL produce an actionable diagnosis with the existing lifecycle's update or recovery route and registered source location. Tool discovery or a version string alone SHALL NOT prove conversation attachment, profile execution or delivery. The lead SHALL continue only acceptance-independent work after failed preflight, without profile substitution, raw native-exec bypass, UI automation or replacement helper agents. Native and installed artifact versions SHALL be inspectable without exposing secrets; unrelated sessions SHALL remain intact.

#### Scenario: A consumer lead meets a stale launcher
- **WHEN** MCP discovery or dispatch identifies an installed artifact older than its required contract
- **THEN** the result names the incompatible component and lifecycle remedy before model work, and the lead does not substitute a profile or bypass the managed workflow

#### Scenario: A capable launcher passes the preflight
- **WHEN** the installed MCP and native-runtime contract passes preflight
- **THEN** dispatch uses the configured profile with the ordinary native visibility, board and recovery guarantees, without a separate remembered CLI usage probe

#### Scenario: The manager explains an unknown command
- **WHEN** compatibility callers address an unsupported operation
- **THEN** the diagnostic identifies the unsupported operation and installed build, while active executor recovery guidance points only to the supported plugin lifecycle and does not teach migrated CLI operations

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

### Requirement: Validated structured executor assignment
Executor dispatch SHALL accept a structured assignment as an alternative to existing free text. It SHALL validate declared existing inputs and intended output paths against the allocated checkout, reject missing inputs and path escapes before a model request, and generate the brief with the actual checkout and full committed base. Newly created output files MUST NOT be required to exist. The assignment SHALL retain the agent-selected objective, scope, invariants and acceptance conditions. Resume SHALL preserve partial work and regenerate checkout context without resetting the slot. Existing free-text callers MUST remain compatible.

#### Scenario: Missing declared input
- **WHEN** a structured assignment names an input absent from the allocated checkout
- **THEN** dispatch fails before launching a model and reports the exact missing input

#### Scenario: Input escapes the checkout
- **WHEN** a declared path is absolute, traverses outside the checkout or resolves through an escaping link
- **THEN** the assignment is rejected without reading or writing the escaped target

#### Scenario: New output and existing input
- **WHEN** inputs exist and an owned output is new
- **THEN** dispatch produces a brief naming the actual checkout, base, inputs, outputs and acceptance

#### Scenario: Read-only rendering uses the actual committed base
- **WHEN** a caller supplies a base to the validation/render command
- **THEN** it resolves a full commit and refuses an invalid or mismatching base without changing the checkout

### Requirement: Concise titles and four executor slots

New executor native conversation titles SHALL begin with `CEx` and retain their profile and assignment owner. The shipped orchestration configuration SHALL permit four concurrent executor runs and independently default the shared worktree pool capacity to four without changing the aggregate heavy-command allowance or explicit executor profile.

#### Scenario: Executor terminal identity
- **WHEN** dispatch or continuation exposes an executor conversation
- **THEN** its title uses `CEx ({profile}) - {owner}` and remains distinct from another owner's conversation

#### Scenario: Configured four-slot pool is full
- **WHEN** all four configured shared slots contain live, suspended, reserved or unreviewed tasks of any supported type and a fifth independent owner requests dispatch
- **THEN** the existing pool reports capacity exhaustion without creating another slot or replacing held work

### Requirement: Shared heavy-command admission

Executor heavy commands SHALL use the existing resource and process ownership mechanisms to admit a bounded number of concurrent command trees under one installed aggregate memory envelope and one aggregate CPU ceiling. The envelope SHALL be the job-wide commit limit `JOB_OBJECT_LIMIT_JOB_MEMORY`. Callers from mixed builds SHALL NOT bypass that bound or envelope by locking different files. Independent reads, analysis and edits SHALL remain concurrent. Executor-specific limits SHALL NOT multiply the aggregate memory, CPU or slot allowance. Queue admission and release SHALL be mechanical, with observable waiting, execution, exit and failure. Machine resource settings SHALL remain local.

#### Scenario: Two executors require heavy commands
- **WHEN** independently working executors request overlapping heavy checks and a free slot remains
- **THEN** both admitted command trees run concurrently under the shared aggregate memory envelope and CPU ceiling without lead-managed grants, while a caller beyond the slot bound waits observably

#### Scenario: Resource owner ends
- **WHEN** an admitted command completes, fails or is interrupted
- **THEN** its command tree and admission ownership are released appropriately and a subsequent permitted request can proceed without deleting a live owner's lock

#### Scenario: Mixed builds do not add allowances
- **WHEN** a previous build holds the legacy heavy-command lock and a current executor requests a heavy command
- **THEN** the current request waits or fails through the existing queue path and does not start a second tree beside the legacy holder

### Requirement: Installed autonomy acceptance

Delivery SHALL exercise a small real Rust repair through installed components first, then independent executors, aggregate command admission, clear startup failure, partial-work preservation, continuation and release. Checks SHALL use synthetic projects or owned fixtures and keep private inputs outside the public kit. Global delivery SHALL use the supported lifecycle and preserve unrelated active consumers; a conflicting activation SHALL retain the ready candidate and name the remaining dependency.

#### Scenario: Activation conflicts with another consumer
- **WHEN** the normal installation lifecycle refuses activation because an unrelated active consumer owns affected state
- **THEN** the candidate and its evidence remain available and the conflict is reported without stopping that consumer or replacing links out of band
