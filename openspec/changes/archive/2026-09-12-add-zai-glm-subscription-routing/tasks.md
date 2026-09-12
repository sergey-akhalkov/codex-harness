## 1. Portable Z.AI provider

- [x] 1.1 Add `providers.zai` to `global/opencodex/config.json` from the pinned OpenCodex 2.44.0 registry seed (Coding Plan Chat URL, `openai-chat`, `authMode: "key"`, default `glm-5.3`, `selectedModels: ["glm-5.3"]`, environment-reference `apiKey` only) and verify the file still has no plaintext secrets and still defaults to OpenAI/Astra with xAI OAuth unchanged
- [x] 1.2 Extend `tools/opencodex-config-check.mjs` so environment or keychain API-key references are allowed and plaintext `apiKey` / `apiKeys` still fail, then verify `tests/subscription-config.Tests.ps1` covers valid zai-reference, rejected plaintext, and preserved xAI/middle assertions

## 2. Host-private key login

- [x] 2.1 Add a bounded Z.AI login helper in the same family as `tools/opencodex-login.ps1` that reads the Coding Plan key from stdin or a local form, writes an ACL-hardened host-private store under `CODEX_HOME/harness/subscriptions/`, and never reads `zai.config.toml`; verify an isolated run stores the key privately, prints no secret, and leaves local profile files unchanged
- [x] 2.2 Wire the helper into the subscription lifecycle docs/commands without making stock `ocx login zai` or `ocx provider add --api-key` the delivered path; verify Check/docs name the kit helper and isolated tests still reject plaintext writes through the linked config

## 3. Secret injection for the managed proxy

- [x] 3.1 Extend the bounded OpenCodex process runner so a named secret can become child env `ZAI_API_KEY` without placing the value in request JSON, argv, stdout, or stderr, then verify an isolated fixture process sees the env var while its request file does not contain the secret
- [x] 3.2 Point the managed subscription task at that injection path (user-scope env only as fallback) and verify a dry-run/Check of the service request still contains no credential material

## 4. Catalogue and compatibility

- [x] 4.1 After provider save/sync, assert the generated Codex catalogue lists `zai/glm-5.3` with visibility list, does not list other GLM family ids, and does not replace Astra or `xai/grok-*`; verify with an isolated OpenCodex home/catalogue fixture
- [x] 4.2 Keep global Code Mode enabled for Astra/Grok; keep the routed GLM entry on the pinned routed default (`tool_mode: code_mode_only`) so Codex does not warn on GLM, and verify a fixture session still has Code Mode for Astra while GLM is advertised as Code Mode capable
- [x] 4.3 Confirm the launcher still bypasses OpenCodex injection for explicit `--profile zai` / `-p zai` and verify an argument-classification test plus a hash/content check that local `zai.config.toml` and `zai.models.json` are unmodified by subscription operations

## 5. Lifecycle, docs, and decisions

- [x] 5.1 Update subscription Check/Install/Disconnect so they validate the zai provider row, do not delete the private key store or local profile, and do not claim GLM ready without authorization; verify isolated lifecycle tests and `-WhatIf` output
- [x] 5.2 Update `docs/subscription-models.md`, `docs/project-decisions.md`, and the subscription memory index route so they distinguish the preserved local Responses profile from OpenCodex `zai/glm-5.3`, state the Chat endpoint, and keep xAI as the previously verified OAuth provider; verify local-link and hygiene checks

## 6. Isolated automated evidence

- [x] 6.1 Add deterministic tests for validator, login store, runner injection, catalogue slug, profile-file non-mutation, and disconnect preservation; verify they pass without network and without writing into the live `CODEX_HOME` / `.opencodex`

## 7. Live global acceptance

- [x] 7.1 From an independent terminal, preview then apply subscription install/login so the running proxy loads `zai`, then verify a new ordinary Codex session outside this checkout shows `zai/glm-5.3` in `/model`
- [x] 7.2 Send a bounded `zai/glm-5.3` request through that ordinary session and verify proxy/provider evidence that it hit `https://api.z.ai/api/coding/paas/v4` as GLM, without printing the key
- [x] 7.3 Run `codex --profile zai` after the same install and verify it still uses the local Responses `/api/v1` profile, while a new unprofiled session still defaults to Astra and middle remains `xai/grok-4.6`
- [x] 7.4 Exercise a visible Z.AI failure (invalid/missing key or quota) in an isolated or bounded live probe and verify there is no silent fallback to xAI, another GLM id, or the local profile route

## 8. Ordinary picker allowlist

- [x] 8.1 Restrict the reusable OpenCodex catalogue so ordinary `/model` lists only `gpt-6-astra`, `xai/grok-4.6`, and `zai/glm-5.3` (hide other native GPT ids, other Grok ids, and synthetic `--fast` rows) without changing the local `zai` profile, Astra default, or Grok middle
- [x] 8.2 Extend source validation and isolated catalogue tests so extra GPT/Grok ids and `--fast` rows fail the contract
- [x] 8.3 Update subscription docs and decisions so the ordinary picker is described as exactly those three slugs
- [x] 8.4 Refresh the on-disk Codex catalogue without a destructive proxy restart from this session and verify `codex debug models` lists only the three visible slugs while `codex --profile zai` still lists bare `glm-5.3`. Live `GET /v1/models` on an already running proxy keeps its startup roster until that process is restarted from an independent terminal
