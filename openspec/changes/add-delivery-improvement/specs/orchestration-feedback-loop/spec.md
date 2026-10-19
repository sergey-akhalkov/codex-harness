# Spec Delta

## MODIFIED Requirements

### Requirement: Feedback arrives as triage tasks

Lead and executor feedback about the work, the orchestration, instructions, tools or blockers SHALL be recorded as board triage tasks with bounded context instead of real-time inter-agent chat. The lead SHALL triage accumulated feedback in batches at safe boundaries; routine triage mechanics (listing, merging, voting) SHALL NOT require model calls beyond the lead's bounded judgment. Feedback tasks SHALL preserve the concrete observation, affected scope and reporter identity without copying unbounded transcripts. While `delivery-improvement` is explicitly active, its executor and lead reflection forks SHALL return bounded proposals; only the real lead SHALL create or update these tasks and retain admission, priority and implementation decisions. Exceptional addressed notifications SHALL reference the durable item rather than replace it.

#### Scenario: An executor reports friction twice
- **WHEN** an executor records two separate friction observations during one assignment
- **THEN** both arrive as bounded triage tasks and are processed in the lead's next batch, without interrupting the executor's session

#### Scenario: The lead wants to correct course
- **WHEN** the lead observes a repeated mistake across executors
- **THEN** it records its own feedback task for triage rather than steering every session individually with duplicated context

#### Scenario: Role reflections find actionable friction
- **WHEN** the accepted-task executor reflection and lead synthesis return an attributable problem
- **THEN** the real lead records or updates the bounded board item at its next safe decision boundary without requiring the original reporter to repeat the observation or granting either fork board-write authority

### Requirement: Observations are routed by kind

Each observation SHALL be classified before it competes as an incubator vote. A verified reusable procedure in owned skill scope SHALL be handed to `autonomous-skill-evolution` and SHALL NOT wait for the vote threshold. Process, orchestration, requirement, tool and unclear or material changes SHALL remain incubator items unless they qualify for the existing consequence override or the explicit verified-improvement admission under active `delivery-improvement`. Kit-wide skill or instruction demand from consuming projects SHALL promote to the kit backlog without private consuming-project data; `autonomous-skill-evolution` SHALL execute any subsequent library mutation under its own evaluation contract. Promotion and routing confer eligibility for planning; they SHALL NOT write skill packages or substitute incubator votes or verified-improvement admission for skill-evaluation. Implementing a verified improvement SHALL additionally require the activated mandate and the lead's scoped acceptance of that work.

#### Scenario: A verified procedure is not voted into a skill
- **WHEN** an executor records a reusable diagnostic method with a checkable result in owned skill scope
- **THEN** the observation is handed to skill-evolution instead of accumulating incubator votes for authoring that skill

#### Scenario: Orchestration friction stays in the incubator
- **WHEN** an executor reports process, quota or dispatch friction without a reusable owned procedure or a qualified explicit admission
- **THEN** the item remains in the incubator and is not turned into a skill candidate by this loop

#### Scenario: Kit skill demand does not write the package
- **WHEN** consuming-project feedback about a kit skill or instruction reaches the vote threshold
- **THEN** it promotes to the kit backlog without private project data, and skill-evolution remains the only path that may mutate the library

#### Scenario: A confirmed tool defect follows the activated path
- **WHEN** the lead accepts a bounded, evidenced tool correction under the active improvement mandate
- **THEN** the item can be admitted without votes, routes to its actual project or kit owner, and keeps any required OpenSpec and evaluation gates before implementation or publication

### Requirement: Incubator deduplicates and accumulates votes

The incubator SHALL contain unique improvement items only. During triage, the lead SHALL merge similar incoming feedback into an existing incubator item and record the merge visibly; a merged item or an explicit repeated report SHALL add exactly one vote attributable to a distinct episode and reporter. Repeated votes from the same reporter for the same episode SHALL NOT accumulate, and automated diagnostics SHALL NOT inflate votes. Item identity, merge history and vote provenance SHALL remain inspectable. A reflection's derived report SHALL preserve the originating evidence identity and SHALL NOT count as an independent vote for an already represented observation. A genuinely new episode SHALL retain its own provenance under the existing counting rules.

#### Scenario: Similar feedback arrives from two executors
- **WHEN** triage finds that new feedback describes an existing incubator item
- **THEN** the item gains one vote with visible provenance instead of a near-duplicate fragmenting the signal

#### Scenario: One agent repeatedly votes for its own idea
- **WHEN** the same reporter would add several votes without a new distinct episode
- **THEN** only one vote is counted and the attempt is visible in provenance

#### Scenario: Reflection restates a reported correction
- **WHEN** a reflection recommendation and an executor's feedback refer to the same originating episode
- **THEN** the evidence and analysis are retained on the existing item without increasing its vote count merely because another model summarized it

### Requirement: Vote threshold promotes to backlog

A configurable vote threshold, defaulting to promotion after more than two votes, SHALL move an incubator item into the implementation backlog. Promotion SHALL route by consequence: small improvements become backlog tasks assignable to the lead or an executor; changes to accepted behavior or requirements SHALL become OpenSpec changes; items concerning kit instructions, skills or tools SHALL promote to the kit's own backlog without private consuming-project data. Promotion makes work eligible for planning; it SHALL NOT silently authorize implementation without the applicable workflow. Qualified verified-improvement admission under active `delivery-improvement` SHALL be an additional explicit route without votes; it SHALL NOT lower the threshold for ordinary observations or turn unverified hypotheses into authorized implementation.

