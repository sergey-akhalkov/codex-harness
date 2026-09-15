## 1. Bound Nuphus screenshot results

- [x] 1.1 Add a Nuphus adapter path for `desktop_screenshot` and `desktop_window_screenshot` that converts text-wrapped image bytes into a native image content block or an owned file under `CODEX_HOME/harness/runtime/nuphus`, and verify a no-path fixture result contains no PNG/base64 in `type: text`.
- [x] 1.2 Preserve caller-supplied owned `path` results as path-only and verify the model-visible payload identifies that path without image bytes.
- [x] 1.3 Keep browser snapshot/reference handling unchanged and verify a scoped `browser_snapshot` still returns text/refs rather than being treated as a screenshot conversion.
- [x] 1.4 Cover adapter failure/oversize cases with an explicit bounded error and verify the original diagnostic stays local rather than dumping image text into the MCP result.

## 2. Stop screenshot substitutes for conversation views

- [x] 2.1 Update `global/harness.config.toml`, `global/principles-of-work.md`, `docs/agent-delegation.md` and the token-efficient Nuphus route so conversation visibility is a controller/UI obligation, and verify `codex debug prompt-input` from this checkout no longer instructs capturing Codex windows.
- [x] 2.2 Document list/title/state as the window-identity path and screenshots as visual-question-only, and verify the owning guides distinguish unfinished simultaneous views from a Nuphus visual task.
- [x] 2.3 Leave `orchestrate-subscription-agents` simultaneous-view tasks open and verify this change does not mark them complete or implement hidden model dispatch.

## 3. Restore live shared defaults

- [x] 3.1 Admit `config-overrides` / `config-localize` on a management-healthy native manager without opening general source-consuming runtime, or refresh the recorded `configBridge` through install/update, and verify an ordinary local launch no longer prints the shared-defaults fallback notice.
- [x] 3.2 Keep SourceRuntime closed for stale source-consuming MCP/runtime work and verify a stale-identity CodeGraph/management path still matches the existing serving/management admission tests.
- [x] 3.3 Make Check report missing live shared defaults as a harness fault and verify the diagnostic names the fallback notice or absent developer-instruction/context-management keys.

## 4. Prove a new session and keep honest limits

- [x] 4.1 Run `codex debug prompt-input` from an owned repository outside this checkout through the ordinary installed entry point and verify it contains the current shared developer-instruction marker and does not claim experimental context management from source files alone.
- [x] 4.2 Confirm the effective new-session config includes the accepted experimental context-management setting unless the client/account rejects it, and verify that rejection is recorded without silently changing model or billing.
- [x] 4.3 Exercise one owned Nuphus screenshot without `path` and one with `path` through the installed MCP registration and verify both match the visual/path contract.
- [x] 4.4 Update owning installation, code-tools, token-workflow and decision records with the observed evidence limits, and verify `openspec validate stop-inline-screenshots-and-restore-shared-defaults --strict` passes without weekly-quota percentage claims.
