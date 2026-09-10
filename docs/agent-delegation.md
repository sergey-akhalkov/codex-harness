# Level-based delegation

The pack uses ordinary named Codex agents. The main session chooses how to work
from complexity, risk and the full cost of the accepted result. There is no
separate orchestrator, mandatory delegation cards or approval chain.

| Purpose | Agent | Model | Reasoning |
| --- | --- | --- | --- |
| Preferred middle | `middle` | `xai/grok-4.6` | `xhigh` |
| Backup middle when Grok is unavailable | `middle_backup` | `gpt-6-astra` | `high` |
| Main session; a separate hard slice when needed | `senior` | `gpt-6-astra` | `xhigh` |
| Rare consultation on a hard intellectual blocker | `principal` | `gpt-6-astra` | `max` |

Backup is an alternative middle, not an extra intelligence rung. Names describe
capability, not job titles. The former `grok_reviewer` is removed.

## How selection works

For sufficiently independent routine work the parent prefers Grok. A short edit,
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
authorize `middle_backup`. If there is no new reason to continue, the parent
finishes the bounded task directly and reports the defect.

After restoring a parent session, `resume_agent` can lose the previous Grok
binding and inherit Astra. Do not recover a saved Grok through that tool:
inspect partial work and hand it to a new `middle` with brief context. An old
model record does not prove a new binding. A service-state check error on an
old conversation does not mean the subscription is permanently unavailable.

The parent passes the goal, needed files, change bounds, invariants and
acceptance. Usually `fork_context=false`: needed context, not full history.
Two independent slices can run in parallel on non-overlapping files or separate
worktrees. While they run, the parent does other useful work, then checks the
result and important edge cases.

If Grok is missing from the catalogue or an access, model or quota error is
returned, the parent briefly reports the cause and uses Astra high or does the
task itself. A retry is justified by changed circumstances; do not recheck the
same error on every step. Inspect saved partial work before reassignment. A
transport error is not a reason to call principal. There is no mandatory
middle → senior → principal ladder.

## Global connection and limits

The [harness profile](../global/harness.config.toml) contains a short standing
policy and a limit of two concurrent child threads.
[Astra definitions](../global/agents/README.md) connect through a standing link
`~/.codex/agents/codex-harness`; [middle](../global/opencodex/agents/middle.toml)
is available through the subscription integration link. All four definitions
use `[agents] enabled=false`, so their own delegation is off. These are ordinary
[Codex subagent](https://learn.chatgpt.com/docs/agent-configuration/subagents)
settings.

`developer_instructions` is a scalar: the selected profile replaces the same
base config.toml value rather than concatenating strings. The base file remains
and applies again without that profile. AGENTS.md instructions still load;
explicit CLI and trusted-project settings follow native precedence. Selection
policy is an instruction to the agent, not a forced scheduler. Asking to limit
research by time or tokens does not by itself create a hard limit. Thread limit
and child-spawning prohibition are configuration; timeout and Windows Job Object
for acceptance probes are provided by a separate runner.

Sources are read through direct links on a new start. An already open tool
catalogue may remain previous. Disconnecting subscriptions removes only their
link; Astra levels and the main model remain. Install, check, disconnect and
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
and Windows task. Model probes are opt-in:

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

To replace a provider, change the `middle` binding, subscription configuration
and exact-model checks after confirming the authorized catalogue and
capabilities. Level policy stays the same. OpenAI variants must remain in the
Astra family.

After a main conversation has started, `/btw` and `/side` open a side question.
Return with the TUI prompt (`Ctrl+C`). Nested side chats and review mode are
unsupported. On CLI 0.153.4 both names work with `multi_agent_v2=false`. Side
chats do not isolate files; use a Git worktree for independent edits.
