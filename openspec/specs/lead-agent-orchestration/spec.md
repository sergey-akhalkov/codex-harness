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
inspection, specification creation, executor briefs through harness commands,
steering, acceptance, merge and explicit stop. Global instructions SHALL
point to the skill without duplicating its workflow. Leaving the role or
stopping orchestration SHALL remain explicit and preserve partial work.

#### Scenario: A lead session is activated
- **WHEN** the user invokes the `team-lead` skill with a stage goal in a consuming project
- **THEN** the session reads the validated role configuration, prepares the board records and dispatches executors through harness commands, while sessions without activation behave as before

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

The lead SHALL deliver steering to an executor through the kit's addressed executor message command into that executor's live conversation, and the delivered input SHALL appear in that executor's visible conversation. The message command SHALL address one existing run through the accepted checkout, slot, owner and exact recorded session identity, SHALL verify that identity against the actual run before delivery so a message cannot reach a later occupant of a reused slot, and SHALL accept a short literal text or a UTF-8 file containing multiline text, preserving real line breaks and literal content without shell evaluation. Delivery SHALL go to the same conversation with its context, model, provider, reasoning effort and completed work preserved, and SHALL NOT be implemented through hidden stop/resume, a new conversation, model replacement or re-sending the whole task. While an executor is working, delivery SHALL use the backend's real capability to accept additional input, including at the nearest supported point during a running tool call, and SHALL NOT interrupt that tool call. The command SHALL distinguish queued input, confirmed delivery and error, SHALL NOT present a local file write as delivery to the model, and a retry after an indeterminate result SHALL NOT silently deliver the same context twice. For a completed, stopped or unavailable run it SHALL return an explicit result with a supported next action and SHALL NOT start a new conversation. Waiting and routine event handling SHALL require no model calls, and no repeated status requests SHALL be sent to active executors. Steering SHALL add relevant facts, resolve a request, or correct an established mistake; status-only nudges, hurry demands and repeated messages without new facts SHALL NOT be sent. Executor reports and escalations SHALL reach the lead as board feedback tasks with bounded context rather than unbounded transcript copies.

#### Scenario: A mid-assignment correction arrives
- **WHEN** the lead sends a concrete correction while an executor is working
- **THEN** the input appears in that executor's conversation, the executor continues in the same session and worktree, and no duplicate session is started

#### Scenario: A path correction is delivered verbatim
- **WHEN** the lead sends a multiline UTF-8 correction from a file while the executor is inside a long tool call
- **THEN** the message is delivered at the nearest supported point without interrupting that tool call, its literal text appears in the executor's conversation, and the reported result distinguishes delivery from later application by the executor

#### Scenario: A stale identity is refused
- **WHEN** the addressed slot no longer runs the recorded session, or the slot was reused by another run
- **THEN** message refuses with the observed identity mismatch and names a supported next action instead of delivering to the current occupant

#### Scenario: A finished run cannot be messaged
- **WHEN** message addresses a run whose recorded lifecycle is completed, stopped or unavailable
- **THEN** the command returns that actual state with the continuation remedy and starts no new conversation

#### Scenario: A healthy executor is waiting
- **WHEN** an executor remains active beyond several observation waits without a demonstrated failure
- **THEN** waiting generates no model status requests and the executor's ownership is preserved

#### Scenario: A nudge without new facts is not sent
- **WHEN** the lead is tempted to ask for status or hurry an executor that has no new decision-relevant fact
- **THEN** no message is sent, and steering is reserved for concrete corrections, facts or requirement changes

### Requirement: Urgent executor stop through the kit command

