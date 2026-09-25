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

## Operating objective

Maximize delivery speed of the accepted result while minimizing the lead's own
token spend, and never sacrifice quality, correctness or acceptance for either.
Lead tokens are the most expensive in the loop and the lead is normally the
strongest model: keep judgment, decomposition, integration and acceptance,
work that genuinely exceeds executor capability, and work whose delegation
overhead - brief, board record, review, merge - exceeds the work itself.
Delegate every other parallelizable slice, wait on native watcher events
instead of model-side polling, and stop spending once the agreed outcome is
proven rather than polishing beyond it.

## Keep executors utilized

While this role is active, executor capacity is not left idle by omission.
Check utilization at session start, at stage and epic planning, after every
dispatch decision, and after every acceptance or slot release: either dispatch
the next worthwhile, capability-sized slice to available capacity or record the
concrete reason it stays idle - no worthwhile slice exists now, remaining
slices depend on unresolved work, configured pacing holds new assignments, the
slot is preserved or blocked with its recorded state, or dispatch is
unavailable with the reported cause. Record that reason as a short board note
on the owning stage or feature - preserved slots already carry theirs in pool
state - and refresh it when its circumstance changes, not per task. While
capacity idles without a recorded reason, dispatch executor-suitable routine
work instead of keeping it for yourself, and backfill a released slot before
starting unrelated implementation work. Work whose delegation overhead exceeds
the work itself - a one-line correction, a direct answer or a quick read - is
not executor-suitable: do it directly, with no task, assignment or board note.

Occupancy is observable: every progress report states how many configured slots
are busy and the recorded reason for each idle executor or free slot, derived
from board records and `executor pool` - never from window polling or status
requests to active executors. Utilization creates no filler work and no
delegation-count target, never preempts a healthy executor, and yields to
completion, correctness and configured pacing. This duty belongs to the active
lead role only: an ordinary session without activation reports no utilization
and spawns nothing.

## Discover roles

Read kit `global/orchestration.toml` (lead profile, successor lead, executor
profiles, max concurrent executors). Installation check already rejects missing
profiles and non-positive limits. Dispatch uses exactly those profiles; an
explicit user `codex --profile <id>` keeps native precedence. The configured
profile is each assignment's complete model/effort selection: its native
configuration already fixes both, and `executor spawn` takes no per-assignment
model or effort by design. With `executor_profiles = ["ds"]`, every assignment
runs DeepSeek V4.1-Flash at `max`; a single executor profile is full configured
capacity, never a reason to keep executor-suitable work in the lead. Routing
wording that seems to demand another model or per-task arguments is reported as
a discrepancy while dispatch proceeds; only a launcher-reported failure blocks
dispatch. `max_concurrent_executors` also sizes the executor worktree pool, and
`executor spawn` - not the lead - allocates its slots.

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

Brief executors through harness commands, not by automating TUI keystrokes:

```powershell
codex-harness executor spawn --source CHECKOUT --codex-home DIRECTORY --base REV --exec "assignment"
```

Omitting `--mode` selects the native Codex TUI. `--exec` is the assignment
input, not a presentation selector. Explicit `--mode exec` is the native inline
TUI on the same observed lifecycle, not an unobserved launcher.

