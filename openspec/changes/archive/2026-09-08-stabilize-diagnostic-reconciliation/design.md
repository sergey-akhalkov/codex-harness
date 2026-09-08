## Context

See [proposal](proposal.md). Installed CLI 0.153.4 reads source-linked hooks and a persistent MCP process; the command companion starts a short-lived adapter. Existing code already reuses Serena's LSP transport and installed servers. The reported repository reproduces both failures in a 5-second scan (7,395 files observed, 15,595,998-byte JSON rejected).

## Goals / Non-Goals

**Goals:** bound discovery, preserve source-byte identities and incomplete coverage, recover future tracking and make transport ownership independent of diagnostic success.

**Non-Goals:** changing consumer sources, accepting old unknown edits as clean, replacing all language providers, adding a watcher daemon or raising timeout limits.

## Decisions

- Extract bounded filesystem scanning from journal lifecycle. Use standard scandir traversal and streaming SHA256 with deadline checks, avoid resolving every ordinary file on Windows, resolve reparse paths to preserve root isolation. Keep content hashing rather than mtime-only caching (same-size, restored-mtime edits must remain detectable). Language-size limits belong in analysis only.
- Preserve partial baseline observations. Adopt later observations only as a forward baseline with a durable historical coverage gap. Known changed paths continue to be analyzed; unknown prior edits cannot be reconstructed or silently accepted.
- Keep short SQLite ownership transactions, scoped by canonical workspace/session/transcript, with shared deadlines. Per-invocation transport receipts record completed delivery even for failed analysis. A later Stop must still reconcile newer filesystem contents.
- Infrastructure-only Stop outcomes are informational and remain persisted and visible. Actual diagnostic findings can request one continuation for a new content/finding identity. An active Stop cannot request another continuation. Deduplication must survive alternating failure reasons and transport metadata.
- Reuse Serena and existing language providers. Git alone cannot cover pre-dirty/untracked and arbitrary MCP/shell writes; watchers need a separate lifecycle, overflow recovery and the same snapshot reconciliation. Neither replaces the owning journal or hook delivery policy. Reconsider a maintained watcher only if measured scanning remains the bottleneck.

## Risks / Trade-offs

- Historical edits cannot be recovered without a baseline: retain a coverage gap even after future tracking works.
- Filesystem operations can block below Python: check deadlines between chunks and entries; command hooks retain an outer process timeout. Do not claim hard realtime guarantees on faulty storage.
- Incomplete scans cannot prove absence/deletion: retain known entries and report incomplete coverage.
- Persistent old MCP processes retain old code: validate fresh consumers and document one restart; do not kill user sessions.

## Migration Plan

Back up changed sources and installation records outside the checkout. Apply through the existing linked installer lifecycle and Check. New additive journal fields preserve old state. Verify installed entry points outside harness and the reported repository read-only. Rollback restores only changed kit files/links from the backup; no consumer state is removed.
