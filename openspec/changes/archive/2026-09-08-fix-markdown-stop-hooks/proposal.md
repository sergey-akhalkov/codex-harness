## Why

Markdown diagnostics abort on links into a sibling checkout, then both native and command Stop handlers repeat the same failure and continuation. Successful Stop checks also incorrectly request continuation.

## What Changes

- Resolve explicitly applicable Markdown dependency roots without scanning unrelated directories or weakening source edit boundaries.
- Deliver each unchanged Stop outcome once across handlers, preserve new findings and unresolved status, and never block on successful clearance.
- Reuse a completed native reconciliation by its verified input snapshot, including failures; later writes still require a fresh check.
- Verify the linked global entry points from an external workspace and document activation for running consumers.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `automatic-lsp-diagnostics`: bounded, idempotent completion delivery and Markdown dependencies in applicable roots.

## Impact

Markdown client, diagnostic service/journal, focused regression checks and global installation evidence. Existing edits, timeout bounds, backend availability policy and hook registration remain intact.
