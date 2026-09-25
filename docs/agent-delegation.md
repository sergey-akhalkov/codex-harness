# Agent selection and visible conversations

Selection has two paths; never mix them. Ordinary in-session subagents take
explicit `model` and supported `reasoning_effort` parameters per assignment;
GPT normally leads, Z.AI handles substantial text/code execution, and Grok
handles visual and suitable routine work. Kit executor dispatch takes no
per-assignment model or effort: it runs exactly the configured executor
profiles, and the profile's configured model and reasoning effort are that
assignment's complete explicit selection. The kit configures one executor,
`xai`, binding Grok 4.7 (`grok-4.7`) at `xhigh`; one executor
profile is normal full capacity, not a delegation limit. Choose from task
complexity, risk and the full cost of the accepted result; a separate TOML
file per effort or activity is unnecessary. Supported efforts differ by
model; verify the effective binding instead of assuming every level exists on
every route.

The removed Astra names migrate to these direct arguments. This table preserves
their former meaning; it does not prescribe an effort for every new assignment.

| Former purpose | Retired name | Model argument | Effort argument |
| --- | --- | --- | --- |
| Backup middle when Grok is unavailable | `middle_backup` | `gpt-6-astra` | `high` |
| Main session; a separate hard slice when needed | `senior` | `gpt-6-astra` | `xhigh` |
| Rare consultation on a hard intellectual blocker | `principal` | `gpt-6-astra` | `max` |

For example, use an ordinary agent with `model="gpt-6-astra"` and
`reasoning_effort="high"` instead of `agent_type="middle_backup"`. The same
parameter selection applies to enabled external models. Use `codex --profile xai` with `grok-4.7` / `xhigh` instead of any subscription
middle preset. Those OpenCodex role files are retired. The former
`grok_reviewer` remains retired.

Every active conversation needs its own visible terminal surface with a
distinct title - a dedicated tab in the same terminal, a pane or a window -
showing assignment, model/effort, messages/tool activity and status. Include
any model-backed helpers and explicitly show changes of leader. The controller
opens that surface per conversation before dispatch; a titled terminal tab is
sufficient, and tiling every conversation on screen at once is not required. A
hidden process, raw log or one chat identity masking several conversations is
insufficient. If a required view closes, suspend new model requests, preserve
in-flight work and restore visibility through the owning dispatch command. Do
not resize, move or arrange desktop windows - including the lead's own
terminal - so sessions fit the screen; that ceremony is not part of dispatch.
Do not capture Codex or sibling agent windows with Nuphus screenshots, and do
not poll window pixels for status. Identify non-Codex windows with list,
title, bounds and state; keep screenshots for genuine visual questions about
owned UI.

## Orchestration configuration

Kit-owned [orchestration.toml](../global/orchestration.toml) names the lead
profile, the successor lead used after a confirmed lead quota failure, the
executor profiles, and the maximum concurrent executor count. Installation check
rejects a missing profile or a non-positive limit without substituting another
route. `default` is the native session with no `--profile` flag. Other names must
exist as `CODEX_HOME/<name>.config.toml` or `[profiles.<name>]`.

Synthetic example (not a live consumer):

```toml
schema = 1
lead_profile = "default"
successor_lead_profile = "xai"
executor_profiles = ["xai"]
max_concurrent_executors = 4
```

Dispatch an executor with the installed launcher:

```powershell
codex-harness executor spawn --source CHECKOUT --codex-home DIRECTORY --base REV --exec "assignment"
```

Omitting `--mode` selects the native Codex TUI. `--exec` is the assignment
input, not a presentation selector.

