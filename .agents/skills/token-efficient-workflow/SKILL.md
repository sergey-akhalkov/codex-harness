---
name: token-efficient-workflow
description: Use the installed RTK and Code Mode workflow when verbose tool output, repeated retrieval or task-effort selection is a meaningful cost in coding work. Covers raw recovery and native task boundaries; ordinary short edits need no extra workflow.
---

# Token-efficient workflow

Optimize the total time and context needed for the accepted result, including checks and corrections. Use the installed features; do not install or reconfigure tools merely to follow this skill.

## Shell output

For verbose native commands, use `harness-rtk.exe exec git log -n 80` (or `git status --short`, `rg -n`, `pytest`, `python -m pytest`, `uv run pytest`, `cargo test` with supported human-output options). The accepted hook changes only this entry mode to `compact`; the adapter filters eligible stdout after executing the native program once. It forwards actual arguments, cwd, environment, stdin, stderr and exit status. It intentionally selects a native executable, so use ordinary shell calls for shell functions/aliases. Codex omits the chosen shell from hook input; ordinary commands are therefore unchanged. Never retry to satisfy a hook. Automatic diagnostics and Stop hooks remain inactive.

A compressed result includes an `rtk raw` file locator. Inspect retained stdout when omitted details matter, without executing the command again. Storage keeps up to 32 captures of at most 4 MiB each; larger output passes raw. A locator is local evidence, not a durable report. For exact source, a final review diff, machine output or an exhaustive search, use the original command without the prefix. `HARNESS_RTK_DISABLE=1` bypasses compression; an already explicit `compact` call is never rewritten. Interactive output and unsupported actual arguments pass raw. Do not infer complete coverage from a shortened list. A filter failure leaves raw output usable with a diagnostic.

Use RTK's broader explicit commands only when their runner/format behavior fits the task. In particular, keep the selected Python/uv environment. `rtk gain` uses estimates; it is not a measure of session or subscription savings.

## Code Mode and source work

Use the available `functions.exec` host to orchestrate worthwhile independent calls and select relevant data before returning it to model context. Discover only the needed tool contracts. Await independent calls with `Promise.allSettled`, inspect each outcome and return failures/incomplete coverage with useful detail locators. Keep dependent reads, mutations and approvals sequential. Preserve `isError` and nonzero process exits even in fulfilled promises; do not rely on output truncation as a filter. Store large intermediate results inside Code Mode only when a later step needs them; return images using their image helper.

For code, first verify the MCP project root. Use CBM for relationships with a successful current index; reuse it across unchanged queries. Use Serena for exact symbols/references and suitable semantic changes. Refresh affected graph evidence after edits. Partial language coverage or a stale client requires a fresh client or scoped direct source, not another unchanged failing probe. Retrieve a bounded snippet, not the same source through several tools.

Prefer native `apply_patch` for bounded text edits. Use semantic edits or deterministic generators when they handle the actual change better. Reuse the same inspected context and completed check while its relevant source/config/runtime inputs remain unchanged. Changes, failures and unresolved concerns invalidate the affected evidence. Record accepted results once; avoid duplicate worker investigations and status-only polling.

## Reasoning at task boundaries

The installed CLI cannot change the effort of an already running model turn through an agent tool. Brevity instructions do not change that effort. Preserve current task continuity; do not restart a task just to chase a cheaper setting.

For a new local CLI task, the linked launcher accepts a first-position selector:

```powershell
codex --harness-effort routine exec 'A bounded task with a deterministic check'
codex --harness-effort standard 'A task needing substantive reasoning'
codex --harness-effort demanding 'A complex or high-risk task'
```

These select native `low`, `high`, and `xhigh` respectively. The default remains conservative `xhigh`. Explicit native effort/profile settings take precedence. Select routine only with clear inputs, low risk and a reliable check; use more effort when uncertainty or failures warrant it. For a supported external app-server client, `turn/start.effort` is a turn-boundary setting, not an in-turn model tool.

Keep preferred Grok middle and the installed Astra reserve/senior/principal assignments. This selector is not a reason to bypass Grok, add recursive model calls or change billing. A small task is often cheapest to finish directly. Validate effective effort from native turn/config evidence when measuring it; do not claim quota savings from effort names.
