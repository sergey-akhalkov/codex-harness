## Context

See [proposal](proposal.md). Ordinary Codex sessions already reach OpenCodex
at `127.0.0.1:10100` with pinned **2.44.0**. The linked
`~/.opencodex/config.json` currently declares `openai` and `xai` only, so
`opencodex-catalog.json` lists Astra and `xai/grok-*`. The user-owned local
profile `codex --profile zai` already talks to Z.AI Responses on
`https://api.z.ai/api/v1` and is out of bounds for this change. The launcher
keeps explicit `--profile` / `-p` on its existing bypass, so that path does
not receive OpenCodex `-c` routing. Grok credentials live in local
`~/.opencodex/auth.json`; API keys are different: stock OpenCodex writes
`providers.*.apiKey` into `config.json`, and that file is a symlink to
tracked source.

## Goals / Non-Goals

**Goals:**

- Reuse the pinned OpenCodex `zai` registry preset instead of a custom
  provider id.
- Put `zai/glm-5.3` in the ordinary Codex `/model` list through the existing
  catalogue sync.
- Make that ordinary picker list exactly `gpt-6-astra`, `xai/grok-4.6`, and
  `zai/glm-5.3`.
- Keep tracked source credential-free while the managed proxy process can
  resolve the key at request time.
- Preserve the local `zai` profile, Astra default, Grok middle, and xAI OAuth.

**Non-Goals:**

- Editing or replacing `zai.config.toml` / `zai.models.json`.
- A GLM named subagent, GLM family models other than `glm-5.3`, or changing
  the default model.
- Upgrading OpenCodex, adding OAuth for Z.AI, or sharing one OpenCodex
  Responses wire with the local profile.
- Reading the local profile to copy the key.

## Decisions

1. **Two wires, one subscription.** Z.AI documents Codex as Responses on
   `https://api.z.ai/api/v1` and other coding tools as Chat on
   `https://api.z.ai/api/coding/paas/v4`. The local profile already uses the
   first. OpenCodex `zai` already uses the second with adapter
   `openai-chat`, default `glm-5.3`, and reasoning `low` / `high` / `max`.
   Keep both. Do not point OpenCodex at `/api/v1` to match the profile: the
   proxy translates Codex Responses into the provider Chat adapter, the same
   pattern as Grok. Alternatives rejected: one merged profile; copying the
   Responses URL into OpenCodex; deleting the local profile.

2. **Catalogue identity is `zai/glm-5.3`.** OpenCodex routed slugs are
   `<provider>/<model>`. That matches `xai/grok-4.6` and is the accepted
   picker id. Constrain the provider with `selectedModels: ["glm-5.3"]` so
   the static registry roster (5.2, flash, `[1m]` ids) does not flood
   `/model`. Do not register a bare `glm-5.3` slug in the shared catalogue:
   it collides with a native-looking id. The local profile may keep its own
   bare slug.

3. **Reusable provider row, secret by reference only.** Add `providers.zai`
   to [global/opencodex/config.json](../../../global/opencodex/config.json)
   from the registry seed: `authMode: "key"`, Coding Plan base URL, Chat
   adapter, default `glm-5.3`, `selectedModels: ["glm-5.3"]`. Put
   `apiKey` as an environment reference only (`"${ZAI_API_KEY}"` in JSON).
   The source validator currently rejects any `apiKey` key; extend it to
   allow only environment or keychain references, never plaintext.
   Alternatives rejected: plaintext key in source; stock `ocx login zai` /
   `ocx provider add zai --api-key` (both persist plaintext through the
   symlink); generating a local config.json copy that breaks live source
   links.

4. **Host-private key store plus process env, not request JSON.** Deliver a
   kit login helper in the same family as `tools/opencodex-login.ps1`: read
   the key from stdin or a local form, write it to an ACL-hardened file
   under `CODEX_HOME/harness/subscriptions/`, and never scrape the local
   `zai` profile. Extend the bounded process runner so the managed OpenCodex
   task can resolve a named secret into child env `ZAI_API_KEY` without
   putting the value in the request JSON, arguments, or logs. OpenCodex then
   resolves the environment reference at request time. User-scope
   environment is a fallback when the private store cannot be injected, not
   the primary store. Alternatives rejected: documenting manual `ocx`
   login; storing the key in linked `config.json`; passing the key through
   evidence request files (those files already serialize `environment`).