The kit SHALL provide an executor stop command that urgently stops one exact executor run identified through the accepted checkout, slot, owner and exact recorded session identity. Stop SHALL NOT wait for task completion, a model answer or a long running command: it SHALL use the backend's native interruption signal where that provides urgent interruption, and SHALL boundedly terminate the remaining owned process tree when the signal is unavailable, insufficient or exceeded, without leaving a hung stop lacking a diagnosable result. It SHALL verify actual run and process ownership before acting - an old process id, program-name or window-title match SHALL NOT be sufficient. It SHALL close exactly the stopped run's terminal tab or pane through the existing terminal-surface owner and SHALL NOT close the lead's terminal, neighboring tabs or a window hosting other conversations. It SHALL stop further delivery of pending messages to that run and record their undelivered state honestly. It SHALL preserve changed and untracked files, the checkout, already received results and the information needed to continue: no reset, clean, worktree deletion or loss of partial work SHALL occur, the run SHALL NOT be marked complete, accepted or merged, the slot SHALL NOT be released or freed for another task, and continuation and release SHALL remain separate explicit actions. The lifecycle receipt SHALL record the observed outcome - stopped, already completed, partial stop or error - with an unknown exit code kept unknown, and stop SHALL NOT claim rollback of external actions or recovery of external systems. A repeated stop SHALL be safe and report the actual current state, a stop racing natural completion SHALL report which outcome won, and the result SHALL remain available to the lead after the tab closes. On partial failure the command SHALL name the surviving process or resource, the cause and the next action instead of reporting false success.

#### Scenario: Stop during generation
- **WHEN** the lead stops an executor while the model is generating
- **THEN** the run ends promptly, its owned processes are gone, only its terminal tab closes, the receipt records the stopped state, and its files remain for continuation

#### Scenario: Stop during a child command
- **WHEN** the lead stops an executor while a long child command is running
- **THEN** the child and its owned tree are terminated within the bounded stop path, the executor's tab closes, and the stop result reports actual termination rather than assuming the child ended with the host

#### Scenario: Stop races natural completion
- **WHEN** the run completes while stop is verifying identity or interrupting
- **THEN** the command reports the completed state and its recorded result instead of recording a stop that did not happen

#### Scenario: Stop is repeated
- **WHEN** stop is issued again for an already stopped run
- **THEN** it reports the stopped state without error, without touching another run and without releasing the slot

#### Scenario: Only the executor's tab closes
- **WHEN** the stopped executor shares a terminal window with the lead and other executors
- **THEN** exactly the stopped run's tab closes and every other conversation surface remains usable

#### Scenario: A partial stop is not success
- **WHEN** the host ends but an owned child survives the bounded termination
- **THEN** stop reports the surviving process, cause and next action, and the receipt records a partial stop instead of success

### Requirement: Control-backed executor conversations

Pooled executor dispatch SHALL run its managed conversations, including the default native TUI presentation and its pooled continuation, on a backend that actually accepts addressed input and urgent interruption for the live session. A command wrapper over an operation the backend does not support SHALL NOT be accepted as message or stop delivery, and absent capability SHALL NOT be masked by a successful response. The owning dispatch route SHALL provide the capability with the smallest necessary change while preserving the existing receipt, lease, slot, terminal, presentation, result-recording and process-ownership guarantees, and the change SHALL NOT alter the configured profile's model, provider or reasoning effort. Surfaces that remain unsupported for addressed input SHALL report that fact explicitly with the supported continuation path instead of pretending delivery. Attaching the native frontend SHALL NOT create a second conversation, duplicate an assignment or invalidate the existing message, stop, cache-loss protection or recovery contracts. Input accepted through the native TUI SHALL address the same managed conversation and participate in its observed lifecycle.

#### Scenario: A dispatched executor is addressable

- **WHEN** a pooled executor run is working in managed TUI or explicit exec presentation
- **THEN** its live conversation accepts an addressed message and an urgent stop through the kit commands without a new conversation or task re-send

#### Scenario: An unsupported surface is honest

- **WHEN** message addresses a legacy or unmanaged interactive executor run whose surface has no verified inbound channel
- **THEN** the command reports that surface as unsupported with its continuation remedy instead of reporting delivery

#### Scenario: Profile binding is preserved

- **WHEN** a pooled executor conversation runs on the control-backed route
- **THEN** its recorded and displayed model, provider and reasoning effort remain those of the configured executor profile