#### Scenario: A third distinct vote promotes an item
- **WHEN** an incubator item reaches the configured threshold from distinct counted observations
- **THEN** the item is promoted to the backlog with its history and routed according to its consequence

#### Scenario: A promoted item changes behavior
- **WHEN** a promoted improvement would alter an accepted requirement
- **THEN** it enters the OpenSpec workflow instead of being implemented directly from the backlog

#### Scenario: An unverified idea is raised during improvement mode
- **WHEN** a reflection proposes a general instruction change without an attributable cause and checkable remedy
- **THEN** the idea remains in the appropriate hypothesis or incubator path rather than receiving automatic vote-free implementation authority

### Requirement: Installed deterministic feedback operations

An installed command SHALL expose feedback recording, agent-selected triage, ledger inspection, promotion eligibility and explicit promotion using the existing bd-backed algorithms. It SHALL preserve distinct-reporter/episode vote rules, exclude diagnostic and derived duplicate votes, honor configured batch and promotion limits, and return compact observable results. Semantic grouping and consequence decisions remain caller inputs. Repeated execution MUST NOT add duplicate counted votes or repeat completed promotions. A partial failure SHALL identify applied and failed operations and return nonzero; it SHALL NOT claim atomic success or discard retained board history. Read-only inspection MUST NOT mutate the board. No operation SHALL invoke a model, create another tracker or grant implementation authority. The same operations SHALL support explicitly selected verified-improvement admission, validate its active binding and required lead-supplied basis, and preserve the admission route in the existing ledger without manufacturing votes.

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

#### Scenario: Completed promotion is retried
- **WHEN** the same promotion is requested after its history and labels are already complete
- **THEN** the command successfully confirms the recorded outcome without adding history or changing labels

#### Scenario: Retry requests a different promotion
- **WHEN** a retry explicitly requests a different route or OpenSpec target from the recorded promotion
- **THEN** the command returns a conflict and preserves the recorded outcome without board writes

#### Scenario: Admission is attempted after the mode stops
- **WHEN** a new verified-improvement admission cites an inactive or unrelated work binding
- **THEN** the operation refuses with the concrete scope or state mismatch and preserves the item and ordinary feedback routes

## ADDED Requirements

### Requirement: Verified improvement admission has an explicit basis

While `delivery-improvement` is active, the lead SHALL be able to admit a bounded correction from one attributable observation without votes. The existing item SHALL retain mandate, evidence, cause, owned remedy, relevant use and acceptance, including specification and evaluation gates. Admission SHALL NOT waive full token-cost coverage or observable payback for increased spending at adoption. An assertion alone SHALL NOT qualify.

#### Scenario: A single reproduced failure has a bounded remedy
- **WHEN** the lead verifies a recurring preparation defect, identifies its owning tool and accepts an in-scope checkable correction
- **THEN** the item enters the appropriate work path with an evidence-based admission record and becomes subject to the activated scheduling obligation without fabricated votes

#### Scenario: Evidence becomes inapplicable before assignment
- **WHEN** the task, source or operating conditions change so the admitted basis no longer applies
- **THEN** the lead reassesses or defers the item before dependent work, retaining its previous basis and preventing stale acceptance from silently authorizing a different remedy

#### Scenario: A historical item becomes relevant now
- **WHEN** an existing incubator item has sufficient current evidence and falls within the active mandate
- **THEN** the lead can admit that same item through the verified path while preserving its vote history and prior decisions

#### Scenario: Ordinary optimization has an uncertain opportunity to repay
- **WHEN** a proposed optimization lacks credible relevant reuse or its expected benefit does not justify investigation, implementation, verification and displaced delivery
- **THEN** the lead records a bounded rejection or deferral on the existing item rather than assigning work merely to justify reflection; urgent correctness and integrity work retains its existing consequence path

#### Scenario: One observation supports a lasting improvement
- **WHEN** current evidence supports a bounded correction that reduces future change or maintenance cost while preserving quality and timely useful delivery
- **THEN** the lead can admit it with a concise reason and material trade-offs even without time/token savings, without a new score or repeated-proof quota; adoption still requires nonincreasing full token cost or supported payback for an increase

#### Scenario: A reflection finding requires a release decision
- **WHEN** an attributable finding exposes a concrete incident risk in a pending release
- **THEN** the responsible lead owns mitigation and any release hold through the existing task, retaining a reason and checkable release condition; a reflection recommendation alone neither blocks nor authorizes release


### Requirement: Improvement purpose survives task creation and delegation

Tasks accepted from reflection SHALL be marked as improvement implementation by their existing creation/dispatch owner. Their implementation descendants SHALL retain that purpose. Reflection recommendations SHALL NOT themselves create tasks, and closing improvement work SHALL NOT automatically generate more reflection.

#### Scenario: The real lead delegates a synthesized remedy
- **WHEN** the lead admits and creates or updates an improvement task
- **THEN** its dispatch and any implementation subtasks retain improvement purpose and normal acceptance while being excluded from the automatic accepted-ordinary-task trigger

#### Scenario: A reflection result is retried
- **WHEN** delivery or a board operation is reconciled after interruption
- **THEN** source identity prevents duplicate task creation, duplicate counted votes and loss of the nonrecursive purpose
