# Spec Delta

## Purpose

Give lead and executor sessions separately maintainable instructions and skill selections while sharing common safeguards, preserving task ownership and supporting verified global use.

## ADDED Requirements

### Requirement: One effective responsibility role per managed session

Each managed session SHALL receive the shared core and exactly one responsibility role before its first model turn. Ordinary main sessions SHALL use the lead role; dispatched executors SHALL use the executor role. Applicable project guidance and task inputs SHALL remain available under native precedence. Role selection SHALL NOT change the configured provider, model or effort.

#### Scenario: Main session performs a small task
- **WHEN** an ordinary main session receives a task cheaper to complete directly
- **THEN** its lead context permits direct investigation, implementation and verification without creating an executor, board item or orchestration ceremony

#### Scenario: Executor begins a bounded assignment
- **WHEN** the lead dispatches an executor through the installed harness
- **THEN** the actual initial context contains shared safeguards, executor guidance, applicable project instructions and the assignment, and omits kit-owned lead-only guidance

#### Scenario: Provider selection changes
- **WHEN** an authorized configuration change selects another supported executor model
- **THEN** the executor responsibility role remains the same and the effective model and effort follow the existing routing contract

### Requirement: Shared rules have one authoritative owner

Shared rules SHALL have one canonical source reused by both roles. Role-specific obligations SHALL move to their responsible layer rather than remain in the common prompt with an additional override. The partition SHALL preserve all accepted obligations. External and project-owned instructions SHALL NOT be rewritten or suppressed to manufacture separation.

#### Scenario: Shared integrity rule is updated
- **WHEN** the canonical rule for preserving unrelated work changes
- **THEN** newly started lead and executor sessions receive it without separately edited copies

#### Scenario: Project instructions contain additional responsibilities
- **WHEN** project-owned guidance overlaps a role's responsibilities
- **THEN** the project guidance remains subject to native instruction precedence and any material conflict is surfaced without silently changing its source

### Requirement: Lead retains end-to-end ownership

The lead SHALL retain requirements, consequential scope and design decisions, decomposition, assignment, dependency coordination, integration, final acceptance and regression assessment. It SHALL assess executor evidence and the integrated result, return concrete in-scope defects to the capable original executor, and keep incomplete work open. Executor reports alone SHALL NOT prove completion.

#### Scenario: Local checks pass but integration fails
- **WHEN** an executor's reported checks pass and the combined result violates an agreed requirement
- **THEN** the lead withholds acceptance, supplies the concrete defect and expected behavior, and returns an in-scope correction to the original executor while preserving its work

### Requirement: Executor owns a complete bounded work cycle

Executors SHALL investigate, choose implementation details, change their owned scope, run applicable checks and correct local defects autonomously. They SHALL report actual changes, verification, limitations and remaining work. They SHALL NOT originate agents or executors, take ownership of the overall plan, silently expand scope, or weaken acceptance.

#### Scenario: Local verification reveals a defect
- **WHEN** an applicable check fails because of the executor's change and correction remains in scope
- **THEN** the executor investigates and corrects it without asking the lead to direct each routine edit

#### Scenario: Required correction exceeds the assignment
- **WHEN** the executor establishes that success requires a consequential change outside its ownership
- **THEN** it preserves partial work and sends the lead evidence, the needed decision and a proposed option through the existing feedback channel, continuing independent authorized work when available

#### Scenario: Recursive delegation is attempted
- **WHEN** an executor attempts to originate another agent or managed executor
- **THEN** existing runtime controls prevent that launch and the executor can still ask the lead for help

### Requirement: Shared and role-specific skill selection

The kit SHALL maintain explicit shared, lead and executor memberships for its managed skill selection, with one canonical source per skill. Each role SHALL see the shared selection plus its applicable role selection, subject to existing explicit disables. Skill bodies SHALL remain demand-loaded. Project, user-owned and protected external skill sources SHALL remain intact.

#### Scenario: Lead workflow is unavailable to an executor
- **WHEN** both roles start in the same consumer checkout
- **THEN** the executor's effective catalogue omits kit-owned lead-only skills, the lead retains them, and both can discover and use an enabled shared verification skill

#### Scenario: A project supplies a specialist skill
- **WHEN** a consumer adds a project skill or has an explicit skill disable
- **THEN** role filtering preserves native discovery of the project skill and honors the disable without editing either source

