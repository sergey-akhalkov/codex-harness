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
./install.ps1 -WhatIf
./install.ps1
codex-harness subscription-login xai --source CHECKOUT --codex-home DIRECTORY --user-home DIRECTORY
codex-harness subscription-login zai --source CHECKOUT --codex-home DIRECTORY --user-home DIRECTORY --key-file FILE
./install.ps1 -Mode Check
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

The shim also decodes chunked HTTP response framing before SSE rewriting and
re-encodes it for the client. It removes `tool_choice` from requests whose
tool list is empty. It stores no credentials (Authorization passes through),
registers no scheduled task, and exits when no `codex.exe` process remains.
Remove the shim when Codex or xAI fixes the serialization; re-pointing the
profile at `https://api.x.ai/v1` is the whole rollback.

Each TCP connection gets its own thread with isolated adaptation state; the
only shared state is an atomic connection counter. Multiple concurrent Grok
sessions are safe.

## Update, stop and recovery

```powershell
./install.ps1 -Mode Update -WhatIf
./install.ps1 -Mode Update
./install.ps1 -Mode Check
./install.ps1 -Mode Recover
./install.ps1 -Mode Disconnect
```

`Update` applies pack sources and rewrites the xAI profile. `Recover`
completes an interrupted retirement journal or rolls it back. `Disconnect`
removes the xAI profile, catalog link and ownership state; the Z.AI key file
and local profile stay in place.

Everyday disconnect and restore of only subscriptions:

```powershell
./install.ps1 -SubscriptionsOnly -Mode Disconnect
./install.ps1 -SubscriptionsOnly -Mode Install
```

The `ConfigureRestart` mode is retired together with the OpenCodex proxy; no
subscription task or background process is managed.
