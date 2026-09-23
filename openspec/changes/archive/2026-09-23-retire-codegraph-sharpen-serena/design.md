## Design

### Full removal, host residue only

Unlike the earlier Graphify retirement, no first-party CodeGraph surface
survives: serving, broker, scheduler, observer, runtime, catalogue,
generation, store and transport modules are deleted together with the
dependency-dispatch arms, CLI routes and tests. The Codebase Memory rollback
modules (`cbm_*`), its manual stdio route and its probe kind are removed in
the same change; they existed only to keep a retired registration recoverable,
and the registration journal already covers removal of owned legacy names.
Nothing on the host is deleted: the published package, saved graphs, caches
and account/broker state remain inert data the user can delete manually. If a
future large repository genuinely needs a graph tool, that is a deliberate
new adoption, not a restore.

### Shared infrastructure keeps neutral names

`codegraph_registration` is really the owned MCP registration journal for the
whole managed block, and `codegraph_integration` is really the native
projection planner; both are renamed (`mcp_registration`, `mcp_preparation`)
and stripped of CodeGraph-specific behavior. The journal keeps the retired
skip/conflict names (`codebase-memory`, `graphify`, `codegraph`) so Update
removes owned pre-retirement registrations instead of re-registering broken
servers, and still refuses unowned same-name tables. CLI names move to
`apply-registration` / `prepare-mcp`; the old CodeGraph-specific names are an
accepted break recorded in the retirement evidence.

### Serena surface

Upstream 1.7.0 already curates the default catalogue. The managed proxy hides
memory, onboarding and configuration introspection (native Git records own
project memory; `get_current_config` emits machine-local paths; upstream
`initial_instructions` advises trusting refactors without checks). This
change adds `search_for_pattern` to that hidden set: scoped `rg` is faster,
complete where the bounded Serena answer silently truncated, and already the
documented route for literal text. Everything else stays as upstream delivers
it; tools that duplicate native shell ownership are not added to the managed
surface.

### Instructions

The portable principles' MCP section becomes two-tool routing with concrete
Serena recipes and `rg` for literal text. The bounded retrieval recipes drop
their CodeGraph rows and traps and add symbol-edit recipes with bounded
parameters and mandatory behavioral checks. The code-tools guide documents
the retired selection and host residue.

### Relationship to open work

`fix-mcp-broker-anchor-growth` remains open for its Serena broker
installed-path verification; its CodeGraph account-record fixes are removed
with the CodeGraph code, and its managed CodeGraph handshake verification is
superseded by this retirement. The Serena half of its release checks stays
authoritative there.

### Risks

- A future genuinely large repository loses managed graph discovery; the
  conscious path back is a new adoption decision, not surviving code.
- The CLI renames are a break for any script using the old CodeGraph-specific
  names; the kit's own callers and tests are updated in this change.