#### Scenario: A shared skill is improved
- **WHEN** the canonical shared skill changes
- **THEN** subsequent native skill loading follows the same source for both roles without role-specific copies or wrappers

### Requirement: Role selection is isolated across concurrent sessions

Resolving a role SHALL NOT mutate shared configuration or skill visibility for other live sessions. Existing inspection and run state SHALL identify the effective role, owned instruction revision and skill selection independently of model identity. Invalid or missing required role inputs SHALL cause an actionable preflight failure rather than silent fallback to the undifferentiated prompt.

#### Scenario: Lead and executor run concurrently
- **WHEN** an executor starts while a lead session is using its own skills
- **THEN** neither session changes the other's role context, effective catalogue, credentials or provider settings

#### Scenario: Role input is missing
- **WHEN** a required executor instruction source or managed skill membership cannot be resolved
- **THEN** launch fails before a model request with the missing input and recovery action, preserving the assignment and existing work

### Requirement: Continuation preserves an identifiable role context

Resume, restart and lead succession SHALL preserve the appropriate role and existing recovery guarantees. Active sessions SHALL NOT be rewritten by source updates. Recovery SHALL identify the effective kit context revision and disclose any change caused by native reloading. A fresh run SHALL use current sources and preserve partial work. Legacy runs SHALL remain recoverable with their context limitations identified.

#### Scenario: Sources change during execution
- **WHEN** executor instructions are updated while a run is active and that run is subsequently resumed
- **THEN** the active run remains undisturbed, resume retains the executor role and partial work, any revision change from native reloading is reported, and a new executor run uses the updated revision

#### Scenario: Restart adopts current guidance
- **WHEN** a fresh conversation is deliberately started from a preserved executor run
- **THEN** the executor role is reapplied, any role revision change is reported, and the original assignment, worktree, evidence and feedback routing remain available

#### Scenario: A pre-migration run is resumed
- **WHEN** a retained run has no separate role binding
- **THEN** recovery identifies it as legacy context, preserves its work and existing recovery path, and provides the existing fresh-conversation route to adopt the new role without pretending the old transcript loaded it

### Requirement: Global lifecycle delivers and restores role contexts

Install, update and check SHALL cover role instructions, managed skill selection and their effective consumption outside the kit checkout. Disconnect and recovery SHALL restore prior owned state while preserving unrelated settings, skills, authentication and retained runs. Protected external workflows SHALL remain unmodified.

#### Scenario: Fresh consumer uses the installed capability
- **WHEN** a fresh main session outside the kit starts an executor after installation
- **THEN** native evidence shows each intended context and catalogue, and an actual bounded assignment, correction reply and accepted result use the installed components

#### Scenario: Update is rolled back
- **WHEN** an owned role update fails or is disconnected through the supported lifecycle
- **THEN** prior owned activation can be restored without deleting user configuration, project skills or unfinished executor work

### Requirement: Reduced context is measured across all required surfaces

For the same representative scenarios and task inputs, the executor's required kit instruction load SHALL decrease in both UTF-8 bytes and whitespace-delimited words from an identified baseline. Lead and combined affected guidance SHALL satisfy the existing concise-guidance contract. Accounting SHALL include shared and role text, skill metadata, invoked bodies, required references and generated briefs, identifying external content separately.

#### Scenario: Rules are moved into a mandatory reference
- **WHEN** a shorter role prompt requires reading another file to perform the same assignment
- **THEN** that file's required content is counted and its movement alone is not reported as a reduction

### Requirement: Independent improvements require outcome evidence

Lead, executor and shared guidance SHALL be independently reviewable and reversible. Improvement claims SHALL identify changed owners and compare equivalent representative work, including acceptance quality, regressions, lead intervention, correction cycles and total time to accepted result. Missing evidence SHALL remain unverified; prompt size or model reports SHALL NOT prove benefit. Existing feedback and verification owners SHALL hold evidence.

#### Scenario: A shorter executor prompt increases rework
- **WHEN** a matched comparison reduces instruction load but increases correction effort or misses an acceptance condition
- **THEN** the result records that trade-off and does not claim an established improvement from the size reduction

#### Scenario: A failure belongs to a shared tool
- **WHEN** both roles repeat work because of the same verified tool defect
- **THEN** feedback targets the shared tool owner rather than automatically adding instructions to both role prompts
