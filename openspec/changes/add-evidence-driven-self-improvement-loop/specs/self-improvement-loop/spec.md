## Purpose

Autonomously measure, formulate, implement and evaluate evidence-backed harness improvements using proportionate experiments, preserving correctness, accepted baselines, useful work and durable decisions on the existing Beads board.

## ADDED Requirements

### Requirement: Hypotheses originate in attributable evidence

The loop SHALL admit a hypothesis only with a concrete observation, evidence locator and applicability, a causal explanation, an expected quality/time/resource effect, a plausible counterexample and predeclared acceptance. Sources SHALL include actual task traces, reproducible defects, measured tool behavior and verified implementation or dependency constraints. Observations, inferences and predictions SHALL remain distinguishable. External research SHALL support an applicable mechanism rather than substitute for evidence of a relevant problem. Initial hypotheses SHALL use existing evidence or observation of authorized real work; the loop MUST NOT fabricate observations or synthesize demand solely to remain busy.

#### Scenario: A tool emits large successful outputs
- **WHEN** retained trials show large outputs and repeated context consumption attributable to successful checks
- **THEN** the investigator can propose bounded output with an explicit diagnostic-preservation check and predicted end-to-end effect, without calling byte reduction proven token savings

#### Scenario: No grounded candidate remains
- **WHEN** available observations do not justify another hypothesis
- **THEN** the loop exposes an idle reason and awaits fresh evidence or authorized real work instead of generating arbitrary modifications

### Requirement: An explicit run defines scope and operating inputs

The installed skill SHALL start or resume an explicitly selected loop against an identified project, Beads board, source revision, writable scope, experiment policy and model configuration. The policy SHALL identify permitted researcher and executor routes, the explicitly selected repeatability/identity policy, resource limits, decision scope and any publication authority. Routine initial measurement, hypothesis formulation, experiment selection, planning, implementation, remeasurement, supported adoption, candidate rollback and selection of the next hypothesis within that authority SHALL continue autonomously without per-hypothesis confirmation; material scope or operating-condition changes SHALL require a user decision. Missing route inputs or observations required by the selected identity policy SHALL permit model-free preparation but MUST NOT trigger guessed endpoints, changed models or paid fallbacks. The selected API-observed policy SHALL keep unavailable weight hashes and hardware details visible as limits without requiring them as setup inputs.

#### Scenario: Model details arrive later
- **WHEN** the local endpoint or exact serving configuration has not been supplied
- **THEN** the loop can validate its planning and owned inputs, identifies the missing runtime inputs, and starts no dependent model trial

#### Scenario: A hypothesis needs a new external service
- **WHEN** an otherwise promising experiment requires access outside the authorized scope
- **THEN** dependent work waits for that decision while independent authorized work and evidence are preserved

### Requirement: Simplification is a first-class improvement hypothesis

For an observed cost, the loop SHALL consider whether existing capability, no change, simplification or subtraction can satisfy the intended outcome before proposing additional machinery, and SHALL examine removable complexity first: a simplification candidate SHOULD reduce the maintained entity surface AND improve accepted-task speed or token economy together. Additions and enhancements SHALL be proposed only after the loop finds no further removable complexity, and SHALL be judged on all three dimensions together - lower token consumption, faster work and higher task-closing quality, where quality means accepted tasks that do not later require extensive rework from surfacing critical P0/P1 defects; an addition improving cost while leaving task success or defect behavior unchanged at best SHALL NOT be claimed as a benefit. Applicable treatments SHALL include consolidation, narrower default exposure, on-demand loading, disabling and removal of skills, code/features, instructions, documentation, tools/dependencies, configuration, workflow stages, checks and generated outputs. A hypothesis SHALL identify the burden it removes, the outcomes that remain required, possible lost uses and a falsifiable expected effect on accepted-task time, steps or resources. Work quality, reasoning depth and task-closing ability SHALL remain maximal in every arm; speed or token savings SHALL NOT be accepted in exchange for reduced ability to close tasks correctly. The loop SHALL NOT require a new audit service, exhaustive inventory or fixed deletion quota; review SHALL reuse relevant evidence and existing owners proportionately.

