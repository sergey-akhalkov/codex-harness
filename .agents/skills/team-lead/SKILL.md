---
name: team-lead
description: Activate orchestrated asynchronous development with one lead session that specifies, assigns, unblocks, accepts and merges work while isolated executor sessions implement complete outcomes. Use when the user invokes team-lead or clearly asks for orchestrated asynchronous development, a team-lead workflow, or lead/executor dispatch, including asking to use executors for the current work. Do not use for a small direct task.
---

# Team lead

Enter this role only when the user invokes this skill, clearly asks for
orchestrated asynchronous development, or asks to use executors for the
current work - including asking why executors are unused; none of these
needs the skill name. An ordinary session without that activation must not
spawn executors, create board records, or write orchestration state. Leaving
the role or stopping orchestration is explicit and preserves partial work.

## Operating objective

Maximize delivery speed of the verified result while minimizing token spend,
and never sacrifice quality, correctness or acceptance for either. In
practice: delegate bounded parallelizable work to executors, wait on native
watcher events instead of model-side polling, keep lead turns for judgment,
integration and acceptance, and stop spending once the agreed outcome is
proven rather than polishing beyond it.

## Keep executors utilized

While this role is active, executor capacity is not left idle by omission.
Check utilization at session start, at stage and epic planning, after every
dispatch decision, and after every acceptance or slot release. At each
checkpoint either dispatch the next worthwhile, capability-sized slice to
available capacity or record the concrete reason it stays idle: no
worthwhile slice exists now, remaining slices depend on unresolved work,
configured pacing holds new assignments, the slot is preserved or blocked
with its recorded state, or dispatch is unavailable with the reported cause.
Record the reason as a short board note on the owning stage or feature -
preserved slots already carry theirs in pool state - and refresh it when its
circumstance changes, not per task. While capacity idles without a recorded
reason, do not keep executor-suitable routine implementation work for
yourself: dispatch it or record why it stays with the lead before doing it.
After a slot is released, backfill it with the next dispatchable slice
before starting unrelated implementation work yourself.

Occupancy is observable: every progress report states how many configured
slots are busy and the recorded reason for each idle executor or free slot,
derived from board records and `executor pool` - never from window polling
or status requests to active executors. Utilization creates no manufactured
filler work and no delegation-count target, never preempts a healthy
executor, and yields to completion, correctness and configured pacing. This
duty belongs to the active lead role only: an ordinary session without
activation reports no utilization and spawns nothing.

## Discover roles

Read kit `global/orchestration.toml` (lead profile, successor lead, executor
profiles, max concurrent executors). Installation check already rejects missing
profiles and non-positive limits. Dispatch uses exactly those profiles.
Explicit user `codex --profile <id>` keeps native precedence. Do not substitute
another model. The configured profile is each assignment's complete
model/effort selection: its native configuration already fixes both,
`executor spawn` takes no per-assignment model or effort by design, and their
absence conflicts with no selection rule. With `executor_profiles = ["ds"]`,
every assignment runs DeepSeek V4.1-Flash at `max`; a single executor profile
is full configured capacity, never a reason to keep executor-suitable work in
the lead. Routing wording that seems to demand another model or per-task
arguments is reported as a discrepancy while dispatch proceeds; only a
launcher-reported failure blocks dispatch. `max_concurrent_executors` also
sizes the executor worktree pool, and `executor spawn` - not the lead -
allocates its slots.

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

Route each observation by kind before it competes as an incubator vote: a
verified reusable procedure in owned skill scope is handed to
`autonomous-skill-evolution` as a reference (no skill package writes, `SKILL.md`
untouched), while process, orchestration, requirement, tool, unclear and
material observations stay in the incubator. Promote at `vote_threshold` from
kit `global/orchestration.toml` by consequence: small improvements to backlog
tasks, behavior or requirement changes into OpenSpec entries, kit skill or
instruction demand to the kit backlog with kit-level wording only. A material
correctness, integrity or safety concern promotes immediately under your
consequence override with the reason recorded in history. Sweep the incubator
when you close a stage or epic during acceptance, and when a triage batch finds
it above `incubator_size_cap`; with no lead session active the sweep waits.
Promotion confers eligibility for planning, never silent implementation.

## Assign and brief

Brief executors through harness commands, not by automating TUI keystrokes:

```powershell
codex-harness executor spawn --source CHECKOUT --codex-home DIRECTORY --base REV --exec "assignment"
```

`--source` is the repository checkout, and `executor spawn` is the sole
allocator of executor isolation: it selects a free slot of the harness-owned
pool (sibling worktrees `<repository-name>-wt1` .. `-wtN`, where `N` is
`max_concurrent_executors`), creates that position on first use, synchronizes
the slot with upstream and binds the session to it before the first model
request. `--workspace` is optional and no longer the isolation mechanism: it
must name the source checkout or one of its pool slots, and an ad-hoc worktree
path is refused. `--base REV` starts the slot from another revision than the
fetched upstream default branch; `--owner ID` labels the session binding
(default `exec-<profile>-<pid>`), and dispatching again with the same owner id
rebinds the same slot - including after an interruption - instead of creating
another tree.
An interrupted session continues with `codex-harness executor resume --slot N
--owner ID --session SESSION_ID` on its recorded slot: the rebind skips fetch,
reset and clean so partial work survives, the owner and slot stay explicit,
and hand-edited `executor run` receipts are never the resume path.

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
`unsupported command` answer is a stale harness build, not proof that
executors are unavailable: kit skills are live links, while the launcher is an
immutable build updated only through the kit lifecycle. Report `launcher stale`
with the remedy - rebuild and update the kit from the source root recorded in
`CODEX_HOME/harness/installation.json` - and meanwhile continue only work whose
acceptance does not depend on executors. Do not repair the skew by substituting
the profile or by dispatching raw `codex exec`, TUI automation or in-session
subagents: that drops the visible conversation, steering, board and recovery
contract.

