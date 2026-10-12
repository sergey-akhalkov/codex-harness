# Spec Delta

## Purpose

Turn observed delivery friction during sustained user work into bounded, assigned and verified improvements through an optional independently focused agent, while preserving task ownership, quality and recoverability.

## ADDED Requirements

### Requirement: Explicit activation with a bounded mandate

The installed `delivery-improvement` skill SHALL activate one improvement session for the current lead and authorized work. Activation SHALL cover relevant improvements in the consuming project and canonical kit, including ordinary verified delivery. It SHALL NOT authorize unrelated work, new providers, weakened acceptance or destructive actions. Repeated activation SHALL identify the existing session instead of duplicating it.

#### Scenario: A user enables improvement during a large task
- **WHEN** the user invokes the skill in a supported lead session
- **THEN** the lead starts the distinct visible improvement session, records its work and authority binding, and continues the original task without requiring the user to assemble transport or board bookkeeping

#### Scenario: Another session attempts duplicate activation
- **WHEN** activation targets work that already has a live improvement owner
- **THEN** the existing binding is reported and no second observer model conversation is launched

#### Scenario: Improvement mode is not active
- **WHEN** ordinary development proceeds without explicit activation
- **THEN** no improvement observer or new authority is created and existing lead, executor and feedback behavior remains available

### Requirement: Independent improvement responsibility

The improvement session SHALL receive shared safeguards, its own role guidance and relevant task context independently of lead and executor instructions. It SHALL investigate observations, maintain bounded feedback and follow outcomes. It SHALL NOT dispatch agents, direct executors, edit implementation or instruction sources, accept changes or enlarge its own mandate. The lead SHALL retain decisions, assignment, integration and acceptance.

#### Scenario: A correction implicates the lead
- **WHEN** the original assignment contradicts the correction sent to an executor
- **THEN** the observer can identify the lead-side cause with evidence instead of assuming the executor needs stronger instructions

#### Scenario: The observer's own process needs improvement
- **WHEN** observations support changing improvement-role guidance
- **THEN** the observer submits that proposal through the same lead and executor path without modifying its own instructions or evaluating its own change as accepted

### Requirement: Visible and resource-accounted observation

Each observer conversation SHALL have a dedicated titled terminal surface before model work, showing its assignment, role, actual configured profile/model/effort and live activity. Model routing SHALL use the existing configuration owner without substitution. Observation SHALL require no implementation worktree. Its model work SHALL participate in configured resource admission and pacing; it SHALL NOT be an uncounted extra worker.

#### Scenario: Capacity is occupied
- **WHEN** activation or an observer turn cannot be admitted within the applicable configured limits
- **THEN** observation waits with its cause visible and preserves pending events without preempting a healthy executor or creating an extra checkout

#### Scenario: The observer surface closes
- **WHEN** no visible surface remains attached to its conversation
- **THEN** new observer model work is suspended, in-flight effects are reconciled, and the owning launcher restores visibility before continuation without screenshots or desktop rearrangement

### Requirement: Evidence covers the lead and its executors

Observation SHALL cover authorized visible messages, tool outcomes and assignment/lifecycle events from the bound lead and its executors, including both directions of their exchange. Each observation SHALL retain source and episode identity, instruction/assignment context and evidence locators. Missing, stale or unavailable content SHALL be reported as a coverage limit; delivery metadata alone SHALL NOT be treated as message content.

#### Scenario: A lead sends a correction
- **WHEN** `executor message` delivers a correction, inline or through the supported spill path
- **THEN** the observer can access its actual authorized text and the relevant assignment/result context, distinguish submission from delivery, and attribute the episode without asking the lead to label or copy the message

#### Scenario: Friction occurs in the lead's own work
- **WHEN** the lead repeats a failed tool operation or costly preparation without an executor exchange
- **THEN** its relevant observable tool outcomes can supply evidence to the improvement session

#### Scenario: A session is outside supported observation coverage
- **WHEN** the bound lead or executor cannot provide the required events or addressed delivery
- **THEN** activation or continued coverage reports the missing capability and supported remedy without claiming a complete operative loop, while independent authorized work can continue

### Requirement: Bounded analysis consumes new evidence

Collection, waiting and unchanged-state reconciliation SHALL make no model calls. Observer turns SHALL consume bounded batches of new evidence, with at most one turn in flight and further arrivals retained for the next batch. Routine analysis SHALL respect existing supervision cadence; earlier analysis SHALL require an allowed substantive event. Full transcripts, opaque reasoning state and conversation pixels SHALL NOT be routine inputs.

