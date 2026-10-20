# Design

## Context

See [proposal.md](proposal.md). The role separation and shared worktree pool changes are delivered prerequisites; verify their current installed contracts rather than reimplementing them. The unaccepted persistent observer is withdrawn. Its continuous feed, long-lived role, batching/backoff and lifecycle adapter are not part of the replacement.

Reusable owners are `executor_control.rs`, `executor_observation.rs`, `executor_message.rs`, native run receipts and terminal hosts; `role_context.rs` and `global/roles.toml`; `task_worktree.rs` and the existing admission owner; `board_lifecycle.rs`, `board_feedback.rs` and `feedback_cli.rs`; installation and token/outcome evaluation. Preserve the accepted behavior of those owners and adapt only the missing contracts.

Local contract inspection established native Codex 0.163.0 fork, model/profile and sandbox overrides, and a fork schema with source turn boundaries and goal-continuation control. This is interface evidence, not proof of safe fork execution or cross-provider context compatibility. Beads 1.3.0 exposes `events tail --since N --follow`, ordered committed mutations and explicit truncation errors. The journal is optional, local to a replica and branch, and does not cover raw SQL mutations or changes imported by sync. Setup and recovery must account for these boundaries.

## Goals / Non-Goals

**Goals:** learn from accepted work and actual corrections; give each role a focused retrospective; return actionable recommendations to the real lead; apply useful changes without displacing the user's objective; prove installed operation and net benefit.

**Non-Goals:** a persistent model observer, a timer-driven reflection loop, automated prioritization, automatic prompt publication, recursive reflection, a new tracker or experiment framework, or guaranteed findings/savings.

## Decisions

### 1. Accepted board closure is the trigger

An explicitly enabled binding covers the current lead and authorized work. The normal lead acceptance path records a small acceptance reference on the existing task, binding its assignment/run identity, accepted revision or artifacts and relevant source sessions. The lead closes that task through ordinary beads operations. The interface owns routine metadata; users do not reconstruct IDs or separately request reflection.

A model-free consumer of the board's native event journal treats that closure as eligible only when it belongs to ordinary work, has the applicable lead acceptance and exact task/run binding, and has not already been consumed. An executor report or `lead_review` is not acceptance. Cancelled, duplicate, rejected, unrelated and improvement tasks do not qualify. An imported closure or a gap in events cannot be assumed to represent a new accepted local task.

The consumer belongs to the existing enabled lead/run lifecycle, not a separate agent or globally orphaned daemon. Enabling establishes journal availability and the current activation boundary before watching new closures; no retrospective sweep of every historical task. It reuses an already enabled journal and records any owned configuration change for conservative restoration. Stopping does not disable journal use owned by another consumer.

Persist the checkpoint together with source replica/branch identity and the exact acceptance identity in existing private lifecycle state. A sequence number alone is not portable across replicas. Replayed events reconcile retained work; reopening/reclosing the same acceptance does not run again, while a genuinely new accepted assignment revision can qualify. A truncation, sync, source change or unknown external effect produces an explicit recovery state; re-read only bound task records, do not silently skip or relaunch all closed cards. Native event collection makes no model calls.

### 2. Two short-lived role-specific forks

After admission, fork the accepted executor conversation at a stable completed boundary and invoke `executor-self-reflection`. It examines the original assignment, work, lead corrections and final checks: ambiguities/missing inputs, its own avoidable mistakes and unnecessary work, and concrete changes that could prevent them. The existing assignment and acceptance retain all relevant executor sessions if correction used a successor. Do not substitute a current reused checkout for the reviewed artifacts.

Retain the result, source identity, coverage and usage before releasing the fork. A completed retained executor reflection, including a valid no-finding result, makes a fork of the corresponding lead conversation eligible. Invoke `lead-self-reflection` with that lead context and the executor result. It independently assesses framing, delegation, review demands, changed requirements, executor mistakes, infrastructure and lead-side overhead. Criticism from either role is evidence to test, not truth. The lead fork synthesizes bounded recommendations and disagreement/uncertainty.

