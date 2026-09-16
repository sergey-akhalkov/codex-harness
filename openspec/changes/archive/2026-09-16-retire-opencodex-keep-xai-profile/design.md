## Context

See proposal.md for why OpenCodex must go. Today Codex sessions inherit
`openai_base_url = http://127.0.0.1:10100/v1` from the managed proxy. Grok
OAuth already exists in harness-core (`subscription-login xai`) but it feeds
OpenCodex, not a Codex provider. Z.AI already has the desired shape:
`~/.codex/zai.config.toml` plus `codex --profile zai` on Responses with a
host-private key. Codex 0.154 custom providers accept only `wire_api =
"responses"` and can fetch tokens via `[model_providers.*.auth].command`.
OpenCode 1.18.29 authenticates the same public xAI client id and talks to
`https://api.x.ai/v1` with Chat Completions; that is evidence the
subscription reaches `api.x.ai`, not evidence that Codex Responses works.

## Goals / Non-Goals

**Goals:**

- Gate all routing and uninstall work on a live subscription Responses probe.
- Reuse Codex profile/provider machinery; add only the Rust OAuth/token helper
  Codex cannot do itself.
- Restore ordinary `codex` to native Astra with no localhost proxy.
- Remove owned OpenCodex process, task, package and injections after the gate.
- Keep Z.AI on its existing Responses profile and keep OpenCode's own OAuth.

**Non-Goals:**

- Do not make OpenCode the Codex runtime or share refresh ownership with it.
- Do not buy or require an xAI pay-as-you-go key.
- Do not close `orchestrate-subscription-agents`; only stop that work from
  treating OpenCodex as the Grok transport once this change succeeds.
- Do not keep a compatibility OpenCodex binary "just in case" after success.
- Do not change MCP, RTK, screenshot or instruction-floor work from earlier
  token-burn changes.

## Decisions

- **Spike before uninstall.** A bounded `codex-harness` probe (Rust) uses
  subscription OAuth, `POST https://api.x.ai/v1/responses` for `grok-4.6`, and
  one trivial tool. Success authorizes profile + retirement. Failure leaves
  OpenCodex in place. Alternative of uninstalling first was rejected: Grok in
  Codex would disappear if Responses is unsupported.
- **Native profile, not a new proxy.** Target `~/.codex/xai.config.toml`
  mirroring Z.AI: `model_provider`, `model = "grok-4.6"`, catalog JSON,
  `wire_api = "responses"`, `base_url = "https://api.x.ai/v1"`, auth command
  instead of a bearer token in the file. Alternative of keeping a tiny local
  translator was rejected: that is OpenCodex under another name.
- **User-approved one-field compatibility shim (2026-09-15).** Live evidence
  proved the pure native path blocked upstream: Codex 0.154 echoes Responses
  reasoning items with `content: null`, api.x.ai rejects exactly that shape on
  every follow-up request of a tool turn, no Codex configuration can change
  the serialization, `wire_api = "chat"` was removed from Codex 0.154, and
  0.154.0 was the latest published CLI. The user explicitly chose a minimal
  kit-owned shim over waiting. It is deliberately unlike OpenCodex: one JSON
  field removed from request bodies, byte-transparent streaming through the
  existing task-forward curl transport, Authorization passes through (no
  credential storage or refresh), no Responses-to-Chat rewriting (so no
  history re-send burn), no Task Scheduler entry, started by the launcher for
  `xai` sessions, and self-exits when no `codex.exe` process remains. The shim
  must be removed once Codex or xAI fixes the echo serialization; re-pointing
  the profile at `https://api.x.ai/v1` is then the whole rollback.
- **Shim scope after the feature-parity request (2026-09-15).** The user asked
  for feature parity with ordinary profiles. The shim therefore adapts the
  known Codex 0.154 <-> api.x.ai deltas and nothing else: custom tool
  declarations become function declarations (with the freeform contract moved
  into the description), echoed `custom_tool_call`/`custom_tool_call_output`
  items become their function equivalents, function calls for those tools are
  turned back into `custom_tool_call` items in the SSE stream (argument delta
  events for them are suppressed), chunked response framing is decoded before
  rewriting and re-encoded after, `external_web_access` is stripped from
  web_search declarations, and `content: null` is removed from echoed
  reasoning items. No history rewriting, no Responses-to-Chat conversion, no
  credential handling, and no support for `namespace` tools: MCP servers,
  multi-agent tools and the built-in app namespaces stay disabled on the xai
  profile until api.x.ai accepts the `namespace` tool type.
