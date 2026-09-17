# External provider subscriptions in Codex CLI

Status: globally connected. Grok runs through the native `codex --profile xai`
provider with a kit-owned local compatibility shim; Z.AI keeps its existing
`codex --profile zai` Responses profile. The OpenCodex proxy, its Windows task
and its runtime sources are retired. See
[subscription-model-routing](../openspec/specs/subscription-model-routing/spec.md)
and [project decisions](project-decisions.md#subscriptions).

## Connect and authorize

From the pack checkout:

```powershell
& <build>\codex-harness.exe install --subscriptions-only --source . --codex-home <CODEX_HOME> --user-home <USER_HOME> --preview
& <build>\codex-harness.exe install --subscriptions-only --source . --codex-home <CODEX_HOME> --user-home <USER_HOME>
codex-harness subscription-login xai --source CHECKOUT --codex-home DIRECTORY --user-home DIRECTORY
codex-harness subscription-login zai --source CHECKOUT --codex-home DIRECTORY --user-home DIRECTORY --key-file FILE
& <build>\codex-harness.exe check --subscriptions-only --source . --codex-home <CODEX_HOME> --user-home <USER_HOME>
```

The native login writes a private OAuth store under
`CODEX_HOME/harness/subscriptions/xai-oauth.json`. It never reads or writes
OpenCode `auth.json`. Login opens a local page that continues to xAI; if xAI
issues a one-time code, paste it into that local form. Do not send codes in
chat. Login is limited to six minutes.

The Z.AI login writes an ACL-hardened key file under
`CODEX_HOME/harness/subscriptions/zai-key.txt`. The local
`codex --profile zai` files are preserved, not managed by the installer.

The installer writes `xai.config.toml` and links `xai.models.json` into
`CODEX_HOME`. The profile points at a local compatibility shim on
`127.0.0.1:56122` that forwards to `https://api.x.ai/v1`. No `openai_base_url`
is injected into the ordinary base `config.toml`; the ordinary default model
remains GPT-6 Astra.

After moving the pack to another computer, authorize again. Do not add keys,
access/refresh tokens or Authorization headers to JSON or TOML in the repo.

## Model selection

From any project:

```powershell
codex
codex --profile xai
codex --profile zai
```

Ordinary `codex` uses the shared GPT-6 Astra default. Grok is opt-in via
`--profile xai` (model `grok-4.6`, provider `xai`, reasoning `xhigh`).
Z.AI GLM-5.3 is opt-in via `--profile zai`. The launcher starts the shim
only for `xai`-profile invocations; it self-exits when no `codex.exe` process
remains.
The started process is the selected build's `codex-harness.exe`. A leftover
shim holding `56122` is reused only while it belongs to the selected build; a
shim from an earlier build is replaced on the next `xai` launch (see the
compatibility shim section). It self-exits when no `codex.exe` remains.

For delegated work select the model and a supported effort directly. The
delegation rules live in [agent delegation](agent-delegation.md).

## Compatibility shim

Codex 0.154 and api.x.ai have wire-format mismatches that the shim adapts on
`127.0.0.1:56122`:

1. Codex echoes Responses `reasoning` items with `content: null`, which
   api.x.ai rejects. The shim removes that field.
2. Codex declares `custom` tool types (apply_patch, Code Mode exec) that
   api.x.ai does not accept. The shim translates declarations to `function`
   with the freeform contract in the description, and rewrites streamed
   `function_call` items for those tools back into `custom_tool_call`.
3. Codex uses `namespace` tool declarations for MCP, multi-agent and app
   subtools. The shim flattens them into per-subtool `function` declarations
   and rewrites calls bidirectionally.
4. The `web_search` tool carries an `external_web_access` field that
   api.x.ai rejects. The shim strips it.

5. Grok often emits whole numbers as JSON floats (`30000.0`) in tool
   arguments. Codex 0.154 rejects those for integer fields (`u64`, `i32`,
   `usize`), so the shim rewrites whole floats to integers before the
   client sees the call.
6. Grok often decorates the patch markers (`*** Begin Patch ***`,
   `*** End Patch ***`, `*** End of File ***`). Codex's apply_patch
   validator accepts only the undecorated marker lines and otherwise rejects
   the whole call, so the shim rewrites just those marker lines and keeps the
   patch body byte-identical.

The shim decodes chunked HTTP response framing before SSE rewriting whenever
the request declares tools, including namespace-only MCP turns that have no
`custom` tools, and re-encodes the stream for the client. It removes
`tool_choice` from requests whose tool list is empty. It stores no credentials
(Authorization passes through), registers no scheduled task, and exits when no
`codex.exe` process remains.

A chunked Responses body that arrives in the same read as the HTTP head is
decoded and rewritten too; otherwise a one-packet MCP `function_call` keeps
the flattened `namespace__name` form, Codex does not route it, and the TUI
can stay on that MCP name instead of `Working`.
Streamed tool arguments are taken by Codex from `response.output_item.done`
and `response.completed` (verified with a scripted upstream), which is exactly
where the shim applies its argument rewriting.

The shim reports its own build identity on
`http://127.0.0.1:56122/__harness/xai-shim/identity` and honors a retirement
request on `/__harness/xai-shim/retire`. The launcher reuses a running shim
only when that identity is the selected build's `codex-harness.exe`; a shim
left over from an earlier build is retired (in-flight streams drain first) and
replaced, so fixes actually reach new sessions. A listener without identity
reporting (a shim from before this check) is reused with an explicit notice
and is replaced once all Codex sessions have exited.
Remove the shim when Codex or xAI fixes the serialization; re-pointing the
profile at `https://api.x.ai/v1` is the whole rollback.

Each TCP connection gets its own thread with isolated adaptation state; the
only shared state is an atomic connection counter. Multiple concurrent Grok
sessions are safe.

## Update, stop and recovery

```powershell
& <build>\codex-harness.exe update     --subscriptions-only --source . --codex-home <CODEX_HOME> --user-home <USER_HOME>
& <build>\codex-harness.exe check      --subscriptions-only --source . --codex-home <CODEX_HOME> --user-home <USER_HOME>
& <build>\codex-harness.exe recover    --subscriptions-only --source . --codex-home <CODEX_HOME> --user-home <USER_HOME>
& <build>\codex-harness.exe disconnect --subscriptions-only --source . --codex-home <CODEX_HOME> --user-home <USER_HOME>
```

`Update` applies pack sources and rewrites the xAI profile. `Recover`
completes an interrupted retirement journal or rolls it back. `Disconnect`
removes the xAI profile, catalog link and ownership state; the Z.AI key file
and local profile stay in place.

Everyday disconnect and restore of only subscriptions:

```powershell
& <build>\codex-harness.exe disconnect --subscriptions-only --source . --codex-home <CODEX_HOME> --user-home <USER_HOME>
& <build>\codex-harness.exe install    --subscriptions-only --source . --codex-home <CODEX_HOME> --user-home <USER_HOME>
```

The `ConfigureRestart` mode is retired together with the OpenCodex proxy; no
subscription task or background process is managed.
