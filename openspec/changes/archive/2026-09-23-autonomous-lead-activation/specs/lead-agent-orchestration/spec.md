## RENAMED Requirements

- FROM: `### Requirement: Explicit lead activation through the team-lead skill`
- TO: `### Requirement: Lead activation through the team-lead skill`

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
