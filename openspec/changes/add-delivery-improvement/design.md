# Design

## Context

See [proposal.md](proposal.md) for motivation and scope. The change crosses session ownership, feedback admission, instruction delivery and installation, so a design is required.

Inspected foundations:

- `crates/codex-harness/src/executor_message.rs` owns addressed messages, delivery outcomes, retries and originating-lead routing. Its receipt entries establish identity and delivery status; they are not a sufficient source of inline message text. Spill payloads already have owned local storage.
- `crates/codex-harness/src/executor_control.rs` retains observed control records and exposes completed conversation items while suppressing transient token deltas. `executor_observation.rs` and the native receipt/watch path own lifecycle observation. These are reusable evidence and transport foundations, not proof that both roles already expose every required event to an observer.
- `crates/harness-core/src/board_feedback.rs` and `crates/codex-harness/src/feedback_cli.rs` own deterministic beads operations. `board-workflow` and `team-lead` own semantic triage and scheduling guidance. Current promotion does not promise an execution opportunity.
- `crates/codex-harness/src/executor_assignment.rs` already represents objectives, inputs, outputs, invariants, acceptance and the result consumer. Recorded assignment bases, Git revisions and retained patches can anchor code review; managed role receipts distinguish resolved guidance from guidance a continued thread actually saved. These are attributable inputs, not permission to infer missing code or instruction consumption.
- Observation receipts carry session identity, state, exit status, round/tool counts and bounded evidence locators with truncation and coverage flags. The [token-audit owner](../../../docs/memory/token-audit.md) provides recorded usage summaries and retained detail; reuse that accounting for scoped cost assessment instead of building another rollout reader or loading session logs into the observer.
- `global/orchestration.toml` owns profiles, executor limits and feedback limits. Existing delegation and installation owners cover visible conversations, partial-work preservation and model binding. A managed lead endpoint supports reverse messaging; an unmanaged lead currently lacks that guarantee.
- `separate-lead-executor-contexts` proposes the common role resolver, `global/roles.toml`, role source files and native skill filtering; its implementation is not delivered by the planning artifacts. `unify-reusable-worktree-pool` separates checkout allocation from conversation launch. Their work and acceptance remain with those changes.

The existing feedback spec's incubator-only routing and lead-only semantic triage need explicit deltas. The new capability owns the optional observer and the extra scheduling obligation; ordinary lead/executor acceptance, pool safety and experiment gates remain authoritative.

## Goals / Non-Goals

**Goals:** close one real correction-to-applied-improvement path early; give analysis a separate attention context; remove repeated work near its cause; make accepted improvements compete for actual execution opportunities; bound observation cost and preserve reliable recovery.

**Non-Goals:** another lead, a new scheduler that interprets the board, an always-running model loop, a universal transcript index, an automatic prompt optimizer, a new provider selection framework, or a replacement for the experimental `improve` controller. This mode does not promise improvements on every task or guaranteed savings.

## Decisions

### Measurement and decision before implementation

Keep the initial evidence and uncertainty in `proposal.md`; do not turn the
audit's small sample into an incidence or savings claim. Task 1.3 must qualify
real pre-solution inputs and freeze the comparison before observer code or
guidance is evaluated. Reuse the existing outcome-policy, skill-evaluation and
local evidence owners, not a new experiment framework.

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
evidence checks and no observer; do not cripple it or deny it normal tools. The
treatment enables the proposed observer and scheduling path. Freeze task/source
and acceptance identities, model/effort, initial cache state, configured
capacity, and balanced arm order. Use fresh contexts and isolated task inputs;
neither arm receives the sibling solution. The repeated-use unit retains its
own declared state across its three uses. Retain all selected units, including
failures, no-finding turns, retrieval, implementation, checks, coordination,
restoration and displaced delivery. Neither a faster unrelated task nor a
replay of fixed responses substitutes for this agent-behavior comparison.

