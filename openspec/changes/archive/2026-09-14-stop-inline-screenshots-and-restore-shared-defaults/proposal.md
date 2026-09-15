## Why

Ordinary Codex sessions started burning weekly model limits on 13–14 September even though HTTP sampling counts did not spike. Local thread evidence shows the spend is per-turn context: Nuphus desktop screenshots without a file path return PNG as nested JSON text (about 0.8–1.0 MiB per call), and the installed launcher falls back without live shared defaults, so developer instructions and experimental Astra context management never reach new sessions. The visibility policy added on 12 September made agents capture conversation windows; the first megabyte-scale inline screenshot appears at 13 September 01:51.

## What Changes

- Stop Nuphus desktop and window screenshots from entering model context as base64 or nested JSON text. Prefer a local file path or a native image block; keep screenshots for genuine visual questions, not conversation-visibility checks.
- Change agent policy so simultaneous conversation views remain a UI/controller requirement. Until those views exist, keep work in the already visible main conversation and identify windows with list/title/state rather than screenshots of Codex itself.
- Restore live shared-default injection for ordinary local sessions, including developer instructions and the accepted experimental context-management trial. Stale native build identity must not silently drop those defaults; fallback without them is a reported degraded mode, not a successful kit session.
- Verify the repaired screenshot contract and an actual new-session prompt/config outside this checkout. Do not claim weekly-quota percentages from character counts.

## Capabilities

### New Capabilities

### Modified Capabilities

- `global-code-tools`: Nuphus screenshot results must stay bounded and visual; adapters must not wrap image bytes as model-visible text.
- `token-efficient-agent-workflow`: Code Mode and MCP result shaping must keep images as images and omit screenshot payloads from ordinary text aggregation.
- `mcp-tool-selection`: Desktop inspection may not use screenshots to prove conversation visibility or poll Codex windows.
- `agent-delegation`: The simultaneous-view rule must not authorize screenshot capture into model context while controller views remain unfinished.
- `linked-global-kit`: Ordinary `codex` sessions must load live shared defaults; source-stale runtime fallback must remain explicit and recoverable.
- `native-context-workflow`: Experimental Astra context management is accepted only when a new session actually receives the effective setting.

## Impact

Owners: `tools/code-tools/nuphus_proxy.py`, `global/harness.config.toml`, `global/principles-of-work.md`, `.agents/skills/token-efficient-workflow`, `docs/code-tools.md`, `docs/agent-delegation.md`, `docs/installation.md`, `docs/global-instructions.md`, launcher/`config-overrides` admission in `crates/codex-harness` and `crates/harness-core`, and Check diagnostics. Related unfinished work in `orchestrate-subscription-agents` keeps simultaneous native views; this change does not implement those views or close that change. Shrinking the global principles body and hiding the Code Mode Apps catalogue are follow-ups, not this scope. Ordinary hooks stay off except the accepted RTK exception. Fast, native memories and GPT-5 remain excluded.
