# orchestration-feedback-loop Specification

## Purpose

Turn lead/executor feedback into durable, deduplicated demand signals with an
incubator and vote promotion, refresh agent instructions through safe session
succession, and pace orchestration spend so improvements must earn their token
cost. Skill-library mutation, in-process catalogue delivery and same-session
compact recovery remain owned by `autonomous-skill-evolution`.

## Requirements

### Requirement: Feedback arrives as triage tasks

Lead and executor feedback about the work, the orchestration, instructions, tools or blockers SHALL be recorded as board triage tasks with bounded context instead of real-time inter-agent chat. The lead SHALL triage accumulated feedback in batches at safe boundaries; routine triage mechanics (listing, merging, voting) SHALL NOT require model calls beyond the lead's bounded judgment. Feedback tasks SHALL preserve the concrete observation, affected scope and reporter identity without copying unbounded transcripts.

#### Scenario: An executor reports friction twice
- **WHEN** an executor records two separate friction observations during one assignment
- **THEN** both arrive as bounded triage tasks and are processed in the lead's next batch, without interrupting the executor's session

#### Scenario: The lead wants to correct course
- **WHEN** the lead observes a repeated mistake across executors
- **THEN** it records its own feedback task for triage rather than steering every session individually with duplicated context

### Requirement: Observations are routed by kind

Each observation SHALL be classified before it competes as an incubator vote. A verified reusable procedure in owned skill scope SHALL be handed to `autonomous-skill-evolution` and SHALL NOT wait for the vote threshold. Process, orchestration, requirement, tool and unclear or material changes SHALL remain incubator items. Kit-wide skill or instruction demand from consuming projects SHALL promote to the kit backlog without private consuming-project data; `autonomous-skill-evolution` SHALL execute any subsequent library mutation under its own evaluation contract. Promotion and routing confer eligibility for planning; they SHALL NOT write skill packages or substitute incubator votes for skill-evaluation.

#### Scenario: A verified procedure is not voted into a skill
- **WHEN** an executor records a reusable diagnostic method with a checkable result in owned skill scope
- **THEN** the observation is handed to skill-evolution instead of accumulating incubator votes for authoring that skill

#### Scenario: Orchestration friction stays in the incubator
- **WHEN** an executor reports process, quota or dispatch friction without a reusable owned procedure
- **THEN** the item remains in the incubator and is not turned into a skill candidate by this loop

#### Scenario: Kit skill demand does not write the package
- **WHEN** consuming-project feedback about a kit skill or instruction reaches the vote threshold
- **THEN** it promotes to the kit backlog without private project data, and skill-evolution remains the only path that may mutate the library

### Requirement: Incubator deduplicates and accumulates votes

The incubator SHALL contain unique improvement items only. During triage, the lead SHALL merge similar incoming feedback into an existing incubator item and record the merge visibly; a merged item or an explicit repeated report SHALL add exactly one vote attributable to a distinct episode and reporter. Repeated votes from the same reporter for the same episode SHALL NOT accumulate, and automated diagnostics SHALL NOT inflate votes. Item identity, merge history and vote provenance SHALL remain inspectable.

#### Scenario: Similar feedback arrives from two executors
- **WHEN** triage finds that new feedback describes an existing incubator item
- **THEN** the item gains one vote with visible provenance instead of a near-duplicate fragmenting the signal

#### Scenario: One agent repeatedly votes for its own idea
- **WHEN** the same reporter would add several votes without a new distinct episode
- **THEN** only one vote is counted and the attempt is visible in provenance

### Requirement: Vote threshold promotes to backlog

A configurable vote threshold, defaulting to promotion after more than two votes, SHALL move an incubator item into the implementation backlog. Promotion SHALL route by consequence: small improvements become backlog tasks assignable to the lead or an executor; changes to accepted behavior or requirements SHALL become OpenSpec changes; items concerning kit instructions, skills or tools SHALL promote to the kit's own backlog without private consuming-project data. Promotion makes work eligible for planning; it SHALL NOT silently authorize implementation without the applicable workflow.

#### Scenario: A third distinct vote promotes an item
- **WHEN** an incubator item accumulates votes above the configured threshold from distinct episodes
- **THEN** the item is promoted to the backlog with its history and routed according to its consequence

#### Scenario: A promoted item changes behavior
- **WHEN** a promoted improvement would alter an accepted requirement
- **THEN** it enters the OpenSpec workflow instead of being implemented directly from the backlog

### Requirement: Consequence override without votes

The lead SHALL be able to promote an incubator item immediately, without waiting for votes, when evidence shows material correctness, data-integrity or safety consequences. The override SHALL record the concrete consequence and reason. Votes SHALL remain a signal of frequency; they SHALL NOT be the only path to action for material risks.

#### Scenario: A rare but severe issue is reported once
- **WHEN** a single report identifies a correctness or integrity risk with concrete evidence
- **THEN** the lead can promote it immediately with a recorded reason instead of waiting for repeated votes

### Requirement: Incubator hygiene has a deterministic owner and trigger

