# Spec Delta

## Purpose

Turn lessons from accepted executor work into bounded, role-specific self-reflection and verified improvements, preserving the real lead's authority, the user's main objective and reliable nonrecursive operation.

## ADDED Requirements

### Requirement: Explicit activation with a bounded mandate

Installed activation SHALL enable the two-stage reflection chain for the current lead and authorized work. It SHALL cover relevant project and kit improvements without authorizing unrelated work, new providers, weakened acceptance or destructive actions. Repeated activation SHALL reuse the same lifecycle binding. No persistent observer model SHALL be launched.

#### Scenario: A user enables improvement
- **WHEN** the user enables the feature in a supported lead session
- **THEN** the installed interface prepares event access, scope and recovery without requiring manual endpoints or task identifiers, and ordinary work continues

#### Scenario: Duplicate activation
- **WHEN** another activation addresses the same work
- **THEN** it identifies the existing owner and launches no duplicate consumer or reflection

#### Scenario: Mode is disabled
- **WHEN** ordinary development proceeds without activation
- **THEN** no automatic reflection runs and ordinary lead, executor and feedback behavior remains available

### Requirement: Accepted ordinary work is the automatic trigger

The system SHALL start executor reflection only after the lead accepts and closes a bound ordinary executor task. Acceptance SHALL identify the assignment, source sessions and accepted result. First reports, intermediate corrections, cancellations, arbitrary closed cards and elapsed time SHALL NOT qualify. The interface SHALL maintain routine binding metadata.

#### Scenario: The lead accepts the corrected result
- **WHEN** the lead records acceptance of the bound executor work and closes its task
- **THEN** one reflection becomes eligible with the original assignment, correction history and accepted result references

#### Scenario: The executor submits an initial report
- **WHEN** the executor finishes a turn or submits work awaiting lead review
- **THEN** reflection does not start until the work is accepted and its task closed

#### Scenario: A task is closed for another reason
- **WHEN** a cancelled, rejected, duplicate or unrelated task closes without applicable acceptance
- **THEN** no reflection is scheduled and the closure is not represented as verified success

### Requirement: Durable board events preserve trigger identity

Event consumption SHALL be model-free and scoped to the enabled board source and activation boundary. The system SHALL preserve source identity, checkpoint and accepted-work identity across restart, expose gaps and reconcile uncertain effects before retry. Duplicate events SHALL NOT duplicate reflection. Existing board data and unrelated journal consumers SHALL be preserved.

#### Scenario: Events are replayed after restart
- **WHEN** a closure already associated with retained reflection work is read again
- **THEN** the existing work is reconciled without another model launch

#### Scenario: The same acceptance is closed again
- **WHEN** a task is reopened and closed without a new accepted assignment/result
- **THEN** the prior reflection remains authoritative; a genuinely new accepted work identity can qualify separately

#### Scenario: Journal coverage is unavailable
- **WHEN** events were disabled, pruned, imported by sync or read against a changed source identity
- **THEN** the system reports the boundary, preserves pending work and uses supported scoped reconciliation rather than silently skipping or launching historical closed tasks

#### Scenario: The feature prepares and stops event access
- **WHEN** activation owns a necessary journal setting and the feature later stops
- **THEN** setup and recovery are handled by the interface while preserving preexisting settings, another consumer's use and retained records

### Requirement: Executor self-reflection has its own skill and scope

The installed `executor-self-reflection` skill SHALL analyze a native fork of the accepted executor conversation. It SHALL examine assignment clarity, avoidable executor mistakes, wasted actions, lead corrections and final checks, distinguishing observed facts from hypotheses. It SHALL return retained recommendations or no finding and SHALL NOT implement changes, mutate boards or dispatch agents.

#### Scenario: Missing input caused avoidable rework
- **WHEN** the accepted work includes a correction caused by an absent or contradictory assignment input
- **THEN** executor reflection identifies the concrete input and proposed earlier provision without treating the whole task as an executor failure

#### Scenario: The executor missed an existing requirement
- **WHEN** the original assignment and checks establish the requirement was already clear
- **THEN** reflection considers its own implementation or verification practice rather than automatically blaming the lead