Each stage is one finite analysis job; it can use bounded reads for a stated question, then completes. This is not another implementation lead. Read-only capabilities must cover tools as well as filesystem permissions: no board mutation, agent dispatch, source writes or self-publication. Override inherited operational role/goal behavior through the native owner before model work and verify it actually took effect. Do not decode opaque reasoning or compaction state. Native fork context is the source's available context, not a promise that every historical byte survived compaction. Supply source references and explicit gaps; retrieve relevant visible messages, original instructions/assignment, revisions and checks when needed. Missing evidence yields uncertainty, not a fabricated complete review.

Keep the lead and executor skills separate because their responsibilities and optimized guidance differ. Reuse shared launch, accounting, result retention and transport; do not create two duplicated frameworks or a third observer prompt. Resolve each stage's declared role-specific model/profile/effort explicitly through the existing configuration owner and verify actual binding before request. No silent model/provider/billing substitution, inherited executor binding for a lead fork, or claim that forking makes input free.

### 3. Durable result delivery advances the chain

The executor result is durable input to lead reflection, not a direct improvement assignment. The controller records each stage's native identity and result acknowledgment, and never equates process exit, a final-looking message or elapsed silence with a verified result. Unknown outcomes reconcile against native state and retained artifacts before retry. Existing completion events and receipts drive advancement.

Lead reflection returns a compact result to the exact real lead/task. Delivery must be observed or honestly retained as pending/unknown; queued or submitted does not mean consumed. Use the existing addressed channel and safe-boundary processing. The current ordinary-main-session capability gap must be closed through the supported session owner before global readiness, with preserved conversation/work, no second lead and no hidden manual polling ritual. If the supported native route is unavailable, name the missing capability and verified remedy before a dependent launch.

Several already pending executor results for one lead can share one lead reflection at the next available admission boundary. Do not add a timer to wait for speculative future results or delay a ready single result. Each source acceptance/result is associated with exactly one retained synthesis outcome; later results remain pending. Main work never waits for either reflection.

### 4. Purpose prevents recursive improvement

Own purpose metadata at task/dispatch creation: ordinary work, reflection, or improvement implementation. Bind it to actual task/session lineage; labels and titles are helpful display, not the only enforcement. Improvement implementation and its descendants inherit the nontriggering purpose even when split, retried, resumed, renamed or closed normally. Reflection sessions never trigger reflection. Unknown or contradictory purpose refuses automatic launch with a precise status.

The real lead creates any accepted improvement task with that purpose. The lead may explicitly request a bounded review under existing authority, but automation does not relabel it as ordinary work or recursively create successors. Normal code review, acceptance, defect correction and installation checks still apply to improvement implementations.

### 5. Capacity, cost and main-work priority

Use existing shared admission for each active reflection stage. A waiting event/result owns no implementation worktree or permanently reserved model slot. Every model conversation has its own titled visible surface before its first request, showing actual role/profile/model/effort and live activity. Loss of visibility suspends new model work and invokes the existing owning recovery path.

Healthy executors are not preempted. Pending reflection waits for ordinary available capacity, without delaying launch of more consequential user work. Keep one ordinary improvement implementation in flight within existing capacity; this is neither a fixed project limit nor a reserved lane. Current correctness/integrity blockers retain ordinary urgent handling.

Reflection and improvement implementation may proceed in parallel with ordinary
delivery when resources and dependencies allow. Neither is a routine gate on
ordinary task acceptance or release. The responsible lead can hold an affected
release for a concrete potential incident, considering impact, likelihood,
exposure, mitigation and delay cost without waiting for the incident to occur.
The existing task retains the reason and checkable release condition. Reflection
supplies evidence; it cannot impose or lift the hold. Background duration alone
is not displaced delivery;
record actual contention, lead distraction or integration conflicts, and count
overlapping wall-clock spans once while retaining all participant costs.

