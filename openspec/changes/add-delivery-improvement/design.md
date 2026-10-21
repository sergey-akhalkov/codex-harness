# Design

## Context

See [proposal.md](proposal.md). The role separation and shared worktree pool changes are delivered prerequisites; verify their current installed contracts rather than reimplementing them. The unaccepted persistent observer is withdrawn. Its continuous feed, long-lived role, batching/backoff and lifecycle adapter are not part of the replacement.

Reusable owners include `executor_control.rs`, `executor_observation.rs`, `executor_message.rs`, native run receipts and terminal hosts; `role_context.rs` and `global/roles.toml`; `task_worktree.rs` and the existing admission owner; `board_lifecycle.rs`, `board_feedback.rs` and `feedback_cli.rs`; and the installation lifecycle. Preserve their accepted behavior and adapt only missing contracts.

Local contract inspection established native Codex fork, model/profile and sandbox overrides, plus a fork schema with source turn boundaries and goal-continuation control. This is interface evidence, not proof of safe fork execution or cross-provider context compatibility. Beads exposes `events tail`, ordered committed mutations and explicit truncation errors. The journal is optional, local to a replica and branch, and does not cover raw SQL mutations or changes imported by sync. Setup and recovery must account for these boundaries.

The current implementation resolves the executor reflection stage from the configured executor profile (`ds` in the shipped kit) and the lead stage from `lead_profile`. A prior real executor-reflection attempt failed and is being recovered. No successful two-stage chain, current live-lead consumption or global installation is yet proven.

## Goals / Non-Goals

**Goals:** learn from accepted work and actual corrections; give each role a focused retrospective from the exact relevant source; supply the retained executor result to lead synthesis; return actionable recommendations or no finding to the real lead; apply useful changes without displacing the user's objective; prove the installed sequential chain and preserve honest cost evidence.

**Non-Goals:** a persistent model observer, timer-driven reflection, automated prioritization, automatic prompt publication, recursive reflection, a new tracker or experiment framework, guaranteed findings, a mandatory comparative study, or a claim of measured savings without evidence.

## Decisions

### 1. Accepted board closure is the trigger

An explicitly enabled binding covers the current lead and authorized work. The normal lead acceptance path records a small acceptance reference on the existing task, binding its assignment/run identity, accepted revision or artifacts and relevant source sessions. The lead closes that task through ordinary beads operations. The interface owns routine metadata; users do not reconstruct IDs or separately request reflection.

A model-free consumer of the board's native event journal treats that closure as eligible only when it belongs to ordinary work, has the applicable lead acceptance and exact task/run binding, and has not already been consumed. An executor report or `lead_review` status is not acceptance. Cancelled, duplicate, rejected, unrelated and improvement tasks do not qualify. An imported closure or gap in events is not assumed to represent a new accepted local task.

The consumer belongs to the existing enabled lead/run lifecycle, not a separate agent or globally orphaned daemon. Enabling establishes journal availability and the current activation boundary before watching new closures; no retrospective sweep of historical tasks. It reuses an already enabled journal and records any owned configuration change for conservative restoration. Stopping does not disable journal use owned by another consumer.

Persist the checkpoint with source replica/branch identity and exact acceptance identity in existing private lifecycle state. A sequence number alone is not portable across replicas. Replayed events reconcile retained work; reopening and reclosing the same acceptance does not run again, while a genuinely new accepted assignment revision can qualify. A truncation, sync, source change or unknown external effect produces an explicit recovery state; re-read only bound task records, do not silently skip or relaunch all closed cards. Native event collection makes no model calls.

### 2. Two short-lived role-specific forks

After admission, fork the accepted executor conversation at a stable completed boundary and invoke `executor-self-reflection`. It examines the original assignment, relevant work, lead corrections and final checks: ambiguities or missing inputs, its own avoidable mistakes and unnecessary work, and concrete changes that could prevent them. The existing assignment and acceptance retain all relevant executor sessions if correction used a successor. Do not substitute a current reused checkout for the reviewed artifacts.

The executor reflection runs under the configured executor profile, `ds` in the shipped kit, at that profile's declared model and effort. Retain the result, source identity, coverage and usage before releasing the fork. A completed retained executor reflection, including a valid no-finding result, makes a fork of the corresponding lead conversation eligible.

Invoke `lead-self-reflection` with the lead context and the retained executor result. It independently assesses framing, delegation, review demands, changed requirements, executor mistakes, infrastructure and lead-side overhead. Criticism from either role is evidence to test, not truth. The lead fork synthesizes bounded recommendations, disagreement and uncertainty. It uses the configured `lead_profile`.

Each stage is one finite analysis job; it can use bounded reads for a stated question, then completes. Read-only capabilities cover tools as well as filesystem permissions: no board mutation, agent dispatch, source writes or self-publication. Override inherited operational role/goal behavior through the native owner before model work and verify it took effect. Do not decode opaque reasoning or compaction state. Native fork context is the source's available context, not a promise that every historical byte survived compaction. Supply source references and explicit gaps; retrieve relevant visible messages, original instructions/assignment, revisions and checks when needed. Missing evidence yields uncertainty, not a fabricated complete review.