- **Namespace flattening (2026-09-15/16, user requirement).** The user made
  MCP, multi-agent and app namespaces a hard requirement for the shim. Codex
  namespace declarations carry the complete subtool list with JSON schemas,
  and Codex routes namespaced invocations expressed as `function_call` items
  with `name` plus a `namespace` field. The shim therefore flattens every
  `namespace` declaration into ordinary function declarations named
  `<namespace>__<subtool>`, rewrites echoed namespaced calls to the flattened
  name, and rewrites streamed flattened calls back into the namespaced form.
  Argument delta events pass through unchanged because the item type does not
  change. This restores the pre-namespace wire shape for api.x.ai without any
  discovery protocol emulation.
- **Owned Rust auth helper.** Codex will call a harness binary that prints a
  short-lived access token to stdout. Login continues as browser/device OAuth
  with the public xAI client already used by OpenCodex/OpenCode; tokens stay
  in `CODEX_HOME/harness/` (or equivalent private store), not in the profile
  TOML and not in OpenCode's `auth.json`. Alternative of pointing Codex at
  OpenCode's store was rejected: shared refresh would desync both clients.
- **Catalog owned by the kit, not OpenCodex.** Ship a small `xai.models.json`
  with verified `grok-4.6` window/effort, without `code_mode_only` and without
  900k GLM windows. Prefer `code_mode = false` on the xAI profile unless the
  spike shows Responses+tools need Code Mode. Alternative of reusing the
  OpenCodex catalog was rejected: it is how late compact and Code Mode got
  forced.
- **Default session is Astra.** After retirement, strip `openai_base_url`,
  OpenCodex catalog path, and `model = "xai/grok-4.6"` from the machine-local
  base config. Grok is `--profile xai` (or an explicit documented override).
  Alternative of keeping Grok as the base model without a proxy was rejected:
  ordinary sessions would still depend on xAI for every turn.
- **Lifecycle removes OpenCodex completely after the gate.** Install/Update
  stop connecting `global/opencodex`, the Windows task, and the npm/bun
  package. Disconnect/Check treat leftover localhost injection as unhealthy.
  OpenCodex sources may remain in git only as retired/historical files until a
  follow-up deletes unused code; they MUST NOT be live-linked. Alternative of
  a dormant package for rollback was rejected by the user: the point is to
  stop the proxy.
- **Z.AI unchanged as a pattern.** Do not move GLM onto OpenCodex or onto the
  xAI helper. `codex --profile zai` stays the Responses key path.

## Risks / Trade-offs

- [api.x.ai rejects Codex Responses / tools / streaming] -> Spike fails; keep
  OpenCodex; do not ship a half-removed transport. Record the exact status.
- [OAuth client/device vs localhost callback differs from OpenCode] -> Prefer
  the already delivered harness browser-only login if it yields tokens that
  `api.x.ai` accepts; otherwise add device-code in the same helper. Do not
  copy OpenCode tokens.
- [Codex profile cannot set all required Grok headers] -> Put stable non-secret
  headers in the provider table; if secret or per-request headers are required
  beyond bearer auth, the spike fails and OpenCodex stays.
- [Users still have `model = xai/grok-4.6` in base config] -> Retirement MUST
  restore native GPT and report leftover proxy keys; do not leave a broken
  default.
- [Orchestration still assumes a live proxy] -> After success, that change's
  remaining tasks use `--profile xai` / native provider, not OpenCodex. This
  change does not mark those tasks complete.
- [Token helper called on every turn] -> Use Codex `refresh_interval_ms` and
  a private cached access token; helper stays small and silent on success.

## Migration Plan

1. Implement and run the Responses spike against the live subscription from an
   isolated helper; keep OpenCodex running until it passes.
2. Add native login/helper, `xai` profile, catalog and installer wiring;
   verify `codex --profile xai` from another repo while the proxy still exists.
3. Stop the owned task, remove injections, disconnect OpenCodex, restore Astra
   as the ordinary model, verify Check and a GPT session without localhost.
4. Delete or retire live OpenCodex package connection; keep credentials.
5. Rollback if Grok profile breaks: reconnect OpenCodex only from the previous
   installer path while the spike result still says failure. After a recorded
   successful spike and uninstall, rollback is reinstall of the retired
   component, not a silent leftover proxy.

## Open Questions

None that change the specs. Exact OAuth grant (browser callback vs device
code) follows whichever grant the spike proves against `api.x.ai`.
