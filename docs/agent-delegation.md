# Agent selection and visible conversations

Selection has two paths; never mix them. Ordinary in-session subagents take
explicit `model` and supported `reasoning_effort` parameters per assignment;
GPT normally leads, Z.AI handles substantial text/code execution, and Grok
handles visual and suitable routine work. Kit executor dispatch takes no
per-assignment model or effort: it runs exactly the configured executor
profiles, and the profile's configured model and reasoning effort are that
assignment's complete explicit selection. The kit configures one executor,
`ds`, binding DeepSeek V4.1-Flash (`deepseek-flash`) at `max`; one executor
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
parameter selection applies to enabled external models. Use `codex --profile xai` with `grok-4.6` / `xhigh` instead of any subscription
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
max_concurrent_executors = 2
```

Dispatch an executor with the installed launcher:

```powershell
codex-harness executor spawn --source CHECKOUT --codex-home DIRECTORY --base REV --exec "assignment"
```

Skills and `orchestration.toml` are live links into the kit checkout, while the
launcher is an immutable native build changed only by an explicit build and
install. A checkout updated after the last install can therefore reference an
`executor` command the installed launcher does not contain. Probe the launcher
before the first dispatch with `codex-harness executor --help`; it must print
the executor usage. `unsupported command` names a stale build, not unavailable
executors: rebuild and update through the
[installation lifecycle](installation.md#install-and-verify) from the source
root recorded in `CODEX_HOME/harness/installation.json`. Until the update,
orchestration stays blocked - no profile substitution, no raw `codex exec`, TUI
automation or in-session helpers, which would drop visibility, steering, board
and recovery guarantees. `codex-harness --version` prints the installed
build's source identity when its build record is present, so a stale launcher
is identifiable without guessing.

That command uses `codex --profile xai` from the example above. An unlisted
`--profile` is an error. Explicit user `codex --profile <id>` keeps native
precedence over role configuration. The live kit routes every executor
through `ds` (DeepSeek V4.1-Flash, `max`) and accepts no per-assignment model
or effort override by design: the profile is the selection. If any
instruction appears to demand per-assignment model/effort for executor
dispatch, that demand belongs to ordinary in-session agents; dispatch the
configured profile and report the discrepancy. Only a launcher- or
installation-check-reported failure - missing profile, stale build, no free
slot, fetch failure - blocks dispatch, with its exact cause and remedy;
never a routing-rule interpretation, the single executor profile or unknown
quota. Quota succession looks up the successor
profile's model in the native catalog; the verified handoff seed remains a
configured `zai/glm-5.3` binding, not a hardcoded provider role.

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
synthetic input into a conversation. If the lead is not in a terminal at all,
it falls back to a visible native TUI (`CREATE_NEW_CONSOLE`) with the same
restore. It does not use headless `codex exec --json`. Do not pass
`--worktree` together with `--remote`; attach with `-C` at the bound pool slot.
Steering stays `executor steer` (`turn/start`), not TUI keystrokes.
Spawn returns after the tab or window is open so the lead keeps working.
The default exec mode streams the assignment in that visible tab and exits on
completion, so the tab closes itself; a mid-work stop is detected by the lead's
watcher and the exact session continues through `codex-harness executor resume
--slot N --owner ID --session SESSION_ID` on its recorded slot, which rebinds
the slot without resetting partial work.
While an executor runs, one native watcher process checks the board review
queue, the assignment's result artifact and executor liveness and emits a
single event; the lead blocks on that event between other work instead of
polling executor process ids from model turns or reading window pixels.
Assignments live on the beads board; executors set `lead_review` when done
instead of closing. The lead reviews that inbox and its own `assignee=lead`
tasks.

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
stop. Ordinary sessions without that activation spawn nothing.

## Executor utilization

While the lead role is active, configured executor capacity is not left idle
by omission. At each checkpoint - session start, stage and epic planning,
after every dispatch decision, and after every acceptance or slot release -
the lead either dispatches the next worthwhile, capability-sized slice or
records the concrete reason the capacity stays idle: no worthwhile slice, an
unresolved dependency, configured pacing, a preserved slot, or dispatch being
unavailable with its reported cause. A routing-rule conflict, the single
configured executor profile, a model preference or unknown quota is not an
idle reason: dispatch proceeds and the discrepancy is reported. A released
slot is backfilled with the
next dispatchable slice before the lead starts unrelated implementation work.
Lead progress reports state busy slots against the configured concurrency
limit with each idle reason, taken from board and pool records rather than
window polling. Idle capacity without a recorded reason is a lead workflow
defect; manufactured filler work and delegation-count targets are not
remedies. Requirements live in the
[delegation](../openspec/specs/agent-delegation/spec.md) and
[orchestration](../openspec/specs/lead-agent-orchestration/spec.md)
specifications; the `team-lead` skill owns the live workflow wording.

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
promotes an item to the backlog in routing order: small improvements to backlog
tasks, behavior or requirement changes into OpenSpec, and kit instruction or
tool demand to the kit's own board with kit-level wording only. A material
correctness, integrity or safety finding promotes immediately under the lead's
consequence override with the reason recorded. The lead sweeps the incubator on
two triggers it already observes - closing a stage or epic during acceptance,
and a triage batch finding it above `incubator_size_cap` - archiving stale items
with visible reasons instead of deleting evidence. The `board-workflow` skill
owns the record formats; the `team-lead` skill owns the workflow.

Pacing uses three scoped sources only: the native Codex/GPT limit snapshot the
CLI records for itself, actual provider refusals, and bounded dashboard
snapshots the user supplies. Unknown stays unknown - no probe call and no local
request-count remainder - and unknown holds the configured limits rather than
raising them. Below 70% used keeps configured concurrency and cadence, 70% or
more halves new concurrency and the triage batch (at least 1), and 90% or more,
or an observed refusal, waits for the reset with concurrency 1 and a `low`
effort ceiling. Pacing changes new assignments only: a healthy executor keeps
its slot, model and instructions, and tasks released by one reset are spread by
a 120-second stagger instead of bursting together. Decisions are recorded on the
board with reason, basis and expiry, and withdrawn with a revoke record.

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
sharing one checkout. An interrupted session resumes through
`codex-harness executor resume --slot N --owner ID --session SESSION_ID`:
the recorded slot is rebound for the same owner without fetch, reset or clean,
so partial work survives, while a fresh spawn resynchronizes and never
continues a dirty slot. Hand-editing dispatch receipts for `executor run` is
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
presence, tree state, state, lease, owner, base, disposition, reason) plus
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

Combine related routine into one substantial assignment. Splitting a pair of
short functions between two children increased parent time and spend in the
first comparison. State material input bounds up front. When only waiting
remains, use a bounded 30–60 second wait instead of frequent polling; lack of
progress must be visible.

That interval is not the assignment deadline. A message to a working agent must
add facts, correct an established error or change the task. An empty or
intermediate result requires checking current work; by itself it does not
establish quota exhaustion or authorize automatic GPT takeover. Reconcile the
delivery mechanism and partial result before continuation or reassignment; return
in-scope corrections to the capable original executor.

After restoring a parent session, `resume_agent` can lose the previous Grok
binding and inherit Astra. Do not recover a saved Grok through that tool:
inspect partial work and hand it to a fresh explicitly selected Grok with brief context. An old
model record does not prove a new binding. A service-state check error on an
old conversation does not mean the subscription is permanently unavailable.

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
./tests/subscription-consumer.Tests.ps1 -RunModelProbes -GrokModel xai/grok-4.6 -GrokReasoningEffort xhigh -Scenario Delegation
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
