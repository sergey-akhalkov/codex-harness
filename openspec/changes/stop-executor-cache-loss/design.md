## Context

See proposal.md for motivation. Spawn uses an owned app-server conversation; exact-session resume still uses the observed native JSONL launcher. Both write native per-response `token_usage_record` events to their exact session rollout. The CLI JSONL surface alone only exposes final turn usage, which is too late for this policy.

## Goals / Non-Goals

Use one detector and the existing process ownership, receipt and bounded stream reader. Do not change provider routing, shorten accepted tasks, periodically message the model, or claim that client-side interruption repairs an upstream cache.

## Decisions

- Read the rollout whose metadata verifies the session supplied by the native event stream. Windows kernel directory-change notifications and non-transient native events trigger incremental reads in the existing host loop, without a new polling thread. Windows can delay size/write notices until cached writes flush (reproduced with an open writer; documented by [Microsoft](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-findfirstchangenotificationa)). Therefore reconcile the open file's size once per second, reading only if it grew; before discovery retry the bounded lookup. This avoids idle content reads while covering delayed notifications. Rearm before reading to retain concurrent appends, bound catch-up to 1 MiB per iteration and retain the existing bounded line reader. This covers both entry paths without a proxy or backend-specific cumulative notification semantics.
- Apply the fixed conservative thresholds in the requirement to DeepSeek only. Reuse cumulative recorded input to reject duplicate/out-of-order usage; retain historical warmup on resume, but count misses only from this run's start time. Missing evidence is explicit coverage loss.
- Use the existing host-owned Job to terminate the affected child tree immediately at detection, before receipt locks or a potentially blocking native interruption request. Record stopped or partial-stop, evidence and a nonzero outcome in existing receipt owners. The host does not wait for a model, test or build to finish.
- Recovery belongs to the lead: the failure surface gives an exact `executor restart` command with source, home, slot, owner and predecessor session. Restart adopts that slot without synchronization, reset, clean or release and uses a new control-backed conversation, not native session resume. The original assignment is retained separately from the bounded handoff so repeated restarts do not accumulate old prompts.
- Preserve commits, tracked/untracked files, local verification artifacts and the original rollout. Capture at most eight short visible reports/tool actions/results in the existing receipt; never decode reasoning. The fresh prompt names the old session for targeted visible-history lookup, requires inspection of the actual worktree, and labels interrupted commands/results as unverified. It does not promise to restore unsaved internal model state. Recurrent loss requires investigation rather than an automatic restart loop.
- Validate with synthetic native usage and owned process fixtures, then exercise the installed entrypoint outside the checkout. No paid diagnostic model calls.

## Risks / Trade-offs

- Usage arrives after billing, and a next request may already be in flight: this is a repeated-loss circuit breaker, not a hard currency cap.
- Provider eviction or a legitimate prefix change can cross the thresholds: the user explicitly chose interruption over continuing that spending. Recovery uses a fresh conversation in the same worktree, directed by the lead; it does not repeat the assignment from scratch.
- Older CLI versions may omit per-response usage: report unavailable monitoring and do not claim protection from nonexistent counters.
- Persistent upstream loss is not diagnosed by these counters alone. Official DeepSeek documentation describes automatic best-effort prefix caching; request capture or provider-side evidence would be needed to establish the underlying cause.

## Migration Plan

Build and test the affected native targets, deploy through `codex-harness deploy`, and verify the installed host against an owned synthetic fixture outside the checkout. Existing running hosts retain their old executable; new spawn/resume hosts receive the policy. Existing installation recovery remains the rollback route.