Primary outcomes are independently accepted task success rate, later P0/P1
rework within the frozen observation horizon, total time through acceptance and
native recorded token usage. Finding counts and admission latency are diagnostic
metrics only. Freeze metric coverage, the horizon, practical effect thresholds
and any uncertainty/repetition rule in the existing comparison policy from the
baseline measurements before implementation; an unset threshold is a blocked
experiment, not zero. Preserve the owner's addition gate: quality must improve
with no critical regression and both time and token cost must improve by their
declared useful margins. Do not add time and tokens into an invented score.
Missing counters, a verdict-changing bound, an unchanged success rate or absent
relevant reuse cannot be reported as supported addition benefit.

Stop after the declared panel. Reject when quality regresses or complete evidence
fails the frozen benefit gate; classify insufficient attribution as inconclusive.
A further trial needs a named new decision-changing observation and its own
predeclared scope, not repeated sampling until favorable. Keep unsuccessful
observer code/guidance out of accepted source and installation; restore verified
baseline selection and retain the inactive candidate and evidence. Any broader
claim requires predeclared representative corroboration under the existing
evaluation owner. Product acceptance below remains required even after a
favorable pilot; a transport fixture or completed vertical slice is not benefit.

### 1. Add one optional role and a small lifecycle entry point

Add `.agents/skills/delivery-improvement/SKILL.md` and `global/roles/improvement.md`. Extend the shared role descriptor/resolver from `separate-lead-executor-contexts`; do not copy the lead or executor prompt into the new role. Its skills cover investigation, project context, evidence and board intake. Delegation, implementation, integration and self-publication remain unavailable responsibilities. Source and instruction files are read-only to this role; scoped board updates and runtime acknowledgments are its permitted writes.

Use a small Rust-owned `codex-harness delivery-improvement start|status|stop|resume` adapter as the proposed user-facing lifecycle, implemented over the existing control host, terminal surfaces, receipts and configured profile resolution. These commands do not exist yet. `start` derives the current managed lead, authorized task and installed kit from verified records, creates one binding and launches the observer. Repeating it finds the existing binding. `status` is model-free and reports coverage, pending analysis/decision references, batching or budget waits, consumed allowance, lifecycle and concrete blockers; board status is obtained through the existing board CLI, not duplicated in the host.

The skill invokes the adapter and the existing lead workflow rather than asking the user to coordinate endpoints or IDs. A native contract check must establish event access and addressed notification for the current lead, including a supported ordinary main-session activation path. Reuse existing managed-lead attachment or safe-boundary succession where needed, preserving the actual conversation and work. If the installed native contract cannot support that path, activation must give the cause and verified remedy; it must not silently launch a replacement lead, require an unannounced everyday restart ritual, or claim an operative loop. Closing that gap is implementation work in the current session owner, not a user responsibility or permission to fall back to manual polling.

A role is independent of provider/model. Resolve the observer through the configured executor-profile selection and record the effective binding. There is no new model profile, cheaper-model substitution or recursive model call. A title identifies the improvement role independently of its model.

### 2. Share capacity without permanently reserving an executor slot

An observer needs a conversation, not a source-writing worktree. Extend the existing admission owner to count a running observer turn alongside live implementation assignments within the configured concurrent-work ceiling. Its idle event wait does not hold an implementation slot or acquire a worktree; every new turn must reacquire admission. Do not lower the executor limit merely to reserve an always-idle observer, and do not run its model work as an invisible extra beyond the ceiling.

When all capacity is occupied, events remain pending. At the next ordinary allocation boundary, pending analysis participates in allocation before lower-priority new work; healthy executors are never preempted. The practical trade-off is explicit: under saturation analysis can wait for capacity. A substantive urgent signal can notify the lead through the existing event mechanism without spawning another hidden model. Provider refusal or lost visibility preserves pending inputs and blocks dependent model work with the existing recovery rules.

The first version allows one ordinary improvement implementation at a time. This is a work-in-progress limit within existing capacity, not a promise of a permanently reserved lane, a project-count limit or a deadline. Urgent defect handling remains the existing consequence path. If shared admission cannot be separated from checkout allocation without the pool change, integrate that prerequisite first rather than inventing another pool.

### 3. Extend existing event observation with bounded incremental consumption

Bind observation to the lead identity, its task and its recorded executor lineage. Subscribe through existing managed session owners to all newly completed visible user and assistant text in that scope, including both directions of addressed exchanges. Expose the starting cursor and current task context at activation without automatic whole-history backfill. Native message type and channel define the feed; neither code nor another model ranks messages by keywords, importance or predicted failure. The observer interprets meaning. This preserves clues expressed as ordinary prose without an anomaly classifier.

