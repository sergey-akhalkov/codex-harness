# Grok Responses spike receipt

Live probe on 2026-09-15 against SuperGrok Heavy, OpenCodex still running on port 10100.
Command: `codex-harness xai-responses-probe --user-home <host> --evidence <private-temp>`.
OAuth came from the existing harness browser-oauth store (`~/.opencodex/auth.json`), not OpenCode and not a pay-as-you-go key.

Observed:
- HTTP 200 at `https://api.x.ai/v1/responses`
- reported model `grok-4.6`
- one function/tool call (`echo`) succeeded (`tools: true`)
- non-stream request (`streaming: false`); streaming was not required
- grant: `browser-oauth`
- no extra Grok CLI headers were needed beyond `Authorization: Bearer` and JSON Responses body
- Code Mode was not used; a plain Responses function tool was enough

This is sufficient to write `codex --profile xai` as Responses + auth helper, with `code_mode = false` unless a later Codex session proves otherwise.
Raw machine receipt (no tokens): `evidence/xai-responses-probe.json`.

## Codex 0.154 tool-turn interop receipt (2026-09-15)

`evidence/codex-0154-xai-tool-turn-interop.json` records the live verification after the kit
profile constraints (apply_patch tool omitted from the catalog, `web_search = "disabled"`,
`features.multi_agent = false`):

- the first Responses request of a tool turn is accepted and Grok executes a real function
  tool call whose output comes back;
- every follow-up request in the same turn fails with HTTP 400 because Codex 0.154 echoes
  reasoning items with `content: null`, which api.x.ai refuses to decode;
- replaying the identical request with only the `content` field removed succeeds, and the
  `encrypted_content` string is echoed byte-identical, so corruption is ruled out.

Native `codex --profile xai` therefore stays blocked on an upstream fix (Codex serialization
or xAI tolerance). Routing and uninstall tasks must not proceed past this gate; OpenCodex
stays connected.

Additional negative checks recorded in the JSON receipt: xAI Chat Completions works with the
same OAuth grant (both the tool call and the tool-result follow-up return HTTP 200), but
Codex 0.154 no longer accepts `wire_api = "chat"` for providers; and 0.154.0 was the latest
published `@openai/codex` at the time of the check, so no newer release fixes the echo yet.

## Shim verification receipt (2026-09-15)

The user approved a minimal kit-owned shim for the one-field defect. Implementation:
`crates/harness-core/src/xai_responses_shim.rs` plus the `xai-responses-shim` subcommand.
Live isolated verification against `https://api.x.ai` through the shim: `codex exec
--profile xai` exited 0, Grok ran a real shell tool call, wrote the unique marker to
proof.txt, completed the previously failing follow-up request, and returned the marker in
the final answer. Private evidence directory was removed after the check.

Launcher wiring verification (2026-09-15): through the managed launcher after a CoreOnly
rebuild, `codex -V` and `codex --profile zai -V` exit 0 without starting the shim, while
`codex exec --profile xai` starts it from the recorded bridge build, completes the same
marker tool turn end-to-end, and exits 0. Pester launcher checks: 179 assertions.

Real-home receipt (2026-09-15): after stopping the owned task instance and installing the
native subscriptions through the recorded bridge build, `codex exec --profile xai` from a
separate temporary git repository exited 0 with `model: grok-4.6`, `provider: xai`,
executed the shell tool call through the shim and returned the marker. Ordinary
`codex exec` showed `model: gpt-6-astra`, `provider: openai`
with no localhost routing; its only failure was the account's ChatGPT usage limit, which is
external to this change. The xai profile additionally sets `features.apps = false` and
disables the kit MCP servers per name, because the host base config contributes
`namespace`-typed MCP tools that api.x.ai rejects.

Autostart audit (2026-09-15, user-requested): the only OpenCodex-related autostart surface
is the owned task `codex-harness-subscriptions-<hash>`, whose XML carries a `LogonTrigger`
and `StartWhenAvailable`; unregistering it in the retirement steps removes that autostart.
No matching Windows services, HKCU/HKLM Run values, startup-folder entries or global npm
packages were present. Transitional hazard recorded for section 3: the PowerShell
subscription routing module still restarts the proxy task and re-injects
`openai_base_url` on `install.ps1 -SubscriptionsOnly`; dropping that path is part of
removing OpenCodex from kit lifecycle.