#### Scenario: TUI input belongs to the observed run

- **WHEN** a user submits input through the managed native frontend while its run accepts input
- **THEN** it reaches the same conversation once and its accepted work is reflected in the run lifecycle, without an older turn's completion prematurely finishing that work

#### Scenario: Stop retains exact ownership with a frontend attached

- **WHEN** the lead stops one of two active managed TUI runs
- **THEN** the existing urgent-stop contract interrupts and contains only that run, closes its owned surface, preserves its files and continuation identity, and leaves the other executor and lead running


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

### Requirement: Observable executor lifecycle and bounded result

The existing dispatch and receipt owners SHALL distinguish request acceptance, terminal creation, observed native start, running, completion, failure and lead acceptance. Native session identity, checkout/base, changed files, actual reported checks and outcomes, remaining work, limitations, required decision and a detail locator SHALL be available without manual rollout searches. Completion and errors SHALL reach the lead through the native observation path. Empty completion SHALL be an output defect, not evidence of model, authentication or quota unavailability. Every model conversation SHALL retain its distinct visible terminal surface.

For managed TUI runs, `executor watch` SHALL retain its existing receipt/slot addressing, options, blocking behavior, timeout behavior, bounded text/JSON review result and exit meanings: 0 for completion, 1 for an unsuccessful terminal outcome, and 2 for timeout or unavailable coverage. Native TUI presentation SHALL NOT by itself make coverage unavailable. Watch SHALL learn completion from the current run's correlated native outcome with the final result or output defect preserved, independently of the frontend's willingness to exit. Prior-session or prior-turn events, quiet output and frontend process termination alone SHALL NOT establish successful completion. A timeout SHALL NOT stop the run. No model-side status polling, periodic model messages, terminal scraping or manual rollout search SHALL be required to wait. A lost host or connection SHALL remain diagnosable and SHALL NOT be reported as success or leave waiting unbounded beyond the requested timeout. Historical receipts SHALL remain honestly readable without inventing missing coverage.

Additional reply-workflow observation SHALL extend this base contract without replacing it. A native turn ending while a required reply remains unresolved SHALL report the workflow's nonterminal waiting result rather than completion or empty-output defect. Its action-required result SHALL NOT change the meanings of completion, failure and timeout/unavailable results above or authorize slot release.

#### Scenario: Terminal opens but native start fails

- **WHEN** a dispatch creates its terminal and its child cannot start or exits unsuccessfully
- **THEN** the receipt and observer report the actual stage and failure, preserve diagnostics and do not report successful model execution

#### Scenario: Work completes

- **WHEN** an executor finishes its accepted work with no unresolved reply hold
- **THEN** a native observer emits a bounded completion or output-defect event with the exact session, slot, result and detail locator without model-side status polling

#### Scenario: Interrupted assignment resumes

- **WHEN** an original assignment stops with partial work
- **THEN** continuation uses its recorded exact session and checkout without reset, retains visible identity and preserves changes for correction and acceptance

#### Scenario: Accepted slot is released

- **WHEN** the lead accepts and integrates an outcome and records its disposition
- **THEN** the existing release operation records that decision before resetting the slot for reuse; unreviewed work remains preserved

#### Scenario: Watch completes while the frontend would otherwise await input

- **WHEN** the current assignment has a persisted terminal result with no unresolved reply hold but its native TUI would ordinarily remain at an input prompt
- **THEN** the same blocking watch call returns the bounded result and correct exit status without requiring the user or lead to close the frontend first

#### Scenario: Watch timeout leaves the executor working

- **WHEN** the requested watch timeout expires while the managed run is active
- **THEN** watch returns 2 with the ongoing state and detail locator, the TUI and assignment remain active, and a subsequent watch can observe the same run

#### Scenario: Completion carries no usable final answer

- **WHEN** the current turn reports completion, no unresolved reply hold remains and no nonempty final answer can be retained
- **THEN** watch reports an output defect with its diagnostics and no false success or claim that the model, authentication or quota was unavailable

#### Scenario: Old terminal events do not finish a continuation