Low or absent observed use SHALL be a lead for investigation, not a finding of uselessness. Usage claims SHALL identify the observation interval, task/environment coverage and telemetry gaps. Review SHALL include applicable rare recovery, compatibility, explicit invocation and indirect consumers; unresolved supported use SHALL remain explicit. Correctness, integrity, recovery and required quality SHALL remain binding even when a capability gives no everyday speed or token saving. Fewer lines, files, skills or exposed names alone SHALL NOT establish net benefit.

#### Scenario: A skill has no recorded invocations
- **WHEN** bounded usage evidence finds no invocation of an installed skill
- **THEN** the loop checks observation coverage, supported uses and actual context exposure, and can propose retention, changed exposure, consolidation or removal with a reason rather than automatically deleting it

#### Scenario: A rarely used recovery path adds no normal-task saving
- **WHEN** a simplification candidate would remove recovery behavior still required by the product
- **THEN** ordinary-task nonuse cannot justify retirement, and the candidate must preserve that behavior or await an explicitly revised product requirement

#### Scenario: An addition duplicates an existing route
- **WHEN** an existing capability can satisfy the evidenced need with less total work and equivalent required behavior
- **THEN** the loop proposes reuse or simplification with its acceptance and cost basis instead of assuming that improvement requires another feature

### Requirement: Removal requires informed and scoped user approval

Before implementing a removal of existing code, features or skills, the loop SHALL present a reviewable proposal and obtain an explicit user decision recorded through `orchestration-feedback-loop`. The proposal SHALL identify target paths/capabilities and source basis, evidence and its gaps, observed versus predicted benefit, what users lose and in which scenarios, affected callers/configuration/installations, retained requirements and checks, alternatives including no change, and a recovery route. Unverified dependencies or consumer access SHALL be disclosed rather than treated as absent. The approval SHALL identify the authorized targets, behavior loss and action scope: isolated experiment, accepted-source integration and/or installed publication.

Read-only investigation, planning and an unapplied diff preview SHALL be allowed before approval. Applying the removal SHALL wait even in a candidate worktree or evaluation workload. Disabling, de-registration or consolidation that withdraws an existing capability SHALL use the same gate; renaming the action SHALL NOT bypass it. Ordinary additions and edits preserving capability remain under existing run authority, but deleting existing code or skill definitions as a simplification treatment SHALL use this gate. General loop-start authority, low usage, an agent-authored decision or a favorable benefit result SHALL NOT substitute for approval. Silence SHALL leave dependent work pending while independent authorized work can continue.

One approval SHALL suffice for all stages expressly covered by its unchanged scope. Before a later stage or resume, the loop SHALL confirm that the actual target, behavior loss, dependency findings and action remain covered and the approval has not been withdrawn. Material changes or uncovered publication SHALL require a new decision; routine continuation of the approved scope SHALL NOT create another approval ritual. Benefit and required checks SHALL still gate adoption, and approval SHALL NOT waive unrelated acceptance, authorize evidence destruction or allow a candidate to remove its own control safeguards.

#### Scenario: A removal has a plausible benefit but no approval
- **WHEN** a skill or code-removal hypothesis has a complete plan and an unapplied preview but no explicit user approval
- **THEN** no executor applies the removal, the loop reports the lost capability and required decision, and independent eligible hypotheses can proceed

#### Scenario: A removal is used as workload B
- **WHEN** A is to be measured by implementing a removal task B in both arms
- **THEN** B's approval must cover those isolated implementations before either arm can apply the removal; workload status does not exempt it

