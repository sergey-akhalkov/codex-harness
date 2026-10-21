## Purpose

Autonomously measure, formulate, implement and evaluate evidence-backed harness improvements using proportionate experiments, preserving correctness, accepted baselines, useful work and durable decisions on the existing Beads board.

## ADDED Requirements

### Requirement: Hypotheses originate in attributable evidence

The loop SHALL admit only hypotheses grounded in retained observations with a causal mechanism, applicability, predicted effect, counterexample and predeclared acceptance. Missing or merely asserted data SHALL leave implementation unqualified.

#### Scenario: Admission checks the problem evidence
- **WHEN** a candidate is considered for hypothesis admission
- **THEN** The loop SHALL admit a hypothesis only with a concrete observation, evidence locator and applicability, a causal explanation, an expected quality/time/resource effect, a plausible counterexample and predeclared acceptance.
- **AND** Sources SHALL include actual task traces, reproducible defects, measured tool behavior and verified implementation or dependency constraints.
- **AND** Observations, inferences and predictions SHALL remain distinguishable.
- **AND** External research SHALL support an applicable mechanism rather than substitute for evidence of a relevant problem.
- **AND** Initial hypotheses SHALL use existing evidence or observation of authorized real work; the loop MUST NOT fabricate observations or synthesize demand solely to remain busy.

#### Scenario: A tool emits large successful outputs
- **WHEN** retained trials show large outputs and repeated context consumption attributable to successful checks
- **THEN** the investigator can propose bounded output with an explicit diagnostic-preservation check and predicted end-to-end effect, without calling byte reduction proven token savings

#### Scenario: No grounded candidate remains
- **WHEN** available observations do not justify another hypothesis
- **THEN** the loop exposes an idle reason and awaits fresh evidence or authorized real work instead of generating arbitrary modifications

#### Scenario: The cited observation is missing or only asserted
- **WHEN** a proposal cites an unavailable record, a prediction, or an unverified task-completion claim as its problem evidence
- **THEN** candidate implementation remains unqualified until actual attributable data is retained, and the missing evidence cannot be replaced by more persuasive wording

#### Scenario: Current source already addresses the historical problem
- **WHEN** source and retained checks show that the proposal's assumed defect has already been addressed
- **THEN** the loop reconciles remaining acceptance and qualifies any historical control before further implementation, rather than treating unchecked tasks as proof that the defect still exists

### Requirement: An explicit run defines scope and operating inputs

A loop run SHALL bind its project, source, authority, experiment policy and actual model route before dependent effects. Routine in-scope transitions SHALL proceed autonomously; missing inputs or material scope changes SHALL hold dependent work without substituting routes.

#### Scenario: Run inputs establish the operating boundary
- **WHEN** a loop run is started or resumed
- **THEN** The installed skill SHALL start or resume an explicitly selected loop against an identified project, Beads board, source revision, writable scope, experiment policy and model configuration.
- **AND** The policy SHALL identify permitted researcher and executor routes, the explicitly selected repeatability/identity policy, resource limits, decision scope and any publication authority.
- **AND** Routine initial measurement, hypothesis formulation, experiment selection, planning, implementation, remeasurement, supported adoption, candidate rollback and selection of the next hypothesis within that authority SHALL continue autonomously without per-hypothesis confirmation; material scope or operating-condition changes SHALL require a user decision.
- **AND** Missing route inputs or observations required by the selected identity policy SHALL permit model-free preparation but MUST NOT trigger guessed endpoints, changed models or paid fallbacks.
- **AND** The selected API-observed policy SHALL keep unavailable weight hashes and hardware details visible as limits without requiring them as setup inputs.

#### Scenario: Model details arrive later
- **WHEN** the local endpoint or exact serving configuration has not been supplied
- **THEN** the loop can validate its planning and owned inputs, identifies the missing runtime inputs, and starts no dependent model trial