Feature-parity receipt (2026-09-15): with the shim translating the custom-tool family and
stripping `external_web_access`, a live `codex exec --profile xai` turn created a file with
a real `apply_patch` call (exit 0, no shell fallback; the wire trace showed byte-exact
patch input). The model occasionally starts the patch header as `*** Begin Patch ***` and
self-corrects after Codex's validator error, so apply_patch turns succeed with at most one
retry; the shim's function description carries the exact format. `web_search = "live"` and
`features.code_mode = true` are enabled (Codex warns that grok-4.6 does not advertise Code
Mode support; the translated exec declaration is accepted). MCP-namespace, multi-agent and
app-namespace tools remain disabled: api.x.ai rejects the `namespace` tool type and no
declarative translation exists for dynamically discovered sub-tools.

Code Mode receipt (2026-09-15): a live `codex exec --profile xai` turn invoked the
translated `exec` custom tool; the Code Mode isolate computed 6*7, emitted 42 through
`text()` and wrote the proof file. The turn's only failure was a final xAI 429 rate limit
after the successful tool call, external to the transport.

OpenCodex retirement receipt (2026-09-15/16): the subscription lifecycle now writes a
schema-version-2 state that owns only the native xAI profile and catalog. Install/Update retire
legacy version-1 records behind a recovery journal (task removal, link removal, state rewrite)
and Check reports legacy records as degraded until Update runs. On the live host the owned task
was stopped and unregistered (with its LogonTrigger autostart), the `.opencodex` config link, the
role link and `opencodex-catalog.json` were removed, the interrupted retirement journal was
recovered through the native manager, and Check now reports `connected` with no port. The
PowerShell routing module, the OpenCodex service host, login scripts and their tests were deleted;
`install.ps1 -SubscriptionsOnly` and the activation coordinator now call the native manager
directly, and `ConfigureRestart` is retired. After retirement a live `codex --profile xai` turn
still executed a real MCP tool call and apply_patch end-to-end (exit 0). Orphan
`codex-harness-subscriptions-*` tasks left by interrupted verification runs were removed; the
scheduler now holds none.

Post-retirement hardening (2026-09-16, user-reported session errors): the catalog now
sets `tool_mode: "code_mode"`, which removes Codex's "model does not advertise Code Mode
support" warning without forcing code-mode-only routing; and the shim drops `tool_choice`
from requests whose tool list is empty (auxiliary Codex requests such as compaction or
title generation), which api.x.ai rejects with `invalid-argument`. A live MCP +
apply_patch turn after both fixes ran warning-free and exited 0.

Parallel-session receipt (2026-09-16, user question): three concurrent `codex --profile xai`
sessions through a single shim process all exited 0, each wrote and verified its own unique
marker, all showed `provider: xai`, and no cross-session marker leakage was detected. The
shim's per-connection design (thread per TCP connection with its own `ToolAdaptation`,
`SseAdapter`, `ResponseFramer`, `Forwarder` and curl subprocess) has no shared mutable state
beyond the atomic connection counter.

Final acceptance receipt (2026-09-16): all nine scenarios passed in a single run from a
temporary git repository: xai tool turn (exit 0, unique marker), ordinary codex routing
(`provider: openai`), GPT default without proxy injection, Z.AI profile and key present,
Check `connected` with OpenCodex absent, auth failure with missing store (explicit error),
disconnect removes native wiring, reconnect restores it, and no OpenCodex task or proxy
remains in the scheduler or on port 10100.

Namespace parity receipt (2026-09-16): with namespace flattening enabled and
`multi_agent` plus the kit MCP servers re-enabled on the xai profile, a live turn through
the shim executed the real `nuphus/desktop_screen_size` MCP tool (`mcp: nuphus/
desktop_screen_size (completed)`), received `2520x1680`, and wrote the value into
mcp-proof.txt with apply_patch; exit 0. Fixture probing established the wire contract:
flat function names are not routed by Codex, while `function_call` items with `name` plus
`namespace` are routed and executed.