Reasoning, tool calls, execution steps, raw tool results and repeated system or role prompts are not routine feed items. A visible message remains intact even if it quotes a command or explains a decision: exclusion follows record type, not wording. Lifecycle/control metadata still supports routing, admission and recovery without becoming an activity transcript. Do not scrape windows, decode reasoning or recursively read every log. A failure visible only in an excluded tool result is outside routine text coverage; preserve existing operational failure reporting and state that limit honestly.

Use existing board commands to fetch the current task, its recorded assignments, direct blocking dependencies and linked feedback. Bootstrap with current state rather than every historical comment; follow subsequent changes to descriptions, acceptance, status, blockers, comments and decisions using available revision or content identity. Retain context needed to interpret each change. The observer can request another issue or bounded search for a stated question; do not recursively load the board. Unchanged reads start no model turn. A completed card is a claim to check against acceptance evidence.

Use native message IDs, revisions and run generations to correlate retries and reused slots. Deduplicate mirrored deliveries by provenance, not identical wording. Keep source cursors, pending batch references and the active turn in existing private run state; beads remains the task and decision owner. Retain message text and board changes or pin existing locators before acknowledgment. A delivery hash is not text, and rotation must expose a coverage gap. Do not publish private locators or transcripts.

Model-free collection coalesces pending input while one turn runs. Routine analysis obeys existing supervision cadence; earlier turns still require a permitted substantive event or explicit request, not guessed text meaning. Repeated no-finding outcomes reduce call frequency by accumulating bounded batches or waiting for an existing work boundary. Install finite pacing defaults and flush opportunities for pending-input waits, task completion or explicit requests, subject to budget and admission. An empty timer never calls the model. Backoff delays processing without sampling text or requiring special progress reports.

The observer receives bounded text and board batches with references. It acknowledges retained findings or compact no-finding outcomes associated with consumed input. Split oversized messages into ordered parts with continuation state instead of truncating or summarizing them before analysis. Missing detail, unavailable boards and uncertain model outcomes never acknowledge unfinished evidence. The observer proposes semantic grouping; existing feedback commands apply it deterministically.

For an explicit unresolved question, retrieve only the relevant assignment, code diff, source symbols, check result or operation excerpt through its current owner. Preserve source identity and attributable timing where available. Each request belongs to the episode's allowance and states the question it can resolve. Uncertainty does not justify automatically reading every trace or source file.

Extend the existing orchestration configuration owner with validated finite defaults for per-turn context, episode analysis and detail retrieval, cumulative observer work per task and no-finding pacing. Count board intake, carried context and investigation as well as new messages. Enforce turn/context limits and account for available native usage, distinguishing estimates from measurements. Report reservation overruns and block further overspend without promising an exact provider token ceiling. Code enforces admission, not semantic value. A budget pause retains input, sends one bounded lead notice and exposes the supported continuation choice. Input arrivals, waits, resume and fresh contexts never reset task accounting; changing an allowance requires a recorded lead decision.

When carried context reaches its bound, continue through the existing visible session lifecycle with compact findings, unresolved questions, board references and source cursors. Preserve unread input and decisions without replaying consumed conversations or repeating investigations. All-text coverage still costs in proportion to that text; batching reduces repeated overhead, not required coverage. Measure savings and expose any backlog or budget-blocked tail before claiming complete analysis.

### 4. Diagnose the owning cause and reuse the board

For each candidate, compare the observed correction with the assignment, source revision, applicable instructions and actual result. Distinguish a new user requirement, faulty lead brief, executor mistake, tool failure, unavailable infrastructure and an unclear cause. A proposed global rule must not be learned merely by copying a lead's accusation.

For code feedback, reconstruct the relevant chain: original requirements and assignment; executor change against its recorded base; the identified lead's exact feedback; subsequent correction; affected checks. Use recorded commits or already retained patches, never assume a reused worktree holds the reviewed code. Retrieve affected source/symbols and necessary callers only as the question requires. Missing snapshots or checks stay unknown; source review does not prove runtime behavior.