#### Scenario: A hypothesis needs a new external service
- **WHEN** an otherwise promising experiment requires access outside the authorized scope
- **THEN** dependent work waits for that decision while independent authorized work and evidence are preserved

### Requirement: Simplification is a first-class improvement hypothesis

The loop SHALL consider reuse and removable complexity before additions, preserve task-closing quality and apply the declared treatment-specific benefit gate. Nonuse or reduced size alone SHALL NOT justify removal or establish benefit.

#### Scenario: Treatment selection preserves quality and the addition gate
- **WHEN** the loop selects or evaluates a simplification, subtraction, addition or enhancement
- **THEN** For an observed cost, the loop SHALL consider whether existing capability, no change, simplification or subtraction can satisfy the intended outcome before proposing additional machinery, and SHALL examine removable complexity first: a simplification candidate SHOULD reduce the maintained entity surface AND improve accepted-task speed or token economy together.
- **AND** Additions and enhancements SHALL be proposed only after the loop finds no further removable complexity, and SHALL be judged on all three dimensions together - lower token consumption, faster work and higher task-closing quality, where quality means BOTH accepted tasks that do not later require extensive rework from surfacing critical P0/P1 defects AND the overall task success rate; an addition improving cost while leaving task success or defect behavior unchanged at best SHALL NOT be claimed as a benefit, and a measured drop in task success rate SHALL reject the benefit and restore the accepted baseline even when the change contains no defect of its own.
- **AND** Applicable treatments SHALL include consolidation, narrower default exposure, on-demand loading, disabling and removal of skills, code/features, instructions, documentation, tools/dependencies, configuration, workflow stages, checks and generated outputs.
- **AND** A hypothesis SHALL identify the burden it removes, the outcomes that remain required, possible lost uses and a falsifiable expected effect on accepted-task time, steps or resources.
- **AND** Work quality, reasoning depth and task-closing ability SHALL remain maximal in every arm; speed or token savings SHALL NOT be accepted in exchange for reduced ability to close tasks correctly.
- **AND** The loop SHALL NOT require a new audit service, exhaustive inventory or fixed deletion quota; review SHALL reuse relevant evidence and existing owners proportionately.

#### Scenario: Nonuse claims retain supported uses and uncertainty
- **WHEN** usage or size evidence is used to justify a proposed treatment
- **THEN** Low or absent observed use SHALL be a lead for investigation, not a finding of uselessness.
- **AND** Usage claims SHALL identify the observation interval, task/environment coverage and telemetry gaps.
- **AND** Review SHALL include applicable rare recovery, compatibility, explicit invocation and indirect consumers; unresolved supported use SHALL remain explicit.
- **AND** Correctness, integrity, recovery and required quality SHALL remain binding even when a capability gives no everyday speed or token saving.
- **AND** Fewer lines, files, skills or exposed names alone SHALL NOT establish net benefit.

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

Applying removal of accepted code, features or skills SHALL require the current informed user decision covering the exact targets, lost behavior and action stage. Preparation and ordinary experiment recovery SHALL preserve their distinct authority; benefit alone SHALL NOT grant removal consent.

#### Scenario: A removal is prepared for an informed decision
- **WHEN** the loop prepares to implement a removal of existing capability
- **THEN** Before implementing a removal of existing code, features or skills, the loop SHALL present a reviewable proposal and obtain an explicit user decision recorded through `orchestration-feedback-loop`.
- **AND** The proposal SHALL identify target paths/capabilities and source basis, evidence and its gaps, observed versus predicted benefit, what users lose and in which scenarios, affected callers/configuration/installations, retained requirements and checks, alternatives including no change, and a recovery route.
- **AND** Unverified dependencies or consumer access SHALL be disclosed rather than treated as absent.
- **AND** The approval SHALL identify the authorized targets, behavior loss and action scope: isolated experiment, accepted-source integration and/or installed publication.

