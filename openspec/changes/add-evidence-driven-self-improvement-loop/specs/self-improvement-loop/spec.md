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

The installed skill SHALL start or resume an explicitly selected loop against an identified project, Beads board, source revision, writable scope, experiment policy and model configuration. The policy SHALL identify permitted researcher and executor routes, resource limits, decision scope and any publication authority. Routine planning, implementation and measurement within that authority SHALL continue autonomously; material scope or operating-condition changes SHALL require a user decision. Missing model details SHALL permit model-free preparation but MUST NOT trigger guessed endpoints, changed models or paid fallbacks.

#### Scenario: Model details arrive later
- **WHEN** the local endpoint or exact serving configuration has not been supplied
- **THEN** the loop can validate its planning and owned inputs, identifies the missing runtime inputs, and starts no dependent model trial

#### Scenario: A hypothesis needs a new external service
- **WHEN** an otherwise promising experiment requires access outside the authorized scope
- **THEN** dependent work waits for that decision while independent authorized work and evidence are preserved

### Requirement: Every implemented hypothesis has a complete planning contract

Before changing any candidate implementation, the loop SHALL satisfy the per-hypothesis OpenSpec prerequisite owned by `orchestration-feedback-loop`. This includes the hypothesis serving as the evaluation task. Both arms SHALL consume the same frozen task contract, including its applicable planning artifacts and independent acceptance. A change to that contract during a comparison SHALL invalidate comparability rather than silently update one arm.

#### Scenario: A small instruction hypothesis has no specification
- **WHEN** an executor is about to implement a one-line instruction change without its complete linked OpenSpec artifacts
- **THEN** dispatch is prevented with the missing planning prerequisite identified

#### Scenario: Task acceptance is revised between arms
- **WHEN** the evaluation task's requirements or checks change after its baseline attempt
- **THEN** the previous attempt cannot be paired with the revised candidate attempt

### Requirement: Real improvement tasks rotate through candidate and workload roles

The loop SHALL support the sequence A evaluated on B, then B evaluated on C, without requiring reciprocal A/B comparisons or a separate synthetic coding suite. It SHALL prepare a frozen candidate A, solve the frozen real task B with baseline H and candidate H+A, independently accept each solution, decide A, and retain exact useful implementations of B. The next baseline SHALL be H+A only after a supported adoption of A; otherwise it SHALL remain H. Before B becomes the next candidate, its exact selected implementation SHALL be integrated and checked against that current baseline, with any changed revision recorded. Implementing B successfully SHALL NOT establish B's own benefit or close its hypothesis prematurely.

#### Scenario: A is adopted and B is next
- **WHEN** A's declared acceptance passes and both attempts at B are retained
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
- **WHEN** an independently supported benefit decision authorizes integration within the run's scope
- **THEN** only the evaluated candidate changes enter the mainline after combined-tree checks, with any changed base or conflict resolution requiring revalidation of affected benefit evidence before adoption

### Requirement: Correctness and benefit remain independent decisions

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
- **THEN** resume verifies the exact decision and installed experimental identity, completes or repairs that transition without a new trial or duplicate decision, and reports the recovered phase

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