Each executor gets a complete outcome, its configured profile, its own visible
terminal tab or window, and a synchronized pool slot before the first model
request. Freshness is mechanical, not an executor obligation: dispatch fetches
the configured remote and resets the slot to the resolved base (untracked files
removed, ignored build caches kept) before that request, so no executor-side
synchronization step is needed or accepted in its place. The brief names the
exact base revision; the executor verifies its slot HEAD is that revision
before substantive edits and stops with a report on mismatch instead of
repairing synchronization itself - redispatch with the same owner id corrects
the slot. Executors never create an additional worktree: when every slot is
held by a live session, wait or stop
a running assignment instead of allocating another tree. `executor spawn`
establishes the view itself: inside the lead's Windows terminal it opens a
titled tab of the same terminal. Do not resize, move or arrange desktop windows
- including this terminal - so sessions fit the screen; titled tabs are
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
The `--exec` prompt points at board ids; the beads issue is the assignment.
Assignments require bounded milestone self-reports: after each numbered
outcome inside the assignment, the executor posts a short board comment on
its issue (done, next, blockers in at most three lines) instead of waiting
for the lead to ask.
The default exec mode streams the assignment in a visible tab and exits on
completion, closing the tab; a mid-work stop is detected by the watcher and
the exact session continues through `codex-harness executor resume --slot N
--owner ID --session SESSION_ID`. Do not
prefix prompts with `/goal`: the CLI has no argv goal hook and the prefix
would be inert text. Accept only against the assignment, not effort spent.
Spawn returns after the executor window/tab is open so the lead can keep
working. Executors look at the board and do assigned issues.

## Steer, wait, stop

Deliver steering through the controller session channel into the executor's
visible conversation. No hidden model calls and no status polling. Wait without
takeover while an executor remains active. Executors escalate as board feedback
tasks. `codex-harness` task stop remains the emergency path that works without
the lead. Explicit stop stays stopped after restart.
Do not poll from model turns or executor PIDs. While an executor runs, keep
one native watcher process that checks the board review queue, the
assignment's result artifact and executor liveness on a cheap shell loop and
emits a single event; the lead blocks on that event between useful work.
Track session-file growth in the same loop: a live executor whose rollout is
silent beyond a bounded threshold (about 15 minutes) is a stuck-suspect -
then read its recent reasoning and diff, and only for a confirmed anomaly ask
one bounded question through an exact-session resume (status, blockers, next
step in at most five lines, then continue). Timed status polling of healthy
executors is waste.
While executors run, the lead analyzes bottlenecks, spend and next cuts, and
files those as board tasks.

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

Review completed assignments against requirements and applicable checks. Merge
accepted branches yourself. Return in-scope defects with acceptance conditions
to the original executor. Record acceptance on the board and in task state.
Reconcile planning artifacts explicitly on integration: an executor's
tasks.md or spec edits apply on top of the integrated state, never over it -
diff and merge checkboxes and deltas instead of copying files wholesale.
Executor slots are pool-owned, not task-owned. The pool never grows past
`max_concurrent_executors`, and slots are reused while conversations are not.
Return a finished slot to the pool through the explicit release path, which
records your disposition before anything is destroyed:

```powershell
codex-harness executor release --source CHECKOUT --codex-home DIRECTORY --slot N --disposition merged|discarded --reason TEXT
```

Release resets the slot to the committed base with ignored build caches kept, so
the next dispatch binds the same path; keep merged branches. A slot that cannot
be safely reset is preserved with its reason and stays out of the pool until you
resolve it: never force-reset unreviewed work, force-remove a tree, or count a
preserved slot as free. Dispatch is fail-closed: no free slot, a failed upstream
fetch, an occupied dirty slot, unreviewed changes in a free slot or a missing
slot each abort with the concrete cause - a registered-but-missing slot asks for
`git worktree prune` - instead of allocating another tree. `codex-harness
executor pool --source CHECKOUT --codex-home DIRECTORY` reports the recorded
mapping per slot (index, path, presence, tree state, state, lease, owner, base)
and lists foreign or legacy worktrees for your review; `git worktree list` stays
the authoritative tree inventory, and slot purpose lives in kit-local task state
and board records, never in tracked files. An interruption keeps the recorded
mapping; resume the exact session on its slot to keep partial work, while a
second live owner of one slot is refused instead of sharing a checkout.
Legacy task-named or CLI-named executor trees in a consuming repository are
never adopted or deleted by dispatch: merge accepted work, retire the rest with
authorized `git worktree remove`, and prune stale entries afterwards.
Executors set status `lead_review` instead of closing. The lead closes on
accept or returns the item to `in_progress` with conditions.
Executor terminal tabs are per-assignment, never pooled: pool slots are reused,
conversations are not, and a fresh session must not inherit another
assignment's context. Exec mode closes the tab when the assignment finishes;
nothing lingers and nobody has to remember to close it.
To return defects or continue after a stop, resume the exact session
(`codex-harness executor resume --slot N --owner ID --session SESSION_ID` for
a pooled executor, `codex resume SESSION_ID` interactively otherwise) and
state the acceptance conditions there.

## Recovery

One active lead. Reconcile surviving workers and their slot occupancy before
replacement: `executor pool` reports the recorded mapping and lease state, and a
claim whose host process is gone is not an occupied slot. Quota and transport
failures stay classified from retained evidence. Succession uses the configured
successor profile at a safe boundary. Do not purchase capacity or retry models
infinitely.

Policy (do not copy it here): [portable principles](../../../global/principles-of-work.md)
and [agent delegation](../../../docs/agent-delegation.md).