#### Scenario: Preparation does not apply an unapproved removal
- **WHEN** a removal is investigated, previewed or proposed for experimental execution
- **THEN** Read-only investigation, planning and an unapplied diff preview SHALL be allowed before approval.
- **AND** Applying the removal SHALL wait even in a candidate worktree or evaluation workload.
- **AND** Disabling, de-registration or consolidation that withdraws an existing capability SHALL use the same gate; renaming the action SHALL NOT bypass it.
- **AND** Ordinary additions and edits preserving capability remain under existing run authority, but deleting existing code or skill definitions as a simplification treatment SHALL use this gate.
- **AND** General loop-start authority, low usage, an agent-authored decision or a favorable benefit result SHALL NOT substitute for approval.
- **AND** Silence SHALL leave dependent work pending while independent authorized work can continue.

#### Scenario: Later stages reuse only applicable consent
- **WHEN** an approved removal advances to another stage or resumes
- **THEN** One approval SHALL suffice for all stages expressly covered by its unchanged scope.
- **AND** Before a later stage or resume, the loop SHALL confirm that the actual target, behavior loss, dependency findings and action remain covered and the approval has not been withdrawn.
- **AND** Material changes or uncovered publication SHALL require a new decision; routine continuation of the approved scope SHALL NOT create another approval ritual.
- **AND** Benefit and required checks SHALL still gate adoption, and approval SHALL NOT waive unrelated acceptance, authorize evidence destruction or allow a candidate to remove its own control safeguards.

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

Each hypothesis SHALL use one OpenSpec change from targeted initial measurement through decision and verified adoption or restoration. Candidate edits SHALL require complete qualified artifacts and a frozen falsifiable experiment; raw evidence and decisions SHALL remain with their existing owners.

#### Scenario: The investigation establishes its planning owner
- **WHEN** a hypothesis begins targeted initial measurement
- **THEN** Each hypothesis SHALL have its own OpenSpec change under the lifecycle owned by `orchestration-feedback-loop`, established before its first targeted baseline measurement.
- **AND** Existing observations can motivate that change without a speculative candidate implementation.
- **AND** The initial contract SHALL identify the problem, measurement question, scope, inputs, measurement tasks and evidence destination; the same change SHALL then own hypothesis formulation, experiment design, implementation, remeasurement, decision and restoration or adoption.
- **AND** Raw evidence SHALL remain in existing local storage with references, not be copied into planning artifacts or a second journal.

#### Scenario: Implementation requires the qualified frozen plan
- **WHEN** candidate implementation or a material plan revision is considered
- **THEN** Before candidate edits, the change SHALL satisfy the complete per-hypothesis planning prerequisite, including the falsifiable mechanism, independent acceptance, chosen experimental unit, baseline basis, meaningful effect, controls, stopping and escalation/deferral rules.
- **AND** Evidence that changes the hypothesis or required scope SHALL revise the plan explicitly before dependent work; comparative results SHALL NOT retroactively redefine success.
- **AND** An ordinary evaluation workload that is not itself a harness-improvement hypothesis SHALL use its existing contract or the candidate's experiment contract without an artificial second hypothesis or OpenSpec change.

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

#### Scenario: The experiment promises measurement without a decision rule
- **WHEN** artifacts say to measure improvement but omit the primary outcome, unit, useful-effect threshold, retained regression constraints, observation horizon or stopping rule
- **THEN** formal OpenSpec validity does not authorize candidate implementation; those fields must be qualified and frozen in the existing experiment contract first

### Requirement: Autonomous experiments select sufficient work and preserve the accepted baseline

Within run authority, the loop SHALL execute the qualified data-to-decision cycle using a sufficient real experiment. Only supported candidates SHALL enter accepted use; rejection or inconclusive evidence SHALL leave the accepted baseline verified and conflicting work blocked until recovery is known.