Install finite per-job investigation/turn/context limits and cumulative allowances per authorized work binding. Include source fork context, both reflection stages, failed/no-finding runs, reads and retries; expose recorded usage separately from reservations or estimates. Arrival, resume, a new fork or repeated closure cannot reset cost accounting. Overrun or unknown usage is visible; no further overspend is silently admitted. No finding is a normal completed outcome, not a reason to retry or manufacture work.

### 6. The real lead owns decisions and implementation

Synthesis returns the observation, source basis, likely cause, smallest remedy, appropriate owner, credible next use, expected avoided work and a proportionate check. Prefer removing work or fixing an input/interface/check over adding instructions. Skill or role guidance is appropriate where judgment or a reusable procedure needs it; do not learn a new rule from an unsupported accusation.

Only the real lead creates/updates beads tasks, deduplicates against relevant prior items, accepts, rejects or defers, and dispatches implementations. One clear attributable defect is enough for a bounded correction; no votes, repeated-signal waiting or full experiment ceremony is imposed on a deterministic fix. Uncertain hypotheses retain existing evaluation routes. A derived reflection is not another independent evidence vote.

At the next safe boundary the lead decides. A ready accepted improvement gets a suitable allocation opportunity or a concrete competing priority/blocker and observable return condition. Rejected unchanged recommendations are not periodically resent. The lead may complete a trivial cheaper-than-handoff correction directly. Routine operations and adoption use existing board owners, not a second recommendation tracker.

### 7. Delivery, publication and recovery retain their owners

Project-specific evidence stays local. Shared remedies use a kit-owned task and public-safe synthetic reproduction; private provenance remains with the consumer. Required OpenSpec, skill-creator/skill-evolution evaluation, installation and actual new-session consumption remain necessary. Reflection cannot change its own guidance, acceptance or protected external workflows.

A stop prevents new reflection and improvement assignments under that mandate while preserving retained outcomes and already assigned executor work according to the user's stop scope. Lead exit prevents orphaned launches; a verified successor can take over pending work through existing lifecycle ownership. Revalidate purpose, scope, source boundaries, actual model and guidance revision before continuation. Retain distinct operation, observation and restoration outcomes and do not replay uncertain effects.

Installed enable/status/disable/recovery must own routine prerequisites and report cause and next action. Reuse the existing `delivery-improvement` command namespace if useful, without preserving an observer-specific implementation solely for compatibility: it was not delivered. The two skills, native trigger, fork chain, feedback decision and consuming sessions are one end-to-end feature.

### 8. Configuration permission is separate from lifecycle activation

`self_reflection_enabled` in `global/orchestration.toml` is a default-on,
boolean permission for new reflection model stages. True only permits work
under an explicitly activated lifecycle; it never creates or reactivates one.
False stops new executor and lead model calls through every dispatch owner
while ordinary lead, executor and acceptance work remains available.

Each new stage re-reads the effective source configuration. A host that is
already running therefore observes an on-to-off change without restart. A stage
already executing may finish and retain its result; the coordinator checks
again before the dependent lead stage. The existing controller owns the lock,
checkpoint, recovery and retained records. The normal acceptance owner resolves
the effective permission before closing the board task. An explicit `--source`
keeps its current meaning; otherwise a recognized kit checkout uses its own
configuration and a consumer without one uses the installed kit source recorded
by the normal installation. The project remains the pool-provenance source and
is not silently retargeted to the kit. The permission is recorded in the
acceptance binding and public receipt. Default-true bindings omit the field on
serialization so retained acceptance metadata keeps the legacy shape; false is
explicit. Accepting while false therefore succeeds but creates an ineligible
closure; the existing event evaluator skips it as its normal `Ignored` result
even if the flag returns to true before the first poll. Closures otherwise
observed during the disabled interval advance the normal event boundary without
admission. Neither path sweeps the disabled interval after re-enable or
duplicates retained work, and this adds no refusal or persisted-state
vocabulary. A disabled `process` still authenticates the managed lead and active
binding before advancing the retained checkpoint; a foreign lead is refused
without changing the checkpoint or records.
Status reports the effective value, the source path and why model work is
stopped.

