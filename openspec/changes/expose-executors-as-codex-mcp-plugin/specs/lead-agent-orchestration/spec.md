# Spec Delta

## MODIFIED Requirements

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

### Requirement: Globally verified orchestration

Delivery SHALL include installation, update, check, recovery and disconnection through the supported global kit lifecycle. Fresh ordinary Codex sessions outside this checkout SHALL exercise configured lead/executor dispatch, board workflow, distinct native conversations accessible through /agents in one CLI, worktree isolation, steering, recovery and merge through the real entry point, with actual subscribed tool work on a locally selected real external development task whose applicable acceptance passes and whose results are consumed. Failure acceptance SHALL use owned targets and deterministic quota/transport faults rather than exhausting accounts. Public records SHALL contain no consumer-specific data; a toy fixture alone SHALL NOT establish complete delivery.

#### Scenario: A real consuming task completes
- **WHEN** the globally installed workflow completes a locally selected external development task
- **THEN** its actual result and applicable checks demonstrate integrated use, evidence names the tested identities privately, and public records contain no consumer-specific data

#### Scenario: Recovery is accepted without exhausting a subscription
- **WHEN** an owned test injects a realistic quota failure at the lead and worker boundaries
- **THEN** the installed control path performs the expected succession and preserves work, while separate bounded live requests verify the actual configured profile bindings

### Requirement: Concise titles and four executor slots

New executor native conversation titles SHALL begin with `CEx` and retain their profile and assignment owner. The shipped orchestration configuration SHALL permit four concurrent executor runs and independently default the shared worktree pool capacity to four without changing the aggregate heavy-command allowance or explicit executor profile.

#### Scenario: Executor terminal identity
- **WHEN** dispatch or continuation exposes an executor conversation
- **THEN** its title uses `CEx ({profile}) - {owner}` and remains distinct from another owner's conversation

#### Scenario: Configured four-slot pool is full
- **WHEN** all four configured shared slots contain live, suspended, reserved or unreviewed tasks of any supported type and a fifth independent owner requests dispatch
- **THEN** the existing pool reports capacity exhaustion without creating another slot or replacing held work

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

## REMOVED Requirements

### Requirement: Lead steering and executor escalation without waste

**Reason**: The dedicated terminal or CLI/control-transport contract is superseded by the single MCP plugin and native conversation runtime; its user-facing guarantees remain in the replacement requirements.

**Migration**: `executor-mcp-plugin` / Native bidirectional communication; Correlated questions remain actionable; Honest delivery evidence. Existing executable compatibility remains covered separately; the active workflow has no legacy-command dependency.

### Requirement: Urgent executor stop through the kit command

**Reason**: The dedicated terminal or CLI/control-transport contract is superseded by the single MCP plugin and native conversation runtime; its user-facing guarantees remain in the replacement requirements.

**Migration**: `executor-mcp-plugin` / Urgent stop isolates one executor. Existing executable compatibility remains covered separately; the active workflow has no legacy-command dependency.

### Requirement: Control-backed executor conversations

**Reason**: The dedicated terminal or CLI/control-transport contract is superseded by the single MCP plugin and native conversation runtime; its user-facing guarantees remain in the replacement requirements.

**Migration**: `executor-mcp-plugin` / Native bidirectional communication; Configured profiles without drift; Urgent stop isolates one executor. Existing executable compatibility remains covered separately; the active workflow has no legacy-command dependency.

### Requirement: Deterministic executor terminal targeting

**Reason**: The dedicated terminal or CLI/control-transport contract is superseded by the single MCP plugin and native conversation runtime; its user-facing guarantees remain in the replacement requirements.

**Migration**: `executor-mcp-plugin` / Native conversations in one CLI interface. Existing executable compatibility remains covered separately; the active workflow has no legacy-command dependency.

### Requirement: Observable executor lifecycle and bounded result

**Reason**: The dedicated terminal or CLI/control-transport contract is superseded by the single MCP plugin and native conversation runtime; its user-facing guarantees remain in the replacement requirements.

**Migration**: `executor-mcp-plugin` / Observable run outcomes; Preserve exact continuation and fresh recovery. Existing executable compatibility remains covered separately; the active workflow has no legacy-command dependency.

### Requirement: Executor tabs close when the host ends

**Reason**: The dedicated terminal or CLI/control-transport contract is superseded by the single MCP plugin and native conversation runtime; its user-facing guarantees remain in the replacement requirements.

**Migration**: `executor-mcp-plugin` / Native conversations in one CLI interface; Urgent stop isolates one executor. Existing executable compatibility remains covered separately; the active workflow has no legacy-command dependency.

