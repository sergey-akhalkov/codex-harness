## Context

See [proposal](proposal.md). The global AGENTS.md is a symbolic link to `global/principles-of-work.md`. Five local MCPs and two Apps integrations are exposed. Serena already advertises a semantic preference; Codebase Memory was initially missing this project's index. Server availability therefore does not explain or ensure tool selection.

## Goals / Non-Goals

Use one short globally loaded policy and keep machine-specific observations in the acceptance report. No new policy engine, package changes, mandatory calls on trivial tasks, or broad tool-output dumps.

## Decisions

- Extend the linked instructions directly, retaining the lifecycle and rollback by restoring only this section. A separate skill would require an additional activation decision and would not meet the user's request for global AGENTS.md.
- Route by task: Codebase Memory for repository relationships; Serena for exact symbol operations; harness-lsp for diagnostics and additional supported navigation; Graphify for a verified selected graph; Nuphus for authorized UI work. Prefer connected Apps for matching remote resources.
- Keep initialization lazy and scoped. Check index coverage and language support, narrow before pagination, and avoid duplicating reads across tools. Avoid a universal MCP-first rule for shell execution or text edits.
- Use live calls through the exposed tools, an owned external Python/graph/browser fixture, and a named child. Native `debug prompt-input` verifies new-session loading without extra model billing.

## Risks / Trade-offs

- Wrong/stale graph or partial parsing → identity, freshness and coverage checks; source fallback for gaps.
- Added setup and context cost → task-specific triggers and bounded responses, no unmeasured savings claim.
- UI side effects → owned browser fixture and read-only desktop metadata; no user-window input.
- Child model/connector failure → retain original error, use the configured reserve only for observed unavailability, and scope evidence to actual calls.

## Migration Plan

Back up the instruction source to the owned local probe directory, update its MCP section, verify linked bytes and native initial context from two outside directories. Existing links propagate the change; fresh sessions load it. Roll back only the added section, preserving subsequent unrelated edits. No service restart is needed.