#### Scenario: Only the isolated experiment was approved
- **WHEN** the experiment supports benefit but the user authorized only experimental removal
- **THEN** the evidence is retained while integration, baseline advancement and live publication remain pending their applicable authority

#### Scenario: An approved removal resumes with unchanged scope
- **WHEN** approval covers experiment and integration, required evidence passes and the scoped targets and consequences remain unchanged
- **THEN** the loop can complete those covered stages without another question, preserving the approval reference and actual effects

#### Scenario: New evidence reveals another supported consumer
- **WHEN** preparation or recovery finds a consumer whose capability loss was absent from the approved proposal
- **THEN** dependent removal or activation stops for an updated informed decision without discarding evidence or treating the previous approval as blanket permission

#### Scenario: The user declines a removal
- **WHEN** the user refuses the presented proposal
- **THEN** the loop retains the capability and decision, and does not repeat the same request without a new evidential basis or user instruction

### Requirement: Every hypothesis has one planning contract from initial measurement

Each hypothesis SHALL have its own OpenSpec change under the lifecycle owned by `orchestration-feedback-loop`, established before its first targeted baseline measurement. Existing observations can motivate that change without a speculative candidate implementation. The initial contract SHALL identify the problem, measurement question, scope, inputs, measurement tasks and evidence destination; the same change SHALL then own hypothesis formulation, experiment design, implementation, remeasurement, decision and restoration or adoption. Raw evidence SHALL remain in existing local storage with references, not be copied into planning artifacts or a second journal.

Before candidate edits, the change SHALL satisfy the complete per-hypothesis planning prerequisite, including the falsifiable mechanism, independent acceptance, chosen experimental unit, baseline basis, meaningful effect, controls, stopping and escalation/deferral rules. Evidence that changes the hypothesis or required scope SHALL revise the plan explicitly before dependent work; comparative results SHALL NOT retroactively redefine success. An ordinary evaluation workload that is not itself a harness-improvement hypothesis SHALL use its existing contract or the candidate's experiment contract without an artificial second hypothesis or OpenSpec change.

#### Scenario: Initial measurements precede a concrete solution
- **WHEN** observed friction justifies investigation but no candidate mechanism has yet been selected
- **THEN** the loop creates the hypothesis's change and records the targeted measurement plan before executing it, then develops the hypothesis and remaining artifacts in that same change

#### Scenario: A small instruction hypothesis has no specification
- **WHEN** an executor is about to implement a one-line instruction change without its complete linked OpenSpec artifacts
- **THEN** dispatch is prevented with the missing planning prerequisite identified

#### Scenario: A build is the evaluation workload
- **WHEN** a specified hypothesis is evaluated by building an existing project and no separate improvement B is being implemented
- **THEN** that workload has frozen inputs and independent acceptance under the experiment contract without a fabricated B change or implementation

#### Scenario: Task acceptance is revised between arms
- **WHEN** the evaluation task's requirements or checks change after its baseline attempt
- **THEN** the previous attempt cannot be paired with the revised candidate attempt

### Requirement: Autonomous experiments select sufficient work and preserve the accepted baseline

The loop SHALL use the experiment-selection contract in `harness-outcome-evaluation` to choose the smallest sufficient real workload and comparison method for each hypothesis before candidate implementation. It SHALL account for expected recurring usefulness, experiment cost, decision uncertainty, consequences of an incorrect decision and reversibility without an invented precision score. A short operation or agent task SHALL be eligible when it preserves the claimed mechanism and required acceptance. Whole-task paired implementations SHALL be selected when shorter work would omit decision-relevant strategy, interactions, corrections or outcomes. Repeated-use or recovery claims SHALL preserve the necessary sequence and state. There SHALL be no mandatory escalation ladder or obligation to implement an unrelated improvement twice.

