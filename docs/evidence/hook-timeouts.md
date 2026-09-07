# Diagnostic hook timeout correction

Verified on Windows with globally installed Codex CLI 0.153.4 on 2026-09-07.
Implementation: [journal.py](../../tools/lsp/journal.py) and
[server.py](../../tools/lsp/server.py). [Tasks](../../openspec/changes/fix-diagnostic-hook-timeouts/tasks.md).

## Causes and correction

The global configuration gives PreToolUse ten seconds and PostToolUse/Stop
thirty seconds. The reported current-session diagnostic batches repeatedly
used about 27.2 seconds. Those are distinct limits; this investigation does
not attribute every historical ten-second warning to a single cause.

Pending results re-expanded their whole language cohort, including completed
members. Analysis also consumed the entire service budget, leaving only
0.1 seconds for final reconciliation; an incomplete final scan invalidated
the completed work. The corrected service reuses verified results only for
identical complete source/configuration/registry generations, prioritizes
unattempted work and reserves final reconciliation time. Source or registry
changes before delivery remain stale. Related completed results are not
scheduled again within the batch.

The pre-edit journal now shares its seven-second allowance across database
lock waits and statements, root bookkeeping and scans. It hashes outside a
write transaction and rechecks baseline ownership before committing. Traversal
checks its deadline for empty directories and after the last file; each
canonical source is hashed once per snapshot. Incomplete work remains explicit.
Native hook timeouts and diagnostic policy were not increased or disabled.

## Verification

- [Progress regressions](../../tests/lsp-hook-progress.py): five passed in
  2.447 seconds, covering partial cohorts, new dependencies, final-scan time,
  late source writes and backend registry changes.
- The same six-file cohort test was run against the previous committed
  service and the corrected service with controlled 65-ms analyses. Before:
  36 analyses, twelve unresolved passes, no convergence. After: six analyses,
  each file once, followed by unchanged status; total 0.439 seconds. This
  isolates scheduling behavior, rather than measuring a language server.
  Local evidence: `%TEMP%/harness-hook-timeouts-comparison.json`.
- [Pre-budget regressions](../../tests/lsp-pre-budget.py): eight passed in
  0.641 seconds, including real SQLite contention across roots and a real
  long SQL statement, scan/write concurrency, baseline preservation, empty
  directory timeout and preserved-mtime edits.
- [Existing adapter tests](../../tests/lsp-adapter.py) with `--real`: all
  33 passed in 43.850 seconds, including installed TypeScript, Python,
  JSON and XML backends, dependency invalidation, late edits, crash/timeout
  behavior and separate consumers. The later registry-revalidation addition
  has its own passing regression above.
- Independent boundary review found one missing final registry check; it
  was corrected and tested. Final review found no additional material issue.

## Global native acceptance

The existing `~/.codex/hooks.json` and `~/.codex/harness/bin/hook.ps1` are
direct links to this checkout. The installed native app-server was launched
with the real global home and a disposable external workspace. No test hook
registration or substitute server was used. Two actual native patches
introduced TypeScript error 2322 and corrected it; automatic feedback reached
the model and the corrected revision received authoritative empty diagnostics.
All seven native acceptance assertions passed.

| Actual native handler | First edit | Correction / completion |
|---|---:|---:|
| PreToolUse command | 1,250 ms | 836 ms |
| PostToolUse MCP | 2,042 ms | 43 ms |
| PostToolUse command companion | 2,241 ms | 841 ms |
| Stop MCP, unchanged workspace | — | 27 ms |
| Stop command companion | — | 883 ms |

Every handler completed without a timeout. Scoped diagnostic reports were
`diagnostics` in 2.015 seconds and `clean` in 0.028 seconds, with no problems.
The whole native fixture exited 0 in 59.228 seconds under a 260-second,
4-GiB Windows job; peak job memory was 194,891,776 bytes. Only its owned
process tree was cleaned up; the model-routing service was not restarted.
Evidence is retained under
`%TEMP%/harness-hook-timeouts-native-396bab2048494881b2e22237d2cdcd8a/`:
`result.json`, `native/report.json`, `native/scoped-diagnostics.json` and
`native/private-agent-events.json`.

## Activation and limits

PreToolUse commands load the changed journal on their next invocation. A
new Codex consumer loads the corrected MCP service through the existing
global entry point. Already running MCP processes retain their imported
Python code: this maintenance session still has the old diagnostic service
and requires restarting Codex to adopt the service correction. Its remaining
old-service warnings do not establish a failure of the fresh consumer above.
No old process was forcibly stopped and no journal was cleared to hide errors.

Genuinely unavailable backends, incomplete source scans or slow analyses can
still return unresolved status. These tests do not claim a clean result for
all existing repository diagnostics or that every possible timeout is removed.
The official [hook contract](https://learn.chatgpt.com/docs/hooks), checked
2026-09-07, documents synchronous handler timeouts in seconds and the
requirement that native MCP hooks use an already connected server.
