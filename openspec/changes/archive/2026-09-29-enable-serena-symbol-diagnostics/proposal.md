## Why

The managed Serena connection already exposes every standard semantic tool,
but Serena 1.7.0 also ships an optional symbol-scoped diagnostics tool that
the generated worker configuration does not include. Post-edit verification
currently has to use file-wide diagnostics, while the harness acceptance loop
needs a bounded "changed symbol plus direct referencers" check; upstream
disabled per-edit diagnostics in 1.7.0, so an explicit bounded diagnostic call
is the supported route.

## What Changes

- Include Serena's optional `get_diagnostics_for_symbol` tool in the generated
  managed worker configuration through `included_optional_tools`, while every
  recorded exclusion (memory, onboarding, configuration introspection,
  `search_for_pattern`) stays unchanged.
- Keep `restart_language_server` and every other optional tool excluded: no
  recorded LSP-stall evidence exists, and the broker lifecycle owns worker
  restarts.
- Update the owning guidance so consumers use the new route and existing
  tools more precisely: a symbol-diagnostics row, the `replace_in_files`
  dry-run/occurrence-selection protocol, `find_symbol` `include_info` as a
  signature-only tier, bounded `substring_matching` discovery, and parallel
  independent reads.
- Acceptance exercises the enabled tool through the installed consumer with
  the rust-analyzer and basedpyright backends and through the code-tools
  install lifecycle.

Out of scope: re-enabling memory, onboarding, introspection or text-search
tools (recorded decisions keep them excluded), `query_project` (needs a
separate Project Server), `restart_language_server` without incident
evidence, and measured token-saving claims for this change; its benefit is
stated as a mechanism, not a measurement.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `global-code-tools`: the Serena model-facing tool list requirement gains an
  explicitly included symbol-scoped diagnostics tool while all recorded
  exclusions remain.

## Impact

- `crates/harness-core/src/serena_configuration.rs` and its tests: render
  `included_optional_tools` next to `excluded_tools`.
- `openspec/specs/global-code-tools/spec.md` delta for the tool-list
  requirement.
- `docs/code-tools.md` and
  `.agents/skills/token-efficient-workflow/references/code-retrieval.md`
  guidance updates.
- Explicit `install --code-tools-only` lifecycle plus a fresh session;
  existing sessions keep the previously loaded catalogue.
- No dependency or package changes: the installed Serena 1.7.0 already ships
  the tool.