Within established authority, the loop SHALL execute the planned initial measurements, formulate the hypothesis, implement the candidate, remeasure, publish a supported decision, settle the resulting runtime state and select the next grounded hypothesis. Adoption SHALL integrate only the exact supported candidate after applicable authority and combined-tree checks. Rejection SHALL restore and verify the accepted runtime without adding the candidate to mainline, preserving the candidate, failed attempts, reason and restoration evidence. An inconclusive candidate SHALL remain inactive; a new measurement SHALL require the declared decision-changing reason and stopping policy, otherwise the loop SHALL defer it and continue eligible work. Unknown or failed restoration SHALL block conflicting work on that resource while independent safe work can continue.

Restoring an owned experimental selection to its unchanged accepted baseline SHALL be part of authorized experiment recovery, not a new retirement of accepted capability. This SHALL NOT waive approval for a candidate that removes existing code, features or skills, authorize unrelated cleanup or erase useful work and evidence.

The loop SHALL also support A evaluated by implementing B under H and H+A, then B evaluated on C, when that work is applicable and justified. A and B SHALL retain their own hypothesis specifications and exact candidate identities. Workload B's correctness SHALL NOT establish its own benefit, and the next hypothesis SHALL NOT be forced to be the preceding workload.

#### Scenario: A short comparison is sufficient
- **WHEN** a build-reuse hypothesis can be decided through the real build/check cycle with required invalidation and correctness checks
- **THEN** the loop measures that cycle in both variants, records the supported scope and moves to the next hypothesis without two feature implementations

#### Scenario: A hypothesis changes the agent's implementation strategy
- **WHEN** a fixed-command probe would bypass the decisions and corrections named by the hypothesis
- **THEN** the loop selects an applicable agent workload through accepted completion, including paired full implementations when needed

#### Scenario: A candidate is rejected
- **WHEN** adequate applicable evidence rejects the candidate or independent acceptance fails
- **THEN** the loop records the scoped reason, verifies the accepted runtime is restored, retains the candidate and evidence, and automatically selects other eligible work without re-proposing the same mechanism on unchanged evidence

#### Scenario: Evidence is inconclusive
- **WHEN** remaining uncertainty can change the decision and another measurement is not justified under the plan
- **THEN** the candidate stays inactive with the missing fact and reconsideration condition recorded, while another eligible hypothesis can proceed

#### Scenario: A is adopted and B is next
- **WHEN** a justified A-on-B experiment supports adoption and B is independently selected as the next hypothesis
- **THEN** the loop advances to the evaluated A baseline, checks an exact useful B revision against it, and evaluates B on an applicable independently specified C

#### Scenario: B requires A to exist in the task inputs
- **WHEN** B cannot be implemented against the same source and requirements in both variants without depending on A's unaccepted source changes
- **THEN** B is not used for that paired comparison and an independent applicable workload is selected

#### Scenario: Restoration has an unknown outcome
- **WHEN** recovery cannot verify which runtime owns a shared measurement resource
- **THEN** the controller preserves the original error and evidence and starts no conflicting experiment until that resource's state is reconciled

### Requirement: Candidates can be selected without changing accepted source

Each hypothesis implementation SHALL have a dedicated branch from an explicit accepted base and an owned Git worktree, with exact revision and local ownership references bound to its Beads card. An existing worktree SHALL be reused only after verifying its ownership, base, active users and preservation of dirty, untracked and unmerged work; otherwise the loop SHALL allocate a separate owned worktree or report the unavailable prerequisite. The loop SHALL preserve baseline and candidate runtime artifacts and offer one explicit selection operation at a safe boundary. Selecting an unchanged prepared variant SHALL require no source revert, source cleanup, new model call or unnecessary rebuild, and SHALL identify the effective runtime actually selected. An active attempt SHALL keep its frozen runtime until it finishes or is explicitly cancelled. Worktree separation alone SHALL NOT be treated as isolation of shared Git history, installed configuration, services or model context.

