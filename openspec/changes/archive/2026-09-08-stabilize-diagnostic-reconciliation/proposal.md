## Why

Automatic diagnostics in a large existing consumer lose the complete pre-edit baseline when one archived JSON exceeds 8 MiB or traversal exhausts its deadline. Reconciliation and competing transport failures then repeatedly interrupt completion without establishing a diagnostic result.

## What Changes

- Separate bounded content change discovery from language-server file limits; preserve partial observations and explicit historical coverage gaps.
- Recover forward tracking in existing sessions without inventing a pre-edit baseline.
- Make native/command reconciliation ownership, receipts, event identities and deadlines coherent across parallel tools and subagents.
- Keep infrastructure limitations visible without automatic continuation loops; retain actual diagnostic findings and revision-based delivery.
- Exercise the global installed hooks, the reported repository read-only, and synthetic edit/failure scenarios outside this checkout. Preserve rollback.
- Assess reuse of maintained LSP and filesystem capabilities; refactor the owning mechanism without replacing working language providers speculatively.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `automatic-lsp-diagnostics`: bounded discovery and explicit partial coverage, recoverable baseline, transport deduplication and non-looping completion.

## Impact

`tools/lsp/journal.py`, diagnostic service and hook bootstrap, regression tests, global installation lifecycle and evidence. No consumer product, PLC specification, roadmap, archive removal or controller access. Existing user state is preserved; running Python MCP processes require restart to load changes.
