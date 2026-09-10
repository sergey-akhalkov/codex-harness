# External provider subscriptions in Codex CLI

Status: globally connected. Uses official Codex CLI with the existing kit
launcher and pinned [OpenCodex](https://github.com/lidge-jun/opencodex) **2.44.0**,
source commit `07b48da8fd63881e848d26e0bd50087864f5573e`. OpenCodex accepts
requests on `127.0.0.1:10100` and forwards them to the selected provider. The
main model remains GPT-6 Astra. The [middle](../global/opencodex/agents/middle.toml)
role selects `xai/grok-4.6` and reasoning `xhigh`. See
[subscription-model-routing](../openspec/specs/subscription-model-routing/spec.md)
and [project decisions](project-decisions.md#subscriptions).

## Connect and authorize

From the pack checkout:

```powershell
./install.ps1 -WhatIf
./install.ps1
./tools/opencodex-login.ps1
./install.ps1 -Mode Check
```

The installer reuses a verified dependency version or installs it in a separate
user directory. `~/.opencodex/config.json` is a direct link to
[the source JSON](../global/opencodex/config.json). Role definitions connect
through `~/.codex/agents/codex-harness-subscriptions`. A hidden Windows task
for the current user starts a foreground process inside a Windows Job Object
with a 2048 MiB limit. After process failure the host retries up to three times
at one-minute intervals. After those attempts the service stays stopped and
errors remain in the log. Ownership and configuration errors do not retry.

On a machine that already has the kit, `./install.ps1 -SubscriptionsOnly` adds
only subscriptions. This mode uses the same component, operation lock and
recovery journal. The Windows task selects an ordinary PowerShell 7.4+
installation, not Microsoft Store PowerShell. Interpreter selection does not
change user PATH or running terminals. The task keeps the installer privilege
level: `HighestAvailable` from an elevated process, otherwise `LeastPrivilege`.
Startup readiness waits at most 90 seconds; stop waits for the owned process
before cleaning its records.

Login opens a local page that continues to xAI. If xAI issues a one-time code,
paste it into that local form; do not send the code in chat. Login is limited
to six minutes and a separate 768 MiB job. Use separate OAuth; do not copy
OpenCode tokens. Do not use ordinary `ocx login` in a process without
interactive stdin: the pinned version reproduced a closed-stdin retry defect.

OAuth stays in `~/.opencodex/auth.json`; logs and installation state stay on
the machine, outside the repository. After moving the pack to another computer,
authorize again. Do not add keys, access/refresh tokens or Authorization
headers to JSON or role TOML.

Local API-key generation through OpenCodex management stores `apiKeys` in its
JSON. That operation is unsupported for the linked portable configuration: the
validator forbids credential-bearing fields, including `apiKeys` and tokens in
role files. Field-name checks do not replace scanning arbitrary strings before
saving to Git.

## Main model and named agents

From any project:

```powershell
codex
codex -m xai/grok-4.6 -c 'model_reasoning_effort="xhigh"'
```

The first start keeps the profile model GPT-6 Astra. The second explicitly
selects Grok. In a GPT session ask: "Have agent middle review …". The native
role sets the model independently of the parent. Check session metadata and
actual proxy requests, not how the agent names itself.

To add another role, create TOML in `global/opencodex/agents/` from
`middle.toml`. Provide unique `name`, `description`, `developer_instructions`,
exact `model = "provider/model-id"` and a supported `model_reasoning_effort`.
For this integration `model_provider = "openai"` keeps Codex native transport
to the local proxy; the `model` prefix selects the actual external provider.
Model fields belong in the role file, not in the main config.toml
`[agents.role]` table. Recheck the contract against the installed
[Codex subagents](https://learn.chatgpt.com/docs/agent-configuration/subagents)
version.

Mixed delegation uses v1. Passing full history and encrypted v2 tasks between
different providers is outside the verified configuration. Automatic model
substitution, fallback and aliases that override an exact identifier are not
configured. An unavailable assigned model should produce a visible error.

## Models, subscription and other providers

An authorized `/v1/models` listing returned `grok-4.6` and `grok-4.5`. A
verified `grok-4.6` request received a server name `grok-4.6-build`. That name
came from xAI; OpenCodex did not choose a different model. The static OpenCodex
catalogue contains other Grok IDs, so a menu string alone does not prove account
availability.

With `authMode: "oauth"` the pinned
[xAI transport](https://github.com/lidge-jun/opencodex/blob/07b48da8fd63881e848d26e0bd50087864f5573e/src/providers/xai-transport.ts)
sends requests to `https://cli-chat-proxy.grok.com/v1`. A paid API with a
separate key is a different mode and is not enabled here. Server-estimated cost
fields are not proof of a separate charge. Access is defined by the
subscription; xAI describes subscription use in external tools in
[Grok for Kilo Code](https://x.ai/news/grok-kilocode).

OpenCodex can add providers in `providers`, perform the matching authorization
and select `provider/model-id` as the main model or a role. For each new
provider, check whether the specific subscription supports OAuth, which models
the authorized catalogue returns, and which address serves the request. An API
adapter does not prove consumer-subscription support. Only xAI is confirmed
here. The pack's browser-only helper is currently implemented for xAI.

Adapter search is explicitly set to Grok 4.6 through xAI OAuth; the automatic
vision helper is off. All assigned OpenAI models belong to the Astra family.
Levels, backup and spend rules are in [agent delegation](agent-delegation.md).

Main GPT still uses Codex authorization. Routing changes apply to new native
sessions; an already running session is not a check of the updated catalogue.

## Update, stop and recovery

To enable the restart policy on an already installed service without stopping
the running process:

```powershell
./install.ps1 -SubscriptionsOnly -Mode ConfigureRestart -WhatIf
./install.ps1 -SubscriptionsOnly -Mode ConfigureRestart
./install.ps1 -SubscriptionsOnly -Mode Check
```

The mode changes only restart parameters of the owned task and its owner
record. Repeat calls are idempotent. If the update is interrupted,
`./install.ps1 -SubscriptionsOnly -Mode Recover` rolls it back from a separate
journal without stopping the proxy. Foreign edits are preserved and require
conflict resolution. New `Install`/`Update` create the task with the same
policy.

This is a background host started at current-user logon through Task Scheduler.
[RestartCount](https://learn.microsoft.com/en-us/windows/win32/taskschd/tasksettings-restartcount)
and [RestartInterval](https://learn.microsoft.com/en-us/windows/win32/taskschd/tasksettings-restartinterval)
cover launch failures; a native check showed that a crash of an already running
process needs the host's own recovery loop. An already running old host needs a
one-time reconnect through `Install` from an independent terminal to load new
code. A current request may break on crash. Grok returns to the catalogue after
the new process is ready; an already open session with the old catalogue needs a
new Codex start. OAuth or request errors without process crash do not restart
the service.

After retries are exhausted, use `./install.ps1 -SubscriptionsOnly -Mode Install`
from an independent terminal. It restarts the component, so finish sessions that
use it first. Successful recovery does not identify the native crash cause or
prove future crashes will not happen.

```powershell
./install.ps1 -Mode Update -WhatIf
./install.ps1 -Mode Update
./install.ps1 -Mode Check
./install.ps1 -Mode Recover
./install.ps1 -Mode Disconnect
```

`Update` applies pack sources and the pinned dependency. Moving to a new
OpenCodex version requires changing [the declaration](../global/opencodex/dependency.json)
and rechecking contracts. `Recover` handles an interrupted transaction; if
foreign processes changed files, it keeps the journal and reports a conflict.
After a stopped integration is recovered, `Install` connects it again.

Full `Disconnect` disconnects the kit together with its MCP and subscriptions.
OAuth is not deleted. To disconnect only subscriptions, use
`Invoke-HarnessSubscriptionRouting -Mode Disconnect` in
[the module](../tools/subscription-routing.psm1) with the same SourceRoot,
UserHome and CodexHome used at install. Disconnect removes owned links and
returns native Codex routing.

Everyday disconnect and restore of only subscriptions, keeping MCP and other
kit capabilities:

```powershell
./install.ps1 -SubscriptionsOnly -Mode Disconnect
./install.ps1 -SubscriptionsOnly -Mode Install
```

If a combined installation is interrupted, `-SubscriptionsOnly` does not bypass
its recovery: run `./install.ps1 -Mode Recover` first.

Stopping the proxy breaks Codex sessions that use it. Returning the native
route on disk applies to new launches; an already open session may keep trying
the old address. Run stop and disconnect commands from an independent terminal
after finishing sessions that depend on the proxy. A global lifecycle test from
Codex is forbidden; isolated checks use separate directories, port and Windows
task. Memory containment remains enabled.

To observe an already running global service, use
[the readiness probe](../tests/subscription-global.Tests.ps1)
`-RunReadinessProbe -BaselinePath <path-to-previous-hashes>`. It does not change
the installation: it checks the same process for at least two minutes, runs
native inspection commands outside the checkout, and verifies configuration and
authorization remain intact. The check fails if readiness is lost and stores a
report without restarting the service.

Bounded-process logs live in `~/.codex/harness/subscriptions/runs/`; results
contain completion reason, exit code and peak memory, without tokens. Exit 124
is timeout, 125 is the memory limit. The 2048 MiB state applies to the whole
OpenCodex process job; it is not a limit for the entire Codex CLI, browser or
other programs. Actual task restart and a Windows reboot are different checks;
a reboot without execution is not claimed.
