## Purpose

Run asynchronous development with one configured lead session that specifies,
assigns, unblocks, accepts and merges work, and executor sessions that
implement complete outcomes in isolated worktrees and visible windows with
durable recovery.

## ADDED Requirements

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

Every executor assignment SHALL run a native `codex --profile <id>` session in its own visible terminal window established before its first model request, and in a dedicated Git worktree created from the task's base revision. The controller SHALL record the worktree mapping with the assignment, preserve it through interruption, and retire it only after the lead merges or explicitly discards the result; executors SHALL NOT write to the shared checkout. Each window SHALL show assignment, role, actual profile/model, supported effective reasoning effort, live messages, tool activity and state. A switchable list, hidden process, raw log or single chat identity masking several conversations SHALL NOT satisfy this requirement. Automatic model-backed helpers SHALL be visible and attributable or disabled. If a required view fails or closes and no other attached view displays that conversation, the controller SHALL suspend new model dispatch for it, reconcile in-flight effects without replay, report the failure and restore visibility before continuing.

#### Scenario: Two executors work in parallel
- **WHEN** the lead dispatches two independent assignments to different configured profiles
- **THEN** both executor sessions run simultaneously in separate windows and separate worktrees with distinct live identities and no shared-checkout writes

#### Scenario: An executor window is lost
- **WHEN** a required executor view fails or closes while its session is active
- **THEN** new model dispatch for that conversation is suspended, in-flight effects are reconciled without replay, and visibility is restored before continuation

#### Scenario: An interrupted worktree is recovered
- **WHEN** a controller interruption occurs while an executor has partial work in its worktree
- **THEN** the recorded worktree mapping and partial changes survive reconciliation and are reused instead of being recreated

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

### Requirement: Lead acceptance and merge

The lead SHALL review each completed assignment against its accepted requirements and applicable checks before acceptance, merge accepted branches itself, and return concrete defects with their acceptance conditions to the original executor when correction is within its scope. A rejected assignment SHALL retain its partial work and worktree until corrected or explicitly discarded. Acceptance decisions and merges SHALL be recorded in task state and reflected on the board; the lead SHALL NOT claim completion for integrated work whose checks have not passed.

#### Scenario: Accepted work is merged
- **WHEN** an executor branch passes the applicable checks and meets its requirements
- **THEN** the lead merges it, the worktree is retired, and the board and task records show the accepted outcome

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

Delivery SHALL include installation, update, check, recovery and disconnection through the supported global kit lifecycle. Fresh ordinary Codex sessions outside this checkout SHALL exercise configured lead/executor dispatch, board workflow, simultaneous visible windows, worktree isolation, steering, recovery and merge through the real entry point, with actual subscribed tool work on a locally selected real external development task whose applicable acceptance passes and whose results are consumed. Failure acceptance SHALL use owned targets and deterministic quota/transport faults rather than exhausting accounts. Public records SHALL contain no consumer-specific data; a toy fixture alone SHALL NOT establish complete delivery.

#### Scenario: A real consuming task completes
- **WHEN** the globally installed workflow completes a locally selected external development task
- **THEN** its actual result and applicable checks demonstrate integrated use, evidence names the tested identities privately, and public records contain no consumer-specific data

#### Scenario: Recovery is accepted without exhausting a subscription
- **WHEN** an owned test injects a realistic quota failure at the lead and worker boundaries
- **THEN** the installed control path performs the expected succession and preserves work, while separate bounded live requests verify the actual configured profile bindings
