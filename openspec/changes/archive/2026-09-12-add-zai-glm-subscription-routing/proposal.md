## Why

The Z.AI GLM Coding Plan is already usable through the local file profile
`codex --profile zai`, but the ordinary Codex `/model` picker only lists Astra
and SuperGrok models routed by OpenCodex. Switching to GLM currently requires a
separate profile instead of the same global session that already hosts Grok.

## What Changes

- Add the pinned OpenCodex `zai` provider to the reusable subscription
  configuration so ordinary Codex sessions can select `zai/glm-5.3` from
  `/model` through the existing local proxy.
- Keep the existing local `zai` file profile and its Responses route
  unchanged. `codex --profile zai` and the OpenCodex route MUST both remain
  usable.
- Store the Z.AI API key in host-private state. The linked OpenCodex source
  configuration MUST remain credential-free; stock OpenCodex key-login that
  writes `apiKey` into that file is not the delivered path.
- Keep GPT-6 Astra as the default model and Grok as the middle role. This
  change does not add a GLM named agent, does not enable GLM family models
  beyond `glm-5.3`, and does not change xAI OAuth.
- Restrict the ordinary Codex `/model` picker to exactly `gpt-6-astra`,
  `xai/grok-4.6`, and `zai/glm-5.3`. Other native GPT ids, other Grok ids,
  other GLM family ids, and synthetic `--fast` rows stay out of that list.
- Document and verify the two Z.AI wires: the preserved local profile uses
  Responses on `https://api.z.ai/api/v1`; OpenCodex uses the Coding Plan Chat
  endpoint `https://api.z.ai/api/coding/paas/v4`.

## Capabilities

### New Capabilities

- None. Z.AI access extends the existing subscription routing capability.

### Modified Capabilities

- `subscription-model-routing`: expose a verified Z.AI GLM Coding Plan model
  in the ordinary Codex catalogue while preserving the independent local
  `zai` profile, keeping credentials off tracked source, and leaving Astra
  default plus Grok middle unchanged. Ordinary `/model` lists only
  `gpt-6-astra`, `xai/grok-4.6`, and `zai/glm-5.3`.

## Impact

Reusable OpenCodex provider configuration, subscription source validation,
host-private key login/install lifecycle, generated Codex catalogue, launcher
profile bypass, and subscription documentation/decisions. The pinned OpenCodex
**2.44.0** dependency stays. Live global activation, a real `/model` listing
outside this checkout, a GLM request through the proxy, and an unchanged
`codex --profile zai` path are required for completion. Private keys, the
local `zai` profile files, and runtime catalogues remain outside Git.
