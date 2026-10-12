# Design

## Context

See [proposal.md](proposal.md) for motivation and scope. The change crosses session ownership, feedback admission, instruction delivery and installation, so a design is required.

Inspected foundations:

- `crates/codex-harness/src/executor_message.rs` owns addressed messages, delivery outcomes, retries and originating-lead routing. Its receipt entries establish identity and delivery status; they are not a sufficient source of inline message text. Spill payloads already have owned local storage.
- `crates/codex-harness/src/executor_control.rs` retains observed control records and exposes completed conversation items while suppressing transient token deltas. `executor_observation.rs` and the native receipt/watch path own lifecycle observation. These are reusable evidence and transport foundations, not proof that both roles already expose every required event to an observer.
- `crates/harness-core/src/board_feedback.rs` and `crates/codex-harness/src/feedback_cli.rs` own deterministic beads operations. `board-workflow` and `team-lead` own semantic triage and scheduling guidance. Current promotion does not promise an execution opportunity.
- `global/orchestration.toml` owns profiles, executor limits and feedback limits. Existing delegation and installation owners cover visible conversations, partial-work preservation and model binding. A managed lead endpoint supports reverse messaging; an unmanaged lead currently lacks that guarantee.
- `separate-lead-executor-contexts` proposes the common role resolver, `global/roles.toml`, role source files and native skill filtering; its implementation is not delivered by the planning artifacts. `unify-reusable-worktree-pool` separates checkout allocation from conversation launch. Their work and acceptance remain with those changes.

The existing feedback spec's incubator-only routing and lead-only semantic triage need explicit deltas. The new capability owns the optional observer and the extra scheduling obligation; ordinary lead/executor acceptance, pool safety and experiment gates remain authoritative.

## Goals / Non-Goals

**Goals:** close one real correction-to-applied-improvement path early; give analysis a separate attention context; remove repeated work near its cause; make accepted improvements compete for actual execution opportunities; bound observation cost and preserve reliable recovery.

**Non-Goals:** another lead, a new scheduler that interprets the board, an always-running model loop, a universal transcript index, an automatic prompt optimizer, a new provider selection framework, or a replacement for the experimental `improve` controller. This mode does not promise improvements on every task or guaranteed savings.

## Decisions

### 1. Add one optional role and a small lifecycle entry point

Add `.agents/skills/delivery-improvement/SKILL.md` and `global/roles/improvement.md`. Extend the shared role descriptor/resolver from `separate-lead-executor-contexts`; do not copy the lead or executor prompt into the new role. Its skills cover investigation, project context, evidence and board intake. Delegation, implementation, integration and self-publication remain unavailable responsibilities. Source and instruction files are read-only to this role; scoped board updates and runtime acknowledgments are its permitted writes.

Use a small Rust-owned `codex-harness delivery-improvement start|status|stop|resume` adapter as the proposed user-facing lifecycle, implemented over the existing control host, terminal surfaces, receipts and configured profile resolution. These commands do not exist yet. `start` derives the current managed lead, authorized task and installed kit from verified records, creates one binding and launches the observer. Repeating it finds the existing binding. `status` is model-free and reports coverage, pending analysis/decision references, lifecycle and concrete blockers; board status is obtained through the existing board CLI, not duplicated in the host.

The skill invokes the adapter and the existing lead workflow rather than asking the user to coordinate endpoints or IDs. A native contract check must establish event access and addressed notification for the current lead, including a supported ordinary main-session activation path. Reuse existing managed-lead attachment or safe-boundary succession where needed, preserving the actual conversation and work. If the installed native contract cannot support that path, activation must give the cause and verified remedy; it must not silently launch a replacement lead, require an unannounced everyday restart ritual, or claim an operative loop. Closing that gap is implementation work in the current session owner, not a user responsibility or permission to fall back to manual polling.

A role is independent of provider/model. Resolve the observer through the configured executor-profile selection and record the effective binding. There is no new model profile, cheaper-model substitution or recursive model call. A title identifies the improvement role independently of its model.

### 2. Share capacity without permanently reserving an executor slot

An observer needs a conversation, not a source-writing worktree. Extend the existing admission owner to count a running observer turn alongside live implementation assignments within the configured concurrent-work ceiling. Its idle event wait does not hold an implementation slot or acquire a worktree; every new turn must reacquire admission. Do not lower the executor limit merely to reserve an always-idle observer, and do not run its model work as an invisible extra beyond the ceiling.

When all capacity is occupied, events remain pending. At the next ordinary allocation boundary, pending analysis participates in allocation before lower-priority new work; healthy executors are never preempted. The practical trade-off is explicit: under saturation analysis can wait for capacity. A substantive urgent signal can notify the lead through the existing event mechanism without spawning another hidden model. Provider refusal or lost visibility preserves pending inputs and blocks dependent model work with the existing recovery rules.

The first version allows one ordinary improvement implementation at a time. This is a work-in-progress limit within existing capacity, not a promise of a permanently reserved lane, a project-count limit or a deadline. Urgent defect handling remains the existing consequence path. If shared admission cannot be separated from checkout allocation without the pool change, integrate that prerequisite first rather than inventing another pool.

### 3. Extend existing event observation with bounded incremental consumption