Keep the lead and executor skills separate because their responsibilities and guidance differ. Reuse shared launch, accounting, result retention and transport; do not create two duplicated frameworks or a third observer prompt. Resolve each stage's declared route explicitly and verify the actual binding before request. No silent model, provider or billing substitution, inherited executor binding for the lead fork, or claim that forking makes input free.

### 3. Durable result delivery advances the chain

The executor result is durable input to lead reflection, not a direct improvement assignment. The controller records each stage's native identity and result acknowledgment, and never equates process exit, a final-looking message or elapsed silence with a verified result. Unknown outcomes reconcile against native state and retained artifacts before retry. Existing completion events and receipts drive advancement.

Lead reflection returns a compact result to the exact current live lead/task. Delivery must be observed and consumed, or retained honestly as pending/unknown; queued or submitted does not mean consumed. Use the existing addressed channel and safe-boundary processing. The supported session owner must preserve conversation/work and avoid a second lead or hidden manual polling ritual. If the native route is unavailable, name the missing capability and verified remedy before a dependent launch.

Several already pending executor results for one lead can share one lead reflection at the next available admission boundary. Do not add a timer to wait for speculative future results or delay a ready single result. Each source acceptance/result is associated with exactly one retained synthesis outcome; later results remain pending. Main work never waits for either reflection.

### 4. Purpose prevents recursive improvement

Own purpose metadata at task/dispatch creation: ordinary work, reflection or improvement implementation. Bind it to actual task/session lineage; labels and titles are helpful display, not the only enforcement. Improvement implementation and its descendants inherit the nontriggering purpose even when split, retried, resumed, renamed or closed normally. Reflection sessions never trigger reflection. Unknown or contradictory purpose refuses automatic launch with a precise status.

The real lead creates any accepted improvement task with that purpose. The lead may explicitly request a bounded review under existing authority, but automation does not relabel it as ordinary work or recursively create successors. Normal code review, acceptance, defect correction and installation checks still apply to improvement implementations.

### 5. Capacity, cost and main-work priority

Use existing shared admission for each active reflection stage. A waiting event/result owns no implementation worktree or permanently reserved model slot. Every model conversation has its own titled visible surface before its first request, showing actual role/profile/model/effort and live activity. Loss of visibility suspends new model work and invokes the existing owning recovery path.

Healthy executors are not preempted. Pending reflection waits for ordinary available capacity without delaying launch of more consequential user work. Keep one ordinary improvement implementation in flight within existing capacity; this is neither a fixed project limit nor a reserved lane. Current correctness/integrity blockers retain ordinary urgent handling.

Reflection and improvement implementation may proceed in parallel with ordinary delivery when resources and dependencies allow. Neither is a routine gate on ordinary acceptance or release. The responsible lead can hold an affected release for a concrete potential incident, considering impact, likelihood, exposure, mitigation and delay cost, with a reason and checkable release condition on the existing task. Reflection supplies evidence; it cannot impose or lift the hold.

Install finite per-job investigation/turn/context limits and cumulative allowances per authorized work binding. Include source fork context, both reflection stages, failed/no-finding runs, reads and retries; expose recorded usage separately from reservations or estimates. Arrival, resume, a new fork or repeated closure cannot reset cost accounting. Overrun or unknown usage is visible; no further overspend is silently admitted. No finding is a normal completed outcome, not a reason to retry or manufacture work. No savings claim follows from an unmeasured or incomplete record.

### 6. The real lead owns decisions and implementation

Synthesis returns the observation, source basis, likely cause, smallest remedy, appropriate owner, credible next use, expected avoided work and a proportionate check. Prefer removing work or fixing an input/interface/check over adding instructions. Skill or role guidance is appropriate where judgment or a reusable procedure needs it; do not learn a new rule from an unsupported accusation.

Only the real lead creates or updates beads tasks, deduplicates against relevant prior items, accepts, rejects or defers, and dispatches implementations. One clear attributable defect is enough for a bounded correction; no votes, repeated-signal waiting or full experiment ceremony is imposed on a deterministic fix. Uncertain hypotheses retain existing evaluation routes. A derived reflection is not another independent evidence vote.

At the next safe boundary the lead decides. A useful recommendation may receive a standard executor assignment under the existing improvement purpose, but no finding, assignment, completed improvement or comparative experiment is required merely to release the mechanism. A no-finding or rejected recommendation is a valid disposition. Rejected unchanged recommendations are not periodically resent. The lead may complete a trivial cheaper-than-handoff correction directly.

### 7. Delivery, publication and recovery retain their owners

Project-specific evidence stays local. Shared remedies use a kit-owned task and public-safe synthetic reproduction; private provenance remains with the consumer. Required specification, skill-evaluation, installation and actual new-session consumption remain necessary. Reflection cannot change its own guidance, acceptance or protected external workflows.

