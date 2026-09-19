# Agent selection and visible conversations

Use ordinary native agents with explicit `model` and supported `reasoning_effort`
parameters. Choose from task complexity, risk and the full cost of the accepted
result; a separate TOML file per effort or activity is unnecessary. GPT normally
leads, Z.AI handles substantial text/code execution, and Grok handles visual and
suitable routine work. Supported efforts differ by model; verify the effective
binding instead of assuming every level exists on every route.

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

Every active conversation must appear simultaneously in its own window or pane,
showing assignment, model/effort, messages/tool activity and status. Include any
model-backed helpers and explicitly show changes of leader. A switchable list
or a hidden transcript is insufficient. If required views disappear, suspend new
model requests, preserve in-flight work and restore visibility before continuing.
The controller opens a window or pane per conversation before dispatch. If a
required view closes, it suspends new model requests, preserves in-flight work
and restores visibility before continuing. Do not capture Codex or sibling
agent windows with Nuphus screenshots, and do not poll window pixels for status.
Identify non-Codex windows with list, title, bounds and state; keep screenshots for
genuine visual questions about owned UI.

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
codex-harness executor spawn --source CHECKOUT --codex-home DIRECTORY --workspace DIRECTORY --exec "assignment"
```

That command uses `codex --profile xai` from the example above. An unlisted
`--profile` is an error. Explicit user `codex --profile <id>` keeps native
precedence over role configuration. Quota succession looks up the successor
profile's model in the native catalog; the verified handoff seed remains a
configured `zai/glm-5.3` binding, not a hardcoded provider role.

Spawn opens the assignment in a new tab of the current Windows terminal when
the lead already runs there (`WT_SESSION`, `wt -w 0 new-tab`). It does not pass
`--focus` / maximized / fullscreen, and it restores the previous foreground
window so another app is not yanked forward. If the lead is not in that
terminal, it falls back to a visible native TUI (`CREATE_NEW_CONSOLE`) with the
same restore. It does not use headless `codex exec --json`. Do not pass
`--worktree` together with `--remote`; attach with `-C` at the managed cwd.
Steering stays `executor steer` (`turn/start`), not TUI keystrokes.
Spawn returns after the tab or window is open so the lead keeps working.
The default exec mode streams the assignment in that visible tab and exits on
completion, so the tab closes itself; a mid-work stop is detected by the lead's
watcher and the exact session continues through `codex exec resume SESSION_ID`.
While an executor runs, one native watcher process checks the board review
queue, the assignment's result artifact and executor liveness and emits a
single event; the lead blocks on that event between other work instead of
polling executor process ids from model turns or reading window pixels.
Assignments live on the beads board; executors set `lead_review` when done
instead of closing. The lead reviews that inbox and its own `assignee=lead`
tasks.

The `team-lead` skill owns lead activation, briefs, steering, acceptance and
stop. Ordinary sessions without that activation spawn nothing.

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

Installed CLI 0.155.1 allocates experimental managed worktrees with
`--enable worktrees --worktree`. `codex exec` may pass that flag; the verified
`--remote` control TUI must not. Attach the window with `-C` at the managed
cwd. Disabled `worktrees` is an explicit limitation: executors do not write to
the shared checkout and do not fall back to an ordinary Git worktree.
Lanes are reused, not rebuilt: after an accepted merge the lead resets the lane
worktree to the new committed base (`git reset --hard <base>` plus
`git clean -fd`, so ignored build caches stay warm) and dispatches the next task
of that lane into it. `crates/harness-core/src/task_worktree.rs` implements the
reset (`reset_for_reuse` distinguishes a reused lane from unresettable state) and
the merge flow reports which of the two happened. Deletion remains the lane
retirement path: native confirmed deletion is TUI-only and refuses dirty,
untracked or ignored trees, so a lane that cannot be reset is preserved with its
reason instead of being deleted or reused blindly, and merged branches are kept.
Lane purpose lives in kit-local task state and board records, never in tracked
files. The authoritative inventory is `git worktree list`, and the configured
`worktree_limit` warns when registered worktrees reach it, because a lane that
was neither reset nor retired is the only way the count grows.

## How selection works

For independent substantial text/code work prefer Z.AI; for visual and suitable
routine work prefer Grok. A short edit,
tightly coupled slice or expensive context handoff is often cheaper to do
directly. Count briefing, execution, waiting, checking, integration and rework.
The number of children is not a savings metric.

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