Bind observation to the lead identity, its task and its recorded executor lineage. Relevant inputs are completed visible messages and tool results, assignments, correction replies, errors, verification outcomes and lifecycle changes. Subscribe through the existing managed session owners; do not scrape terminal windows, decode reasoning or recursively read every session log.

Use native event/message IDs and run generations to correlate retries and reused slots. Keep a small observation cursor, pending batch reference and active analysis turn in the existing private run receipt/state. Beads remains the task and decision owner. Retain necessary message text or pin its existing payload/evidence locator through its current lifecycle before acknowledging it; a delivery hash is not text, and a rotated evidence source must produce an explicit coverage gap. Do not put transcripts or private locators into public kit records.

Model-free collection coalesces events while one analysis turn is running. Routine new-event analysis obeys the existing supervision cadence; delivered corrections, actual errors, help requests and other permitted substantive events can make a batch ready earlier. Reconciliation timers may notice appended or pending data, but an empty timer never starts a model turn. Keep actual command identity, result and attributable timing when available; elapsed silence is not evidence of a stall.

The observer receives a bounded batch with source references and can request a targeted relevant excerpt. It acknowledges only retained outcomes: board findings or a compact no-finding result associated with the consumed batch. Oversized input is split with a continuation cursor; missing detail, unavailable boards or uncertain model outcomes do not silently advance past unfinished evidence. Avoid new semantic similarity services: the observer proposes grouping; existing feedback commands apply it deterministically.

### 4. Diagnose the owning cause and reuse the board

For each candidate, compare the observed correction with the assignment, source revision, applicable instructions and actual result. Distinguish a new user requirement, faulty lead brief, executor mistake, tool failure, unavailable infrastructure and an unclear cause. A proposed global rule must not be learned merely by copying a lead's accusation.

Use the existing item fields and description for evidence reference, diagnosed cause, affected owner, next relevant use, smallest remedy and acceptance. Inspect existing open, deferred and completed items. An observer's summary of another agent's episode is derived evidence, not another vote. Reconsider a rejected basis only with new evidence or changed conditions.

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

Concrete defects use proportionate reproduction and native acceptance. Agent-choice and orchestration-benefit claims use relevant real-agent comparison and the existing evaluation owners. This mode neither requires the complete sequential experiment controller for every tool correction nor bypasses it when its contract applies. The benefit assessment counts observer work, lead intervention, checking, coordination and rework as well as implementation time.

### 7. Recover the loop through existing owners

Use the current run/lead lifecycle to preserve one observer binding across stop, restart and lead succession. If the lead exits without a successor, no new observer turn starts. If a model response or board write has an unknown outcome, reconcile native completion and existing episode-linked records before retrying; do not restart analysis or dispatch solely because a wait expired.

Stopping improvement mode stops new analysis and admissions/assignments under that mandate. It preserves pending evidence and decisions, and does not discard an already assigned executor's work. User stop scope determines whether that executor continues or follows ordinary stop/recovery. On later reactivation, verify task applicability, actual model binding, role revision and retained cursor; a changed model binding requires the existing fresh-conversation handoff, not resuming an incompatible saved thread.

## Risks / Trade-offs

- Observer overhead exceeds useful savings: bound event inputs, coalesce batches, perform no idle model turns, permit no-finding outcomes and measure the complete cost on relevant work.
- Better task intake still leaves work unassigned: make the next allocation obligation explicit and exercise it with a real occupied-then-freed slot; an observer-generated promise is not execution evidence.
- A lead or observer generalizes an incorrect correction: compare original acceptance and results, retain causal uncertainty and verify through the actual affected path.
- Event loss or replay after interruption: retain identifiable inputs until acknowledgment, reconcile existing board effects and expose coverage gaps instead of claiming exactly-once external execution.
- Third-role context reintroduces lead instructions or mutable shared configuration: extend the single resolver, verify native catalogs and effective source identities, and preserve per-session selection.
- Different consumer sessions compete for shared kit changes: use existing board ownership, committed source bases and serialized installation; never overwrite another active session's state.
- An unmanaged lead lacks a live delivery route: prove supported attachment or safe succession early; complete the owner gap before claiming global readiness rather than weakening to periodic manual checks.

## Migration Plan

1. Integrate the applicable role resolver from `separate-lead-executor-contexts`, coordinating the shared admission/allocation boundary with `unify-reusable-worktree-pool`. Keep each prerequisite's outstanding acceptance visible. Confirm the exact installed main-session observation and notification contract using owned inputs.
2. Deliver the smallest complete path through the installed skill, observer, evidence intake, board decision, actual executor assignment and consumed fix in an independent owned consumer. This is an early vertical slice, not completion of the remaining requirements.
3. Extend the existing event, feedback and session tests for bounded batches, duplicates, interruption, visibility loss, capacity saturation, lead succession, malformed/stale scope and private-public boundaries. All maintained test helpers remain Rust.
4. Publish checked role/skill/runtime assets through existing install/update/check/recovery. Verify new sessions and preserve existing active bindings and partial work. Unrelated configuration and ordinary feedback behavior remain unchanged when the mode is off.
5. Complete real-agent acceptance and a proportionate matched benefit assessment. Record supported, adverse or inconclusive findings in existing owners; do not claim speed from prompt size or card counts. Reconcile operation guides and close only executed tasks. Rollback disables new observer activation through the lifecycle and restores prior owned assets while keeping boards, evidence and unfinished work.