#### Scenario: The mechanism determines the experimental unit
- **WHEN** the loop selects an experiment before candidate implementation
- **THEN** The loop SHALL use the experiment-selection contract in `harness-outcome-evaluation` to choose the smallest sufficient real workload and comparison method for each hypothesis before candidate implementation.
- **AND** It SHALL account for expected recurring usefulness, experiment cost, decision uncertainty, consequences of an incorrect decision and reversibility without an invented precision score.
- **AND** A short operation or agent task SHALL be eligible when it preserves the claimed mechanism and required acceptance.
- **AND** Whole-task paired implementations SHALL be selected when shorter work would omit decision-relevant strategy, interactions, corrections or outcomes.
- **AND** Repeated-use or recovery claims SHALL preserve the necessary sequence and state.
- **AND** There SHALL be no mandatory escalation ladder or obligation to implement an unrelated improvement twice.

#### Scenario: A measured decision determines adoption or restoration
- **WHEN** the loop advances an authorized hypothesis through implementation and evaluation
- **THEN** Within established authority, the loop SHALL execute the planned initial measurements, formulate the hypothesis, implement the candidate, remeasure, publish a supported decision, settle the resulting runtime state and select the next grounded hypothesis.
- **AND** Adoption SHALL integrate only the exact supported candidate after applicable authority and combined-tree checks.
- **AND** Rejection SHALL restore and verify the accepted runtime without adding the candidate to mainline, preserving the candidate, failed attempts, reason and restoration evidence.
- **AND** An inconclusive candidate SHALL remain inactive; a new measurement SHALL require the declared decision-changing reason and stopping policy, otherwise the loop SHALL defer it and continue eligible work.
- **AND** Unknown or failed restoration SHALL block conflicting work on that resource while independent safe work can continue.

#### Scenario: Experimental restoration preserves its authority boundary
- **WHEN** an owned experimental selection is restored to its unchanged accepted baseline
- **THEN** Restoring an owned experimental selection to its unchanged accepted baseline SHALL be part of authorized experiment recovery, not a new retirement of accepted capability.
- **AND** This SHALL NOT waive approval for a candidate that removes existing code, features or skills, authorize unrelated cleanup or erase useful work and evidence.

#### Scenario: Workload rotation keeps hypotheses independent
- **WHEN** A-on-B or B-on-C rotation is selected as an applicable experiment
- **THEN** The loop SHALL also support A evaluated by implementing B under H and H+A, then B evaluated on C, when that work is applicable and justified.
- **AND** A and B SHALL retain their own hypothesis specifications and exact candidate identities.
- **AND** Workload B's correctness SHALL NOT establish its own benefit, and the next hypothesis SHALL NOT be forced to be the preceding workload.

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

#### Scenario: Rejected code is retained as evidence
- **WHEN** a rejected or inconclusive candidate's branch or patch is retained
- **THEN** it remains outside accepted source and active installation, restoration verifies the actually consumed baseline and its affected user path, and retained evidence cannot be mistaken for supported delivery

#### Scenario: An experimental effect escaped isolation
- **WHEN** recovery finds an unadopted candidate effect in accepted source, selected guidance/configuration or active installation
- **THEN** the owning recovery path removes or reverts only the exact candidate effect with recovery inputs preserved, verifies restored baseline behavior, preserves unrelated work and reports any unresolved restoration as a blocker for conflicting use

### Requirement: Candidates can be selected without changing accepted source

Candidates SHALL remain on owned branches/worktrees with exact base and revision identities until supported integration. Prepared runtime selection SHALL preserve accepted source, active attempts and unrelated work, with actual consumption verified through the existing lifecycle.