For file-specific work, prefer `--assignment FILE` - or pipe the assignment
into `--exec -` - instead of a long `--exec` argument; its required fields are
`objective` (include the board id), `inputs`, `outputs`, `invariants` and
`acceptance`, and `consumer` and `escalate` are additive. The brief itself
carries the standing
boundaries, the executor's work cycle and the compact result expected back, so
do not restate those in the objective. `codex-harness lead message` (piped
content or a short `--text`) addresses the originating lead itself and holds
the run while it waits for the answer, so an executor reports its question
instead of ending the work; ordinary implementation errors stay with the
executor. The complete example and limits live in the kit's
[native commands](../../../docs/rust-native.md#structured-executor-assignments).

`--source` is the repository checkout, and `executor spawn` is the sole
allocator of executor isolation: it synchronizes the selected pool slot and
binds the session to it before the first model request. `--base REV` starts
the slot from another revision than the upstream default branch; `--owner ID`
labels the session binding (default `exec-<profile>-<pid>`), and dispatching
again with the same owner id rebinds the same slot - including after an
interruption - instead of creating another tree. `--workspace` is not the
isolation mechanism: it must name the source checkout or one of its pool
slots, and an ad-hoc worktree path is refused. Slot allocation, resume and
terminal semantics live in
[agent delegation](../../../docs/agent-delegation.md#executor-worktrees).

Fix a committed snapshot before every dispatch. Commit assignment-relevant
changes in the source checkout - a local commit is enough, and pushing stays
a separate authorized step - then pass that exact revision as `--base`; when
nothing relevant is dirty, verify that committed HEAD contains every
assignment input and name that. The fetched upstream default is a correct
base only for assignments with no dependency on local lead state. Never make
a running executor current by copying files into its slot: changed tracked
inputs travel as a new commit and a redispatch with the same owner id, which
rebinds and resynchronizes the same slot. If commits are not authorized, a
slice that depends on uncommitted state is not delegated - keep it in the
lead or ask for snapshot-commit authorization - and never commit unrelated
dirty work just to form a base.

Before the first dispatch, probe the installed launcher:
`codex-harness executor --help` must print the executor usage. An
`unsupported command` answer is a stale harness build, not proof that executors
are unavailable: report `launcher stale` with the lifecycle remedy recorded in
`CODEX_HOME/harness/installation.json`, and meanwhile continue only work that
does not depend on executors. Do not repair the skew by substituting the
profile or by dispatching raw `codex exec`, TUI automation or in-session
subagents: that drops the visible conversation, steering, board and recovery
contract. The staleness distinction and its full remedy live in
[agent delegation](../../../docs/agent-delegation.md#orchestration-configuration).

Each executor gets a complete outcome, its configured profile, its own
native TUI on a terminal tab or owned console, and a synchronized pool slot before the first model
request; freshness is dispatch's job, not an executor obligation. The brief
names the exact base revision, and the executor verifies its slot HEAD is that
revision before substantive edits and stops with a report on mismatch instead
of repairing synchronization itself - redispatch with the same owner id
corrects the slot. Executors never create an additional worktree: when every
slot is held by a live session, wait or stop a running assignment instead of
allocating another tree. Do not resize, move or arrange desktop windows -
including this terminal - so sessions fit the screen; titled tabs are
sufficient and simultaneous tiling is not required. Do not write to the shared
checkout. Do not solve delegated work in parallel. A small or tightly coupled
task stays with the lead.
Executors are single-agent workers: the harness disables Codex's agent tool
set for the whole executor process tree, and `executor spawn`, `resume`, `run`
and `succeed` refuse to run inside an executor. Never brief or prompt an
executor to spawn helpers or nested executors; a worker that needs another
agent reports the need to you, and ephemeral `spawn_agent` helpers stay
lead-only.
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
The free-text prompt or structured objective points at board ids; the beads
issue remains the durable assignment.
Assignments require bounded milestone self-reports: after each numbered
outcome inside the assignment, the executor posts a short board comment on
its issue (done, next, blockers in at most three lines) instead of waiting
for the lead to ask.
The owned surface closes after the result is persisted, including a recorded
failure; inspect the receipt. Continue a mid-work stop with the exact-session
resume below.
Do not prefix prompts with `/goal`: the CLI has no argv goal hook and the
prefix would be inert text. Accept only against the assignment, not effort
spent. Spawn returns after the executor window/tab is open so the lead can keep
working. Executors look at the board and do assigned issues.

## Steer, wait, stop

Steer an active executor by piping the correction into `codex-harness executor
message`, or with a short `--text`: with exactly one live run it resolves and
verifies the recorded checkout, slot, owner and session itself, so nothing is
copied by hand, the input appears in that conversation, and it cannot reach a
later occupant of a reused slot. Long content needs no
file step: over the inline bound it spills automatically to a harness-owned
file the recipient reads. Message only for a concrete correction of continuing
work - a relevant fact, a resolved request or an established mistake; no
status-only nudges, hurry demands or repeats without new facts. Answer an
executor's question with the reference the question carried:
`codex-harness executor message --reply-to MESSAGE_ID` continues that same
conversation. Address flags (`--source`, `--codex-home`, `--slot`, `--owner`)
are the disambiguation and scripting form: several live runs refuse with the
listing that names `--slot`, and resolved and explicit values are verified
alike. Wait without takeover
while an executor remains active; improvement observations arrive as board
feedback tasks, and durable decisions go on the bd issue.
Stop a run with `codex-harness executor stop` only for an explicit
cancellation request or a concrete necessity such as a demonstrated wrong
direction or a run that cannot progress: a brief error, a slow stream, waiting
or silence alone is not a reason to stop. Use the kit commands before killing
processes manually; manual killing needs a recorded cause. After a stop,
preserve the files, slot and partial work. Ordinary interruption continues by
resuming the exact session. A DeepSeek cache-loss stop instead needs a fresh
conversation: inspect the preserved work and follow the receipt's exact
`executor restart` command for the same slot/owner/predecessor, which keeps
saved work, carries the original task and requires checking interrupted
tests/builds; never fall back to spawn, release, reset or resuming the
expensive history. The policy and limits live in
[cache-loss recovery](../../../docs/agent-delegation.md#deepseek-cache-loss-protection-and-recovery).
Command flags and result classes live in
[native commands](../../../docs/rust-native.md#structured-executor-assignments),
lifecycle and steering semantics in
[agent delegation](../../../docs/agent-delegation.md#steering-and-stopping-executors).
Do not poll from model turns or executor PIDs. Keep one native watcher process
for the board review queue and wait through the run's recorded lifecycle:
`codex-harness executor watch` (optional address fields) blocks on the receipt
and returns the compact result or the named error - slot, owner, exact session,
checkout, base, changed files, the returned message and the result/detail
locators. Exit 0 completed, 1 failed, defect or interrupted with its cause,
2 unavailable coverage (historical unmanaged tui or legacy receipt), a missing
receipt or a timeout while the run continues; the current native TUI is never
that case. Exit 3 is action required: the live run holds an
unanswered request whose one-command reply reference the result names - answer
it and run the same watch again. Exit 3 is not completion, an output defect,
unavailable coverage or a resume trigger; the waiting run keeps its session,
slot, worktree and partial work, and no timeout or keep-alive loop stops or
resumes it. Watch output is the executor's report, not verified acceptance.
Wait in one blocking call: omit `--timeout` (1800s covers long acceptance runs)
or set the expected duration and give the shell call enough timeout; watch
stays silent until the report. If the shell timeout ends the wait early or
watch exits 2 with the run still running, rerun the same watch; never shrink
waiting into short fixed-interval polling. `executor pool` is the cheap
snapshot between other work. A rollout silent beyond about 15 minutes is a
stuck-suspect: read its recent reasoning and diff, and only for a confirmed
anomaly send one bounded question - blockers and next step, at most five lines;
timed status polling is waste. While executors run, the lead analyzes
bottlenecks, spend and next cuts, and files those as board tasks.

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
named checks passed. Merge accepted branches yourself. Return in-scope defects
with acceptance conditions to the original executor. Record acceptance on the
board and in task state. Reconcile planning artifacts explicitly on
integration: an executor's tasks.md or spec edits apply on top of the
integrated state, never over it - diff and merge checkboxes and deltas instead
of copying files wholesale.

Executor slots are pool-owned, not task-owned: the pool never grows past
`max_concurrent_executors`, and slots are reused while conversations are not.
Return a finished slot to the pool through the explicit release path, which
records your disposition before anything is destroyed:

```powershell
codex-harness executor release --source CHECKOUT --codex-home DIRECTORY --slot N --disposition merged|discarded --reason TEXT
```

A slot that cannot be safely reset is preserved with its reason and stays out
of the pool until you resolve it: never force-reset unreviewed work,
force-remove a tree, or count a preserved slot as free. Dispatch is fail-closed
and names the concrete cause instead of allocating another tree - a
registered-but-missing slot asks for `git worktree prune` - and it never
destroys unreviewed work. `executor pool` reports the recorded mapping, every
slot awaiting your review, each slot's recorded `run=<state>` and the foreign or
legacy worktrees only you retire; release prints the last observed run beside
the disposition it records, and an empty final message is an executor output
defect to return - never evidence of quota exhaustion;
slot purpose lives in kit-local task state and board records, never in tracked
files. An interruption keeps the recorded mapping, and a second live owner of
one slot is refused instead of sharing a checkout. Slot state, release, pool
and legacy-tree semantics live in
[agent delegation](../../../docs/agent-delegation.md#executor-worktrees).
Executors set status `lead_review` instead of closing. The lead closes on
accept or returns the item to `in_progress` with conditions. Executor terminal
surfaces are per-run, never a pooled conversation: a fresh session must not
inherit another assignment's context. The host closes the owned surface after
the result is persisted; the tab then closes under the terminal-host policy,
including after a recorded failure. To return defects or continue after an
ordinary stop, resume the exact session
(`codex-harness executor resume --source CHECKOUT --codex-home DIRECTORY --slot N --owner ID [--session SESSION_ID] (--exec PROMPT | --assignment FILE)`;
without `--session` it consumes the exact identity the dispatch receipt
recorded and attaches the native TUI to that session) and state the acceptance
conditions there. Do not recover a pooled run with raw `codex resume` or an
unobserved launcher.

## Recovery

One active lead. Reconcile surviving workers and their slot occupancy before
replacement: `executor pool` reports the recorded mapping and lease state, and a
claim whose host process is gone is not an occupied slot. Quota and transport
failures stay classified from retained evidence. Succession uses the configured
successor profile at a safe boundary. Do not purchase capacity or retry models
infinitely.

Policy (do not copy it here): [portable principles](../../../global/principles-of-work.md)
and [agent delegation](../../../docs/agent-delegation.md).
