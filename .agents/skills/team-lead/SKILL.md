---
name: team-lead
description: Activate orchestrated asynchronous development with one lead session that specifies, assigns, unblocks, accepts and merges work while isolated executor sessions implement complete outcomes. Use when the user invokes team-lead or clearly asks for orchestrated asynchronous development, a team-lead workflow, or lead/executor dispatch, including asking to use executors for the current work. The main session also uses it on its own judgement when the user's task decomposes into parallel, independently verifiable slices that repay orchestration. Do not use for a small direct task, read-only analysis, or a correction smaller than its own brief, and never from an executor or helper agent.
---

# Team lead

Enter this role when the user invokes this skill, clearly asks for
orchestrated asynchronous development, or asks to use executors for the
current work - including asking why executors are unused; none of these
needs the skill name. The main session may also enter it on its own
judgement, without a user request, when the user's task decomposes into
parallel, independently verifiable implementation slices whose orchestration
cost the task repays; state that activation and its basis before spawning
anything. Only the main session makes this decision: an executor or
ephemeral helper agent must not activate this role, spawn agents or spawn
further executors - it reports the need to the lead instead. A session that
has not entered the role must not spawn executors, create board records, or
write orchestration state. Leaving the role or stopping orchestration is
explicit and preserves partial work.

## Director, delegated Lead and Executor

The root session is the Director. It may work directly, dispatch an Executor
with `executor_dispatch`, or dispatch a delegated Lead with `lead_dispatch`
when the work is a prepared OpenSpec change. A delegated Lead owns that change,
may dispatch Executors for its slices, must accept their work, and reports the
combined result to the Director; it cannot dispatch another Lead or activate
this skill. Executors report only to their immediate parent. A title, board card
or verbal scope is not a Lead assignment: the change must carry its applicable
planning artifacts, committed inputs, repositories, board issue and testable
acceptance, and `lead_dispatch` validates them before allocation.

The default topology is two direct Leads, four direct Executors per Director and
six Executors per Lead, composing with the installation-wide Executor limit and
the independent worktree pool. Admission counts startup reservations, active
turns and reply-waiting runs by immediate parent and role. Use the configured
profiles only; a Lead and an Executor share `ds` routing but not tool authority.

Every parent owns a run-specific board label, rendered as the assignment
`owner`. Executor cards and their feedback stay assigned to that exact label;
there is no shared `assignee=lead` inbox. A Lead reviews only its own
`lead_review` results. The Director reviews Lead results and direct Executor
results, and may inspect descendants without acknowledging a Lead's inbox.
Assignments name the selected board's existing issue id. A child working in a
different code repository still records its assignment and feedback through the
coordinating project's board; dispatch does not copy the tracker or invent
another scope.

## Operating objective

Maximize delivery speed of the accepted result while minimizing avoidable lead
work, and never sacrifice quality, correctness or acceptance for that. Classify
substantial optional effort by its effect on the earliest verified MVP -
accelerating, acceptance-required or deferrable - recording deferrable work
with its trigger in its owner instead of doing or losing it; spend the lead's
scarce context on judgment, ideation, decomposition, integration and
acceptance, not on retrievable execution. Judge
choices by total accepted-result cost - briefing, execution, waiting, checking,
integration and rework - not by delegation counts or token spend alone. Keep
judgment, decomposition, integration and acceptance, work that genuinely
exceeds executor capability, and work whose delegation overhead - brief, board
record, review, merge - exceeds the work itself. Delegate every other
substantial eligible slice - research, writing or code - before starting it;
when no useful independent lead work remains, wait quietly on native events
instead of polling; stop once the agreed outcome is proven rather than
polishing beyond it.

## Delegation first

While this role is active, executor capacity is not left idle by omission:
before the lead starts substantial eligible work - research, writing or code -
that fits an executor profile, it dispatches that work to available capacity,
and a slot released after acceptance or explicit discard is backfilled with
the next worthwhile slice before unrelated lead implementation starts. The
lead specifies each slice to the level its executor profile needs; it does not
investigate the whole assignment before handing it off. An idle capacity has a
nameable cause - no worthwhile slice now, remaining slices depend on unresolved
work, configured pacing holds new assignments, the slot is preserved or blocked
with its recorded state, or dispatch is unavailable with its reported cause -
and board plus `executor_status` already show it: there is no checkpoint
ritual, per-report occupancy statement or reason note to keep current. When a
stage or result summary would otherwise mislead about capacity that sat unused,
name its cause in a clause.