### Requirement: Native TUI for managed executor runs

**Reason**: The dedicated terminal or CLI/control-transport contract is superseded by the single MCP plugin and native conversation runtime; its user-facing guarantees remain in the replacement requirements.

**Migration**: `executor-mcp-plugin` / Native conversations in one CLI interface; Configured profiles without drift. Existing executable compatibility remains covered separately; the active workflow has no legacy-command dependency.

### Requirement: Managed TUI lifetime follows the recorded run

**Reason**: The dedicated terminal or CLI/control-transport contract is superseded by the single MCP plugin and native conversation runtime; its user-facing guarantees remain in the replacement requirements.

**Migration**: `executor-mcp-plugin` / Native conversations in one CLI interface; Observable run outcomes; Urgent stop isolates one executor. Existing executable compatibility remains covered separately; the active workflow has no legacy-command dependency.

### Requirement: Spawn-bound executor-to-lead messaging

**Reason**: The dedicated terminal or CLI/control-transport contract is superseded by the single MCP plugin and native conversation runtime; its user-facing guarantees remain in the replacement requirements.

**Migration**: `executor-mcp-plugin` / Verified caller and run authority; Native bidirectional communication. Existing executable compatibility remains covered separately; the active workflow has no legacy-command dependency.

### Requirement: Trusted sender metadata and direct replies

**Reason**: The dedicated terminal or CLI/control-transport contract is superseded by the single MCP plugin and native conversation runtime; its user-facing guarantees remain in the replacement requirements.

**Migration**: `executor-mcp-plugin` / Verified caller and run authority; Correlated questions remain actionable. Existing executable compatibility remains covered separately; the active workflow has no legacy-command dependency.

### Requirement: Executor questions retain a reply-capable session

**Reason**: The dedicated terminal or CLI/control-transport contract is superseded by the single MCP plugin and native conversation runtime; its user-facing guarantees remain in the replacement requirements.

**Migration**: `executor-mcp-plugin` / Correlated questions remain actionable; Observable run outcomes. Existing executable compatibility remains covered separately; the active workflow has no legacy-command dependency.

### Requirement: Native session delivery with observable outcomes

**Reason**: The dedicated terminal or CLI/control-transport contract is superseded by the single MCP plugin and native conversation runtime; its user-facing guarantees remain in the replacement requirements.

**Migration**: `executor-mcp-plugin` / Native bidirectional communication; Honest delivery evidence. Existing executable compatibility remains covered separately; the active workflow has no legacy-command dependency.

### Requirement: Actionable observation of pending executor requests

**Reason**: The dedicated terminal or CLI/control-transport contract is superseded by the single MCP plugin and native conversation runtime; its user-facing guarantees remain in the replacement requirements.

**Migration**: `executor-mcp-plugin` / Observable run outcomes; Correlated questions remain actionable. Existing executable compatibility remains covered separately; the active workflow has no legacy-command dependency.

### Requirement: Size-free message delivery in both directions

**Reason**: The dedicated terminal or CLI/control-transport contract is superseded by the single MCP plugin and native conversation runtime; its user-facing guarantees remain in the replacement requirements.

**Migration**: `executor-mcp-plugin` / Literal input and large native records. Existing executable compatibility remains covered separately; the active workflow has no legacy-command dependency.

### Requirement: Recorded-identity address defaults

**Reason**: The dedicated terminal or CLI/control-transport contract is superseded by the single MCP plugin and native conversation runtime; its user-facing guarantees remain in the replacement requirements.

**Migration**: `executor-mcp-plugin` / Intent-based executor interface; Verified caller and run authority. Existing executable compatibility remains covered separately; the active workflow has no legacy-command dependency.

### Requirement: Standard-input assignment text for executor spawn

**Reason**: The dedicated terminal or CLI/control-transport contract is superseded by the single MCP plugin and native conversation runtime; its user-facing guarantees remain in the replacement requirements.

**Migration**: `executor-mcp-plugin` / Intent-based executor interface; Literal input and large native records. Existing executable compatibility remains covered separately; the active workflow has no legacy-command dependency.

### Requirement: Managed executor transports impose no harness-side record size cap

**Reason**: The dedicated terminal or CLI/control-transport contract is superseded by the single MCP plugin and native conversation runtime; its user-facing guarantees remain in the replacement requirements.

**Migration**: `executor-mcp-plugin` / Literal input and large native records; Observable run outcomes. Existing executable compatibility remains covered separately; the active workflow has no legacy-command dependency.

