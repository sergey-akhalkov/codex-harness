## Context

See [proposal.md](proposal.md) for why this change exists. Observed 13–14 September spend came from per-turn payload size, not more user messages. Two local defects dominate: Nuphus `desktop_window_screenshot` / `desktop_screenshot` without `path` return PNG as nested JSON text, and the installed `configBridge` refuses `config-overrides` when build identity is `SourceStale`, so ordinary sessions miss `developer_instructions` and `[features.context_management]`.

Current owners already exist. `tools/code-tools/nuphus_proxy.py` adapts schemas and browser refs but forwards desktop results unchanged. `crates/codex-harness/src/main.rs` gates `config-overrides` on `Admission::SourceRuntime`. `build_identity` still allows management and CodeGraph serving when source is stale. Shared defaults live in `global/harness.config.toml`; live `~/.codex/config.toml` currently lacks those keys. Simultaneous conversation windows remain unfinished in `orchestrate-subscription-agents`; instruction text already tells agents to establish visible views, which they implemented with screenshots.

## Goals / Non-Goals

**Goals:**

- Make screenshot results visual or path-only before they enter model context.
- Stop using screenshots as a substitute for unfinished conversation views.
- Make ordinary local sessions receive live shared defaults, or fail closed into an explicit degraded mode that Check reports.
- Prove both repairs on an actual new-session prompt/config outside this checkout.

**Non-Goals:**

- Implementing simultaneous native conversation windows (remains `orchestrate-subscription-agents` task 1.4).
- Rewriting global principles for length, hiding the Code Mode Apps catalogue, or changing default reasoning effort.
- Restoring ordinary diagnostic/context/Stop hooks, Fast, native memories, or GPT-5.
- Claiming a weekly-quota percentage from character counts or `tokens_used`.

## Decisions

### 1. Convert screenshot payloads at the Nuphus adapter

Handle `desktop_screenshot` and `desktop_window_screenshot` in `nuphus_proxy.py` after the native call. If the caller supplied `path`, keep the path-only result. If the native result is a text envelope containing image data, write an owned temp file under `CODEX_HOME/harness/runtime/nuphus` or emit a native MCP image content block. Do not JSON-dump the bytes again. Browser snapshots stay text/refs.

Rejected: instruction-only fix. Agents already have “keep screenshots for visual questions” and still omitted `path`. Rejected: disable Nuphus. Desktop list/state and owned UI work remain required. Rejected: require callers to always pass `path`; the adapter must still bound the no-path case because models omit it.

### 2. Treat conversation visibility as a controller gap

Change `global/harness.config.toml`, `global/principles-of-work.md`, `docs/agent-delegation.md` and the token-efficient Nuphus route so agents identify windows with list/title/state and keep work in the visible main conversation until simultaneous views exist. Screenshots of Codex or sibling agent windows are out of policy even after the adapter bounds the bytes.

Rejected: delete the visibility requirement. The parent orchestration change still needs real windows. This change only removes the screenshot workaround.

### 3. Admit live config-overrides without pretending source-consuming runtime is healthy

Keep SourceRuntime closed for source-consuming MCP/runtime work when identity is stale. Allow `config-overrides` / `config-localize` on the management-admitted native manager, because those commands only read `global/harness.config.toml` and emit native `-c` leaves. Alternatively, point the launcher at a current healthy build through the existing install/update path so `installation.json` `configBridge` matches checkout source. Prefer a durable admission split so later source edits do not silently drop shared defaults again. Check must flag a session that started with the fallback notice.

Rejected: copy shared defaults into `config.toml`. That violates live source consumption and fights native TUI writers. Rejected: leave fallback as success. 13 September already recorded that AGENTS loaded while developer-prompt did not.

### 4. Activation evidence is a new-session dump, not thread `tokens_used`

Acceptance uses `codex debug prompt-input` plus the effective config for developer instructions and experimental context management, and a bounded Nuphus screenshot fixture that inspects the MCP result shape. Thread-history `tokens_used` remains diagnostic context, not a quota oracle.

## Risks / Trade-offs

- Native image blocks may still be expensive for vision models. Mitigation: policy forbids conversation-window captures; remaining screenshots should be rare owned-target visuals.
- Temp screenshot files need ownership and cleanup. Mitigation: reuse the existing Nuphus runtime directory and process-ownership cleanup; do not write into user projects.
- Loosening SourceRuntime admission could let other stale commands run. Mitigation: whitelist only config-overrides/config-localize, or refresh the recorded bridge through install/update and keep the whitelist as defense in depth.
- Agents may keep screenshotting after instruction edits until sessions restart. Mitigation: adapter bound is the hard stop; instruction changes apply to new sessions.

## Migration Plan

1. Ship adapter + instruction/config changes in the checkout.
2. Repair or re-admit the installed config bridge; confirm a new outside-checkout `debug prompt-input` contains shared developer instructions and no fallback notice.
3. Exercise one owned screenshot without `path` and one with `path`; confirm no text-wrapped PNG.
4. Rollback: restore previous proxy/instructions; Disconnect/Check remain available. Experimental context management already has an explicit false override.

## Open Questions

None that change this design. Whether later to shrink principles or hide Apps schemas can wait.