#### Scenario: A prepared candidate is switched off and on
- **WHEN** baseline and candidate artifacts remain valid and no measured attempt is active
- **THEN** selection chooses the requested existing runtime and records its identity without modifying the accepted branch, rebuilding unchanged artifacts or retaining a previous executor's solution context

#### Scenario: An existing worktree contains another task's work
- **WHEN** the requested reusable worktree has an active owner or unpreserved modifications or commits
- **THEN** the loop leaves it intact and chooses another owned allocation or reports the conflict without reset, forced checkout or cleanup

#### Scenario: A candidate has not demonstrated benefit
- **WHEN** its implementation is complete but its decision is pending, rejected or inconclusive
- **THEN** its code remains outside the accepted mainline, with its branch and useful evidence preserved; rejecting it does not require reverting the mainline

#### Scenario: An accepted candidate is integrated
- **WHEN** an independently supported benefit decision and applicable authority, including any required removal approval, permit integration within the run's scope
- **THEN** only the evaluated candidate changes enter the mainline after combined-tree checks, with any changed base or conflict resolution requiring revalidation of affected benefit evidence before adoption

### Requirement: Correctness and benefit remain independent decisions

The declared comparison policy SHALL distinguish observed operational metrics from work metrics adjusted for attributable infrastructure waiting, using the rules and coverage gates in `harness-outcome-evaluation`. The loop SHALL NOT interpret unrelated queue imbalance as model/harness benefit or regression, or omit a treatment's own scheduling, polling or build-demand effects. Missing attribution that could change the decision SHALL hold that conclusion as inconclusive without discarding the attempt.

The loop SHALL enforce the declared experiment method and its applicable operating-condition, independent-calibration and distinct-uncertainty requirements from `harness-outcome-evaluation`. Full paired implementation SHALL remain required whenever it is the selected sufficient method, and a shorter method SHALL NOT inherit an unmeasured broader claim. Its decision SHALL distinguish an observed task-scoped effect from repeatable or broader benefit. Cache, recovery and model-strategy changes caused by the treatment SHALL remain outcomes; unresolved nuisance effects SHALL NOT be silently removed or converted into statistical confidence. Insufficient evidence SHALL follow the declared escalation, repetition or deferral policy; component counters and replay evidence SHALL NOT substitute for an unexercised agent or end-to-end effect.

Candidate implementation checks and task acceptance SHALL be independent of candidate-controlled claims. The loop SHALL use `harness-outcome-evaluation` to decide `adopt`, `reject` or `inconclusive`, enforce required correctness, and retain all attempts. A metric improvement SHALL NOT excuse weakened requirements, changed oracles or discarded failures. Adoption SHALL identify the exact tested candidate, its supported task/model scope and baseline; reworked or combined candidates SHALL be checked on their new identities before advancement. Live publication SHALL follow existing installation and skill lifecycles within its established authority rather than follow automatically from a task's closed status.

#### Scenario: Faster output hides an incorrect implementation
- **WHEN** a candidate reduces elapsed time but its task solution fails independent acceptance
- **THEN** that result cannot support adoption, and the failure remains in resource accounting

#### Scenario: The controller itself is the proposed improvement
- **WHEN** a hypothesis changes evaluation or control components
- **THEN** an unchanged evaluator outside the candidate's writable scope assesses the candidate, and it cannot redefine its own acceptance or activate itself in the running controller

### Requirement: Continuous operation preserves recoverable progress

An explicitly started loop SHALL remain available across successive experiments until stopped or constrained by its configured operating policy. It SHALL expose current baseline, active hypothesis and its specification, selected method and workload, phase, last decision, restoration state and next action, including idle, waiting-for-input, blocked and recovery conditions. Stop SHALL suspend new work, resolve or explicitly retain in-flight effects, and preserve evidence and useful patches. Restart/resume SHALL reconcile the board, artifact identities and known process outcomes before continuing; it MUST NOT replay a model attempt, board decision or activation whose outcome is uncertain. Resource limits SHALL bound attempts and retained data without imposing an undocumented lifetime limit on the service.