Hygiene SHALL be owned and performed by the lead session, triggered by conditions it already observes without background schedulers or controller board parsing: whenever the lead closes a stage or epic during acceptance, and whenever a feedback triage batch finds the incubator above its configured size cap, the same lead working session SHALL perform the sweep before completing that activity. The sweep archives items whose context has become stale with the reason visible, keeps merge and vote history inspectable, and prevents unbounded growth. Size checks SHALL use the board's non-interactive reporting without model calls; archive decisions remain lead judgment. When no lead session is active, hygiene waits safely because archived items are restorable when fresh evidence reappears. Hygiene SHALL NOT delete evidence or hide vote provenance.

#### Scenario: A sweep fires on its trigger
- **WHEN** the lead closes a stage during acceptance, or a triage batch finds the incubator above its configured size cap
- **THEN** the same lead session performs the sweep before completing that activity, archiving stale items with visible reasons

#### Scenario: No lead session is active
- **WHEN** a trigger condition holds but no lead session is running
- **THEN** the incubator waits unchanged and the next lead triage batch begins with the sweep, without a background scheduler

#### Scenario: Archived evidence becomes relevant again
- **WHEN** fresh feedback resembles an archived item
- **THEN** the item can be restored or re-opened with its earlier history and votes visible

### Requirement: Instruction-refresh succession

When accepted instruction or skill changes affect an active session, that session SHALL spawn a successor through a deterministic `codex resume` invocation of its profile that selects the exact prior session without an interactive picker, only at a safe boundary after in-flight tool effects complete. The successor SHALL be verified to reload the current instructions and skills; the predecessor SHALL hand over durable task context through the owning records and then stop its own CLI process. Succession SHALL NOT replay external operations with uncertain outcomes or lose partial work.

This requirement owns process replacement for orchestrated workers. Succession SHALL consume the compact revision identity published by `autonomous-skill-evolution` (`name`, canonical path, revision, operation) when the change is a skill-library mutation. It SHALL NOT implement catalogue delivery, in-process activation or same-session compact recovery, and a replaced process SHALL NOT satisfy those `skill-session-awareness` requirements.

#### Scenario: Instructions change while an executor works
- **WHEN** an applicable instruction or skill update is accepted while an executor session is active
- **THEN** a successor resumes that session's context under the new instructions at the next safe boundary and the predecessor process stops after the handover

#### Scenario: Succession does not close in-process recovery
- **WHEN** an orchestrated worker is replaced after a skill revision is accepted
- **THEN** succession may refresh that worker, and same-session compact or in-process activation remains owned by skill-evolution

#### Scenario: A resumed session misses the update
- **WHEN** verification shows a resumed session did not reload current instructions or skills
- **THEN** succession is reported as not established and the gap is fixed before relying on refresh

### Requirement: Budget-honest pacing

Pacing SHALL use fresh, scoped observations: native account-limit reads where the installed contract exposes them, actual provider refusals, and bounded dashboard snapshots supplied by the user where no API exists. Missing or stale telemetry SHALL remain unknown, not zero or unlimited; local request logs SHALL NOT be treated as authoritative remainders; no model call SHALL be made solely to estimate quota. Pacing SHALL adjust new assignment allocation, concurrency, reasoning effort and feedback cadence without preempting a healthy executor, dropping accepted work or adding purchases. Feedback mechanisms themselves SHALL stay bounded so triage cost does not rival the work being improved. This requirement paces account windows for orchestration; it SHALL NOT replace skill-evolution episode or period learning budgets.

#### Scenario: A provider window depletes rapidly
- **WHEN** current observations show fast depletion of an account window
- **THEN** new work is paced or reduced without interrupting active healthy executors, and the reason is visible

#### Scenario: Remainder telemetry is unavailable
- **WHEN** no reliable reading exists for a provider
- **THEN** pacing falls back to observed refusals and bounded backoff rather than inventing a quota percentage

### Requirement: Installed deterministic feedback operations
An installed command SHALL expose feedback recording, agent-selected triage, ledger inspection, promotion eligibility and explicit promotion using the existing bd-backed algorithms. It SHALL preserve distinct-reporter/episode vote rules, exclude diagnostic votes, honor configured batch and promotion limits, and return compact observable results. Semantic grouping and consequence decisions remain caller inputs. Repeated execution MUST NOT add duplicate counted votes or repeat completed promotions. A partial failure SHALL identify applied and failed operations and return nonzero; it SHALL NOT claim atomic success or discard retained board history. Read-only inspection MUST NOT mutate the board. No operation SHALL invoke a model, create another tracker or grant implementation authority.

#### Scenario: Repeated vote and diagnostic observation
- **WHEN** triage includes a repeated reporter/episode or a diagnostic observation
- **THEN** the counted total remains unchanged for that observation

#### Scenario: Failure after an applied action
- **WHEN** a later bd operation fails after an earlier action succeeded
- **THEN** the command returns the applied prefix, the failed action and a nonzero result suitable for recovery

#### Scenario: Real consumer board
- **WHEN** the globally delivered command is used on an isolated outside-checkout bd board
- **THEN** its observable ledger and issue state agree with the native command results

#### Scenario: Consumer uses customized installed limits
- **WHEN** a consumer has no kit configuration of its own and the caller supplies no source override
- **THEN** the command uses the installed kit's limits and identifies that configuration source
