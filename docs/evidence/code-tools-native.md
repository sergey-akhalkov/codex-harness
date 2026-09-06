# Native MCP and hook contract evidence

Observed on 2026-09-06 with the installed official `codex-cli 0.153.4`,
Windows native, PowerShell 7.6.5 and the existing Node runtime. The executable
was resolved from the kit's `harness/installation.json` through the installed
npm package's Windows vendor directory. No upstream package was installed or
updated, and no global configuration was changed.

The executable fixture is [mcp-fixture.mjs](../../tests/fixtures/code-tools-native/mcp-fixture.mjs).
It tests the MCP/hook protocol only; its output is **not language diagnostics**.
The runner is [code-tools-native.Tests.ps1](../../tests/code-tools-native.Tests.ps1).
It uses the existing native [consumer RPC helpers](../../tests/consumer-rpc.ps1)
and [Windows ConPTY fixture](../../tests/ConPty.cs).

## Execution

```powershell
pwsh -NoLogo -NoProfile -File tests/code-tools-native.Tests.ps1 -RunAgent -KeepProbe
```

The final recorded run passed 20 assertions. It made two bounded model requests:
one real native patch through an unprofiled app-server, and one real MCP mutation
through unprofiled CLI `exec`. Both used `gpt-6-astra` with low reasoning.
The ordinary test without `-RunAgent` does not request a model response.

Each run creates an isolated `CODEX_HOME`, workspace and private traces, plus a
uniquely named temporary source directory under the repository fixture folder.
The fixture's hooks are linked from this source directory to the isolated
user `hooks.json`. Existing authentication is available through a temporary
link; its contents are never printed. `-KeepProbe` retains evidence for
inspection. Normal cleanup unlinks references before deleting owned directories,
without traversing links to auth or repository sources.

## Observed contracts

| Boundary | Observation |
| --- | --- |
| MCP registration | Native `mcp add` registers the existing Node executable and repository fixture path; an unrelated base setting survives. |
| Global loading | Unprofiled app-server discovers the linked user hook and calls the registered MCP. No project MCP configuration or launcher profile is involved. |
| Live source | After changing the checkout-owned marker, a new app-server returns `SOURCE_VERSION_TWO`; standalone CLI also receives that version. No reinstall or deployment copy occurs. |
| Hook discovery | `hooks/list` reports `eventName=postToolUse`, `handlerType=mcpTool`, the user source, and initially `trustStatus=untrusted`. |
| Native review | TUI startup opens `Hooks need review`; entering `Review hooks`, inspecting the known fixture and pressing `t` in `/hooks` changes the active count from zero to one. A fresh app-server reports `trusted`. |
| Trust persistence | Only native TUI writes its trust state. No trust hash is synthesized, no internal trust write is performed, and no hook-trust bypass flag is supplied. The hooks symlink survives. |
| Hook source changes | Editing the checkout-owned hook's status message is visible to a new consumer, which reports `trustStatus=modified`; old trust does not authorize the new definition. |
| Native patch | The model calls `apply_patch` to create `hello.txt`. Exactly one automatic MCP hook follows it; the original file-change event remains `completed` and the written text survives. |
| MCP mutation | Standalone CLI calls `write_note` once. Exactly one automatic hook follows; the original `mcp_tool_call` retains `status=completed`, its structured content and source marker. |
| Model-visible delivery | A random nonce created only inside each hook response appears in the model's final answer. Neither user prompt contains it, and neither model calls the diagnostic tool manually. |
| Recursion | Each one-edit scenario records exactly one diagnostic hook call; calls initiated by the native MCP hook do not recursively trigger another hook. |

## Exact successful event and response shapes

The input template used whole-value substitutions for `cwd`, `session_id`,
`turn_id`, `tool_use_id`, `tool_name`, `tool_input`, and `tool_response`.
For the patch, `tool_input` was an object containing `command` and
`tool_response` was a string containing exit code zero and the original patch
output. For the MCP mutation, both were JSON objects. The server registration
name was `native-contract`; its hook tool name was normalized to
`mcp__native_contract__write_note`.

The successful handler uses `type: "mcp_tool"`, `server: "native-contract"`,
`tool: "diagnostics_after_tool"`, an `input` object, `timeout: 30`, and
`statusMessage`. Its matcher is `apply_patch|Bash|mcp__.*`.
The successful MCP `CallToolResult` envelope is:

```json
{
  "content": [{
    "type": "text",
    "text": "{\"hookSpecificOutput\":{\"hookEventName\":\"PostToolUse\",\"additionalContext\":\"automatic result\"}}"
  }]
}
```

No `decision`, `continue`, or tool-output replacement is needed. The native
app-server emits `hook/started` and `hook/completed`; the latter contains a
`context` entry. The recorded patch hook completed synchronously, and its
nonce was reproduced by the following final model message. These observations
match the [official hook contract](https://learn.chatgpt.com/docs/hooks) and
[App Server interface](https://learn.chatgpt.com/docs/app-server).

The native-generated app-server schema exposes `hooks/list` with `cwds`, and
returned hook metadata includes `currentHash`, `key`, `sourcePath`, and
`trustStatus`. No separate native hook-trust RPC was found in that generated
client-request schema. The tested supported trust route is the TUI.

## Limits and remaining acceptance

This closes the tested native transport/output/trust mechanism of task 1.2.
It provides CLI and unprofiled app-server evidence for task 1.1. Actual
installed desktop/IDE loading has not been exercised; it must not be inferred
from these results or from the launcher profile.

The fixture does not prove actual LSP analysis, shell-change detection,
initial-edit preservation before backend readiness, delayed-result
reconciliation, Stop/SubagentStop, child identity, concurrent workspaces,
or production package lifecycle. Those remain separate implementation and
acceptance work. An absent backend must not be interpreted as a clean result.

An initial TUI probe waited for a model banner while native hook review was
already blocking startup. The runner now recognizes that actual startup
review screen. The resulting wait failure was in the test expectation; no
global state or user window was changed.

Raw terminal, auth-linked probe state and model traces were kept only in owned
disposable locations while inspecting results, then removed. This document
retains the non-sensitive observations; it does not retain credentials or
claim the full MCP/LSP feature complete.