#### Scenario: Candidate preparation and selection retain isolation
- **WHEN** a hypothesis receives an implementation checkout or selects a prepared runtime
- **THEN** Each hypothesis implementation SHALL have a dedicated branch from an explicit accepted base and an owned Git worktree, with exact revision and local ownership references bound to its Beads card.
- **AND** An existing worktree SHALL be reused only after verifying its ownership, base, active users and preservation of dirty, untracked and unmerged work; otherwise the loop SHALL allocate a separate owned worktree or report the unavailable prerequisite.
- **AND** The loop SHALL preserve baseline and candidate runtime artifacts and offer one explicit selection operation at a safe boundary.
- **AND** Selecting an unchanged prepared variant SHALL require no source revert, source cleanup, new model call or unnecessary rebuild, and SHALL identify the effective runtime actually selected.
- **AND** An active attempt SHALL keep its frozen runtime until it finishes or is explicitly cancelled.
- **AND** Worktree separation alone SHALL NOT be treated as isolation of shared Git history, installed configuration, services or model context.

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

### Requirement: Retained workload solutions are recovered before new implementation

Before implementer dispatch, the controller SHALL distinguish verified absence from recoverable or unresolved retained workload evidence. It SHALL recover exact independently accepted solutions through existing owners without new model implementation. Unresolved evidence SHALL block substitute work until recovery or a separate reasoned decision within run authority. Recovery SHALL preserve source work, acceptance, scope, planning, removal authority and exact lineage.

#### Scenario: The accepted solution is in an isolated arm repository
- **WHEN** its commit is absent from the candidate repository but its recorded source and acceptance evidence remain available
- **THEN** the controller verifies the original base, revision, independent acceptance and resulting change identity, transfers the exact solution to the owned candidate allocation, and applies existing scope, planning, removal-authority and integration checks without a new implementation conversation or exposing it to another measured executor
- **AND** the source and unrelated work remain intact, and carried implementation acceptance alone grants neither benefit nor adoption authority

#### Scenario: A retained solution cannot be resolved
- **WHEN** its source is missing, its own record is malformed, lookup fails or its identity contradicts the retained acceptance
- **THEN** it remains unavailable rather than absent, its records and partial work are preserved with the exact blocker and recovery action, and repeated resume starts no substitute implementation without a separate reasoned decision on the existing hypothesis owner after exhausted recovery

#### Scenario: Recovery is interrupted after materialization
- **WHEN** the exact retained change is already present but recovery bookkeeping is incomplete
- **THEN** resume reconciles exact source and destination identities and finishes only the missing bookkeeping without applying the change again or dispatching an implementer

#### Scenario: The resulting baseline requires corrections
- **WHEN** carrying the retained change requires conflict resolution or alters its content
- **THEN** the loop preserves the original solution and lineage, records the changed candidate identity, invalidates affected acceptance or benefit evidence and performs the affected checks before claiming acceptance or benefit

#### Scenario: No retained solution exists
- **WHEN** a successful lookup confirms no retained workload solution for the selected hypothesis and the ordinary dispatch gates pass
- **THEN** the controller may start the ordinary implementation path while still refusing to replay active, unknown or already completed attempts

### Requirement: Correctness and benefit remain independent decisions

Independent correctness acceptance and measured benefit SHALL be separate adoption gates. The evaluator SHALL apply the frozen metric, comparability and uncertainty policy to the exact candidate and all attempts; candidate assertions, proxies and successful implementation alone SHALL NOT establish benefit.

#### Scenario: The metric view preserves infrastructure attribution
- **WHEN** the loop evaluates observed or adjusted operational outcomes
- **THEN** The declared comparison policy SHALL distinguish observed operational metrics from work metrics adjusted for attributable infrastructure waiting, using the rules and coverage gates in `harness-outcome-evaluation`.
- **AND** The loop SHALL NOT interpret unrelated queue imbalance as model/harness benefit or regression, or omit a treatment's own scheduling, polling or build-demand effects.
- **AND** Missing attribution that could change the decision SHALL hold that conclusion as inconclusive without discarding the attempt.

