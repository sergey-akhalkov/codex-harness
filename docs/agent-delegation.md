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
parameter selection applies to enabled external models. The subscription-owned
`middle` preset currently remains for lifecycle compatibility; it maps to
`xai/grok-4.6` and `xhigh` and is not required for direct selection. Its retirement
is still open. The former `grok_reviewer` remains retired.

Every active conversation must appear simultaneously in its own window or pane,
showing assignment, model/effort, messages/tool activity and status. Include any
model-backed helpers and explicitly show changes of leader. A switchable list
or a hidden transcript is insufficient. If required views disappear, suspend new
model requests, preserve in-flight work and restore visibility before continuing.
The controller's simultaneous views and automatic quota recovery are still
[under development](../openspec/changes/orchestrate-subscription-agents/tasks.md).
Until verified views are available, keep execution in the visible main conversation;
the previous single-TUI test is not simultaneous-view acceptance.

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
presets. The subscription [middle](../global/opencodex/agents/middle.toml) remains
temporarily connected by its owning lifecycle. Direct native selection does not
need these names. Do not create recursive worker trees; this remains policy until
the controller's shared task-wide ownership and concurrency checks are complete.

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
quota borrow. Settings live in [one JSON](../global/opencodex/config.json).
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

Model-free checks: `tests/consumer.Tests.ps1`,
`tests/subscription-config.Tests.ps1`, `tests/subscription-routing.Tests.ps1`
and `tests/delegation-usage.py`. Isolated lifecycle uses separate homes, port
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

The [usage counter](../tools/delegation-usage.py) takes explicit parent and
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

The counter is available from any directory through the global installation
record, without copying the script into a consuming project. Pass only needed
existing rollout files; keep results and detailed logs outside Git:

```powershell
$usageHome = if ($env:CODEX_HOME) { $env:CODEX_HOME } else { Join-Path $env:USERPROFILE '.codex' }
$usageState = Get-Content (Join-Path $usageHome 'harness/installation.json') -Raw | ConvertFrom-Json
$usageRegistry = Get-Content (Join-Path $usageHome 'harness/code-tools.json') -Raw | ConvertFrom-Json
$usagePython = ($usageRegistry.mcp | Where-Object id -eq 'serena').paths.python
$rolloutPaths = @('<parent-rollout.jsonl>', '<child-rollout.jsonl>')
& $usagePython -B (Join-Path $usageState.sourceRoot 'tools/delegation-usage.py') @rolloutPaths --format markdown --output (Join-Path $env:TEMP 'usage.md')
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