Other useful context has existing owners: specifications define expected behavior; assignment inputs/invariants expose briefing gaps; role and skill receipts establish applicable guidance where consumption is known; beads links show blockers and earlier decisions; command/check evidence tests the diagnosis; usage and activity summaries support cost assessment. Verify each required native access path in task 1.2. None warrants a second archive or automatic loading of all code, instructions and reports.

An executor-practice proposal names a concrete behavior, such as checking an existing invariant or exercising a failure path, and an observable result on representative work. Compare earlier input provision and interface/test corrections with task-specific or reusable guidance. Retain skill/agent evaluation before claiming the executor avoids a defect without additional lead prompting. Do not infer general incompetence from one disputed review.

Use the existing item fields and description for evidence reference, diagnosed cause, affected owner, credible next uses, expected avoided work, proportionate total cost, smallest remedy and acceptance. Estimates, including future reuse, remain predictions. Reject or defer ordinary optimizations without a plausible opportunity to repay, preserving urgent correctness/integrity handling. Inspect relevant open, deferred and completed items without a whole-board reread. An observer's summary of another agent's episode is derived evidence, not another vote. Reconsider a rejected basis only with new evidence or changed conditions.

Prefer removing a redundant step, supplying missing inputs or checking them before dependent work, then fixing the owning interface. Change instructions where judgment or a reusable procedure actually needs guidance. This is also the direction described in OpenAI's [harness engineering account](https://openai.com/index/harness-engineering/): feedback can become environment and tool improvements rather than an expanding general instruction manual. That experience informs the choice; it does not establish this change's benefit.

Extend existing feedback promotion with an explicit verified-improvement admission basis and active-run reference. Validate scope, required fields and recorded lead decision; semantic truth remains the lead's responsibility. Keep ordinary votes, consequence override, skill-procedure routing and the existing experiment hypothesis path. Do not smuggle low-confidence ideas into the new route or treat admission as a skill-evaluation result.

### 5. Make the next allocation decision include accepted improvements

The observer sends one bounded finding notification through the originating-lead transport after retaining the board item. Batches avoid one interruption per low-value observation. A notice needs no reply unless a real unresolved question requires one. The lead decides at the next safe boundary: accept, defer with reason and observable return condition, or reject with basis. These decisions live on the existing item.

Acceptance is followed by readiness work through the existing workflow, including any required specification or checkable brief. At the next suitable free capacity, the lead either assigns the ready improvement or records the concrete conflicting higher-priority work or blocker. Repeated ordinary feature dispatch while a ready accepted improvement has no recorded competing reason violates the new contract. This closes the gap between admission and actual work without adding a board-parsing scheduler.

The observer can notice a due return condition or a skipped agreed opportunity from fresh evidence and escalate the existing item once. It does not send unchanged reminders, take over prioritization or dispatch on its own. The lead can finish a trivial cheaper-than-handoff correction directly; substantial eligible work uses existing executors and committed-base assignments. No suitable finding is a valid result, and ending a task does not require inventing an improvement.

### 6. Keep project, kit and instruction delivery ownership explicit

Project-specific changes remain on the project's board and source. Shared changes use the canonical kit checkout and kit board, with sanitized synthetic reproduction and public-safe descriptions. The consumer retains private evidence and its link to the shared result; no reverse public link reveals consumer identity. Normal task ownership and installation exclusion prevent concurrent consumers from implementing the same accepted kit item or racing global publication.

The activated mandate covers relevant bounded kit changes and ordinary verified installation, as well as project improvements. It does not override explicit limits, protected external workflows, material outcome decisions or scoped removal consent. Required OpenSpec changes are prepared before their dependent implementation. Skill mutation still uses skill-creator/skill-evolution and its existing evaluation/registration owners. The improvement agent may propose changes to itself but cannot implement or self-accept them.

Track implementation checks, lead acceptance/integration, publication and consumption as evidence on the same issue, not as a new report hierarchy. New intended sessions must demonstrate the effective revision. If accepted changes apply to an active session, preserve the existing instruction-refresh succession contract at a safe boundary; do not count a file write or stale session catalog as delivery. For a correction needed immediately, the lead can use the existing addressed message with new relevant facts while durable publication proceeds.

