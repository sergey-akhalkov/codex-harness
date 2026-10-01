## Purpose

Continuously investigate evidence-backed harness improvements by using real improvement tasks as controlled workloads, preserving correctness, reproducibility, useful implementations and durable decisions on the existing Beads board.

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

The installed skill SHALL start or resume an explicitly selected loop against an identified project, Beads board, source revision, writable scope, experiment policy and model configuration. The policy SHALL identify permitted researcher and executor routes, the explicitly selected repeatability/identity policy, resource limits, decision scope and any publication authority. Routine planning, implementation and measurement within that authority SHALL continue autonomously; material scope or operating-condition changes SHALL require a user decision. Missing route inputs or observations required by the selected identity policy SHALL permit model-free preparation but MUST NOT trigger guessed endpoints, changed models or paid fallbacks. The selected API-observed policy SHALL keep unavailable weight hashes and hardware details visible as limits without requiring them as setup inputs.

#### Scenario: Model details arrive later
- **WHEN** the local endpoint or exact serving configuration has not been supplied
- **THEN** the loop can validate its planning and owned inputs, identifies the missing runtime inputs, and starts no dependent model trial

#### Scenario: A hypothesis needs a new external service
- **WHEN** an otherwise promising experiment requires access outside the authorized scope
- **THEN** dependent work waits for that decision while independent authorized work and evidence are preserved

### Requirement: Simplification is a first-class improvement hypothesis

For an observed cost, the loop SHALL consider whether existing capability, no change, simplification or subtraction can satisfy the intended outcome before proposing additional machinery. Applicable treatments SHALL include consolidation, narrower default exposure, on-demand loading, disabling and removal of skills, code/features, instructions, documentation, tools/dependencies, configuration, workflow stages, checks and generated outputs. A hypothesis SHALL identify the burden it removes, the outcomes that remain required, possible lost uses and a falsifiable expected effect on accepted-task time, steps or resources. The loop SHALL NOT require a new audit service, exhaustive inventory or fixed deletion quota; review SHALL reuse relevant evidence and existing owners proportionately.

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

### Requirement: Every implemented hypothesis has a complete planning contract

Before changing any candidate implementation, the loop SHALL satisfy the per-hypothesis OpenSpec prerequisite owned by `orchestration-feedback-loop`. This includes the hypothesis serving as the evaluation task. Both arms SHALL consume the same frozen task contract, including its applicable planning artifacts and independent acceptance. A change to that contract during a comparison SHALL invalidate comparability rather than silently update one arm.

#### Scenario: A small instruction hypothesis has no specification
- **WHEN** an executor is about to implement a one-line instruction change without its complete linked OpenSpec artifacts
- **THEN** dispatch is prevented with the missing planning prerequisite identified

#### Scenario: Task acceptance is revised between arms
- **WHEN** the evaluation task's requirements or checks change after its baseline attempt
- **THEN** the previous attempt cannot be paired with the revised candidate attempt

### Requirement: Real improvement tasks rotate through candidate and workload roles

The loop SHALL support the sequence A evaluated on B, then B evaluated on C, without requiring reciprocal A/B comparisons or a separate synthetic coding suite. It SHALL prepare a frozen candidate A, solve the frozen real task B with baseline H and candidate H+A, independently accept each solution, decide A, and retain exact useful implementations of B. The next baseline SHALL be H+A only after a supported adoption of A and any required removal approval for that transition; otherwise it SHALL remain H. Before B becomes the next candidate, its exact selected implementation SHALL be integrated and checked on its candidate branch against that current baseline, with any changed revision recorded. Implementing B successfully SHALL NOT establish B's own benefit or close its hypothesis prematurely.

#### Scenario: A is adopted and B is next
- **WHEN** A's declared acceptance passes, applicable removal approval covers advancement and both attempts at B are retained
- **THEN** the loop advances the experimental baseline to the evaluated A revision, selects and checks an exact B revision against it, and evaluates B on a new specified real task C

#### Scenario: A is rejected
- **WHEN** A regresses required behavior or fails the predeclared benefit decision
- **THEN** H remains the baseline, A's evidence and decision are retained, and a usable B implementation can still proceed toward evaluation on C

#### Scenario: B requires A to exist in the task inputs
- **WHEN** B cannot be implemented against the same source and requirements in both arms without depending on A's unaccepted source changes
- **THEN** B is not used for that paired comparison; the loop records the dependency and selects an independent applicable real task

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

Candidate implementation checks and task acceptance SHALL be independent of candidate-controlled claims. The loop SHALL use `harness-outcome-evaluation` to decide `adopt`, `reject` or `inconclusive`, enforce required correctness, and retain all attempts. A metric improvement SHALL NOT excuse weakened requirements, changed oracles or discarded failures. Adoption SHALL identify the exact tested candidate, its supported task/model scope and baseline; reworked or combined candidates SHALL be checked on their new identities before advancement. Live publication SHALL follow existing installation and skill lifecycles within its established authority rather than follow automatically from a task's closed status.

#### Scenario: Faster output hides an incorrect implementation
- **WHEN** a candidate reduces elapsed time but its task solution fails independent acceptance
- **THEN** that result cannot support adoption, and the failure remains in resource accounting

#### Scenario: The controller itself is the proposed improvement
- **WHEN** a hypothesis changes evaluation or control components
- **THEN** an unchanged evaluator outside the candidate's writable scope assesses the candidate, and it cannot redefine its own acceptance or activate itself in the running controller

### Requirement: Continuous operation preserves recoverable progress

An explicitly started loop SHALL remain available across successive experiments until stopped or constrained by its configured operating policy. It SHALL expose current baseline, active hypothesis, evaluation task, phase, last decision and next action, including idle, waiting-for-input, blocked and recovery conditions. Stop SHALL suspend new work, resolve or explicitly retain in-flight effects, and preserve evidence and useful patches. Restart/resume SHALL reconcile the board, artifact identities and known process outcomes before continuing; it MUST NOT replay a model attempt, board decision or activation whose outcome is uncertain. Resource limits SHALL bound attempts and retained data without imposing an undocumented lifetime limit on the service.

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

Research, candidate implementation and measured task execution SHALL each use the existing dispatch mechanism with an identified actual model/configuration and its own visible titled terminal surface. A transcript or background process alone SHALL NOT satisfy conversation visibility. If the required surface is unavailable, the loop SHALL preserve in-flight state and suspend new model dispatch. Measurements sharing local inference hardware SHALL not overlap with other loop-generated model work unless that concurrency is an explicitly controlled part of both arms. The loop SHALL report external interference that prevents a valid comparison.

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

The `self-improvement-loop` skill and executable support SHALL be delivered through the existing kit installation/update/recovery lifecycle and usable from an owned project outside this checkout. Acceptance SHALL exercise the real configured local model, actual Beads and OpenSpec, installed tool/instruction consumption, an A-on-B comparison followed by a B-on-C transition, independent task checks and stop/resume. Model-free fixtures SHALL verify failure paths but SHALL NOT replace the real installed path or prove model-backed savings. Missing local model access SHALL keep the corresponding acceptance tasks open.

#### Scenario: An external project invokes the installed skill
- **WHEN** a user starts the loop from an owned outside-checkout project with qualified runtime inputs
- **THEN** the installed skill and controller find the intended board and specification root, perform the sequential experiment path, and expose retained decisions without modifying unrelated installed settings
