## Context

See proposal.md for motivation. Codex MCP registrations for `serena` and
`nuphus` invoke `codex-harness mcp <name>`. That dispatcher still uses
source-runtime admission for every MCP command except CodeGraph. After later
checkout edits the recorded manager prints that source-consuming runtime is
disabled and exits before initialize. CodeGraph already splits serving
admission from that gate. The Serena broker/service path is already
serving-admitted; the stdio frontend is not. Nuphus has no separate service
path.

## Goals / Non-Goals

**Goals:**
- Keep hash-matching source-stale managers able to complete MCP initialize for
  `mcp serena` and `mcp nuphus`.
- Keep `mcp codebase-memory` and other source-consuming runtime gated on a
  healthy source match.
- Document the same restart boundary already used for CodeGraph.

**Non-Goals:**
- Changing Serena, Nuphus or CodeGraph packages, catalogues or resource
  limits.
- Making ordinary Codex startup rebuild native source.
- Extending Check's CodeGraph-specific serving report to a second protocol.
- Relaxing config localize/overrides or selected-build resolution.

## Decisions

- Reuse serving admission instead of deleting the source-stale gate. Admit
  `mcp serena` and `mcp nuphus` the same way as `mcp codegraph` and
  `mcp codegraph-control`. Leave `mcp codebase-memory` on source-runtime
  admission.
- Do not add a new Check handshake inspector for Serena or Nuphus. The
  registered command is the same native manager; CodeGraph Check already
  reports source-stale-but-callable versus cannot-serve for that binary.
- Native tests extend the existing source-stale manager fixture: `--help` and
  a framed initialize must succeed for Serena and Nuphus, while
  `mcp codebase-memory` remains refused.

## Risks / Trade-offs

- A source-stale adapter keeps serving until explicit Update. Accepted: the
  same CodeGraph trade-off; Check already reports rebuild awareness.
- Serena and Nuphus still read live checkout data files (`global/code-tools.json`,
  `global/tool-resources.json`). That matches the live-data rule; missing
  source remains a later serve failure, not an admission refusal for `--help`.

## Migration Plan

Install/Update from current source rebuilds and re-registers a
serving-admitted command. Existing sessions keep their previous MCP catalogue
until restart. After this change, a new Codex start against hash-matching
binaries should handshake Serena and Nuphus without another rebuild even if
the checkout later differs. Rollback is the previous dispatcher gate.