#### Scenario: The chosen method bounds the benefit claim
- **WHEN** the loop determines whether experiment evidence supports its declared claim
- **THEN** The loop SHALL enforce the declared experiment method and its applicable operating-condition, independent-calibration and distinct-uncertainty requirements from `harness-outcome-evaluation`.
- **AND** Full paired implementation SHALL remain required whenever it is the selected sufficient method, and a shorter method SHALL NOT inherit an unmeasured broader claim.
- **AND** Its decision SHALL distinguish an observed task-scoped effect from repeatable or broader benefit.
- **AND** Cache, recovery and model-strategy changes caused by the treatment SHALL remain outcomes; unresolved nuisance effects SHALL NOT be silently removed or converted into statistical confidence.
- **AND** Insufficient evidence SHALL follow the declared escalation, repetition or deferral policy; component counters and replay evidence SHALL NOT substitute for an unexercised agent or end-to-end effect.

#### Scenario: Adoption binds independent acceptance and exact lineage
- **WHEN** a candidate is proposed for adoption, integration or publication
- **THEN** Candidate implementation checks and task acceptance SHALL be independent of candidate-controlled claims.
- **AND** The loop SHALL use `harness-outcome-evaluation` to decide `adopt`, `reject` or `inconclusive`, enforce required correctness, and retain all attempts.
- **AND** A metric improvement SHALL NOT excuse weakened requirements, changed oracles or discarded failures.
- **AND** Adoption SHALL identify the exact tested candidate, its supported task/model scope and baseline; reworked or combined candidates SHALL be checked on their new identities before advancement.
- **AND** Live publication SHALL follow existing installation and skill lifecycles within its established authority rather than follow automatically from a task's closed status.

#### Scenario: Faster output hides an incorrect implementation
- **WHEN** a candidate reduces elapsed time but its task solution fails independent acceptance
- **THEN** that result cannot support adoption, and the failure remains in resource accounting

#### Scenario: Functional success is mistaken for hypothesis success
- **WHEN** implementation passes functional acceptance but the complete comparison fails its predeclared useful-effect requirement
- **THEN** the hypothesis is rejected for that scope, its costs and negative result remain recorded, and functional success alone cannot authorize candidate integration or publication

#### Scenario: An addition is evaluated with a weaker generic policy
- **WHEN** a candidate adds or enhances capability but its policy can pass on one cheaper metric with unchanged task-closing quality
- **THEN** that policy cannot authorize adoption under the confirmed addition constraints; the treatment's required quality, time and token outcomes must all be evaluated on adequate applicable data

#### Scenario: The controller itself is the proposed improvement
- **WHEN** a hypothesis changes evaluation or control components
- **THEN** an unchanged evaluator outside the candidate's writable scope assesses the candidate, and it cannot redefine its own acceptance or activate itself in the running controller

### Requirement: Continuous operation preserves recoverable progress

The loop SHALL preserve attributable phase, ownership and decision state across continued operation, stop and resume. Recovery SHALL reconcile actual effects and frozen inputs without replaying unknown attempts or inventing service-lifetime limits.

#### Scenario: The run continues or recovers across lifecycle boundaries
- **WHEN** an explicitly started loop progresses, stops, restarts or resumes
- **THEN** An explicitly started loop SHALL remain available across successive experiments until stopped or constrained by its configured operating policy.
- **AND** It SHALL expose current baseline, active hypothesis and its specification, selected method and workload, phase, last decision, restoration state and next action, including idle, waiting-for-input, blocked and recovery conditions.
- **AND** Stop SHALL suspend new work, resolve or explicitly retain in-flight effects, and preserve evidence and useful patches.
- **AND** Restart/resume SHALL reconcile the board, artifact identities and known process outcomes before continuing; it MUST NOT replay a model attempt, board decision or activation whose outcome is uncertain.
- **AND** Resource limits SHALL bound attempts and retained data without imposing an undocumented lifetime limit on the service.

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

Every loop model conversation SHALL use its actual configured identity and a distinct visible surface through the existing dispatcher. Lost visibility SHALL suspend new model work; shared inference contention SHALL follow the declared measurement controls.