- **WHEN** a resumed session or a newly accepted turn receives history or terminal events from earlier work
- **THEN** watch remains bound to the current run and accepted work until its own terminal outcome or an observed failure is established


### Requirement: Concise titles and four executor slots

New executor terminal titles SHALL begin with `CEx` and retain their profile and assignment owner. The shipped orchestration configuration SHALL permit four concurrent executor slots without changing the aggregate heavy-command allowance or explicit executor profile.

#### Scenario: Executor terminal identity
- **WHEN** dispatch or continuation opens an executor terminal
- **THEN** its title uses `CEx ({profile}) - {owner}` and remains distinct from another owner's terminal

#### Scenario: Configured four-slot pool is full
- **WHEN** all four configured slots contain live or unreviewed work and a fifth independent owner requests dispatch
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

### Requirement: Executor tabs close when the host ends

When dispatch opens an executor conversation as a Windows Terminal tab, that tab SHALL close after the host process ends, including when the recorded run failed, was interrupted, or ended as an output defect. Closing SHALL NOT send a terminal command, SHALL NOT close the lead's window, a sibling tab, or another conversation, and SHALL NOT change the receipt's recorded state or exit code. The receipt, detail file and control log remain the inspection surface. An owned console that is not a terminal tab SHALL keep returning the run's own exit code. A process killed before the host can exit is outside this close path; stop continues to close its tab by ending that run's host.

#### Scenario: A failed executor tab closes
- **WHEN** an executor tab's host records a failed run and the host process ends
- **THEN** that tab closes, the receipt still records the failure and its cause, and no other conversation's tab closes

#### Scenario: A successful executor tab still closes
- **WHEN** an executor tab's host records a completed run and the host process ends
- **THEN** that tab closes and the receipt records the completed outcome

#### Scenario: An owned console keeps the run exit code
- **WHEN** the same failed run is hosted in an owned console instead of a terminal tab
- **THEN** the console process returns the run's own non-zero exit code and the receipt records the failure

### Requirement: An oversized final-message read does not fail a completed turn

After a turn has completed, the host SHALL read the final assistant message without treating a transport size limit on a full-thread read as a failed run. When that read exceeds the transport limit and the turn already delivered a nonempty assistant message, the host SHALL record that message and complete the run. When no such message was delivered, the host SHALL record an output defect that names the size limit, and SHALL NOT report success. In either case the host SHALL NOT terminate the owned child tree as a crash and SHALL NOT leave the failure unrecorded. A final-message read that fails for a reason other than the transport size limit SHALL still fail the host and terminate the owned child tree.

#### Scenario: The full thread exceeds the transport limit
- **WHEN** a turn completes, its assistant message was already delivered, and reading the whole thread exceeds the transport limit
- **THEN** the run is recorded completed with that message, the child tree is not terminated as a crash, and the receipt does not say the final message could not be read

#### Scenario: No delivered message and the thread read is too large
- **WHEN** a turn completes without a delivered nonempty assistant message and the full-thread read exceeds the transport limit
- **THEN** the receipt records an output defect that names the size limit, and the run is not reported as a successful completion

#### Scenario: Another final-message read failure still fails the host
- **WHEN** a completed turn's final-message read fails for a reason other than the transport size limit
- **THEN** the host records the failure, terminates the owned child tree, and does not report a successful run

### Requirement: Native TUI for managed executor runs

Ordinary pooled `executor spawn`, `executor resume` and `executor restart` SHALL present each managed conversation in the native Codex TUI on its dedicated titled terminal surface without requiring an additional caller option. The explicit TUI mode SHALL provide the same managed observation and control. The frontend SHALL display the exact bound conversation, assignment, actual model and supported effective effort, live messages, tool activity and state. It SHALL retain the recorded slot cwd, configured provider/model/effort and single-agent restrictions. Controller output SHALL NOT interleave with or corrupt the native TUI. Before the first model request, dispatch SHALL establish the frontend's attachment to that conversation and its live terminal surface; a process or tab existing alone SHALL NOT establish readiness. A failed or unsupported attachment SHALL report its cause and recovery action instead of silently substituting a text stream or an invisible conversation. Existing command spellings, including the assignment input named `--exec`, SHALL remain accepted; explicitly selected exec presentation SHALL retain its supported observation behavior.

