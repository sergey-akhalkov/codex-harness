## MODIFIED Requirements

### Requirement: Explicit lead activation through the team-lead skill

The kit SHALL provide a `team-lead` skill, delivered through its installation
lifecycle, that activates the lead role in an ordinary Codex session of a
consuming project. The role SHALL be entered explicitly by skill invocation,
by a user request that clearly asks for orchestrated asynchronous
development, or by a user request to use executors or lead/executor dispatch
for the current work - including a user question about why executors are
unused; such requests SHALL NOT require the skill name. An ordinary session
SHALL NOT spawn executors or create orchestration state without one of those
activations. The skill SHALL own the lead workflow instructions: role
configuration discovery, board setup and inspection, specification creation,
executor briefs through harness commands, steering, acceptance, merge and
explicit stop. Global instructions SHALL point to the skill without
duplicating its workflow. Leaving the role or stopping orchestration SHALL
remain explicit and preserve partial work.

#### Scenario: A lead session is activated
- **WHEN** the user invokes the `team-lead` skill with a stage goal in a consuming project
- **THEN** the session reads the validated role configuration, prepares the board records and dispatches executors through harness commands, while sessions without activation behave as before

#### Scenario: A user executor request activates the role
- **WHEN** the user asks to use executors for the current work, or asks why executors are unused, without naming the `team-lead` skill
- **THEN** the session activates the lead role for that work through the skill and dispatches executor-suitable slices instead of reporting a rule conflict

#### Scenario: An ordinary session is not hijacked
- **WHEN** a user asks for a small direct task in a session where the role was never activated and no executor use was requested
- **THEN** the request is served directly, with no executors, board records or orchestration state created

#### Scenario: The skill is selected from a clear request
- **WHEN** the user asks for orchestrated asynchronous development without naming the skill
- **THEN** the agent selects the `team-lead` skill based on its description before spawning anything, instead of improvising an orchestration workflow