Concrete defects use proportionate reproduction and native acceptance. Agent-choice and orchestration-benefit claims use relevant real-agent comparison and the existing evaluation owners. This mode neither requires the complete sequential experiment controller for every tool correction nor bypasses it when its contract applies. Compare time to accepted result and recorded token usage separately, including observer context/output, no-finding batches, unsuccessful investigation, implementation, lead intervention, checking, coordination, rework and displaced delivery. Use scoped existing usage reports with coverage warnings, not model inspection of raw sessions. A speed gain can cost more tokens; long-term payback needs observed relevant reuse rather than hypothetical unlimited repetition. Preserve adverse and inconclusive results.

### 7. Recover the loop through existing owners

Use the current run/lead lifecycle to preserve one observer binding across stop, restart and lead succession. If the lead exits without a successor, no new observer turn starts. If a model response or board write has an unknown outcome, reconcile native completion and existing episode-linked records before retrying; do not restart analysis or dispatch solely because a wait expired.

Stopping improvement mode stops new analysis and admissions/assignments under that mandate. It preserves pending evidence and decisions, and does not discard an already assigned executor's work. User stop scope determines whether that executor continues or follows ordinary stop/recovery. On later reactivation, verify task applicability, actual model binding, role revision and retained cursor; a changed model binding requires the existing fresh-conversation handoff, not resuming an incompatible saved thread.

## Risks / Trade-offs

- Observer overhead exceeds useful savings: bound cumulative work and carried context, coalesce full text coverage, back off after no-finding analyses, expose pending input and measure both useful and no-benefit cases. All-text coverage has a nonzero cost; savings are not guaranteed.
- Important facts occur only in excluded execution detail: state the coverage limit and retrieve attributable code/check/tool evidence for a concrete question, without silently adding exhaustive review or heuristic event filtering.
- Code or guidance changes after review: anchor findings to assignment bases, reviewed revisions, exact feedback and recorded consumption; preserve gaps instead of blaming an executor from current worktree contents.
- Better task intake still leaves work unassigned: make the next allocation obligation explicit and exercise it with a real occupied-then-freed slot; an observer-generated promise is not execution evidence.
- A lead or observer generalizes an incorrect correction: compare original acceptance and results, retain causal uncertainty and verify through the actual affected path.
- Event loss or replay after interruption: retain identifiable inputs until acknowledgment, reconcile existing board effects and expose coverage gaps instead of claiming exactly-once external execution.
- Third-role context reintroduces lead instructions or mutable shared configuration: extend the single resolver, verify native catalogs and effective source identities, and preserve per-session selection.
- Different consumer sessions compete for shared kit changes: use existing board ownership, committed source bases and serialized installation; never overwrite another active session's state.
- An unmanaged lead lacks a live delivery route: prove supported attachment or safe succession early; complete the owner gap before claiming global readiness rather than weakening to periodic manual checks.

## Migration Plan

1. Integrate the applicable role resolver from `separate-lead-executor-contexts`, coordinating the shared admission/allocation boundary with `unify-reusable-worktree-pool`. Keep each prerequisite's outstanding acceptance visible. Confirm the exact installed main-session observation and notification contract using owned inputs.
2. Deliver the smallest complete path through the installed skill, observer, evidence intake, board decision, actual executor assignment and consumed fix in an independent owned consumer. This is an early vertical slice, not completion of the remaining requirements.
3. Extend the existing event, feedback and session tests for complete text inclusion, trace exclusion, incremental board context, attributed code feedback, finite costs, no-finding pacing, duplicates, interruption, visibility loss, capacity saturation, lead succession, malformed/stale scope and private-public boundaries. All maintained test helpers remain Rust.
4. Publish checked role/skill/runtime assets through existing install/update/check/recovery. Verify new sessions and preserve existing active bindings and partial work. Unrelated configuration and ordinary feedback behavior remain unchanged when the mode is off.
5. Complete real-agent acceptance and the frozen matched benefit assessment above. Record supported, adverse or inconclusive findings in existing owners; do not claim speed from prompt size or card counts. An unproven observer remains outside accepted delivery; a scoped rollback removes its introduced active/source effects through the owning lifecycle, verifies restored behavior and retains evidence and the inactive patch. Reconcile operation guides and close only executed tasks.