The supported explicit exec presentation SHALL reuse a qualified native presentation wherever equivalent; retaining its spelling SHALL NOT require a second conversation or control lifecycle. Any residual compatibility adaptation SHALL name the observed missing native property and preserve the same observation and control contract.

#### Scenario: Default dispatch shows the native conversation

- **WHEN** the lead dispatches an assignment with the existing spawn command and no presentation option
- **THEN** a native TUI is attached to the bound slot and exact managed conversation before its first model request, showing its configured identity and subsequent messages and tools without mixed controller output or a duplicate assignment

#### Scenario: Concurrent executor views remain distinct

- **WHEN** two pooled runs are active in different slots
- **THEN** each TUI displays its own recorded conversation and each observer reports only its bound run, without switching either model or sharing a conversation identity

#### Scenario: Attachment fails before work starts

- **WHEN** the native frontend cannot attach or the target CLI lacks the required capability
- **THEN** no assignment model request is submitted, the failure and remedy are available through the dispatch observation, and no hidden or text-only substitute starts

#### Scenario: Continuation preserves the managed presentation

- **WHEN** the lead resumes the recorded exact session or restarts a fresh session in a preserved slot
- **THEN** the continued run has a native TUI and full observation/control, resume retains the original session, restart records its new session and predecessor, and neither operation resets partial work or replays the old assignment as a new request

### Requirement: Managed TUI lifetime follows the recorded run

Once a managed run reaches its terminal outcome, the host SHALL preserve its result or failure and then end that run's owned frontend/backend and finish its terminal surface automatically, without a manual quit step. This SHALL retain the terminal-host closure policy, real run outcome and exit code, durable detail locators, process-ownership guarantees and partial work. Closing SHALL affect no neighboring conversation or terminal window and SHALL NOT imply acceptance, merge or slot release. A cleanup failure SHALL name surviving owned resources and the recovery action, separately from the run outcome, rather than claiming that closure succeeded. If the only attached frontend is lost during active work, the controller SHALL suspend new model dispatch, interrupt or contain the owned run, preserve its work and record interruption or a more specific observed failure. It SHALL NOT allow unseen continuation or report success from frontend exit alone. Loss of focus or selecting another terminal tab SHALL NOT count as frontend loss.

A native turn ending with an unresolved required reply SHALL NOT establish a terminal run or trigger this cleanup. The existing reply workflow SHALL retain its live surface and resources until reply continuation or an actual failure/stop. Backend cleanup SHALL release only run-owned resources; a shared native server and neighboring sessions SHALL remain available.

#### Scenario: Successful work closes its TUI automatically

- **WHEN** the current run completes with a nonempty final answer and no unresolved reply hold
- **THEN** the answer and successful outcome remain available to watch after the owned frontend closes, run-owned backend resources are released and the executor tab closes, without a user quit command or effects on neighboring tabs or shared services

#### Scenario: An unsuccessful run retains its evidence after closure

- **WHEN** the current run records a failure, interruption or output defect and its host finishes
- **THEN** its TUI surface follows the terminal-host closure policy while watch retains the real unsuccessful outcome, cause and diagnostics instead of inferring success from the host exit used to close a tab

#### Scenario: The user closes an active TUI

- **WHEN** the only frontend disappears while an assignment or its tool is active
- **THEN** further model dispatch is suspended, the owned run is interrupted or contained with surviving processes reported honestly, its partial work and identity remain, and watch reports an unsuccessful outcome with the supported continuation route

#### Scenario: Completion races frontend exit

- **WHEN** a terminal backend event and frontend exit occur near the same time
- **THEN** the retained native evidence determines the outcome, a previously recorded result is not lost, and frontend exit alone never manufactures a successful completion