#### Scenario: Interruption follows a recorded adoption
- **WHEN** the process exits after publishing an adoption decision but before confirming baseline activation
- **THEN** resume verifies the exact decision, applicable removal approval and installed experimental identity, completes or repairs only the authorized transition without a new trial or duplicate decision, and reports the recovered phase

#### Scenario: A provider becomes unavailable
- **WHEN** the configured endpoint returns a persistent availability failure
- **THEN** the loop preserves completed work, reports the cause, uses bounded recovery under the configured policy, and does not substitute a different provider

#### Scenario: A comparison has only one completed arm
- **WHEN** the user stops the loop after the baseline arm
- **THEN** resume can reuse it only if its task, runtime, qualification and environment remain comparable; otherwise it records why remeasurement is required

### Requirement: Every model conversation remains attributable and visible

Every model conversation used for research, candidate implementation or measured task execution SHALL use the existing dispatch mechanism with an identified actual model/configuration and its own visible titled terminal surface. A model-free workload SHALL use the native execution/evidence path without creating an artificial conversation. A transcript or background process alone SHALL NOT satisfy conversation visibility. If the required surface is unavailable, the loop SHALL preserve in-flight state and suspend new model dispatch. Measurements sharing local inference hardware SHALL not overlap with other loop-generated model work unless that concurrency is an explicitly controlled part of both arms. The loop SHALL report external interference that prevents a valid comparison.

#### Scenario: An executor surface closes
- **WHEN** the owning dispatch mechanism reports loss of a required conversation surface
- **THEN** no new model work is started until the surface is restored or the run is explicitly stopped, without inspecting conversation-window screenshots

#### Scenario: Hypothesis generation would contend with measurement
- **WHEN** researcher and measured executor share the configured inference device
- **THEN** their inference is serialized by default and the researcher cannot silently distort one arm's timing

### Requirement: Evidence storage is bounded without becoming another tracker

Beads SHALL remain the durable hypothesis and decision owner. Local execution state SHALL contain only recoverable process/phase information, retained inputs, patches, receipts and measurements linked to those cards; it SHALL NOT become a competing hypothesis queue or decision journal. Retention SHALL protect evidence required for active comparisons, adopted-baseline provenance and pending recovery, with explicit missing-evidence results for unavailable artifacts. Raw transcripts, local endpoints, credentials and machine-specific paths SHALL remain in local owning storage and MUST NOT enter shared source or public board content.

#### Scenario: An old referenced artifact is unavailable
- **WHEN** a proposed comparison or adoption relies on evidence no longer retained
- **THEN** that claim remains unsupported and the loop identifies the missing evidence instead of reconstructing a favorable result from a card summary

### Requirement: The skill and controller are globally delivered and exercised

The `self-improvement-loop` skill and executable support SHALL be delivered through the existing kit installation/update/recovery lifecycle and usable from an owned project outside this checkout. Acceptance SHALL exercise the real configured local model, actual Beads and OpenSpec, installed tool/instruction consumption, autonomous progress from initial measurement through decision and verified restoration or adoption, both a short real-operation comparison and a justified A-on-B comparison followed by a B-on-C transition, independent task checks and stop/resume. The short path SHALL demonstrate that no duplicate implementation or workload hypothesis is required; the full path SHALL remain exercised rather than being replaced by model-free checks. Model-free fixtures SHALL verify failure paths but SHALL NOT replace the real installed path or prove model-backed savings. Missing local model access SHALL keep the corresponding acceptance tasks open.

#### Scenario: An external project invokes the installed skill
- **WHEN** a user starts the loop from an owned outside-checkout project with qualified runtime inputs
- **THEN** the installed skill and controller find the intended board and specification root, perform the autonomous measurement-to-decision path with the selected sufficient method, and expose retained decisions without modifying unrelated installed settings
