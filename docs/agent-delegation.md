# Agent selection and visible conversations

Selection has two paths; never mix them. Ordinary in-session subagents take
explicit `model` and supported `reasoning_effort` parameters per assignment;
GPT-6 Astra at `xhigh` leads, and DeepSeek V4.1-Flash at `max` executes
delegated work; xAI and Z.AI models are not used. Kit executor dispatch takes no
per-assignment model or effort: it runs exactly the configured executor
profiles, and the profile's configured model and reasoning effort are that
assignment's complete explicit selection. The kit configures one executor,
`ds`, binding DeepSeek V4.1-Flash (`deepseek-flash`) at `max`; one executor
profile is normal full capacity, not a delegation limit. Choose from task
complexity, risk and the full cost of the accepted result; a separate TOML
file per effort or activity is unnecessary. Supported efforts differ by
model; verify the effective binding instead of assuming every level exists on
every route.

The DeepSeek profile must set `[features] code_mode = false`: its
[Responses API](https://api-docs.deepseek.com/guides/responses_api/) accepts
`apply_patch` as a custom tool but rejects Code Mode's `exec`. Ordinary command
and function tools remain available. Keep this compatibility setting in the
local `ds.config.toml` profile; the shared default can keep Code Mode enabled.

The removed Astra names migrate to these direct arguments. This table preserves
their former meaning; it does not prescribe an effort for every new assignment.

| Former purpose | Retired name | Model argument | Effort argument |
| --- | --- | --- | --- |
| Backup middle for a hard slice | `middle_backup` | `gpt-6-astra` | `high` |
| Main session; a separate hard slice when needed | `senior` | `gpt-6-astra` | `xhigh` |
| Rare consultation on a hard intellectual blocker | `principal` | `gpt-6-astra` | `max` |

For example, use an ordinary agent with `model="gpt-6-astra"` and
`reasoning_effort="high"` instead of `agent_type="middle_backup"`. Those
OpenCodex role files are retired. The former `grok_reviewer` remains retired.

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
executor profiles, the maximum concurrent executor count, and the shared
worktree pool's directory capacity. Installation check rejects a missing
profile or a non-positive limit without substituting another route. `default`
is the native session with no `--profile` flag. Other names must exist as
`CODEX_HOME/<name>.config.toml` or `[profiles.<name>]`. Pool capacity comes
from `worktree_pool_size`, independent of `max_concurrent_executors`; the
shipped default is four for each, and `worktree_limit` stays
compatibility-only. Install and update materialize the explicit setting for a
configuration written before the capacity split - including a checkout still
carrying the old shipped defaults - writing its former effective capacity
before publication so later executor-limit changes cannot resize the pool.

Synthetic example (not a live consumer):

```toml
schema = 1
lead_profile = "default"
successor_lead_profile = "default"
executor_profiles = ["ds"]
max_concurrent_executors = 4
worktree_pool_size = 4
```

Dispatch an executor with the installed launcher:

```powershell
codex-harness executor spawn --source CHECKOUT --codex-home DIRECTORY [--repository REPO] --base REV --exec "assignment"
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
failure - missing profile, missing role inputs, stale build, no free slot,
fetch failure - blocks dispatch, with its exact cause and remedy; never a
routing-rule interpretation, the single executor profile or unknown quota.
Quota succession looks up the successor profile's model in the native
catalog; the verified handoff seed stays a configured binding, not a
hardcoded provider role.

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
single event; the lead blocks on that event between other work and keeps
blocking when no useful independent work remains, instead of polling executor
process ids from model turns, duplicating the assignment's investigation or
reading window pixels.
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

## Managed responsibility roles

A managed session resolves its responsibility role from the source root its
installed build recorded - [roles.toml](../global/roles.toml) and the
fragments it names in the kit checkout - through the runtime's shared role
resolver, then composes the result with the applicable native user, project,
profile and session context before any model request. `executor spawn`,
`resume`, `run` and `succeed` resolve the executor role; a lead session opened
through the harness resolves the lead role. The consuming checkout never needs
to own kit role assets, and native precedence is preserved: an explicit user
`--profile` still wins, retained explicit skill disables and unrelated or
project skills stay untouched, and no shared user configuration is mutated.
Missing or inconsistent role inputs refuse with their exact cause before any
launcher or model request; a dispatch keeps its slot bound and recoverable
instead of hosting a conversation without its responsibility role.

Each control receipt records the role, the revision this dispatch resolved,
the developer text a native thread is known to save (`saved`) and what a
continued predecessor recorded. Resume and restart re-resolve the role at the
new dispatch, record it and report the effective state honestly
(`role context: role=... revision=... saved=... previous=...`): a fresh start
or restart consumes the role text the current dispatch resolved, while a
resumed native thread keeps the developer text saved at its start, carried
forward unchanged across repeated resumes (skills the live catalogue gained
still refresh); no report names the freshly resolved revision as text a
resumed thread loaded. A replaced receipt whose original saved text is not
reliably recorded reports `saved=unrecorded`, and a receipt written before
role recording reports `role context: unrecorded`: both stay usable, and the
next fresh dispatch, resume or restart records the current role.

Inspect a composition without a model call:
`codex-harness skills catalogue --role lead|executor [--profile ID]`
applies the installed build's resolved role to the native skill read and
refuses when those role inputs are missing instead of reporting a set the
managed session would not see. Passing `--profile ID` selects the native
profile layer the managed session selected (the installed profile file or
its `profiles.<name>` table; `default` adds nothing), so its retained
context, including a skill that profile disables, reaches the composed
native read; the command reports
`role_context=role=...;profile=...;revision=...` and composes no model
routing. The `codex-harness check` report lists each role as `resolved` with
its revision or as a `gap` naming the exact missing input; a gap blocks
only the sessions whose role inputs are missing and fails nothing else.

## Executor utilization

While the lead role is active, substantial eligible investigation, writing or
code is dispatched to available executor capacity before the lead starts it,
and released capacity is backfilled before unrelated lead work starts. An idle
capacity's cause stays identifiable from board and `executor pool` state: no
checkpoint ritual, reason note or occupancy narration is required, and a
summary that would otherwise mislead about capacity that sat unused names the
cause in a clause. Requirements live in the
[delegation](../openspec/specs/agent-delegation/spec.md) and
[orchestration](../openspec/specs/lead-agent-orchestration/spec.md)
specifications, and the priority rules are owned by the
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
`team-lead` skill owns the workflow. The evidence-driven sequential experiment
loop is a separate mechanism with its own hypotheses, candidate selection and
removal gates: use the `self-improvement-loop` skill and the
[`codex-harness improve` controller](rust-native.md#improvement-controller);
it counts no incubator votes and does not change this promotion policy.

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
and rework counted in both arms. The `codex-harness feedback ledger` view keeps
the recorded decision and the comparison it supports separate: an adoption is
supported only when the recorded comment names both arms, a positive matched
count, finite tolerance and timing values, regression arithmetic consistent with
the recorded arm seconds and delivery inside the declared tolerance. Missing,
contradictory or malformed records stay visible with their limitations, and the
newest record naming the item controls the status - a newer unreadable record
never silently restores an older adoption. These checks read the recorded board
comment; they do not rerun the comparison or certify it independently. An
inconclusive or rejected comparison, or no record at all, leaves the improvement
unadopted, and a user-accepted trade-off remains a recorded decision rather than
demonstrated improvement. The gate evaluates orchestration defaults; it is not
the skill-evaluation contract for library mutations.

## Executor worktrees

`executor spawn` and the ordinary `codex-harness worktree` commands allocate isolation through one durable task owner: one pool per repository, shared by executor sessions and ordinary tasks so neither can bypass or overwrite the other's held position. Slots are sibling directories `<repository-name>-wt1..wtN` bounded by `worktree_pool_size` (the legacy executor-derived capacity when the configuration omits it). `--source` names the kit/configuration checkout that selects profiles and capacity, defaulting to the installed kit source recorded under `--codex-home`; `--repository` names the independent target repository whose pool, slot records and receipts the dispatch uses and defaults to `--source`, so an existing caller keeps its previous target and a consuming repository needs only to be a Git checkout, not a kit configuration of its own.

Managed ordinary, lead and executor sessions suppress the competing native `codex_worktrees` tool surface through the shared configuration and the forwarded `features.worktrees=false` leaf; [native commands](rust-native.md#native-worktree-tool-suppression) owns the precedence rules, the conflict report and the model-free catalogue checks. A conversation that was already running when the suppression arrived keeps the tools it registered, and no in-place update exists: end it and start again through the normal route, for example a new ordinary session or `codex-harness lead start --session THREAD_ID` to resume a managed lead thread with a refreshed catalogue. A higher-precedence explicit `features.worktrees=true` keeps the tools; the managed start reports that configuration conflict with its removal remedy rather than successful managed-pool delivery.

A fresh dispatch claims the first free position and binds it to the owner as a durable task with its generation handle and exact committed base before the first model request: the refreshed upstream default branch by default, or an explicit `--base` commit resolved locally without a fetch. The claim records the creating process and stays exclusively owned until that dispatch's host publishes its lease, including while no host lease exists yet. A competing dispatch, whether it uses another owner or the same owner, cannot reclaim the position or replace the pending assignment or receipt; it refuses with that position's cause instead of allocating past the pool or holding exclusion for the model conversation. Two free positions still start independently. A fresh dispatch is also admitted against `max_concurrent_executors`, independently of the pool: free positions alone do not grant another active executor run, a run holds concurrency only while its host lease is live or its startup claim still excludes other dispatches, and a held or suspended task keeps its position without consuming concurrency. The claim-record lock is only the short mutation bound: if it stays busy, dispatch refuses rather than overwriting. A confirmed-dead creator's clean unused reservation can be reclaimed by the next dispatch, which keeps the original failure, but a durable task binding is never freed by process exit alone, and a former owner cannot continue a reassigned task. Repeating an owner continues its held binding without a second checkout or reset; `--task ID --handle HANDLE` hands a held ordinary task to the executor session with the same checkout, base, generation handle and partial work preserved. `--workspace` may name only the target checkout or one existing pool position inside the configured capacity. Ad-hoc worktrees are refused, and a position holding unreviewed work is preserved rather than reset.

A fresh preparation fetches the configured remote once before the claim and resets the position to the resolved base, removing untracked files while retaining ignored caches such as `cache/`; a fetch failure aborts before any mutation, and a continued binding is never fetched or reset. The lead merges accepted work, then explicitly records `merged` or `discarded` with its reason through `executor release` or `codex-harness worktree release`. Release freezes the observed generation of the exact binding, so a superseded release refuses instead of resetting whatever holds the position later. It resets a clean/reviewed position to the chosen base and removes only that run's recorded message payloads; unrelated messages survive. A live owner is never released or reset. A position that cannot reset safely is preserved with its reason (exit 2) rather than reported free.

Ordinary tasks use the same pool without a conversation:

```powershell
codex-harness worktree acquire --task ID [--repository REPO] [--base REV]
codex-harness worktree continue --task ID [--handle HANDLE]
codex-harness worktree list
codex-harness worktree release --task ID --disposition merged|discarded --reason TEXT [--handle HANDLE] [--base REV]
```

Acquire resolves the exact local base without fetching and prints the pool line with the slot receipt (slot, path, presence, state, base, task, handle, owner, disposition, reason); repeating it for a held task returns the same binding, and a different requested base refuses as a conflict. Continue returns the held binding; a stale generation handle, a released binding or a live foreign session refuses with its exact cause. List reports every configured position plus registered-missing and foreign directories for lead review.

`executor pool` is the cheap authoritative mapping of index, path, presence, tree/lease/run state, repository, owner, base, task, handle, disposition and reason. It shows the same durable bindings as `worktree list`, including ordinary tasks, and also reports foreign, legacy and beyond-pool worktrees for lead review. `git worktree list` remains the tree inventory; `worktree_limit` is compatibility only. Legacy task-named worktrees are neither adopted nor deleted automatically: preserve their work, merge or record a discard, then remove them only by an authorized explicit Git operation.

An older harness reports upgraded pool records as unreadable with a missing
`schema` field. This is a compatibility refusal that protects held work during
rollback. Keep the records and worktrees, including released positions, and
restore a compatible harness through the installation lifecycle before using
that pool. Deleting records or rewriting their discriminator can expose held
tasks to the older host's expired-session cleanup. The current reader accepts
legacy records without rewriting them during inspection; an authorized state
transition writes the protected format.

Reducing `worktree_pool_size` below a held position refuses new acquisition;
status and continuation keep the held task visible and recoverable. Restore
the previous capacity or finish and explicitly release those tasks before
acquiring more work. A failed release leaves its binding held with the cause;
resolve that cause and retry the release instead of deleting its record.

Retiring a pool directory is separate from reusable release. Preserve and
integrate its result (or explicitly discard it), release the task through the
harness, then use `git worktree remove -- PATH` for that exact listed directory
when its ignored caches are no longer needed. Keep the released ownership
record. A later acquisition can recreate the same configured position; it
does not allocate a replacement outside capacity. For a registered but missing
directory, first resolve the preserved work and verify the path is really
absent, then follow the reported `git worktree prune` remedy. A held binding
still requires its own recovery; pruning is not task release.

## Observed executor lifecycle

Spawn, resume and restart present the managed conversation in the native Codex TUI; explicit `--mode exec` uses the same observed lifecycle with the native inline TUI, not an unobserved launcher. The host records `dispatch-accepted`, `native-start`, `running`, `completed`, `failed`, `defect` and `interrupted` in `$CODEX_HOME/harness/executor-pool/spawn-<N>.json`, with the exact native session, final-message locator and bounded detail file. A created tab is not a native start, frontend exit is not a finished turn, and historical receipts keep their recorded coverage.

The host owns one app-server child in a Windows Job. Startup, identity, observation and receipt writes fail closed; abnormal host death reaps the owned child tree, while ordinary completion preserves unrelated background work. Losing the only frontend suspends dispatch, but an unfocused tab does not. Receipt writes are locked and atomic. After the result is persisted the owned frontend/backend end and the terminal closes; cleanup failures name survivors without changing the outcome. Closure is not acceptance, merge or release, and an unresolved reply keeps the surface open.

The frontend relay drains ready traffic in both directions and waits only while
both are idle. A quiet user does not throttle streamed progress or heartbeat
traffic. A frontend reconnect attaches to the running conversation without
replaying the assignment. A stalled or reconnecting TUI alone does not prove
the executor stopped: use its receipt/watch result before choosing recovery.
Installing a relay fix applies to new hosts; already running hosts keep their
loaded binary and should not be interrupted merely to update presentation.

Control observations wait for socket readiness and read without blocking. Idle
and partial-frame waits preserve the connection and do not replay requests;
actual disconnects remain failures. This replaces repeated Windows blocking
receive timeouts, which [Microsoft documents](https://learn.microsoft.com/en-us/windows/win32/winsock/sol-socket-socket-options)
as leaving the connection indeterminate. Retained executor failures reported
I/O error 997, but its exact OS trigger was not reproduced in the short local
test. The confirmed regression was an 80 ms observation taking 549 ms while
a frame trickled in; the corrected transport passes that deadline case and
128 idle/partial-frame cycles, heartbeat, disconnect and backpressure checks
(`cargo test -p harness-core --lib task_control::tests --locked`). A transport
failure is not evidence against the executor's model or implementation approach.
The affected `executor_control`, `executor_message`, `executor_observation`,
`executor_stop` and `executor_warning` integration targets passed, as did the
opt-in model-free app-server binding check with Codex 0.159.3 and workspace
clippy with warnings denied. These checks do not establish a recurrence rate
for 997 in long model-backed sessions.

Wait through the record, never model polling or rollout searches:

```powershell
codex-harness executor watch --source CHECKOUT --codex-home DIRECTORY --slot N
codex-harness executor watch --receipt FILE [--json]
```

`--receipt` is absolute; `--owner`, `--timeout` and `--poll` match executor help. Address fields are optional when recorded state identifies one live run. Watch returns bounded state/identity/checkout/base/changed-file/result locators and the executor report, explicitly separating committed and working changes. Exit 0 completes, 1 names failure/defect/interruption, 2 means missing/unavailable coverage or timeout while the run continues, and 3 means an unanswered reply request: answer its printed reference and watch again. A native turn completion is pending finalization, not success and not an output defect, while the host has not yet retained the exit status and final result: the same watch keeps observing that run. Exit 0, in text or JSON, requires the recorded successful exit and a readable bounded result. Once that exit is recorded, a missing or unusable result is an output defect (exit 1). Timeout while finalization is still pending returns 2 with an explicit finalization cause, leaves the run working, and does not stop or replay the executor. Prior-session or prior-turn events, quiet output and frontend exit do not establish success. Timeout does not stop or resume the run.

Keep one watcher per run. Short tool yields continue that watcher; at the supervision boundary use one compact `executor pool` snapshot and, only if progress is unclear, inspect the latest bounded evidence once. Process existence, log growth, elapsed time, silence and a missing patch prove neither progress nor a stall.

`executor run --file` is the visible host. It propagates the launcher outcome, treats an empty completion as an output defect, and never presents a completion record as proof that claimed checks passed. Exact-session recovery is the worktree resume command; cache loss uses the fresh restart below.

An attachment timeout includes at most 4096 UTF-8 bytes of visible console text,
read only while the exact frontend belongs to that console. This diagnostic
neither captures pixels nor sends input and never substitutes for attachment.
Keep the failure in the owning local receipt: a resumed console can contain
private conversation text. Unavailable text is reported without hiding the
original timeout or changing its cleanup.
## Steering and stopping executors

Steer a continuing run by piping content into `codex-harness executor message`; a short `--text` or explicit UTF-8 `--file` remains a fallback. With one live run the command resolves and verifies checkout, home, slot, owner and exact session itself; several runs produce a bounded listing that names `--slot`. Oversized piped input spills automatically and is reported as spill evidence, never confused with model delivery. Message only to add a fact, resolve a request or correct an established mistake—not for status, hurry or repetition without new facts.

An executor asks through `codex-harness lead message` from piped/text/file input; `--notify` sends a no-reply notice. The request reference lets the lead answer with `codex-harness executor message --reply-to MESSAGE_ID`, continuing the same conversation without reconstructing an address or resuming. `lead start` registers the lead endpoint that spawn inherits. When an ordinary lead has no endpoint, the command can use the registered native CLI's session queue to address the exact originating thread in its recorded home. This route starts no second conversation or listener; an invalid recorded endpoint is not replaced with it.

Queue acceptance means the input is queued; delivery is reported only when the
lead's own conversation evidence contains it. A timeout or ambiguous failure
keeps the attempt indeterminate, and repeating the same message reconciles it
without another send. Legacy receipts without the originating home cannot use
the ordinary route. A changed registered upstream binary is also refused before
send: repair its installation registration before retrying. The native queue
boundary is exercised on CLI 0.162.1; unsupported routes retain their diagnostic
and recovery action rather than claiming delivery.

Stop only for explicit cancellation or a concrete necessity: demonstrated wrong direction, inability to progress, or a resource conflict the executor cannot resolve. Waiting, silence, slowness and timeout are not causes. The kit stop preserves files, slot and partial work, verifies recorded identity, closes only that run's surface and records the outcome; manual killing has none of those guarantees. Continue an ordinary interruption by exact-session resume, cache loss by restart, and release only by explicit lead decision.

## Provider-neutral cache-loss protection and recovery

Every observed managed executor host monitors its exact session's native
per-response input/cache counters, whatever provider and model its receipt
resolved - control-backed spawn and observed exact-session resume alike.
Support is proven at runtime: a recorded response with at least 100,000 input
tokens and 90% cached input warms that run, and only a warmed run is subject to
the stop policy. Provider branding, a static allowlist entry or
numeric-but-never-warmed counters prove nothing. After warmup, three
consecutive distinct responses each missing at least 100,000 tokens and 50% of
input trigger termination of the owned process tree. Cold starts, duplicate
observations, below-threshold responses that reset the consecutive count and
historical losses on resume do not count; earlier history on an explicit resume
can establish warmup but never a new miss. The host terminates before waiting
on control acknowledgments or writing its stop receipt; it does not wait for a
model, test or build to finish.

Windows file-change notifications and native events trigger bounded reads of
appended records. A one-second open-file size check covers delayed Windows
notifications; unchanged content is not periodically reread. Only the exact
session's counters are read, and no monitoring request is sent to the model.
Usage arrives after billing, so the three responses and an already in-flight
request can still cost money. This is a repeated-loss guard, not a currency cap
or a fix for the upstream cache. Missing, invalid or never-proven counters are
reported as unavailable coverage with the run's resolved provider and model.
Already running older hosts do not gain protection from installing a new
binary.

Routine guard status - `waiting-for-usage`, `warming`, `armed` and other
below-threshold transitions - stays in the receipt and host log; it adds no
terminal diagnostic line and no user warning. The run's own truncated cache-guard
journal `endpoint-<index>.cache-guard.log` beside the receipt carries those
status lines; the app-server child owns `endpoint-<index>.log` itself. Degraded coverage - invalid or
unavailable counters, rollout truncation, a session-identity change - is
delivered through Codex's native warning presentation when a native executor
frontend is attached, so it appears in that frontend's warning count and viewer
instead of the terminal input area.
A compatibility surface without a native consumer keeps the diagnostic on its
receipt/log surface and records `no-native-consumer` instead of claiming native
warning presentation. The receipt records each degraded diagnostic as
`native-sent`, `undelivered` or `no-native-consumer`: a native-format send is
not evidence that the user opened the viewer, an undelivered diagnostic is
never reported as a shown warning, and a delivery failure does not terminate
otherwise healthy model work. Cache loss stays a terminal non-success outcome,
not a dismissible warning.

The receipt's `cacheGuard` records the counters, the resolved `provider` and
`model`, `runtimeSupport` (`proven`, `unproven`, `invalid` or `unavailable`)
and `diagnosticDelivery` (`native-sent`, `undelivered` or
`no-native-consumer`). The failed watch result retains the stop cause and an
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
the single `ds` profile (DeepSeek V4.1-Flash, `max`) - with no substitution:
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
input bounds up front. Routine supervision runs at most once per 15 minutes of
wall-clock time, batched across active executors, as described with
`executor watch` above. Check earlier only for a delivered result, explicit
error, help request, new or corrected user instruction or concrete risk to
correctness or shared resources. Do independent work between events; when none
remains, keep blocking on the watcher instead of polling, rereading worker
source, diffs or logs, duplicating the investigation or manufacturing status
work. The interval is not a task deadline, and silence is not a stall.

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
files only, not credentials, services, ports or databases. Preserve aggregate
resource limits across workers. The parent does
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

If an assigned model is missing from the catalogue or an access, model or quota
error is returned, the parent briefly reports the cause and continues on the
configured route; it neither substitutes a provider silently nor falls back to
xAI/Z.AI. A retry is justified by changed circumstances; do not recheck the
same error on every step. Inspect saved partial work before reassignment. There
is no mandatory escalation ladder.

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

All assigned OpenAI models belong to Astra. The xAI and Z.AI routes remain
installed but are not used for assigned work; the automatic vision helper is
off. If a selected model lacks a capability, that must
become a visible limit or a separate explicit assignment, not a hidden ChatGPT
quota borrow. Routing settings are owned by the native subscription lifecycle
described in [subscription models](subscription-models.md).

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
