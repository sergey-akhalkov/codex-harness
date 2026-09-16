## Why

OpenCodex is the always-on Responses-to-Chat proxy that currently carries
subscription Grok (and OpenCodex GLM) into Codex CLI. Measured sessions show
the burn is not the visible stream: the proxy advertises 500k/1M windows,
forces `code_mode_only`, and rewrites Responses into Chat Completions, so
history and reasoning are resent every turn. The user wants Grok through an
explicit Codex profile like Z.AI, and OpenCodex fully disconnected and
removed. OpenCode already authenticates SuperGrok Heavy over OAuth to
`https://api.x.ai/v1` without that proxy; Codex 0.154 custom providers speak
only Responses, so a native Rust helper is in scope if that wire works.

## What Changes

- Prove, before dependent routing work, that subscription Grok answers a
  real Codex-shaped Responses request at `https://api.x.ai/v1` with OAuth
  (`grok-cli:access` / `api:access`). No xAI pay-as-you-go key. If the
  spike fails, OpenCodex stays and this change stays incomplete.
- After that proof, ship `codex --profile xai` as a native Codex custom
  provider: Responses, owned OAuth token helper, host-private store, bounded
  catalog. Ordinary `codex` returns to Astra and no longer points at
  `127.0.0.1:10100`.
- Ship the smallest possible kit-owned local shim on `127.0.0.1` for the one
  verified Codex 0.154 <-> api.x.ai defect (echoed reasoning items with
  `content: null`): it strips only that field from Responses request bodies,
  streams everything else unchanged, stores no credentials, registers no
  scheduled task, is started by the launcher for `xai` sessions and exits when
  no `codex.exe` process remains. User-approved 2026-09-15 after the pure
  native path was proven blocked upstream; remove it again when Codex or xAI
  fixes the serialization.
- **BREAKING:** uninstall and delete the managed OpenCodex integration:
  background task, proxy, `openai_base_url` injection, OpenCodex catalog,
  linked `global/opencodex` runtime, and the OpenCodex package from kit
  lifecycle. Existing OpenCode OAuth remains untouched. Z.AI stays on
  `codex --profile zai` (Responses + key), not on a proxy.
- Grok is opt-in via the profile (or an explicit equivalent override), not
  the machine default. Native GPT remains the ordinary session. Delegation
  still selects Grok by model/effort when the profile or equivalent provider
  is available.
- Auth helper may be new Rust in this pack. It prints a short-lived access
  token to stdout for Codex `[model_providers.*.auth]`, refreshes privately,
  and never writes secrets into tracked source.

## Capabilities

### New Capabilities

- None.

### Modified Capabilities

- `subscription-model-routing`: Grok subscription access becomes a native
  Codex profile/provider; OpenCodex proxy, catalog and background host are
  removed after the Responses spike passes.
- `subscription-runtime-recovery`: recovery applies to the remaining native
  subscription pieces (login/helper/profile), not to a managed OpenCodex
  process or Task Scheduler proxy host.
- `linked-global-kit`: installation no longer connects or depends on
  OpenCodex; Grok/Z.AI profile wiring is part of the portable kit without a
  deployed proxy.

## Impact

- `crates/harness-core`, `crates/codex-harness`: login, lifecycle, launcher
  defaults, diagnostics; new OAuth/token helper if needed.
- `global/opencodex`, installer/subscription modules, portable
  `harness.config.toml` / catalog injection.
- Host: stop/remove owned Windows task, restore native `config.toml`
  provider, add `xai.config.toml` analog to the existing Z.AI profile.
- Docs and memory: subscription models, installation, project decisions.
- Open `orchestrate-subscription-agents` still needs Grok as an assignable
  executor; it must not keep OpenCodex as the required transport after this
  change succeeds.
- OpenCode remains a separate product; this pack does not adopt it as the
  Codex runtime.