#### Scenario: No useful improvement is found
- **WHEN** analysis finds no attributable remedy worth pursuing
- **THEN** a valid no-finding outcome is retained and the executor reflection completes without invented recommendations or retries for a better answer

### Requirement: Lead self-reflection synthesizes both perspectives

A retained executor-reflection outcome SHALL automatically make the corresponding lead reflection eligible. The installed `lead-self-reflection` skill SHALL run in a separate native fork of the lead with the executor outcome as context. It SHALL examine framing, delegation, corrections and lead-side work independently, then return recommendations to the real lead without creating tasks or directing implementations.

#### Scenario: A correction implicated both roles
- **WHEN** executor reflection and the lead conversation describe different causes
- **THEN** the lead fork checks the original requirements, actual feedback and results, preserves supported disagreement and synthesizes the smallest appropriate remedy

#### Scenario: Lead criticism introduced a new requirement
- **WHEN** a correction changes the accepted task rather than identifying an existing defect
- **THEN** the synthesis does not classify the change as a preventable executor mistake

#### Scenario: Several executor outcomes are already pending
- **WHEN** one lead reflection can consume multiple retained outcomes for the same bound work
- **THEN** they may share one synthesis with source attribution, without waiting for speculative future results or replaying previously consumed outcomes

### Requirement: Forks have bounded reflection authority

Before model work, each fork SHALL have the intended reflection role, declared model binding and read-only analysis capabilities. Inherited source goals SHALL NOT continue automatically. Forking SHALL preserve the source session and work. The system SHALL NOT decode opaque reasoning or claim that a fork guarantees complete historical input or free context reuse.

#### Scenario: The source has an active goal and write tools
- **WHEN** a reflection fork is prepared
- **THEN** it cannot continue the source implementation, mutate code or boards, dispatch agents or alter its own guidance, and the original conversation remains unaffected

#### Scenario: A source context was compacted or is unavailable
- **WHEN** relevant evidence is absent from available fork context
- **THEN** the reflection uses bounded authorized visible-source references or reports the missing coverage rather than reconstructing opaque state or asserting a complete review

#### Scenario: Effective routing differs from the declaration
- **WHEN** fork model, provider, effort or role context does not match the bound configuration
- **THEN** dependent model work refuses with the cause and remedy instead of substituting another model or billing route

### Requirement: Evidence remains attributable to accepted work

Reflection SHALL relate the original assignment and requirements to submitted artifacts, actual lead feedback, corrections and checks. Available evidence SHALL retain source/session and revision identity. Relevant technical detail SHALL be retrieved for a concrete question, with missing links explicit. Current contents of a reused worktree SHALL NOT stand in for reviewed work.

#### Scenario: The worktree was reused
- **WHEN** the current checkout differs from the accepted result
- **THEN** analysis uses the retained accepted revision or patch and source references, or reports missing evidence

#### Scenario: An operation's cause remains unclear
- **WHEN** visible discussion is insufficient to distinguish a tool failure from user or executor error
- **THEN** the reflection retrieves only relevant authorized operation/check detail and preserves uncertainty if it remains unavailable

#### Scenario: Acceptance metadata claims success
- **WHEN** the card is closed but a finding depends on an unverified runtime result
- **THEN** closure establishes trigger eligibility only and does not substitute for actual evidence of that runtime result

### Requirement: Session purpose prevents recursive improvement

The system SHALL distinguish ordinary work, reflection and improvement implementation through owned task/session lineage metadata. Only accepted ordinary executor work SHALL trigger automatic reflection. Improvement descendants SHALL remain excluded across subdivision, retry, resume and rename. Normal review and acceptance SHALL still apply to improvements.

#### Scenario: An improvement executor completes
- **WHEN** the real lead accepts and closes an improvement implementation
- **THEN** no automatic executor or lead reflection starts for that work

#### Scenario: Improvement work is split or resumed
- **WHEN** an improvement creates implementation subtasks or successor sessions
- **THEN** the nontriggering purpose is inherited and cannot be lost by a new title, slot or session

#### Scenario: Purpose metadata is contradictory
- **WHEN** a closure cannot be associated with an unambiguous eligible lineage
- **THEN** automatic launch refuses with a concrete cause instead of assuming ordinary work

### Requirement: Reflection is visible and shares capacity