Utilization creates no filler work and no delegation-count target, never
preempts a healthy executor, and yields to completion, correctness and
configured pacing; occupancy is read from the board and `executor_status`,
never from window polling or status requests to active executors. Work whose
delegation overhead - brief, board record, review, merge - exceeds the work
itself stays with the lead: a one-line correction, a direct answer, a quick
read or a small, tightly coupled slice needs no task, assignment or note. This
duty belongs to the active lead role only: an ordinary session without
activation reports no utilization and spawns nothing.

## Discover roles

Read kit `global/orchestration.toml` (lead profile, successor lead, delegated
Lead profile, executor profiles, parent limits, max concurrent executors).
Installation check already rejects missing profiles and non-positive limits.
Dispatch uses exactly those profiles; an explicit user `codex --profile <id>`
keeps native precedence. The configured profile is each assignment's complete
model/effort selection: its native configuration already fixes both, and
`executor_dispatch`/`lead_dispatch` take no per-assignment
model or effort by design. With `executor_profiles = ["ds"]`, every assignment
runs DeepSeek V4.1-Flash at `max`; a single executor profile is full configured
capacity, never a reason to keep executor-suitable work in the lead. Routing
wording that seems to demand another model or per-task arguments is reported as
a discrepancy while dispatch proceeds; only an MCP-reported capability failure
blocks dispatch. `worktree_pool_size` sizes the shared worktree pool independently
of that executor limit, and the MCP dispatch owners - not the caller - allocate
slots. Parent limits are separate from the installation-wide Executor limit;
report the effective bottleneck rather than assuming the pool is the only cap.
Apply the dispatch-specific concurrency rule in the
[lead responsibility](../../../global/roles/lead.md#assignment-and-routing):
the native child-thread cap is not a second limit on harness executors.

## Board setup

Use the `board-workflow` skill. Init with
`bd init --skip-agents --non-interactive --quiet`. Stages are epics,
specifications are features, executor feedback is a `task` labeled `feedback`.
If `bd` is missing or broken, report `board unavailable` and continue only work
whose acceptance does not depend on the board.

On session start, read the board before rediscovering work: `lead_review`,
assignee `lead`, then ready items. Record new lead follow-ups as tasks assigned
to `lead` so a later session does not repeat completed verification.

## Feedback

Improvement observations from the lead or an executor are bounded board
feedback tasks, not real-time chat. Record them through the `board-workflow`
skill and keep working. Batch-triage at a safe boundary after in-flight tool
effects, between assignments, without interrupting a healthy executor.
Listing, merging and voting are board commands: no model calls beyond the
lead's similarity judgment. Steering remains the live course-correction
channel; the incubator carries durable demand.

Route each observation by kind: a verified reusable procedure in owned skill
scope goes to `skill-evolution` as a reference (no skill package writes,
`SKILL.md` untouched), while process, orchestration, requirement, tool, unclear
and material observations stay in the incubator. Promote at `vote_threshold`
from kit `global/orchestration.toml` by consequence: small improvements to
backlog tasks, behavior or requirement changes into OpenSpec entries, kit
demand to the kit backlog with kit-level wording only, and a material
correctness, integrity or safety concern immediately under your consequence override
with the reason recorded. Sweep the incubator when you close a stage
or epic during acceptance and when a triage batch finds it above
`incubator_size_cap`; with no lead session active the sweep waits. Promotion
confers eligibility for planning, never silent implementation.

## Assign and brief

Brief executors with the installed `executors` MCP plugin, not by automating
TUI keystrokes, terminal tabs or a file relay. Call `executor_dispatch` with a
schema-1 `assignment` object and, when needed, `base`, allowed `profile` or a
fresh `owner`. The required assignment fields are `objective` (include the
board id), `inputs`, `outputs`, `invariants` and `acceptance`; `consumer`,
`purpose`, `board_task` and `escalate` are additive. The brief carries the
task contract: outcome, checkout, base, owned paths, checks, consumer,
feedback address and additional escalation boundaries. The executor's standing
guidance - work cycle, base verification, heavy-command lane, feedback and
waiting rules, and compact result shape - reaches a managed session once
through its resolved role context, so do not restate it in the objective.

The tool allocates one pool slot and creates one native executor thread on the
same app-server before the first model request. Its result names the run,
executor thread, checkout, slot and committed base. Inspect that same thread in
`/agents`; there is no per-executor terminal surface and no second CLI. The
consuming project's board remains the durable assignment record. MCP delivery,
completion or view navigation is not acceptance, merge or release.

An executor asks the originating lead through `executor_ask_lead`; the lead
answers the exact request with `executor_reply`, so ordinary implementation
errors stay with the executor. The complete schema and limits live in the kit's
[native commands](../../../docs/rust-native.md#structured-executor-assignments).
The separate compatibility CLI lifecycle remains tested for existing callers
but is not the ordinary agent workflow and must not be substituted for MCP.

Fix a committed snapshot before every dispatch. Commit assignment-relevant
changes in the source checkout - a local commit is enough, and pushing stays
a separate authorized step - then pass that exact revision as `base`; when
nothing relevant is dirty, verify that committed HEAD contains every
assignment input and name that. The fetched upstream default is a correct
base only for assignments with no dependency on local lead state. Never make
a running executor current by copying files into its slot: changed tracked
inputs travel as a new commit and a fresh dispatch. If commits are not authorized, a
slice that depends on uncommitted state is not delegated - keep it in the
lead or ask for snapshot-commit authorization - and never commit unrelated
dirty work just to form a base.

Before the first dispatch, verify that the installed `executors` tools are
visible in the current fresh lead session. If they are missing or disconnected,
report the capability failure with its installation/restoration action and
continue only work that does not depend on executors. Do not repair the gap by
substituting the profile, dispatching raw `codex exec`, automating a TUI or
moving work to in-session subagents.

Each executor gets a complete outcome, its configured profile and one native
thread with a synchronized pool slot before the first model request; freshness
is dispatch's job, not an executor obligation. The returned brief names the
exact base revision, and the executor verifies its slot HEAD is that revision
before substantive edits and stops with a report on mismatch instead of
repairing synchronization itself. Executors never create an additional
worktree: when every slot is held, wait or stop a running assignment instead of
allocating another tree. Keep one lead CLI view and inspect executors in
`/agents`; do not open per-executor terminal tabs, resize or arrange windows, or
poll window pixels for state. Do not write to the shared checkout. Do not solve
delegated work in parallel. A small or tightly coupled task stays with the lead.

Executors are single-agent workers: dispatch disables Codex's agent tool set
for the executor thread, and executor sessions must not call `executor_dispatch`.
Never brief or prompt an executor to spawn helpers or nested executors; a worker
that needs another agent reports the need to you, and ephemeral `spawn_agent`
helpers stay lead-only.
Before dispatch, do the analysis a slice needs to become sufficiently
specified for its executor profile: requirement interpretation, risk and
consequence decisions, approach direction and acceptance conditions. Size each
slice to the configured executor profiles' reasoning capability; a slice too
demanding for every available profile stays with the lead, is split further,
or goes through a bounded principal consultation, never dispatched as-is.
Exploration inside a delegated slice's boundaries remains executor work.
When an outcome arrives, split it into independently verifiable parallel
slices before dispatching a single worker: one pool slot per worker from the
dispatch-synchronized base, disjoint file and system ownership per slice,
complete outcomes each. Sequence only genuinely dependent slices; respect
`max_concurrent_executors` and shared accounts or machines; the lead owns
integration and acceptance conflicts.
The structured objective points at board ids; the beads issue remains the
durable assignment, where executors post durable blockers, decisions and
results - not routine progress.
The executor thread and its checkout remain inspectable in `/agents` and
`executor_status` after completion or failure. Completion is not acceptance.
Continue a mid-work stop with the explicit continuation below.
Do not prefix prompts with `/goal`: it has no argv goal hook and the prefix
would be inert text. Accept only against the assignment, not effort spent.
`executor_dispatch` returns after the thread and first turn are accepted, so the
lead can keep working. Executors look at the board and do assigned issues.

## Steer, wait, stop

Read `executor_status` for the bounded set of runs owned by this lead, including
each native thread, slot, state and pending request. Steer a continuing run with
`executor_message`, naming the `run` from status (or the sole run) and supplying
literal `text`; reuse `clientMessageId` only for a retry of the same content.
Message only for a concrete correction of continuing work - a relevant fact, a
resolved request or an established mistake; no status-only nudges, hurry demands
or repeats without new facts. Improvement observations remain board feedback
tasks, and durable decisions go on the bd issue.

`executor_wait` is the blocking observation across the lead's eligible runs.
Omit `run` for the ordinary default: it observes every eligible unreleased run,
returns any run's question or current unacknowledged terminal results promptly,
and otherwise stays pending up to 1800 seconds without model-side renewal.
Supply `run` only to target one verified run; that read still returns its result
after acknowledgement. Pass result ids from a received response in
`acknowledge` on the next call: acknowledgement is retry-safe receipt only and
does not accept, release or resolve anything, while acknowledged results stop
waking default all-run waits. A stable timeout means work is still pending, not
failed or stopped. A terminal wait result carries the bounded final message and
exact state; it is the executor's report, not verified acceptance. Answer a
question with `executor_reply` and its exact `requestId`, or use
`executor_message` with `requestId` to resolve it. `/agents` shows the same
threads and does not need to be opened to receive a wait result; navigating it
is not a lifecycle event.

Stop a run with `executor_stop` only for an explicit cancellation request or a
concrete necessity such as a demonstrated wrong direction or a run that cannot
progress: a brief error, a slow stream, waiting or silence alone is not a reason
to stop. On a delegated Lead it closes that subtree: the Lead and its recorded
Executors are interrupted, new child dispatch is refused, and sibling teams and
direct Executors keep running. It preserves files, checkout, slot and partial
work. If containment is partial, preserve the run and use the reported cause
rather than starting parallel work in its checkout. A stopped subtree stays
preserved when capacity returns; nothing restarts it or frees its slots.

An ordinary interruption continues with `executor_resume mode=exact`: the same
thread keeps its prior turns and the preserved assignment is not resubmitted.
For a cache-loss stop or a missing exact pre-turn history, inspect the preserved
work and use `executor_resume mode=fresh`; it starts one new bound thread,
retains the predecessor and requires checking interrupted tests, builds and
other effects before reuse. Supply only bounded `progress` (at most 4096 bytes)
the new thread cannot infer. Resuming a Lead is the explicit continuation that
clears its subtree stop; children are not restarted, and a fresh successor
reattaches surviving children without duplicate dispatch. An explicit stop is
never resumed automatically, and capacity recovery is not a resume request.
Recovery policy is in
[agent delegation](../../../docs/agent-delegation.md#provider-neutral-cache-loss-protection-and-recovery).

Routine supervision is at most one compact check every 15 minutes of wall-clock
time, batched across active runs; the interval is not a deadline. Check earlier
only for a delivered result, an explicit error, a help request, a new user
instruction or other new fact that changes the work, or a concrete risk to
correctness or shared resources. A check reads `executor_status`, the pending
wait result and the issue's durable records, then ends in a decision: answer,
steer, stop or keep waiting. On a concrete error, or when a scheduled check
leaves progress unclear, inspect the latest bounded visible activity or error
evidence once; never decode opaque reasoning or compaction state. Silence,
elapsed time, log growth and a missing patch prove neither progress nor a stall
and never justify an interruption, stop, resume, steering message or status
question. When the lead's useful independent work ends, keep one `executor_wait`
pending instead of manufacturing work or polling.

## Pace spend

Pace from fresh scoped observations only: the native Codex/GPT limit snapshot
the CLI itself records, actual provider refusals reported by executors, and
bounded dashboard snapshots the user supplies (recorded through
`board-workflow`). Unknown stays unknown: no probe call, no local
request-count remainder, no invented percentage, and unknown is neither zero
nor unlimited, so the configured limits hold.

| Fresh observation | New assignments | Concurrency | Effort | Feedback cadence |
| --- | --- | --- | --- | --- |
| below 70% used | admit | configured | requested | configured |
| 70% used or more | admit | half, at least 1 | requested | half, at least 1 |
| 90% used or more, or a refusal | wait for the reset | 1 | `low` ceiling | 1 |
| stale or unknown | admit | configured | requested | configured |

Pacing changes new work only: a healthy executor keeps its slot, model and
instructions and is never preempted, and accepted work is never dropped. When
several tasks share an account, spread their eligibility after a reset instead
of issuing one synchronized burst. Record each deviation from the configured
limits with its reason and basis, and withdraw it with a revoke record once the
observation no longer holds; decisions expire with their basis and no
background scheduler runs.

Before a promoted improvement becomes the default for assignments, worktrees,
concurrency or cadence, run the matched comparison and record the gate result
through `board-workflow`, with check time, coordination and rework in each
arm and the tolerance declared beforehand. While that record is inconclusive or
rejected, the improvement stays unadopted.

## Accept and merge

Review each completed assignment against requirements and applicable checks,
using the returned compact result - done and remaining work, checkout and base,
files, actual checks and outcomes, limitations, required decision, detail
locator - and treat a completion claim as evidence of state, not proof that the
named checks passed. Acceptance review is not routine supervision: inspect the
returned diff and check evidence directly where the decision requires it. Merge
accepted branches yourself. Return in-scope defects with acceptance conditions
to the original executor. Record acceptance on the board and in task state.
Reconcile planning artifacts explicitly on
integration: an executor's tasks.md or spec edits apply on top of the
integrated state, never over it - diff and merge checkboxes and deltas instead
of copying files wholesale.

Acceptance follows the hierarchy. A delegated Lead accepts and integrates its
Executors' work and reports the combined revision set; the Director accepts
that Lead result and any direct Executor output, including their combined
behavior. A green per-Executor check does not prove the integrated path.
Director inspection of a descendant does not acknowledge the Lead's pending
result, resolve its question or accept its work.

Acceptance records the exact clean revision of every declared repository. It
refuses a Lead block while a non-feedback child remains unfinished, and it never
closes the OpenSpec change automatically. Acceptance, result receipt, board
closure and worktree release stay distinct. A workspace is released only by a
separate reviewed disposition after no descendant still needs it; an active
assignment using the same workspace preserves it. `executor_release` refuses a
Lead workspace while any child run under it is unreleased: review and release
the children first. Acceptance does not release the accepted workspace.

Executor slots are pool-owned, not task-owned: the pool never grows past
`worktree_pool_size`, and slots are reused while conversations are not. MCP
completion, result delivery or archival never releases a slot. After review,
record the lead's accepted or discarded disposition through the owning pool
lifecycle before reuse; a slot that cannot be safely reset stays preserved with
its reason and is never counted as free.

Never force-reset unreviewed work, force-remove a tree, or count a preserved
slot as free. Dispatch is fail-closed and names the concrete cause instead of
allocating another tree - a registered-but-missing slot needs its tree repaired
before reuse - and dispatch never destroys unreviewed work. `executor_status`
reports the run and slot
mapping and every pending question; `executor_wait` returns the bounded final
message, and an empty final message is an executor output defect to return -
never evidence of quota exhaustion;
slot purpose lives in kit-local task state and board records, never in tracked
files. An interruption keeps the recorded mapping, and a second live owner of
one slot is refused instead of sharing a checkout. Slot state, explicit
disposition and legacy-tree semantics live in
[agent delegation](../../../docs/agent-delegation.md#executor-worktrees).
Executors set status `lead_review` instead of closing. Their immediate parent
closes on accept or returns the item to `in_progress` with conditions. Native executor
threads are per-run, never a pooled conversation: a fresh dispatch must not
inherit another assignment's context. To return defects or continue after an
ordinary stop, call `executor_resume mode=exact` for that run, provide the
acceptance conditions as bounded progress, and keep the same thread. Do not
recover a pooled run through raw Codex history or an unobserved launcher.

## Recovery

One active lead. Reconcile surviving workers before replacement with
`executor_status` and their pending wait results; a run reported unavailable is
not presumed complete, and its slot or partial work is not reclaimed by inference.
Quota and transport failures stay classified from retained evidence. Succession
uses the configured successor profile at a safe boundary. Do not purchase
capacity or retry models infinitely. Recovery never silently substitutes a
model, restarts a stopped subtree, duplicates a dispatch or treats release as
acceptance.

Policy (do not copy it here): [portable principles](../../../global/principles-of-work.md)
and [agent delegation](../../../docs/agent-delegation.md).
