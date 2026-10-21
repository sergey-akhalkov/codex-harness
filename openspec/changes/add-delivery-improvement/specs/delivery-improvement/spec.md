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

### Requirement: Configuration permission gates new reflection model work

The installed configuration SHALL define `self_reflection_enabled`, a boolean permission that defaults to true. True SHALL permit only an explicitly activated lifecycle and SHALL NOT activate a project. False SHALL prevent new executor and lead reflection model calls while ordinary lead, executor, acceptance and feedback work remains available. Each new stage SHALL re-read the effective value. Retained work, pending identities, usage, recovery and event boundaries SHALL be preserved.

#### Scenario: The permission is omitted
- **WHEN** an existing or new configuration omits `self_reflection_enabled`
- **THEN** it defaults to true and preserves the current explicitly activated behavior without activating a project by itself

#### Scenario: Model work is disabled
- **WHEN** `self_reflection_enabled` is false
- **THEN** no new executor or lead reflection model call starts from manual, automatic, continuation or internal dispatch, enable refuses before lifecycle mutation, and ordinary acceptance still completes

#### Scenario: The flag changes between stages
- **WHEN** a host has retained an executor reflection and the flag changes from true to false before the lead stage
- **THEN** the executor result, identity, usage and pending lead stage remain retained, no lead model call starts, and a later true value resumes the retained lead stage without repeating the executor

#### Scenario: Status explains stopped model work
- **WHEN** the caller reads status while the flag is false or the lifecycle is not active
- **THEN** the result names the effective flag, configuration path and concrete reason no new model work may start

#### Scenario: The permission has an invalid type
- **WHEN** the configuration supplies a string, integer or other non-boolean value
- **THEN** configuration loading fails before model work or lifecycle mutation

### Requirement: Acceptance snapshots the reflection permission

Ordinary acceptance SHALL remain successful while permission is false and SHALL snapshot it in the acceptance binding and public receipt. Without `--source`, it SHALL read installed kit configuration while retaining the project as pool provenance. The receipt SHALL report `selfReflectionEnabled: false` and `automaticTrigger: false`. Such a closure SHALL remain ineligible after a later true value without a disabled-interval poll. Retained pre-disable work SHALL not be duplicated or swept.

#### Scenario: An ordinary closure is accepted while model work is disabled
- **WHEN** an ordinary executor result is accepted while `self_reflection_enabled` is false
- **THEN** acceptance succeeds with its normal purpose, provenance, results and usage, the public receipt reports the permission as false with `automaticTrigger` false, and no reflection model call starts

#### Scenario: Acceptance omits an explicit source
- **WHEN** a consumer without its own orchestration configuration runs ordinary acceptance without `--source` and the installed kit records the kit source
- **THEN** acceptance reads that installed permission, preserves the consumer project in pool provenance, succeeds, and reports the permission snapshot

#### Scenario: The flag returns before a disabled-period closure is observed
- **WHEN** an ordinary closure was accepted while `self_reflection_enabled` was false and the flag becomes true before its first event poll
- **THEN** the existing event evaluator treats it as its ignored result, advances the normal checkpoint, creates no reflection record or model call, and does not sweep retained pre-disable work

#### Scenario: Closures occur while disabled
- **WHEN** ordinary accepted closures occur while the flag is false, whether observed during the disabled interval or accepted under false and re-enabled before their first poll
- **THEN** no reflection record or model call is created for those closures and retained pre-disable work is not duplicated or swept after re-enable

### Requirement: Acceptance permission metadata remains compatible

Default-true acceptance permission metadata SHALL omit the field on serialization, and legacy metadata without it SHALL deserialize as true. A false permission SHALL remain explicit in board metadata and the receipt. The event evaluator SHALL treat a false-at-acceptance closure as its existing ignored result, not a new refusal code or persisted-state variant.

#### Scenario: Legacy default-true metadata is read
- **WHEN** an acceptance binding serializes with permission true, or retained metadata omits the field
- **THEN** the true binding omits the field and the omitted legacy value deserializes as true

#### Scenario: A false-at-acceptance closure is observed after re-enable
- **WHEN** the flag returns to true before the first poll of a closure accepted while false
- **THEN** the closure is ignored at the existing event boundary with no record, model call, new refusal code or persisted-state variant

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

Before model work, each fork SHALL have the intended reflection role and read-only analysis capabilities, with inherited source goals suppressed. Executor reflection SHALL use the configured executor profile (`ds` in the shipped kit) and lead reflection SHALL use the lead profile. Forking SHALL preserve the source session and work and SHALL NOT claim complete historical input or free context reuse.