Every reflection conversation SHALL have a distinct titled visible surface before model work, identifying its assignment, role and actual profile/model/effort. Each active stage SHALL use existing shared admission without an implementation worktree or permanent slot reservation. Waiting SHALL preserve inputs and SHALL NOT preempt healthy executors or block the main task.

#### Scenario: Capacity is occupied
- **WHEN** a reflection stage cannot be admitted
- **THEN** it remains pending with its cause visible while ordinary work continues and no hidden model call is made

#### Scenario: A reflection surface closes
- **WHEN** the required view is lost
- **THEN** new model work stops until the owning lifecycle restores visibility and reconciles in-flight work, without screenshots or desktop rearrangement

### Requirement: Reflection has finite cumulative costs

Installed defaults SHALL bound each reflection's turns, investigation and supplied context, and cumulative reflection work for the authorized task. Both stages, source context, unsuccessful work, no-finding outcomes and retries SHALL be accounted for. Recorded usage and estimates SHALL be distinguished. Resume or a new fork SHALL NOT reset accumulated costs.

#### Scenario: A job reaches its allowance
- **WHEN** further investigation cannot be admitted
- **THEN** a bounded inconclusive outcome or explicit budget pause preserves evidence and identifies the missing fact and continuation choice

#### Scenario: Usage is unavailable or exceeds reservation
- **WHEN** the native response has incomplete usage or a measured overrun
- **THEN** status exposes that limitation, enforces remaining admission limits and makes no exact token-cap or subscription-saving claim

#### Scenario: Nothing newly eligible has completed
- **WHEN** time passes without a new accepted ordinary task or retained stage result
- **THEN** no reflection, transcript reread or worker status request is triggered

### Requirement: Durable outcomes advance the chain safely

Each stage SHALL retain its native identity, outcome, coverage and usage before downstream effects. Executor outcome retention SHALL precede lead reflection; lead synthesis SHALL reach the exact real lead through verified delivery. Submission, queuing, completion and consumption SHALL remain distinct. Unknown outcomes SHALL be reconciled before retry.

#### Scenario: A worker prints a final-looking report
- **WHEN** native completion and durable outcome have not been established
- **THEN** the next reflection stage is not started merely because the text resembles a final result

#### Scenario: Delivery is interrupted
- **WHEN** a synthesis notification may have been applied but has no conclusive receipt
- **THEN** recovery reconciles that same delivery and does not blindly resend or create another synthesis

#### Scenario: The ordinary lead lacks a supported return path
- **WHEN** activation cannot establish automatic delivery for the actual current lead
- **THEN** it reports the missing capability and supported preservation/remedy before dependent launch, and readiness remains unproven until the session owner supplies the path

### Requirement: Findings identify a useful owning remedy

Recommendations SHALL distinguish facts, hypotheses and predictions, identify the likely cause, smallest remedy, appropriate owner, credible next use and checkable result. They SHALL consider subtraction and input/interface/check fixes before expanding guidance. One attributable defect can justify a bounded correction; repeated signals, votes and a quota of findings SHALL NOT be required.

#### Scenario: A clear tool defect can be repaired
- **WHEN** one attributable failure has an owned bounded remedy and relevant check
- **THEN** the recommendation can be considered immediately without waiting for another occurrence

#### Scenario: An incident does not justify a global rule
- **WHEN** the evidence lacks an attributable cause or credible future use
- **THEN** the result remains a bounded hypothesis or no finding rather than an expanding instruction

#### Scenario: A reusable executor practice is proposed
- **WHEN** evidence supports changing implementation judgment or self-verification
- **THEN** the recommendation names the behavior and representative check and preserves applicable skill/agent evaluation

### Requirement: The real lead owns tasks and decisions

Only the real lead SHALL admit, reject or defer recommendations and create/update improvement tasks through existing beads/feedback owners. It SHALL deduplicate relevant prior items, preserve source identity and avoid synthetic votes. Reflection runtime records SHALL NOT become a second task tracker. Board failures SHALL preserve the result and expose applied and pending effects.

#### Scenario: A synthesis is useful
- **WHEN** the real lead receives an actionable recommendation
- **THEN** it decides at the next safe boundary and creates or updates the appropriate bounded task, with purpose, verification and assignment readiness