### Parallel candidate implementation and predeclared evaluation

Keep the initial evidence and uncertainty in `proposal.md`; do not turn the
audit's small sample into an incidence or savings claim. Task 1.3 must qualify
real pre-solution inputs and freeze the comparison before reflection candidate
model dispatch, evaluation or adoption. Isolated code and guidance implementation,
builds and model-free checks may proceed in parallel with qualification. Keep
that candidate inactive and outside accepted delivery, preserve the accepted
baseline, and count the implementation work in the existing cost accounting.
This concurrency may leave an unused candidate if the pilot rejects it; it
does not relax any comparison or product acceptance criterion. Reuse the existing
outcome-policy, skill-evaluation and local evidence owners, not a new experiment
framework.

The initial panel is three matched real tasks: reconciliation of stale planning
against existing implementation; diagnosis of missing/contradictory evidence;
and healthy text-heavy work with no useful improvement to make. Use applicable
retained task inputs or actual authorized work, never fabricate a production
incident. Add one matched sequence of three relevant uses of an accepted
preventive correction to test the claimed reuse mechanism. A missing applicable
unit leaves that claim unqualified rather than being replaced after results by
an easier case. These four matched units are a bounded pilot, not a confidence
estimate or proof of benefit across arbitrary projects.

The control is the existing lead/feedback workflow with its ordinary bounded
evidence checks and no automatic reflection; do not cripple it or deny it normal tools. The
treatment enables the two-stage reflection and scheduling path. Freeze task/source
and acceptance identities, model/effort, initial cache state, configured
capacity, and balanced arm order. Use fresh contexts and isolated task inputs;
neither arm receives the sibling solution. The repeated-use unit retains its
own declared state across its three uses. Retain all selected units, including
failures, no-finding turns, retrieval, implementation, checks, coordination,
restoration and displaced delivery. Neither a faster unrelated task nor a
replay of fixed responses substitutes for this agent-behavior comparison.

The objective is to reduce the full cost of future changes and maintenance while
delivering useful results promptly. Before candidate model dispatch, declare the
concrete benefit to examine, its mechanism and relevant use, independent quality
acceptance, observation horizon, complete cost coverage and bounded stopping
conditions in the existing evaluation owner. No mandatory percentage savings,
weighted score or closed list of benefits replaces the lead's engineering judgment.
Reuse the existing maintenance-basis and outcome evidence owners where applicable;
do not build another evaluation framework.

Quality must be no worse than the control, with no P0/P1 or other critical
regression. Equal independent task success is allowed, including when both arms
accept every task; equality alone is not benefit. A failed candidate outcome cannot
be offset by another unit's success. Retain accepted task outcomes, later rework,
total time through acceptance and native recorded token usage as separate evidence.
Missing usage remains unknown, never zero or evidence of nonincrease. If missing
cost coverage could change the token-cost or payback decision, adoption remains
inconclusive; a separately supported maintenance benefit does not waive this.

Tokens are the spending currency. Compare the full accepted-result path, including
seed work, reflection, implementation, checks, coordination, failed attempts and
later corrections within the declared relevant-use horizon. A larger prompt can
repay its input cost by preventing correction cycles; prompt size and per-call
usage alone do not determine value. At equal or lower full token cost, supported
lasting benefit needs no mandatory savings percentage. Increased full token cost
requires a predeclared concrete payback mechanism, observable benefit measures
and bounded horizon, with actual use and checks supporting the claimed return.
Measures may concern correction cycles, delivery time, defects or future change
effort; these are examples, not a closed checklist or a token-to-quality score.
A forecast or qualitative confidence alone cannot establish payback. Retain the
baseline and cumulative adopted costs so separate small increases cannot reset
the comparison or conceal drift. Prefer lower long-term token cost alongside
faster delivery and preserved or better quality. Bounded investigation can test
an uncertain hypothesis; admission to investigation is not adoption.