#### Scenario: No new work evidence arrives
- **WHEN** the observation interval expires with no new eligible evidence or due decision condition
- **THEN** no observer model turn, transcript reread or worker status request occurs

#### Scenario: Several related corrections arrive together
- **WHEN** new events arrive during analysis of a related episode
- **THEN** the controller retains and coalesces the arrivals, avoiding concurrent observer turns and repeated analysis of acknowledged evidence

#### Scenario: A healthy executor is silent
- **WHEN** elapsed time or a yielded wait is the only new information
- **THEN** the observer does not infer a stall, ask for status, interrupt work or recommend reassignment on that basis

### Requirement: Findings distinguish causes from symptoms

The observer SHALL compare observations with the applicable task and evidence before proposing a cause or remedy. Findings SHALL distinguish observations, hypotheses and predictions, identify the next relevant use and name a checkable result. It SHALL consider no change, subtraction and correction of the owning interface before adding guidance, and SHALL allow a batch to produce no proposal.

#### Scenario: A message introduces a new user requirement
- **WHEN** apparent corrective feedback actually changes the accepted task
- **THEN** it is not classified as a preventable executor failure merely because it required another message

#### Scenario: A repeated missing input can be detected before work
- **WHEN** evidence shows that an assignment repeatedly lacks an input needed for implementation
- **THEN** the proposal targets earlier input provision or validation at its owner, identifies the prevented failure, and compares that remedy with adding another standing instruction

#### Scenario: One incident does not support a general rule
- **WHEN** an isolated observation lacks an attributable cause or likely relevant recurrence
- **THEN** it remains a bounded hypothesis, requests only decision-relevant investigation, or yields no proposal instead of expanding global guidance

### Requirement: Existing boards own improvement work

Findings, lead decisions, assignments and outcomes SHALL use existing beads records and feedback operations. The observer SHALL search relevant open, deferred and completed items before creating a new task, preserving evidence and prior conclusions. Runtime observation progress SHALL reference those owners rather than duplicate task state. Board failures SHALL remain explicit and preserve undelivered findings for recovery.

#### Scenario: A known problem recurs
- **WHEN** new evidence concerns an existing improvement with the same applicable cause
- **THEN** the existing item gains the attributable evidence rather than a duplicate task or synthetic vote

#### Scenario: A prior rejection is reconsidered
- **WHEN** the observer proposes revisiting a completed rejection
- **THEN** the proposal names the new evidence or changed condition and links the previous decision instead of rearguing the same basis

### Requirement: Lead decisions occur at work boundaries

The lead SHALL consider submitted findings at the next safe boundary after in-flight effects, choosing accept, defer or reject on the existing item. Acceptance SHALL establish scope, verification and assignment readiness. Deferral SHALL name a concrete reason and observable return condition; rejection SHALL retain its basis. Notifications SHALL reference durable findings and SHALL NOT become a second progress channel.

#### Scenario: A useful finding arrives while the lead is integrating
- **WHEN** a nonurgent finding is delivered during an in-flight integration operation
- **THEN** the operation continues and the lead decides on the finding at the next safe boundary before moving to unrelated work

#### Scenario: A deferral condition is reached
- **WHEN** recorded dependency or capacity evidence satisfies the item's return condition
- **THEN** the item returns to the lead's next decision batch once, without repeated unchanged reminders or a new feedback task

### Requirement: Accepted improvements receive execution opportunities

At the next suitable allocation opportunity, the lead SHALL dispatch an accepted ready improvement or record the concrete higher-priority work or blocker and a return condition. Indefinite deferral until all project work ends SHALL NOT satisfy this obligation. One ordinary improvement implementation SHALL run at a time within existing executor capacity; urgent correctness or integrity work SHALL follow existing consequence handling.

#### Scenario: An executor slot becomes available
- **WHEN** an accepted improvement is ready and a suitable slot is available with no recorded higher-priority conflict
- **THEN** the lead assigns the improvement before filling that opportunity with lower-priority unrelated work, using the normal committed-base brief, execution and acceptance path

#### Scenario: Current delivery takes priority
- **WHEN** an improvement would delay more consequential accepted work
- **THEN** the lead records that trade-off and an observable return condition on the item instead of silently dropping it or reserving unused capacity

#### Scenario: The lead can fix a trivial issue more cheaply
- **WHEN** an accepted in-scope correction costs less to complete and check directly than to hand off
- **THEN** the lead can complete it directly under ordinary authority, recording the same result evidence without manufacturing an executor assignment

### Requirement: Improvements preserve project and publication boundaries