#### Scenario: A known or rejected idea returns
- **WHEN** a recommendation concerns an existing item
- **THEN** the lead retains new attributable evidence on that item and reopens a rejection only for new evidence or changed conditions

### Requirement: Improvements receive proportionate execution opportunities

A ready improvement SHALL receive capacity or a concrete higher-priority blocker and return condition. Reflection and improvements SHALL NOT routinely gate ordinary acceptance or delivery. The responsible lead MAY hold an affected release for a concrete incident risk, with a reason and checkable release condition. One ordinary improvement SHALL run at a time within existing capacity; urgent correctness/integrity work retains existing handling. Trivial corrections MAY be completed directly.

#### Scenario: Suitable capacity becomes available
- **WHEN** a ready improvement has no higher-priority conflict
- **THEN** the lead assigns it through the ordinary committed-base, implementation and acceptance path, marked as improvement work

#### Scenario: Main work has higher priority
- **WHEN** an optimization would delay more consequential delivery
- **THEN** the lead records the concrete trade-off and return condition rather than silently dropping the item or reserving idle capacity

### Requirement: Improvements preserve project and publication boundaries

Private evidence SHALL remain with its owner. Shared-kit changes SHALL use a kit-owned task, public-safe inputs and normal specification, skill-evaluation, installation and consumption checks. Activation SHALL NOT change material outcomes, operating conditions, protected external workflows or authority without the applicable user decision.

#### Scenario: A private task reveals a shared defect
- **WHEN** the remedy belongs to the reusable kit
- **THEN** the kit assignment uses sanitized reproduction and acceptance while private provenance and identities remain outside shared source and output

#### Scenario: A remedy would narrow acceptance
- **WHEN** a recommendation changes the promised outcome or needs new authority
- **THEN** the real lead obtains the relevant decision before dependent implementation

### Requirement: Guidance changes are actually consumed

Implementation, integration, publication and consumption SHALL remain distinguishable. Skills and role guidance SHALL use their existing authoring/evaluation and installation owners. A published file SHALL NOT prove that an active session reloaded it. New and continuing sessions SHALL retain honest effective revision and recovery information.

#### Scenario: Guidance is published during ongoing work
- **WHEN** a checked skill or role update is installed
- **THEN** an intended new session is verified to consume it and any needed active-session refresh uses the existing safe-boundary lifecycle

#### Scenario: The updated skill has not been loaded
- **WHEN** installation passes but intended consumption is unobserved
- **THEN** that stage remains unverified rather than being reported as improved agent behavior

### Requirement: Improvement claims retain complete outcome evidence

Correctness SHALL be verified through the real affected entry point. Benefit SHALL include both reflections, source context, coordination, implementation, checks, rework and displaced delivery, with time and tokens assessed separately. Agent behavior claims SHALL retain real-agent/skill evaluation. Supported, adverse and inconclusive outcomes SHALL remain distinct.

#### Scenario: A deterministic defect is fixed
- **WHEN** its original failure and relevant counterexamples can be exercised directly
- **THEN** proportionate native acceptance suffices for the fix without imposing unrelated whole-task experiments

#### Scenario: Messages decrease but quality worsens
- **WHEN** a guidance change reduces messages while missing a requirement
- **THEN** it is rejected as a demonstrated improvement and prior accepted guidance remains recoverable

#### Scenario: Post-acceptance reflection is evaluated
- **WHEN** comparison measures the proposed automatic chain
- **THEN** the declared horizon includes relevant work after the trigger, counts complete seed/reflection/improvement costs and does not credit reflection for the source task's earlier success

#### Scenario: The experiment is not qualified
- **WHEN** baseline evidence, frozen identities, the benefit mechanism and relevant use to examine, quality acceptance and horizon, complete cost coverage or stopping conditions are missing
- **THEN** candidate model dispatch, evaluation and adoption are held; planning validity or working transport cannot substitute

#### Scenario: Isolated implementation proceeds during qualification
- **WHEN** model-free work is independent of qualification
- **THEN** authoring, builds and native checks can proceed with the candidate inactive and outside accepted delivery, preserving all product and comparison acceptance