A stop prevents new reflection and improvement assignments under that mandate while preserving retained outcomes and already assigned executor work according to the user's stop scope. Lead exit prevents orphaned launches; a verified successor can take over pending work through existing lifecycle ownership. Revalidate purpose, scope, source boundaries, actual model and guidance revision before continuation. Retain distinct operation, observation and restoration outcomes and do not replay uncertain effects.

Installed enable/status/disable/recovery owns routine prerequisites and reports cause and next action. Reuse the existing `delivery-improvement` command namespace if useful, without preserving an observer-specific implementation solely for compatibility: it was not delivered. The two skills, native trigger, fork chain, feedback decision and consuming sessions are one end-to-end feature.

### 8. Configuration permission is separate from lifecycle activation

`self_reflection_enabled` in `global/orchestration.toml` is a default-on boolean permission for new reflection model stages. True only permits work under an explicitly activated lifecycle; it never creates or reactivates one. False stops new executor and lead model calls through every dispatch owner while ordinary lead, executor and acceptance work remains available.

Each new stage re-reads the effective source configuration. A host that is already running observes an on-to-off change without restart. A stage already executing may finish and retain its result; the coordinator checks again before the dependent lead stage. The existing controller owns the lock, checkpoint, recovery and retained records. The normal acceptance owner resolves the effective permission before closing the board task. The selected source keeps its pool-provenance meaning, whether explicit or the default project.

A recognized kit checkout supplies its configuration; an ordinary consumer uses the kit source recorded by the normal installation. Pool provenance is not silently retargeted to the kit. The permission is recorded in the acceptance binding and public receipt. Default-true bindings omit the field on serialization so retained acceptance metadata keeps the legacy shape; false is explicit. Accepting while false succeeds but creates an ineligible closure; the existing event evaluator skips it as its normal `Ignored` result even if the flag returns to true before the first poll. Closures otherwise observed during the disabled interval advance the normal event boundary without admission. Neither path sweeps the disabled interval after re-enable or duplicates retained work. A disabled `process` still authenticates the managed lead and active binding before advancing the retained checkpoint; a foreign lead is refused without changing the checkpoint or records. Status reports the effective value, source path and why model work is stopped.

### 9. Release acceptance uses the real sequential chain

The user's release decision makes one successful real sequential chain sufficient: an accepted ordinary task starts executor reflection, the retained executor result starts lead reflection, lead synthesis is delivered to and consumed by the current live lead, and that lead records whether any recommendation warrants normal follow-up. Applicable current regression checks and verification of the global installation are also required.

The previous failed executor reflection is not success. Model-free tests, fixtures, retained plans and a printed final message do not establish semantic success or current-lead consumption. A successful retry or a new real accepted chain may satisfy the chain evidence. The live lead may choose a standard executor assignment for an actionable recommendation, but no recommendation, improvement task or comparative campaign is required for release.

Optional later evaluation may assess whether recommendations improve subsequent work. It is not a release gate, does not authorize a savings claim, and does not retroactively improve the accepted source task.

## Risks / Trade-offs

- Full native forks add model cost: record available usage, preserve unknown values and do not claim savings from fewer sessions or unmeasured runs.
- Accepted-task triggering cannot repair an in-flight source task: immediate defect handling remains the lead's normal responsibility.
- A closed card may mean cancellation or duplicate: require scoped acceptance and ordinary-purpose identity, not status or prose alone.
- Journal retention, sync or branch/replica changes can hide events: retain identity-aware checkpoints and expose or reconcile gaps before dependent launches.
- Forks may inherit goals, tools or stale role behavior: verify effective read-only reflection context through native fixtures and installed real acceptance.
- Missing ordinary-lead delivery can strand findings: do not declare delivery until the current live lead has consumed the synthesis.
- Improvements can consume useful capacity without recursion: keep main-work priority, the one-improvement limit and full cost accounting.
- Reused worktrees or compacted contexts may hide evidence: retain exact revisions and source references and report missing coverage.
- Analysis can produce plausible but unsupported recommendations: preserve facts, hypotheses and uncertainty and leave the decision with the live lead.

## Migration Plan

1. Preserve rejected observer patches and evidence for recovery, keep their active selection out of delivered workspaces, and retain the withdrawn implementation as inactive recovery input only.
2. Verify the implemented event, permission, role, fork, purpose, visibility, delivery and recovery contracts with current model-free native checks and documented limitations.
3. Run one real accepted ordinary task through executor reflection, retained-result handoff, lead reflection and current live-lead consumption; a failed attempt remains unsuccessful evidence and recovery continues without inventing a second semantic path.
4. Install or update the global kit and verify the two role selections, `ds` executor routing, lead routing, default-on switch, visible surfaces, disable/recovery behavior and preservation of unrelated state outside this checkout.
5. Reconcile the owning documentation and operating guidance, record the live lead's recommendation disposition, close only checks actually observed, and leave optional benefit evaluation as future work rather than a release condition.
