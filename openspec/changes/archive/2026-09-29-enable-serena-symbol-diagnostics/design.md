## Context

Serena 1.7.0 ships `get_diagnostics_for_symbol` as an optional tool that is
advertised only when `included_optional_tools` names it. The managed worker
home is rendered by `serena_configuration.rs`, currently with
`excluded_tools` only, and Serena's per-edit diagnostics are disabled
upstream, so explicit bounded calls remain the only diagnostic route.

## Goals / Non-Goals

**Goals:**

- Advertise symbol-scoped diagnostics (changed symbol, optionally its direct
  referencers) with bounded answers, without automatic diagnostics.
- Keep catalogue, generated guidance and rejection behavior in agreement.
- Make the everyday recipes name the route and sharpen existing-tool
  defaults without adding tools.

**Non-Goals:**

- No re-enablement of memory, onboarding, introspection or text-search tools.
- No `restart_language_server`, `query_project` or line-surgery tools.
- No token-audit measurement inside this change; benefit is a mechanism.

## Decisions

1. **Use `included_optional_tools` in the rendered worker config.** It
   composes with the existing `excluded_tools`; `fixed_tools` is mutually
   exclusive with both, and a client-side filter would disagree with the
   advertised catalogue. Alternative rejected: a separate unrestricted
   context (loses the managed selection).
2. **Change one optional entry only.** Every recorded exclusion stays, so
   `search_for_pattern` remains explicitly rejected and the debug escape
   hatch is unchanged. Alternative rejected: enabling several optional tools
   for symmetry; each advertised schema rides along with every request and
   the others have no evidenced everyday demand.
3. **Recipes own the usage guidance.** `code-retrieval.md` gains the
   diagnostics row and protocol sharpening (`replace_in_files`
   dry-run/occurrence selection, `find_symbol` `include_info`,
   `substring_matching`, parallel independent reads);
   `docs/code-tools.md` gets one connecting sentence. `AGENTS.md` already
   mandates explicit diagnostics and stays compact.
4. **Acceptance through real consumers.** A fresh managed session must
   exercise the tool on the Rust backend (this repository) and the Python
   basedpyright backend, including a symbol with a known diagnostic and its
   clearance, plus the existing render tests and `install --code-tools-only`
   regeneration. A backend that fails delivery keeps the operation
   unverified rather than silently advertised.

## Risks / Trade-offs

- [One more schema in every request] -> single narrow tool; answers bounded
  by its own limits; no automatic calls.
- [Backend-specific behavior] -> acceptance covers both retained backends;
  failure is reported as unverified, not delivered.
- [Existing sessions keep the old catalogue] -> document the fresh-session
  requirement, as today's configuration changes already do.

## Migration Plan

Render `included_optional_tools`, update tests/docs/recipes, run the
explicit `install --code-tools-only` lifecycle, start a fresh session and
verify the advertised catalogue and real calls. Rollback: remove the entry,
regenerate the worker home, restart sessions.
