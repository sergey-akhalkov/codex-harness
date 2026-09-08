## Context

See [proposal](proposal.md). Native hooks cap PreToolUse at ten seconds and PostToolUse/Stop at thirty. The service has a 27-second batch budget, while the independent pre journal intends seven seconds. Current reports repeatedly reach approximately 27.2 seconds.

## Goals / Non-Goals

Preserve the finite limits, accurate dependency invalidation and independent baselines while making partial batches converge. This change does not suppress diagnostics, increase hook timeouts, modify compiler policies or restart the model-routing proxy.

## Decisions

- Store the source/configuration generation with accepted diagnostic results and reuse completed entries only for those same inputs and file bytes. Keep pending and stale entries visible; generations prevent a completed caller from hiding a changed dependency.
- Reserve part of each batch for final content reconciliation and journal delivery. Deduplicate related results within a batch. A failed final snapshot remains unresolved.
- Check traversal deadlines even for directory-only trees and hash each canonical file once per snapshot. Source hashing stays outside SQLite write transactions; transaction-time baseline rechecks preserve concurrent ownership. All Pre database waits consume one absolute deadline.
- Retain globally linked source entry points and existing hook contracts. Verification uses disposable external workspaces, real installed backends and controlled scheduling for overflow/locking cases.

## Risks / Trade-offs

- Cached results could hide dependency changes: reuse requires identical complete source/configuration generation; regression includes changes between partial batches.
- Genuine slow or unavailable servers can remain unresolved: preserve explicit failure status and finite cleanup, without indefinite retries.
- Existing MCP processes hold imported Python code: verify updated global entry points in fresh consumers; avoid terminating the control session or unrelated consumers.

## Migration Plan

Sources are read through existing global links; no credential migration or new dependency is needed. New result metadata is additive and old entries require fresh verification. Rollback restores only the changed diagnostic sources; baseline and diagnostic history remain recoverable.