Skills and `orchestration.toml` are live links into the kit checkout, while the
launcher is an immutable native build changed only by an explicit build and
install, so a checkout updated after the last install can reference an
`executor` command the installed launcher lacks. Probe with
`codex-harness executor --help` before the first dispatch; `unsupported command`
names a stale build, not unavailable executors: rebuild and update through the
[installation lifecycle](installation.md#install-and-verify) from the source
root recorded in `CODEX_HOME/harness/installation.json`. Until then
orchestration stays blocked - no profile substitution, no raw `codex exec`, TUI
automation or in-session helpers. `codex-harness --version` prints the installed
build's source identity when its build record is present, so a stale launcher
is identifiable without guessing.

Dispatch uses the configured executor profile from the example above; an
unlisted `--profile` is an error, and an explicit user `codex --profile <id>`
keeps native precedence over role configuration. The profile is the complete
model/effort selection and `executor spawn` accepts no per-assignment override
by design. If any instruction appears to demand one for executor dispatch, that
demand belongs to ordinary in-session agents: dispatch the configured profile
and report the discrepancy. Only a launcher- or installation-check-reported
failure - missing profile, stale build, no free slot, fetch failure - blocks
dispatch, with its exact cause and remedy; never a routing-rule interpretation,
the single executor profile or unknown quota. Quota succession looks up the
successor profile's model in the native catalog; the verified handoff seed
stays a configured binding, not a hardcoded provider role.

Spawn opens the assignment in a new tab of the lead's own Windows Terminal
window when the lead already runs there (`WT_SESSION`). Windows Terminal has no
supported address for the calling process's window - `wt -w 0` resolves to the
most recently used window of the current desktop, which is the user's focused
window, not the lead's - so dispatch finds the lead's window through the
console parenting of its pseudo console window, briefly holds it foreground,
resolves the tab there, and restores the user's previous foreground window and
selected tab once the titled tab is observably open; the lead window never
stays stolen. When that window cannot be addressed (another virtual desktop or
a blocked activation), the tab opens in the stable per-checkout window
`codex-harness-<repository>`, which the terminal creates on first use, and
`--terminal-window` targets an explicitly named window for setups that prefer
one. Spawn does not pass `--focus` / maximized / fullscreen and never sends
synthetic input into a conversation.
Without `WT_SESSION`, spawn opens a visible console instead of a tab. Steering
stays `executor message`, not TUI keystrokes. Spawn returns after the surface
is open. Presentation, closure, watch and exact-session recovery are the
[observed lifecycle](#observed-executor-lifecycle).
While an executor runs, one native watcher process checks the board review
queue, the assignment's result artifact and executor liveness and emits a
single event; the lead blocks on that event between other work instead of
polling executor process ids from model turns or reading window pixels.
Assignments live on the beads board; executors set `lead_review` when done
instead of closing. The lead reviews that inbox and its own `assignee=lead`
tasks.

A structured assignment's brief carries the result consumer, the standing
escalation boundaries, the executor's own investigate, implement, check and
self-correct cycle, and the compact result expected back: done and remaining
work, the checkout and base worked from, files, actual checks, limitations,
required decision and detail locator. A declared `consumer` or `escalate`
trigger extends that contract instead of replacing it, and an executor resolves
ordinary implementation errors - syntax, API names, failing checks - itself,
reaching its lead through the installed message channel only when a trigger is
actually hit.
The schema, optional fields, defaults and limits live in
[native commands](rust-native.md#structured-executor-assignments).

Every executor session runs as a single-agent worker. Dispatch, resume and
instruction-refresh succession pass Codex CLI's built-in
`agents.enabled=false` configuration, so the session's tool set contains no
agent-spawning or agent-messaging tools and its instructions contain no
multi-agent usage guidance - even though the `ds` model catalog advertises
multi-agent v2. The installed launcher applies the same setting to every Codex
process started inside an executor, so a raw nested `codex` invocation is
still single-agent, and `executor spawn`, `resume`, `run` and `succeed` refuse
to run under the executor environment marker with an error naming the lead as
the owner of further delegation. Ephemeral `spawn_agent` helpers are
therefore lead-only: an executor that needs another agent or executor reports
the need instead of creating one. Clearing the marker and calling the
registered upstream Codex executable directly is outside the supported entry
points; the guards are not a sandbox.

The `team-lead` skill owns lead activation, briefs, steering, acceptance and
stop. Activation comes from a user request or from the main session's own
judgement for parallelizable work that repays orchestration; ordinary
sessions without that activation spawn nothing, and an executor reports a
further delegation need to the lead instead of widening orchestration.

## Executor utilization

While the lead role is active, configured executor capacity is not left idle
by omission: at each utilization checkpoint the lead dispatches the next
worthwhile, capability-sized slice or records the concrete reason the capacity
stays idle, and released capacity is backfilled before unrelated lead work
starts. Idle capacity without such a reason, or a report that hides it, is a
lead workflow defect. Requirements live in the
[delegation](../openspec/specs/agent-delegation/spec.md) and
[orchestration](../openspec/specs/lead-agent-orchestration/spec.md)
specifications, and the live checkpoints, recorded reasons, occupancy
reporting and priority rules are owned by the
[`team-lead` skill](../.agents/skills/team-lead/SKILL.md) rather than restated
here.

## Instruction-refresh succession

When an accepted instruction or skill change must reach an active executor
session, replace its CLI process through the verified non-interactive resume
path instead of steering the stale session:

```powershell
codex-harness executor succeed --request FILE
```

The request names the exact session id (never `--last` or a picker), the
configured profile, the workspace, the private session state root or its
recorded pointer, the compact revision identity published by skill-evolution
(`name`, canonical `path`, `revision`, `operation`), the durable task context
and a private evidence directory. The command makes no model calls: it waits
for a safe boundary after in-flight tool effects, writes the handover record
into the owning task records, confirms the predecessor process stopped, spawns
`codex exec resume <SESSION_ID>` with the recorded sandbox and approval
policy, and verifies from the successor's own session rollout that the current
instructions and skill revision were reloaded. A stale published revision or
a missing reload is reported as `succession not established` with a non-zero
exit; the gap is fixed before the refresh is relied on. The successor is a
bounded continuation turn, not an interactive view; same-session compact
recovery, catalogue delivery and in-process activation remain owned by
`autonomous-skill-evolution`.

## Board workflow

Asynchronous assignment and executor feedback use the consuming project's
`beads` (`bd`) CLI. Kit install delivers pinned v1.3.0 onto `harness/bin` with
`--board-only` and the `board-workflow` skill through core skill linking. The
controller does not parse the board. Missing or broken board state is an
explicit limitation, not a substitute tracker. Init is
`bd init --skip-agents --non-interactive --quiet`. Stages are epics,
specifications are features, executor feedback is a `task` labeled `feedback`,
and `bd status` / `bd list --label feedback` / `bd epic status` are the
non-interactive reports. Do not run `bd setup codex` from kit install.

## Improvement loop

Feedback is a board queue, not chat: lead and executor observations become
bounded `feedback` tasks, batch-triaged by the lead at safe boundaries.
Listing, merging, voting and promotion are `bd` commands; only the lead's
similarity and consequence judgments are model work. The incubator holds unique
items, a merge or repeated report adds exactly one vote per distinct episode and
reporter, and `vote_threshold` (kit `global/orchestration.toml`, default 3)
promotes an item in routing order with kit-level wording only. A material
correctness, integrity or safety finding promotes immediately under the lead's
consequence override with the reason recorded. The lead sweeps the incubator
when it closes a stage or epic and when a triage batch finds it above
`incubator_size_cap`, archiving stale items with visible reasons instead of
deleting evidence. The `board-workflow` skill owns the record formats; the
`team-lead` skill owns the workflow.

Pacing uses three scoped sources only: the native Codex/GPT limit snapshot the
CLI records for itself, actual provider refusals, and bounded dashboard
snapshots the user supplies. Unknown stays unknown - no probe call and no local
request-count remainder - and unknown holds the configured limits. Below 70%
used keeps configured concurrency and cadence, 70% or more halves new
concurrency and the triage batch (at least 1), and 90% or more, or an observed
refusal, waits for the reset with concurrency 1 and a `low` effort ceiling.
Pacing changes new assignments only: a healthy executor keeps its slot, model
and instructions, and tasks released by one reset are spread by a 120-second
stagger instead of bursting together. Decisions are recorded on the board with
reason, basis and expiry, and withdrawn with a revoke record.

No improvement becomes a default for assignments, worktrees, concurrency or
cadence before a matched comparison declares its tolerance in advance and shows
unchanged-or-better quality inside that tolerance, with check time, coordination
and rework counted in both arms. An adopted record is required: an inconclusive
or rejected comparison, or no record at all, leaves the improvement unadopted.
The gate evaluates orchestration defaults; it is not the skill-evaluation
contract for library mutations.

## Executor worktrees

`executor spawn` is the sole allocator of executor isolation. It maintains a
fixed pool of ordinary Git worktrees (`git worktree add --detach`) created as
sibling directories of the source checkout and named `<repository-name>-wt1`
through `<repository-name>-wtN`, where `N` is `max_concurrent_executors` from
[orchestration.toml](../global/orchestration.toml). Positions are fixed, so a
dispatch that finds every slot held by a live session aborts naming them
instead of registering another tree. `--workspace` is optional and no longer
the isolation mechanism: it must be the source checkout or one of its pool
slots, while an ad-hoc task-named worktree path is refused with a migration
hint. `--base REV` starts the slot from another revision than the upstream
default branch, and `--owner ID` labels the session binding (default
`exec-<profile>-<pid>`); a second live owner of one slot is refused instead of
sharing one checkout. An ordinary interrupted session resumes through
`codex-harness executor resume --source CHECKOUT --codex-home DIRECTORY --slot N --owner ID [--session SESSION_ID] (--exec PROMPT | --assignment FILE)`:
the recorded slot is rebound for the same owner without fetch, reset or clean,
and the host attaches the native TUI to that exact session without a new
conversation or a replay of completed work. Partial work survives. A fresh
spawn resynchronizes and never continues a dirty slot. `--session` remains
supported and takes precedence;
without it, resume consumes the exact session the dispatch receipt recorded,
keeps that identity across failed resume attempts and refuses when nothing was
recorded instead of choosing another session by recency. For an automatic cache
stop, use the fresh-conversation recovery below instead. Hand-editing dispatch
receipts for `executor run` is
not the resume path. The recorded mapping (index, path,
owner, synchronized base) and its lease live in kit-local task state under
`$CODEX_HOME/harness/executor-pool/`, never in tracked files, and a claim whose
host process is gone is no longer an occupied slot.

Freshness is a property of dispatch, not an executor obligation. Before the
first model request, spawn fetches the configured remote (the remote named
`origin`, or the single configured remote), resolves that remote's default
branch - or the explicit `--base` override - runs `git reset --hard <base>` and
`git clean -fd` (ignored build caches stay warm) and verifies a clean HEAD.
The base is the lead's committed snapshot: before dispatch the lead commits
assignment-relevant source-checkout changes locally (pushing stays a separate
authorized step) and names that revision with `--base`, or verifies that
committed HEAD already contains every input. The upstream default branch is
the base only for assignments with no dependency on local lead state; if
commits are not authorized, a slice depending on uncommitted state stays in
the lead instead of being dispatched from a stale base. Copying files into a
live slot is not synchronization: changed tracked inputs travel as a new
commit and a redispatch with the same owner id, which rebinds and
resynchronizes the same slot. Executor briefs name the exact base; the
executor verifies its slot HEAD equals that base before substantive edits and
reports a mismatch instead of repairing it.
Fail-closed applies throughout: a failed fetch, an unresolvable base, a missing
slot, an occupied dirty slot or unreviewed changes in a free slot aborts
dispatch with the concrete cause and leaves the slot untouched, so a stale base
is never a silent fallback and no extra tree is allocated. A collision - a pool
position occupied by anything that is not a registered worktree of this
checkout - is refused instead of adopted or replaced.

Slots move through `free`, `synchronizing`, `occupied`, `awaiting-review` and
`released`; a slot is occupied only while its bound session is live. A dirty
slot whose session has ended awaits review: dispatch neither selects nor resets
it, reports it with its reason, and the lead must merge the work or record an
explicit discard before it re-enters the pool. Release is explicit, and it
records the disposition before anything is destroyed:

```powershell
codex-harness executor release --source CHECKOUT --codex-home DIRECTORY --slot N --disposition merged|discarded --reason TEXT [--base REV]
```

The slot is then reset with `reset_for_reuse` (see
`crates/harness-core/src/task_worktree.rs`) to the named base or the source
checkout's committed HEAD, keeping ignored caches, or preserved with its
limitation and reported as awaiting review again (exit code 2); a live owner is
never reset beneath. `codex-harness executor pool --source CHECKOUT
--codex-home DIRECTORY` reports the recorded mapping per slot (index, path,
presence, tree state, state, lease, run, owner, base, disposition, reason) plus
foreign or legacy worktrees and any worktree beyond the configured pool for
lead review; `git worktree list` remains the authoritative tree inventory, and
merged branches are kept.

`worktree_limit` is superseded by the pool size: the field stays accepted in
`orchestration.toml` for compatibility, but dispatch no longer warns at a
threshold and cannot exceed `max_concurrent_executors` by construction. Legacy
task-named and CLI-named executor worktrees in a consuming repository are
neither adopted nor deleted automatically: the lead reviews them, merges
accepted work, removes retired trees with authorized `git worktree remove` and
prunes stale entries. Pool slots are ordinary Git worktrees, so once their work
is preserved they can be removed manually and leave no harness-specific state
in the repository.

## Observed executor lifecycle

Ordinary `executor spawn` with no `--mode`, or with explicit `--mode tui`,
plus `executor resume` and `executor restart`, present the managed
conversation in the native Codex TUI. Explicit `spawn --mode exec` uses that
same observed control lifecycle with the native inline TUI (`--no-alt-screen`):
not a second renderer and not an unobserved `codex exec` launcher. The host
records the lifecycle in
`$CODEX_HOME/harness/executor-pool/spawn-<N>.json`: `dispatch-accepted`,
`native-start`, `running`, `completed`, `failed`, `defect` and `interrupted`,
beside the exact native session, the final-message locator
(`message-<N>.txt`) and a bounded detail file (`stream-<N>.jsonl`). A created
tab is not a native start, and frontend exit is not a finished turn.
Historical unmanaged tui receipts and legacy receipts keep the coverage they
recorded; the current native TUI does not make watch coverage unavailable.

The host owns one `codex app-server` child in a Windows Job. An abnormal host
death reaps that child tree; an ordinary run end preserves the session's
remaining background members. Host identity, the bounded detail file and the
initial record must succeed before the child starts. A later startup, read or
record failure fails the host with its cause instead of running another
backend or reporting success. Losing the only frontend suspends further model
dispatch and contains the owned run. An unfocused or unselected tab is not
frontend loss, and a retained completion is not overwritten or reported as
success because the frontend closed. The child's output is retained at a
kit-local log whose bounded tail is shown when the run fails. One writer at a
time updates a receipt (a kit-local lock serializes the
console dispatcher's window record and the host's lifecycle record, and each
write replaces the document atomically), so concurrent writers cannot lose
each other's fields.

After the result is persisted, the host ends that owned frontend and backend,
then the existing terminal-host close policy finishes the tab. The tab host
exits 0 after recording the outcome, including a recorded failure, so the tab
closes; watch reads the receipt's exit code, not the tab process code. An
owned console still returns the run's own code. A cleanup failure names each
surviving owned resource and its recovery action without changing the recorded
state or exit code. Closure is not acceptance, merge or slot release. An
unresolved required reply is not a terminal run and does not close the
surface. Inspection is the receipt and watch, not a leftover tab.

The lead waits for an executor through that record; no model polling and no
rollout search is involved:

```powershell
codex-harness executor watch --source CHECKOUT --codex-home DIRECTORY --slot N
codex-harness executor watch --receipt FILE [--json]
```

`--receipt` must be an absolute path. Optional `--owner`, `--timeout` and
`--poll` match `codex-harness executor --help`.

Watch blocks until the run reaches a terminal state and then prints bounded
review data: state, slot, owner, exact session, checkout, base, changed files,
the executor's returned message (reported by the executor, not verified
acceptance), the result, detail and stderr locators and the exit code. Changed
files are reported in two bounded segments - committed changes compared with
the recorded base through `git diff <base>..HEAD`, and the current working tree
including untracked files - so a committed executor result never reads as "no
changes", and truncation is named. Watch exits 0 for a completed run, 1 for
failed, defect or interrupted runs, and 2 when coverage is unavailable
(historical unmanaged tui or legacy), the receipt is missing, or the timeout
expires while the run continues. Exit 3 is action required: the run is live
with an unanswered reply request, and the result names the run and the request
references whose one-command reply answers it. The lead answers and runs the
same watch again; exit 3 is not completion, an output defect, unavailable
coverage or a resume trigger, and the waiting run keeps its session, slot,
worktree and partial work instead of being resumed or released. Timeout does
not stop the executor. An interrupted
host is reported with its reason and an unknown exit code, never as a
completion. `executor pool` adds `run=<state> session=<id>` per slot, and
`executor release` prints the last observed run beside the disposition it
records while still refusing to reset a live or unreviewed slot.
Waiting follows that one-event shape: retain one native watch per run, with
`--timeout 900` for the 15-minute supervision boundary. Completion or failure
can return earlier. Short tool yields resume the same pending wait; they do
not justify another watcher, worktree inspection or status-only update.
At the boundary, batch one compact state/activity check across active executors;
`executor pool` supplies the state snapshot. If progress is unclear, inspect
the latest bounded activity/error evidence once, identify a concrete blocker
or report what evidence is missing, and choose the next action. A live process,
growing log or missing patch alone proves neither progress nor a stall.
Resume watching ongoing work; elapsed time alone is no reason to steer or stop it.
The shared supervision rule lives in
[`global/harness.config.toml`](../global/harness.config.toml).

An observed `executor run --file` is that tab or console host. It reports the
same states on its visible surface, propagates the launcher's own exit code,
exits 0 only for a completed turn with a nonempty final message, exits 3 when
a completed turn wrote an empty or missing final message (an output defect,
not model unavailability), and exits 1 for a failed or interrupted stream. An
empty completion is never reported as success. A Windows Terminal tab host
exits 0 after recording that outcome so the tab closes. A completion record
is evidence of execution state, not proof that the executor's claimed checks
passed. Exact-session recovery is the resume command in
[executor worktrees](#executor-worktrees). Cache-loss recovery stays the fresh
restart below, also on the native TUI.

## Steering and stopping executors

Steer a continuing run with `codex-harness executor message` and stop one with
`codex-harness executor stop`; both address the recorded checkout, slot, owner
and exact session and verify that identity against the live run before
acting, so input cannot reach a later occupant of a reused slot. The exact
flag surface and result classes live in
[native commands](rust-native.md#structured-executor-assignments); the rules
below say when each command is justified.

An executor's own question travels the reverse direction and needs no address:
`codex-harness lead message --text TEXT` (or a UTF-8 `--file`) sends one
literal payload to the originating lead recorded for that run, and `--notify`
marks a notice that requests no reply. The envelope carries the sender, run,
session, worktree and assignment metadata with a request reference, so the
lead answers through the same command owner:
`codex-harness executor message --reply-to MESSAGE_ID --text TEXT`. That
reference replaces the address fields and continues the same conversation in
place, without resume. Asking is exceptional - a material ambiguity, an
authority or access boundary, or a dependency the executor cannot obtain after
investigating the available facts. Routine progress, repeated status and
ordinary implementation errors stay off the channel and on the bd issue, and
nobody discovers endpoints, receipts, process ids or sessions by hand to reach
a run: the commands resolve the recorded identity themselves.

Message a continuing executor for a concrete correction, a relevant fact or a
requirement change: steering adds facts, resolves a request or corrects an
established mistake. No status-only nudges, hurry demands or repeats without
new facts, and no question to a healthy executor about what it is doing -
waiting is not a reason to steer. A correction continues the same conversation
in place; a completed, stopped or unavailable run is reported with its actual
state and the exact-session resume remedy, never revived.

Stop only for an explicit cancellation request or a concrete necessity - a
demonstrated wrong direction, a run that cannot make progress, or a resource
conflict the executor cannot resolve. A brief error, a slow or silent stream,
waiting or silence alone does not justify stop; inspect the recorded state
and the current work first. Use the kit's stop command rather than killing
processes by hand: manual process killing has no identity check, tab closure
or honest receipt and needs a recorded cause. A stop preserves the files,
checkout, slot and partial work and claims no completion; continue by
resuming the exact session (except cache-loss recovery below), and release
the slot only as its own explicit decision. The same explicit stop ends a run
that is waiting for an answer: waiting is a live state, and no timeout, silent
period or unanswered request stops or resumes it by itself.

### DeepSeek cache-loss protection and recovery

New observed DeepSeek executor hosts monitor their exact session's native
per-response input/cache counters. After a response with at least 100,000 input
tokens and 90% cached input, three consecutive responses each missing at least
100,000 tokens and 50% of input trigger termination of the owned process tree.
Cold starts, duplicate observations and historical losses on resume do not
count. The host terminates before waiting on control acknowledgments or writing
its stop receipt; it does not wait for a model, test or build to finish.

Windows file-change notifications and native events trigger bounded reads of
appended records. A one-second open-file size check covers delayed Windows
notifications; unchanged content is not periodically reread. No monitoring request is
sent to the model. Usage arrives after billing, so the three responses and an
already in-flight request can still cost money. This is a repeated-loss guard,
not a currency cap or a fix for the upstream cache. Missing/invalid counters
are reported as unavailable coverage. Already running older hosts do not gain
protection from installing a new binary.

The receipt's `cacheGuard` and failed watch result retain the counters and an
exact recovery command. The lead reviews preserved work and runs that command:

```powershell
codex-harness executor restart --source CHECKOUT --codex-home DIRECTORY --slot N --owner ID --session PREVIOUS_SESSION_ID
```

Restart starts a fresh conversation on the native TUI in the same worktree, keeping its commits,
uncommitted/untracked files and saved test/build evidence. It does not fetch,
reset, clean or release the slot. The old rollout remains available. The new
session receives the recorded original assignment plus a bounded checkpoint
of recent visible activity and must inspect the actual work before continuing;
an interrupted command is not proof of success. It does not replay the whole
old context or restore unsaved internal model state. If no assignment was
retained, supply the original `--assignment FILE` or `--exec PROMPT`. A stale
predecessor identity or live owner is refused. Use the configured binding;
investigate recurring loss instead of creating an automatic restart loop.

## How selection works

Kit executor assignments go to the configured executor profile - currently
the single `xai` profile (Grok 4.7, `xhigh`) - with no substitution:
in-session routing preferences never override it and never justify
withholding a dispatch; report a wording conflict and dispatch anyway. A
short edit,
tightly coupled slice or expensive context handoff is often cheaper to do
directly. Count briefing, execution, waiting, checking, integration and rework.
The number of children is not a savings metric.

Before dispatch, perform the analysis each slice needs to become sufficiently
specified for its executor profile - requirement interpretation, risk and
consequence decisions, approach direction and acceptance conditions - and size
slices to the configured profiles' reasoning capability. A slice too demanding
for every available profile stays with the lead, is split further, or uses a
bounded principal consultation; it is never delegated as-is. Investigation
inside a delegated slice's boundaries remains executor work.

Combine related routine into one substantial assignment and state material
input bounds up front. Routine supervision runs once every 15 minutes, as
described with `executor watch` above. Check earlier only for a delivered
result, explicit error, help request, new user instruction or concrete risk to
correctness or shared resources. Do independent work between events; do not
repeatedly reread worker source, diffs or logs.

That interval is not the assignment deadline. A message to a working agent must
add facts, correct an established error or change the task. An empty or
intermediate result requires checking current work; by itself it does not
establish quota exhaustion or authorize automatic GPT takeover. Reconcile the
delivery mechanism and partial result before reassignment.

After restoring a parent session, `resume_agent` can lose the previous
subscription binding. Do not recover such a session through that tool: inspect
partial work and hand it to a fresh explicitly selected agent with brief
context. An old model record does not prove a new binding, and a service-state
check error on an old conversation does not mean the route is permanently
unavailable.

The parent passes the concrete result, inputs, dependencies, change bounds,
invariants, resource ownership, meaningful check and parent consumer. Usually
`fork_context=false`: needed context, not full history. Keep this in the existing
brief or task; a separate assignment document is not required. A research-only
assignment can finish with an evidenced answer and its limits, while an
automation assignment needs a replayable result with checked preconditions,
effects and return or recovery behavior.

Two independent slices can run in parallel on non-overlapping files or separate
worktrees. Shared desktops, services, installed applications and devices need
one owner, isolated allocation or serialized interaction; worktrees isolate
files only. Preserve aggregate resource limits across workers. The parent does
useful independent work without repeating the investigation, then checks the
returned result, validity conditions and unresolved restoration or dependencies.

Supporting work is delivered when its intended parent consumes the result and
required integration and restoration pass. A discovery report or experimental
script leaves promotion into the existing mechanism owner and parent acceptance
pending. On interruption or reassignment, retain verified partial work, check
its current conditions and continue from that boundary. Decomposition can also
be performed directly by the parent when a worker would add overhead. The
[portable principles](../global/principles-of-work.md#speed-feedback-and-recovery)
own when to reconsider the method, including successive different failures in
one unlearned layer.

If Grok is missing from the catalogue or an access, model or quota error is
returned, the parent briefly reports the cause and chooses an available capable
route with explicit binding. A retry is justified by changed circumstances; do not recheck the
same error on every step. Inspect saved partial work before reassignment. A
transport error is not a reason for Astra max consultation. There is no mandatory
escalation ladder.

## Global connection and limits

The [harness profile](../global/harness.config.toml) contains a short standing
policy and a limit of two concurrent child threads.
The [former Astra source directory](../global/agents/README.md) remains linked
at `~/.codex/agents/codex-harness` for installer compatibility and contains no
presets. OpenCodex routing, including its subscription middle, is retired;
direct native selection does not need those names. Do not create recursive
worker trees; this remains policy until
the controller enforces one active lead and the configured executor concurrency bound.

`developer_instructions` is a scalar: the selected profile replaces the same
base config.toml value rather than concatenating strings. The base file remains
and applies again without that profile. AGENTS.md instructions still load;
explicit CLI and trusted-project settings follow native precedence. Selection
policy is an instruction to the agent, not a forced scheduler. Asking to limit
research by time or tokens does not by itself create a hard limit. Thread limit
is configuration; generic-worker recursion is prohibited by policy. Timeout and Windows Job Object
for acceptance probes are provided by a separate runner.

Sources are read through direct links on a new start. An already open tool
catalogue may remain previous. Disconnecting subscriptions removes only their
link; direct native Astra selection and the main model remain. Install, check, disconnect and
recovery commands are in [subscription models](subscription-models.md).
Do not stop the proxy from a session that uses it.

## Subscriptions and auxiliary calls

All assigned OpenAI models belong to Astra. Adapter search is explicitly aimed
at Grok 4.6 through xAI OAuth; the automatic vision helper is off. Grok 4.6
supports its own image input. If a selected model lacks a capability, that must
become a visible limit or a separate explicit assignment, not a hidden ChatGPT
quota borrow. Routing settings are owned by the native subscription lifecycle
described in [subscription models](subscription-models.md).
Grok `xhigh` support is described in the
[official reasoning contract](https://docs.x.ai/developers/model-capabilities/text/reasoning).

Paid API keys, purchases and hidden model substitution are not enabled. Astra
backup spends the same ChatGPT subscription as the parent. An unknown remaining
quota is not zero. Tokens help compare runs but do not exactly convert a weekly
limit: [Codex accounts for model, context, reasoning and tools](https://learn.chatgpt.com/docs/pricing).

For remaining ChatGPT quota use ordinary Codex account status; the app-server
contract is [`account/rateLimits/read`](https://learn.chatgpt.com/docs/app-server).
Keep observation time and limit scope: a snapshot belongs to the account, not
only the current task. For Grok, use available Grok-subscription limit
information and actual exhaustion responses; a reliable weekly-quota remaining
endpoint for this OAuth integration is not confirmed.
[Grok FAQ](https://docs.x.ai/grok/faq) describes subscription limits.
Local `~/.opencodex/usage.jsonl` reflects served requests, not a guaranteed
subscription remainder. Do not mix those values with Codex cumulative counters
without correlation and duplicate removal. Unknown remainder does not require
preliminary model requests before every delegation.

## Checks and replacing a model

Model-free checks are native: the delegation usage, outcome and consumer
suites under `crates/codex-harness/tests/` (see
[native checks](rust-native.md)). Isolated lifecycle uses separate homes, port
and Windows task. Legacy model probes are opt-in and capture private logs, but
do not yet open all active conversations simultaneously. Do not run them as the
new workflow's acceptance until their visible-view integration is implemented:

```powershell
& (uv python find) tests/agent-delegation.py --run-model-probes --scenario all
./tests/subscription-consumer.Tests.ps1 -RunModelProbes -GrokModel xai/grok-4.7 -GrokReasoningEffort xhigh -Scenario Delegation
```

The first probe compares the same tasks for direct Astra and two Grok children,
checks the result independently, and separately checks Astra-level bindings. One
short max consultation in it is a configuration check, not a production
threshold. Private evidence stays in a temporary directory.

The native usage counter `codex-harness delegation-usage` takes explicit parent and
child paths, keeps the last cumulative record of each thread for former
consumers, and separately removes repeats by stable response ID. Inherited
history can overlap cumulative sums; with incomplete data or series disagreement
the reconciled total is unknown. Missing, conflicting and unattributable data
are marked. The counter does not load full journals into the main-agent context.
Including an estimate in every working task is unnecessary. `null` means no
data; a provider total of zero with `thread_count=0` means none of its threads
were among the supplied files, not a zero remaining quota. The `partial` flag
conservatively spreads to aggregates on warnings. A failed run keeps available
spend data and the acceptance-failure cause.

The counter is available from any directory through the installed native
manager. Pass only needed
existing rollout files; keep results and detailed logs outside Git:

```powershell
$usageHome = if ($env:CODEX_HOME) { $env:CODEX_HOME } else { Join-Path $env:USERPROFILE '.codex' }
$usageState = Get-Content (Join-Path $usageHome 'harness/installation.json') -Raw | ConvertFrom-Json
$rolloutPaths = @('<parent-rollout.jsonl>', '<child-rollout.jsonl>')
& $usageState.configBridge delegation-usage @rolloutPaths --format markdown --output (Join-Path $env:TEMP 'usage.md')
```

A previous saved model probe can also be rechecked without spending the
subscription:

```powershell
& (uv python find) -B tests/agent-delegation.py --revalidate '<private-evidence-path>'
```

To replace a provider, select its enabled model and supported effort directly,
and update subscription configuration and exact-model checks where needed after
confirming the authorized catalogue and capabilities. OpenAI variants must remain in the
Astra family.

After a main conversation has started, `/btw` and `/side` open a side question.
Return with the TUI prompt (`Ctrl+C`). Nested side chats and review mode are
unsupported. On CLI 0.153.4 both names work with `multi_agent_v2=false`. Side
chats do not isolate files; use a Git worktree for independent edits.