5. **Do not disable global Code Mode.** Astra and Grok sessions keep
   `features.code_mode`, and the routed `zai/glm-5.3` row advertises Code
   Mode like every other routed row (`tool_mode: "code_mode_only"`, the
   pinned OpenCodex default for routed chat providers). The earlier
   `codexToolMode: "shell"` opt-out removed that advertisement and made
   Codex warn on every GLM switch; the user rejected that warning. Do not
   set global `code_mode = false` to silence model-specific metadata gaps.

6. **Lifecycle stays the existing subscription component.** Install/Update
   publish the new provider row from source. Login is a separate bounded
   command. Check must prove: source has `zai` without plaintext secrets;
   local profile files are untouched; catalogue lists `zai/glm-5.3`; xAI
   OAuth still works. Disconnect removes owned OpenCodex routing and must
   not delete the Z.AI key store or the local `zai` profile. Restart of the
   managed proxy is required before an already running process sees the
   provider; an already open Codex session is not the check for the new
   catalogue.

7. **Ordinary picker allowlist.** Hide every native GPT id except
   `gpt-6-astra` with top-level `disabledModels`. Constrain xAI with
   `selectedModels: ["grok-4.6"]` (and the same `models` allowlist) so other
   Grok ids do not appear. Set `fastRows: false` so synthetic `--fast`
   selectors are not advertised. Direct requests for hidden ids are not the
   product path; the local `zai` profile catalogue stays independent.

## Risks / Trade-offs

- Linked `config.json` plus stock OpenCodex key login would write the secret
  into Git → do not document or invoke that path; validator and Check fail
  closed on plaintext; login helper owns authorization.
- `selectedModels` vs registry defaults → pin `glm-5.3` explicitly and
  assert the catalogue does not list other GLM ids.
- Native GPT roster and live xAI discovery → pin the three-slug picker in
  source validation so a later OpenCodex native or Grok id cannot re-enter
  `/model` unnoticed.
- Coding Plan Chat endpoint may reject a key that only works on Responses
  `/api/v1` → verify with an isolated live request during acceptance; a
  visible 401/403 is the failure, not a fallback to the local profile.
- Windows task environment currently serializes overrides into request JSON
  → secret injection must be a runner feature, not a hashtable value.
- OpenCodex keychain can fail in a non-interactive task → prefer kit-owned
  store plus env injection; keychain is not the primary path.
- Hiding Code Mode from the routed GLM row (`codexToolMode: "shell"`)
  triggers Codex's per-model warning → keep the routed default
  `tool_mode: code_mode_only`; keep Code Mode for Astra/Grok.
- Live catalogue writes are host-local → never commit `opencodex-catalog.json`.
- An already running OpenCodex process keeps its startup `disabledModels` /
  `fastRows` / xAI roster for live `GET /v1/models`. `ocx sync` updates the
  on-disk Codex catalogue that `/model` reads; restart the managed proxy from
  an independent terminal after sessions that use it have finished if the live
  OpenAI list must match.

## Migration Plan

Add the portable `zai` provider and validator exception for references only.
Add the private login helper and secret-resolving runner path. Preview with
`-WhatIf`, then `Install`/`SubscriptionsOnly` so the linked config gains
`zai` without touching the local profile. User pastes the Coding Plan key
into the kit login helper (same key as the profile, not copied from it).
Restart the managed proxy from an independent terminal after sessions that
use it have finished. Confirm `/model` shows `zai/glm-5.3`, a GLM request
hits `api.z.ai/api/coding/paas/v4`, and `codex --profile zai` still uses
`/api/v1`. Rollback is subscription Disconnect plus leaving the local
profile and private key store in place.

## Open Questions

None. Picker id, profile preservation, Chat vs Responses, and secret
boundary are decided. Remaining work is implementation and live evidence.
