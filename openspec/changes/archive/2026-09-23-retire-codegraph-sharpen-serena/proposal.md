## Proposal

Remove CodeGraph, Graphify residue and the Codebase Memory rollback route from
the kit entirely, and make Serena the explicit primary code tool with
strengthened routing instructions.

## Motivation

Measured local rollout evidence (last 30 days, real Code Mode tool
invocations): Serena 689 calls, Nuphus 662, CodeGraph 49 - and 41 of the
CodeGraph calls fall in the 2026-09-13..15 acceptance window of archived
changes; after 2026-09-18 organic use is 0-3 calls/day. A live session also
showed every managed `codegraph_search` failing with `CodeGraph client is not
connected` while `check --code-tools-only` still reported the registration
`connected`. CodeGraph carried the largest first-party maintenance surface
(about 7.5k source and 5.5k test lines, roughly twice the Serena integration),
and the remaining Codebase Memory rollback modules were dead weight behind an
already-retired registration. The user selected full removal over keeping an
explicit local route: unused rescue paths are carrying costs, not insurance.

## What Changes

- The managed global MCP selection is Serena and Nuphus. All first-party
  CodeGraph modules, CLI routes, dependency planning, tests and evidence rows
  are deleted; the same cleanup removes the `cbm_*` rollback modules, the
  `mcp codebase-memory` route and Graphify remnants. Shared packages, saved
  graphs, caches and account/broker state stay on the host as inert residue.
  Update still removes an owned `codegraph`/`codebase-memory`/`graphify`
  registration recorded by an earlier version through the neutral
  `mcp_registration` journal.
- The shared registration journal and projection planner are renamed to
  `mcp_registration` / `mcp_preparation`; their CLI entry points become
  `mcp apply-registration` and `mcp prepare-mcp` (the old names were
  CodeGraph-specific).
- Serena's managed model-facing catalogue hides `search_for_pattern` in
  addition to memory, onboarding and configuration-introspection tools;
  `HARNESS_SERENA_UNFILTERED=1` keeps the debugging escape hatch. Literal
  text, regex, configuration and document search route to scoped `rg`
  (measured: cold Serena pattern search 21.5 s versus 24-26 ms for scoped
  `rg`, plus silent match truncation under the Serena answer limit).
- Routing instructions (portable principles, bounded retrieval recipes and
  the code-tools guide) make Serena-first work explicit: structure overview
  before reads, bounded symbol bodies, exact references before impact claims,
  symbol-level edits instead of line surgery, explicit diagnostics with
  truthful freshness, and native checks for behavior.
