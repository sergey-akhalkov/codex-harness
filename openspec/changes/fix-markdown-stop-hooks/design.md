## Context

The linked kit uses a warm native MCP plus an independent command fallback. The journal intentionally retains failed work, so using its pending queue to recognize a native completion incorrectly reruns failures. Stop delivery currently blocks every non-unchanged report, including clean. See [proposal](proposal.md).

## Goals / Non-Goals

**Goals:** preserve diagnostic truth and current revisions while eliminating repeated continuation and duplicate output. Restore ordinary sibling Markdown link validation.

**Non-Goals:** new language installations, disabling hooks, increasing timeout limits, changing model routing or clearing existing diagnostic state.

## Decisions

- Keep a verified input signature with native completion, independent from the unresolved work queue. The companion compares current bytes and registry before reuse; exceptions/incomplete snapshots cannot create a reusable receipt.
- Atomically remember semantic completion feedback in the existing per-workspace/session/agent journal. Exclude timing, transport identifiers and report paths. Preserve meaningful revision, diagnostic and failure changes. New failures can continue once; clean feedback is informational. This avoids relying only on the native stop flag.
- Markdown dependencies permit narrow read-only access to explicit local links. Keep enumeration and edit boundaries unchanged; do not add a whole sibling or parent tree as an implicit workspace.

## Risks / Trade-offs

- Suppressing stale feedback could hide new work → compare actual source/registry signatures, test later writes and distinct sessions.
- Concurrent handlers could both publish → use a single journal transaction for feedback identity.
- External Markdown access could become overly broad → check exact dependency paths and keep recursive discovery inside the workspace; exercise rejected unrelated requests.
- Running MCP processes keep imported code → verify a fresh global consumer and document the session restart boundary; do not kill user sessions.

## Migration Plan

Existing source links pick up command changes immediately and MCP changes on next consumer startup. No registration or trust change is needed. Rollback restores only this change's code; journal metadata additions are backward compatible.