#### Scenario: The source has an active goal and write tools
- **WHEN** a reflection fork is prepared
- **THEN** it cannot continue the source implementation, mutate code or boards, dispatch agents or alter its own guidance, and the original conversation remains unaffected

#### Scenario: A source context was compacted or is unavailable
- **WHEN** relevant evidence is absent from available fork context
- **THEN** the reflection uses bounded authorized visible-source references or reports the missing coverage rather than reconstructing opaque state or asserting a complete review

#### Scenario: Effective routing differs from the declaration
- **WHEN** executor reflection does not resolve the configured executor profile, lead reflection does not resolve the lead profile, or model, provider, effort or role context differs from the bound configuration
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

### Requirement: Cost and benefit claims remain honest

Reflection and improvement work SHALL retain available stage identity, usage, coverage and cost evidence. Recorded usage SHALL remain distinct from estimates or reservations, missing data SHALL remain unknown, and an unmeasured run SHALL NOT be reported as evidence of savings. An optional later evaluation MAY assess whether recommendations improve subsequent work; it SHALL NOT be a release prerequisite or proof of savings.

#### Scenario: A completed stage records its costs
- **WHEN** a reflection stage retains a completed result
- **THEN** its available source identity, usage and limitations remain with the result; historical source-session usage is not counted again as new reflection usage, while actual new fork input, including inherited context, remains charged as reported by the native runtime

#### Scenario: Usage is missing or an allowance is exceeded
- **WHEN** native usage is unavailable or the measured work exceeds its admission limit
- **THEN** status preserves the unknown or overrun, enforces remaining admission, and makes no exact savings claim

#### Scenario: A later evaluation is optional
- **WHEN** meaningful outcomes are assessed after the real chain
- **THEN** the evaluation may inform future work but its absence or uncertainty neither blocks release nor establishes benefit

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

### Requirement: Real sequential chain gates release

Harness release SHALL require one real sequential installed chain: an accepted ordinary task's executor reflection completes, its retained result is supplied to lead reflection, and that synthesis is consumed by the current live lead. Applicable current regression checks and global installation verification SHALL pass. A failed attempt, fixture, plan or model-free check SHALL NOT substitute for this evidence.

#### Scenario: The real chain completes
- **WHEN** an accepted ordinary task closes through the installed event path
- **THEN** executor reflection completes under its configured role and `ds` executor route, lead reflection receives the retained executor result under its distinct lead role, and the current live lead consumes the synthesis

#### Scenario: The executor result precedes lead reflection
- **WHEN** executor reflection has not produced a retained result
- **THEN** lead reflection does not start, and a printed or final-looking message alone is not treated as the retained result

#### Scenario: The prior attempt failed
- **WHEN** an earlier real executor-reflection attempt did not complete successfully
- **THEN** release remains pending and recovery or another real accepted chain must produce the required sequential evidence without relabeling the failure as success

#### Scenario: Model-free checks pass without the real chain
- **WHEN** fixtures and native tests pass but no real sequential chain has been observed
- **THEN** mechanical scope may be recorded as verified while release acceptance remains incomplete

#### Scenario: Global installation is verified
- **WHEN** the installed kit is exercised outside the source checkout
- **THEN** both skills, distinct roles, `ds` executor routing, lead routing, visible surfaces, switch, delivery and recovery remain available without erasing unrelated boards, settings or retained work

### Requirement: The live lead owns recommendation disposition

After receiving synthesis, the current live lead SHALL record whether any recommendation warrants normal follow-up. No finding, standard improvement-executor assignment, implemented improvement or comparative benefit threshold SHALL be required for release. Reflection SHALL NOT be a routine release gate; only a concrete incident risk may justify a lead-owned hold with a reason and checkable condition.

#### Scenario: No finding is returned
- **WHEN** synthesis reports no attributable recommendation worth pursuing
- **THEN** the live lead records that disposition and release is not delayed for a manufactured finding or implementation

#### Scenario: A recommendation is actionable
- **WHEN** synthesis returns an attributable recommendation with an owned remedy
- **THEN** the live lead may assign a standard executor under the existing improvement purpose, defer it, reject it or decide no action is warranted, and release does not require the chosen follow-up to be completed

#### Scenario: A concrete incident risk is found
- **WHEN** synthesis exposes a concrete potential incident in a pending release
- **THEN** the responsible lead may hold the affected release with a recorded reason and checkable condition; reflection cannot impose or lift the hold by itself
