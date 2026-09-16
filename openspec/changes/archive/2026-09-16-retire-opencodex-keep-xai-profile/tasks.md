## 1. Prove subscription Grok on Codex Responses

- [x] 1.1 Add a bounded native probe that uses existing harness xAI OAuth (no OpenCode token copy, no pay-as-you-go key) to `POST https://api.x.ai/v1/responses` for `grok-4.6` with one tool call; verify it records HTTP status, model id and whether tools/streaming work, without writing access tokens into git or diagnostics.
- [x] 1.2 Run that probe against the live SuperGrok Heavy account from an isolated helper while OpenCodex remains running; if it fails, stop this change with OpenCodex still connected and do not implement profile or uninstall tasks.
- [x] 1.3 If the probe succeeds, record the working grant (browser vs device), required headers, and that Code Mode is unnecessary unless the probe proved otherwise; verify the receipt is enough to write the xAI profile without guessing protocol details.

## 2. Native xAI profile and token helper

- [x] 2.1 Implement the Rust auth helper Codex can exec: private store, refresh, stdout access token only; verify deterministic tests cover missing/expired credentials, empty stdout, and that secrets never appear in tracked files.
- [x] 2.2 Reuse/adapt `subscription-login xai` so login writes the harness store used by that helper and no longer requires an adopted OpenCodex package; verify login help, closed-stdin browser/device behavior, and that OpenCode `auth.json` is not read or written.
- [x] 2.3 Add portable `xai` profile sources (config, catalog, helper registration) modeled on Z.AI: Responses, `https://api.x.ai/v1`, `grok-4.6`, auth command, no `code_mode_only`; verify Check/preview show the files as kit-owned references without secrets.
- [x] 2.4 Connect `codex --profile xai` through the installer without injecting `openai_base_url`; verify from another repository a tool-using Grok turn, native session model identity, and that ordinary `codex` still uses GPT while OpenCodex may still be present.
- [x] 2.5 Implement the kit-owned one-field Responses shim on `127.0.0.1` (strip `content: null` from echoed reasoning items, byte-transparent streaming, no credential storage, self-exit when no `codex.exe` remains) with unit/integration tests; verify a live multi-step Grok tool turn through it against api.x.ai.
- [x] 2.6 Wire shim lifecycle into the launcher: start it only for `xai`-profile invocations with a readiness wait, keep the self-exit rule, point the installed profile at the shim port, and verify ordinary `codex` and `codex --profile zai` never start it.
- [x] 2.7 Restore xai feature parity through shim adaptation: translate custom tool declarations and call items bidirectionally (apply_patch/exec usable at api.x.ai), strip the unsupported `external_web_access` field so `web_search = "live"` works, decode chunked response framing before SSE rewriting, enable `code_mode`; verify a live apply_patch tool turn through the shim. MCP-namespace and multi-agent tools stay disabled until api.x.ai accepts the `namespace` tool type.
- [x] 2.8 Make MCP, multi-agent and app namespaces work on the xai profile (user requirement: the shim is pointless without them): flatten `namespace` tool declarations into per-subtool function declarations, rewire echoed `function_call{namespace}` items to flattened names, rewrite streamed calls back to the namespaced form Codex routes, and re-enable `multi_agent` plus kit MCP servers; verify a live MCP tool call whose result is written by apply_patch.

## 3. Restore native GPT and remove OpenCodex

- [x] 3.1 Stop and unregister the owned subscription Windows task and proxy process without touching unrelated tasks; verify the process is gone, port 10100 is not required, the task's LogonTrigger/StartWhenAvailable autostart is removed with it, no OpenCodex service, Run key, startup-folder entry or package autostart remains, and the recovery journal can still undo an interrupted uninstall.
- [x] 3.2 Remove live OpenCodex links, catalog injection, `openai_base_url` / experimental WS injection, and `model = "xai/grok-4.6"` from the ordinary base config, restoring Astra; verify Check flags leftover localhost routing as unhealthy and a new ordinary `codex` session does not talk to 127.0.0.1:10100.
- [x] 3.3 Drop OpenCodex from kit lifecycle (dependency declaration, `-SubscriptionsOnly` proxy path, launcher/diagnostics assumptions); verify Install/Update/Disconnect no longer provision the package or task and Z.AI `codex --profile zai` still works.
- [x] 3.4 Retire or delete unused OpenCodex runtime sources from the live kit path after no installer entry remains; verify tracked files contain no private tokens and docs/memory describe Grok as `--profile xai`.

## 4. Compatibility, docs and acceptance

- [x] 4.1 Update subscription, installation and decision docs so delivered Grok is the native profile, OpenCodex is retired, and orchestration remaining tasks must not require the proxy; verify local-link/factual checks on the edited docs.
- [x] 4.2 Add/adjust native tests for lifecycle without a proxy, profile auth command, GPT default, and unhealthy leftover injection; verify they pass without talking to the live xAI account except the already-recorded spike receipt.
- [x] 4.3 Run outside-checkout acceptance: `codex --profile xai` tool turn, ordinary `codex` GPT session, Z.AI profile, Check with OpenCodex absent, representative auth failure, disconnect/reconnect of native wiring; verify each spec scenario has evidence and this change is not marked complete if the spike never passed.