Judge the concrete effect on affected users and likely future work against the
cost of implementation, checks, coordination, deployment, recovery, ongoing
maintenance and displaced useful delivery. Simpler interfaces, fewer incidents,
less context burden, easier diagnosis or replacement are possible benefits, not
an exhaustive checklist. More code, instructions, tools or architectural machinery
must justify their recurring cost; polishing without useful delivery is a cost
too. Prefer the smallest effective correction, including no change. Real use and
checks must support the benefit mechanism, while expected long-term effects remain
explicit forecasts. Record a concise reason and material trade-offs in the existing
decision owner; do not demand repeated signals or arbitrary proof quotas when
available facts suffice, while retaining measured token costs and observable
payback for increased spending. Other experiments retain their explicitly frozen
strict policies.

Stop after the declared panel. Reject when quality regresses or the concrete
benefit does not justify complete costs and risks; classify decision-changing
uncertainty as inconclusive. Lack of immediate time or token savings alone is not
rejection when full token cost does not increase or its payback is supported as
above; unsupported added spending cannot be adopted. Passing correctness alone
is not benefit.
A further trial needs a named new decision-changing observation and its own
predeclared scope, not repeated sampling until favorable. Keep unsuccessful
reflection code/guidance out of accepted source and installation; restore verified
baseline selection and retain the inactive candidate and evidence. Any broader
claim requires predeclared representative corroboration under the existing
evaluation owner. Product acceptance below remains required even after a
favorable pilot; a transport fixture or completed vertical slice is not benefit.


### Evaluation boundary after the redesign

The treatment now starts only after accepted work. It cannot improve the already completed task retrospectively. Qualify the same three problem/no-benefit classes and three-use preventive sequence against real accepted seed work and applicable subsequent work inside the declared horizon. Both arms retain equivalent seed inputs and ordinary lead powers; count seed work, reflections, improvement implementation, checks and all later accepted work. Do not reuse a frozen observer experiment unchanged or count the source task's pre-trigger success as reflection benefit. Establish relevant local inputs before candidate model dispatch, preserving the gate above and every adverse/unknown result.

## Risks / Trade-offs

- Full native forks may cost more than focused input: measure complete input/output costs and stop at finite job allowances; do not claim savings from fewer sessions.
- Accepted-task triggering cannot repair an in-flight source task: immediate defect handling remains the lead's normal responsibility; benefit must appear in later relevant work.
- A closed card may mean cancellation or duplicate: require scoped acceptance and ordinary-purpose identity, not status or prose alone.
- Journal retention, sync or branch/replica changes can hide events: retain identity-aware checkpoints and expose/reconcile gaps before dependent launches.
- Forks may inherit goals, tools or stale role behavior: verify effective read-only reflection context through native fixtures and installed real acceptance.
- Missing ordinary-lead delivery can strand findings: close and exercise the native session-owner gap before declaring the feature delivered.
- Improvements can consume useful capacity without recursion: keep main-work priority, the one-improvement limit and full cost accounting.
- Reused worktrees or compacted contexts may hide evidence: retain exact revisions/source references and report missing coverage.

## Migration Plan

1. Preserve rejected observer patches/evidence for recovery, remove their active selection/code from workspaces and delivery, and reconcile the existing artifacts/tasks. Do not weaken unrelated requirements or alter protected workflows.
2. Prove the installed event/fork/role/delivery boundaries with model-free native fixtures. In parallel qualify baseline data and author the two inactive skills.
3. Deliver the first complete installed accepted-task closure -> executor reflection -> lead reflection -> real lead decision -> accepted improvement path under the qualified evaluation.
4. Complete replay, purpose inheritance, no-finding, budget/capacity, interruption/succession, publication and consumption scenarios; preserve normal mode-off behavior.
5. Apply the frozen benefit gate, restore baseline on rejection/inconclusive results, reconcile current guides and close only proven tasks before archive and push.