#### Scenario: The matched benefit gate fails
- **WHEN** the complete comparison rejects the treatment or remains inconclusive
- **THEN** the candidate stays outside accepted source/installation, baseline behavior is restored and verified, and inactive patches, all costs and adverse evidence are retained without favorable-result retries

### Requirement: Recovery preserves ownership and partial work

Stop, interruption, lead succession and recovery SHALL preserve outcomes, task references and executor work without duplicate reflection, delivery or assignments. Lead shutdown SHALL prevent orphaned launches. Reactivation SHALL verify scope, purpose, source boundaries, actual model and effective guidance before continuation.

#### Scenario: Stop occurs between reflection stages
- **WHEN** executor reflection is retained but lead reflection has not finished
- **THEN** recovery resumes from the retained stage identity without rerunning accepted executor work or duplicating its reflection

#### Scenario: The lead is replaced
- **WHEN** a supported successor takes ownership
- **THEN** pending reflections/results transfer through the existing lifecycle without two active owners

#### Scenario: The user disables the feature
- **WHEN** stopping occurs during reflection or implementation
- **THEN** new reflection and improvement assignments stop while in-flight effects and partial implementation follow the user's stop scope and existing recovery

### Requirement: Installed use proves the complete loop

Global delivery SHALL include both skills, purpose metadata, event trigger, native fork chain, real-lead decisions and recovery while preserving unrelated settings. Acceptance SHALL exercise real installed roles outside the kit checkout from accepted ordinary work through an applied and consumed improvement. Fixtures, reports and planning labels alone SHALL NOT prove delivery or benefit.

#### Scenario: A consumer completes an improvement
- **WHEN** an enabled installed lead accepts ordinary executor work
- **THEN** actual closure causes executor reflection, then lead synthesis, verified return, a real lead decision and accepted improvement consumption, with unchanged original task acceptance

#### Scenario: Improvement completion does not recurse
- **WHEN** that improvement and its implementation descendants complete
- **THEN** the installed event path produces no further automatic reflections while normal acceptance remains enforced

#### Scenario: Update or activation fails
- **WHEN** an owned installation or activation cannot complete
- **THEN** prior accepted behavior can be restored without erasing boards, retained results, unrelated settings or unfinished executor work

### Requirement: Adoption preserves quality and lowers the cost of future work

Adoption SHALL require quality no worse than the control, no P0/P1 regression and a grounded lead judgment that lasting benefit justifies full costs and risks. The objective SHALL be cheaper future changes and maintenance with timely useful delivery. Benefits SHALL NOT be limited to a checklist or mandatory savings. One unit's success SHALL NOT offset another's quality regression; decision-changing uncertainty SHALL remain inconclusive.

#### Scenario: The control already accepts every task
- **WHEN** both arms independently satisfy every task and neither has a quality regression within the frozen horizon
- **THEN** equal success satisfies the quality condition, while adoption still requires a concrete justified benefit and all other applicable acceptance checks

#### Scenario: Maintenance improves without immediate savings
- **WHEN** real use and checks support easier future changes or maintenance while time or token use does not decrease
- **THEN** the lead can accept the change with its concrete benefit, full costs, trade-offs and forecast limits recorded in the existing decision owner, without a numeric savings prerequisite, score or assertion of confidence substituting for grounded judgment

#### Scenario: An improvement adds more burden than value
- **WHEN** added machinery, context burden, operating friction or delayed useful delivery outweighs the supported benefit
- **THEN** the lead rejects or simplifies the change despite passing functional checks, retaining evidence and restoring accepted behavior on rejection

#### Scenario: Improvements proceed alongside ordinary delivery
- **WHEN** reflection and improvement implementation have available capacity and no blocking dependency on ordinary work
- **THEN** they can proceed in parallel without routinely gating ordinary acceptance or delivery; accounting retains all participant costs, counts overlapping elapsed spans once and distinguishes actual interference from background duration

#### Scenario: Reflection exposes a release incident risk
- **WHEN** reflection identifies a concrete potential incident relevant to a pending release
- **THEN** the responsible lead assesses impact, likelihood, exposure, mitigation and delay cost and can hold the affected release with a recorded reason and checkable release condition, without requiring the incident to occur or giving the reflection authority to impose or lift the hold