#### Scenario: Model work is admitted and observed
- **WHEN** research, implementation or a measured attempt requires model execution
- **THEN** Every model conversation used for research, candidate implementation or measured task execution SHALL use the existing dispatch mechanism with an identified actual model/configuration and its own visible titled terminal surface.
- **AND** A model-free workload SHALL use the native execution/evidence path without creating an artificial conversation.
- **AND** A transcript or background process alone SHALL NOT satisfy conversation visibility.
- **AND** If the required surface is unavailable, the loop SHALL preserve in-flight state and suspend new model dispatch.
- **AND** Measurements sharing local inference hardware SHALL not overlap with other loop-generated model work unless that concurrency is an explicitly controlled part of both arms.
- **AND** The loop SHALL report external interference that prevents a valid comparison.

#### Scenario: An executor surface closes
- **WHEN** the owning dispatch mechanism reports loss of a required conversation surface
- **THEN** no new model work is started until the surface is restored or the run is explicitly stopped, without inspecting conversation-window screenshots

#### Scenario: Hypothesis generation would contend with measurement
- **WHEN** researcher and measured executor share the configured inference device
- **THEN** their inference is serialized by default and the researcher cannot silently distort one arm's timing

### Requirement: Evidence storage is bounded without becoming another tracker

Beads SHALL own hypothesis and decision state; local evidence SHALL preserve attributable execution, recovery and baseline provenance under bounded retention. Missing evidence SHALL remain unsupported, and private runtime data SHALL stay outside public source.

#### Scenario: Evidence is stored or retained
- **WHEN** the loop writes or retains hypothesis execution and recovery data
- **THEN** Beads SHALL remain the durable hypothesis and decision owner.
- **AND** Local execution state SHALL contain only recoverable process/phase information, retained inputs, patches, receipts and measurements linked to those cards; it SHALL NOT become a competing hypothesis queue or decision journal.
- **AND** Retention SHALL protect evidence required for active comparisons, adopted-baseline provenance and pending recovery, with explicit missing-evidence results for unavailable artifacts.
- **AND** Raw transcripts, local endpoints, credentials and machine-specific paths SHALL remain in local owning storage and MUST NOT enter shared source or public board content.

#### Scenario: An old referenced artifact is unavailable
- **WHEN** a proposed comparison or adoption relies on evidence no longer retained
- **THEN** that claim remains unsupported and the loop identifies the missing evidence instead of reconstructing a favorable result from a card summary

### Requirement: The skill and controller are globally delivered and exercised

Installed loop acceptance SHALL exercise the actual configured model, board, specifications and consumed assets outside the checkout through measurement, decision, continuation and recovery. Fixtures SHALL NOT replace the required real operation, agent comparisons or installed-path evidence.

#### Scenario: Global readiness is assessed
- **WHEN** the skill and controller are proposed as globally delivered and accepted
- **THEN** The `self-improvement-loop` skill and executable support SHALL be delivered through the existing kit installation/update/recovery lifecycle and usable from an owned project outside this checkout.
- **AND** Acceptance SHALL exercise the actual configured model route, actual Beads and OpenSpec, installed tool/instruction consumption, autonomous progress from initial measurement through decision and verified restoration or adoption, both a short real-operation comparison and a justified A-on-B comparison followed by a B-on-C transition, independent task checks and stop/resume.
- **AND** The short path SHALL demonstrate that no duplicate implementation or workload hypothesis is required; the full path SHALL remain exercised rather than being replaced by model-free checks.
- **AND** Model-free fixtures SHALL verify failure paths but SHALL NOT replace the real installed path or prove model-backed savings.
- **AND** Missing access to the configured model SHALL keep the corresponding acceptance tasks open.

#### Scenario: An external project invokes the installed skill
- **WHEN** a user starts the loop from an owned outside-checkout project with qualified runtime inputs
- **THEN** the installed skill and controller find the intended board and specification root, perform the autonomous measurement-to-decision path with the selected sufficient method, and expose retained decisions without modifying unrelated installed settings