Project-specific evidence SHALL remain in its owning project. A shared-kit improvement SHALL use a kit-owned task, public-safe description and reproducible synthetic inputs, retaining private provenance only locally. Activation SHALL authorize relevant bounded kit work and normal verified deployment, while preserving existing specification, skill-evaluation, removal, scope and user-approval boundaries.

#### Scenario: A private consumer reveals a shared tool defect
- **WHEN** the proposed fix belongs to the reusable harness
- **THEN** a kit-owned executor receives sanitized inputs and acceptance, the consumer retains private evidence and its local link, and no private project identifiers or transcripts enter shared source or public board output

#### Scenario: A proposed improvement changes the promised outcome
- **WHEN** a remedy would narrow acceptance, add a material operating restriction or require new authority
- **THEN** the lead seeks the relevant user decision before dependent work rather than treating improvement activation as blanket consent

### Requirement: Changes are consumed through their existing lifecycle

Implementation, integration, publication and observed consumption SHALL remain distinguishable on the existing item. Instructions and skills SHALL be delivered through their canonical owners and verified lifecycle. Active sessions SHALL NOT be assumed to reload changes; applicability and effective revision SHALL be checked, with required succession using the existing safe-boundary mechanism. No uncertain external effect SHALL be replayed to prove consumption.

#### Scenario: Shared guidance is updated during ongoing work
- **WHEN** a checked change is published for new assignments while earlier sessions remain active
- **THEN** a new intended session is verified to consume the new revision, earlier sessions retain honest revision/coverage status, and any needed current correction uses the existing addressed channel or verified succession

#### Scenario: A published skill was not loaded
- **WHEN** files and installation checks succeed but the intended session has not consumed the update
- **THEN** the item records the unverified consumption and does not claim the affected work has improved

### Requirement: Improvement claims require relevant outcome evidence

The lead SHALL accept correctness through the real affected entry point and assess benefit with proportionate evidence, including coordination, analysis, checking and rework costs. Strategy or agent-behavior claims SHALL use applicable real-agent evaluation; experiment and skill-evaluation gates SHALL remain intact. Supported, adverse and inconclusive results SHALL be distinguishable. Fewer messages or shorter prompts SHALL NOT alone establish improvement.

#### Scenario: A deterministic preparation defect is repaired
- **WHEN** the original failure can be exercised through its real command
- **THEN** acceptance verifies the repaired path and relevant failure cases without requiring unrelated whole-task experiments, and limits any speed claim to measured scope

#### Scenario: A prompt change reduces messages but misses a requirement
- **WHEN** the candidate produces less feedback while degrading acceptance quality
- **THEN** it is not accepted as a demonstrated improvement and the prior accepted guidance remains recoverable

### Requirement: Observation recovery preserves ownership and progress

Stop, interruption, lead succession and observer recovery SHALL preserve acknowledged evidence, pending findings and board references without duplicate tasks, delivery or execution. Acknowledgment SHALL follow retained analysis outcomes, including no-finding outcomes. Lead shutdown or loss of ownership SHALL prevent orphaned new observer turns. Explicit reactivation SHALL verify current scope, model binding and context revision before continuing.

#### Scenario: Observation stops after creating a task but before acknowledgment
- **WHEN** recovery reprocesses the same source episode
- **THEN** it reconciles the existing board result and acknowledges progress without creating a duplicate task or assigning the improvement twice

#### Scenario: A lead session is replaced
- **WHEN** the supported successor takes ownership of unfinished work
- **THEN** the improvement binding transfers through the existing lifecycle without two active owners, retaining unresolved decisions and pending events

#### Scenario: The user disables improvement mode
- **WHEN** the user stops the mode during analysis or implementation
- **THEN** new observer turns and new improvement assignments stop, in-flight effects and partial work are preserved, and an already assigned implementation follows the user's stop scope and normal recovery rather than being discarded

### Requirement: Installed use proves the complete loop

Global delivery SHALL include the skill, role, configuration, event observation, board operations and session lifecycle, preserving unrelated settings and rollback. Acceptance SHALL exercise an actual installed lead, observer and executor outside the kit checkout from correction through verified applied improvement. Tests of transport or model-generated reports alone SHALL NOT establish the complete loop or its benefit.

#### Scenario: A fresh consumer completes an improvement
- **WHEN** the skill is enabled through the installed entry point in an independent owned repository
- **THEN** a real correction produces an evidenced finding, lead decision, actual assignment, accepted fix and observed consumption while the original task remains complete in scope

#### Scenario: An update fails and is rolled back
- **WHEN** installation or observation activation fails after an owned update
- **THEN** the prior accepted installation can be restored without erasing boards, pending findings, user settings or unfinished executor work
