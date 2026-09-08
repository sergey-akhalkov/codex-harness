# Bounded diagnostic reconciliation

Observed on Windows on 2026-09-07, Codex CLI 0.153.4, PowerShell 7.6.5,
and the existing Serena Python 3.13. The owning change is
[stabilize-diagnostic-reconciliation](../../openspec/changes/archive/2026-09-08-stabilize-diagnostic-reconciliation/tasks.md).
Product code, PLC specifications, roadmap and archive data in pmac-emulator were
not edited. No controller operations were performed.

## Causes and changes

| Failure | Observed mechanism | Correction |
|---|---|---|
| Missing baseline | A single unreadable/large file or traversal timeout discarded the entire initial snapshot; later checks could not identify earlier edits | Retain partial observations, preserve earliest revisions, establish forward tracking on recovery and persist an explicit historical coverage gap |
| 8 MiB JSON failure | Discovery imposed the language-input limit and read whole files into memory | Separate `discovery.py`; stream SHA256 in 256 KiB chunks, including large files; apply 8 MiB only to language analysis and return `skipped` for those changed inputs |
| Slow repository traversal | Per-file canonical path resolution and serial Windows file opens consumed the budget | `os.scandir`, boundary checks on explicit/reparse paths, four shared readers and at most eight queued reads per scan; no metadata-only clearance |
| Duplicate/competing handlers | One last-completion receipt was overwritten by subsequent invocations; Stop's 0.25-second receipt scan failed on large roots | Bounded per-invocation receipt history, shared deadlines and content validation before reusing an earlier Stop receipt |
| Ownership race | Checks and claims were separate signals; age alone could allow a live owner to be replaced | Consult the atomic claim, retain live ownership, clean up on exceptions, wait only within the caller's deadline, keep distinct unfinished invocations visible |
| Repeated completion blocks | Infrastructure failures requested a new continuation; changing failure descriptions could replace the previous deduplication signature | Infrastructure is informational, persisted and deduplicated. Real findings can block once per semantic finding/revision; `stop_hook_active` never blocks. Delivery history excludes transport/timing churn |
| Repeated failed analysis | The same unavailable backend was retried on each unchanged tool event | Reuse current-generation failures for 60 seconds; retain their unresolved status and retry on changed inputs or after cooldown |

Runtime files: [discovery.py](../../tools/lsp/discovery.py),
[journal.py](../../tools/lsp/journal.py), [server.py](../../tools/lsp/server.py),
[backend.py](../../tools/lsp/backend.py), [hook.ps1](../../tools/hook.ps1).
The existing hook definitions and their 10/30-second outer timeouts remain valid.
Language-server transport and provider selection remain with the existing backends.

Reconciliation and analysis have separate evidence: a fully observed revision may
still have unavailable or skipped diagnostics. Partial scans do not invent
deletions. Failed final scans cannot accept clean results or provide a verified
input receipt. Earlier unknown edits never become verified merely because a later
baseline was recovered. A read-only first observation does not mark the whole
repository changed.

## Runtime contract and reuse

The [official hook contract](https://learn.chatgpt.com/docs/hooks) was checked
against the installed CLI and real hook events. `Bash` is the native shell
matcher, including `exec_command`; complete placeholders retain JSON types.
Stop blocking creates a continuation, while `systemMessage` is visible in the
UI/event stream. PostToolUse `additionalContext` supplies model-visible findings
and limitations. Child identity uses its transcript, including SubagentStop.

[Serena](https://github.com/oraios/serena) already supplies the reusable LSP
foundation used by this kit. Replacing it with another JSON-RPC/LSP client would
not replace Codex-specific pre/post state, tool identity or Stop delivery. Those
responsibilities now have a separate discovery module and focused regressions.
No new dependency or daemon was introduced.

Git status alone cannot establish a pre-edit baseline for already dirty or
untracked files and arbitrary shell/MCP edits. A filesystem watcher could later
reduce repeated scans, but requires recovery after startup gaps, queue overflow
and reparse changes. It would still need reconciliation. This change uses
standard-library streaming, bounded concurrency and SQLite, which address the
measured bottleneck without adding watcher lifecycle failures.

## Verification

| Check | Result and boundary |
|---|---|
| `tests/lsp-adapter.py --real` | 33 passed, including actual TypeScript/JSON Schema/Python/XML error and clearance, dependencies, deletion, configuration invalidation, competing roots and killed fallback worker isolation |
| `tests/lsp-pre-budget.py` | 9 passed: partial baselines, concurrent Pre, preserved-mtime content edits, actual SQLite contention and shared deadlines |
| `tests/lsp-stop-delivery.py` | 16 passed: infrastructure churn, active Stop, real-finding delivery, missing identities, parallel events, live ownership and receipt freshness; controlled analyzers and real SQLite |
| `tests/lsp-reconciliation.py` | 6 passed: large unchanged/changed files, forward recovery with historical gap, read-only and root isolation |
| `tests/lsp-hook-progress.py` | 5 passed: bounded cohort progress, final-scan reserve and late changes |
| `tests/lsp-installed-reconciliation.py --consumer D:/mekha/mtronics/pmac-emulator` | Passed; 30 recorded events/scenarios. Uses commands from installed hooks.json and MCP command/args from installed config.toml, with actual installed TypeScript server. No connected MCP in fallback cases; native/command race delivers exactly one result. Changed 9 MiB JSON remains skipped/unresolved. Synthetic child transcript/Stop and read-only consumer scenarios included |
| `tests/lsp-native.Tests.ps1 -UseGlobalHome -RunAgent -KeepProbe` | 7 assertions passed. Fresh native Astra session outside checkout, two actual patches, automatic TS2322 then authoritative empty diagnostics, normal completion |
| Same native probe with `-Scenario child` | 6 assertions passed. Native spawn/wait/close observed; child gets actual error/clearance. Five native Post MCP handlers and SubagentStop MCP handler could not run in the child manager; all six corresponding command handlers completed. Parent Stop handlers both completed |
| Global lifecycle | `install.ps1 -Mode Install -WhatIf`, `-Mode Install`, then `-Mode Check` passed; 13 links, unchanged MCP registration, protocol-ready code tools and ready subscription service |

Strict OpenSpec validation, Python/PowerShell parsing, `git diff --check` and
60 local Markdown links/anchors passed. Controlled regression cases complement
the actual entry-point checks; they do not establish every language backend's
behavior under all loads. The assigned middle agent contributed regression
tests; the main agent inspected, corrected and executed them for acceptance.

The reported consumer JSON is 15,595,998 bytes. The original scanner observed
7,395 files in a 5-second run with size/time failures. A complete new scanner run
observed 18,387 files in 4.201 seconds without problems. These are local timings,
not a worst-case guarantee. Profiling found file opens dominated serial time;
the optimized scan still hashes content, including edits that preserve size/mtime.

Installed consumer acceptance observed a fresh Pre in 4.203 seconds, read-only
Post in 5.622 seconds and Stop in 6.049 seconds, all without diagnostic output.
A separate resumed identity with no baseline reported an explicit historical gap
and partial scan (11,264 files), without analyzing the whole consumer project.
Its first Stop was informational; three subsequent Stop calls, including active
continuations, returned empty output in 7.159–16.390 seconds. No false clean status
or repeated completion block was produced. An earlier run under load returned a
partial Pre warning in 8.466 seconds; partial state was retained.

The archive SHA256 before/after consumer checks was
`1f0cb8b6f8be9df3ebf6f71be1eac797d8112490d48db8a9274e24cc722069fa`.
All intentional edit/error fixtures lived under new owned temporary directories.

Private evidence retained under `%TEMP%`:

- `harness-installed-reconciliation-539ck550/report.json` and `progress.json`;
- `lsp-native-5b47a4a69efa48078ff13732ce04f206/` (native events and scoped reports);
- `lsp-native-2979177cdf4f4a2bbf4ded10983a6f19/` (actual child/fallback evidence);
- `harness-reconciliation-adapter-real.log`, `harness-reconciliation-install.log`,
  `harness-reconciliation-check.log`, `harness-reconciliation-native-global.log`
  and `harness-reconciliation-native-child.log`.

## Activation, rollback and limits

`C:\Users\noilw\.codex\harness\installation.json` confirms sourceRoot
`D:\home\sergey-akhalkov\codex-harness`. Installed hooks.json and hook.ps1 are
source links; the registered MCP launches this checkout's `tools/mcp.ps1`.
Idempotent Install is the [documented linked-source lifecycle](../installation.md#update-and-move)
and reuses compatible installed dependencies. No language package upgrade was
needed for this correction.

Restart Codex to replace already running MCP processes, which retain imported
Python modules. A fresh thread in the same long-lived process does not guarantee
reload. New processes use the corrected source. The current maintenance session
still exhibited output from its older imported handler during acceptance; it was
not used as evidence of the new runtime's behavior.

The rollback snapshot is host-local:
`%TEMP%/harness-reconciliation-rollback-5fe436a80fb745afa32a0844570741ba`.
It contains the previous journal, server, backend, hook bootstrap, hook definitions
and installation metadata. To roll back, preserve subsequent edits first and
restore only the changed runtime files from that snapshot; the new discovery
module can remain unused. Rerun Install/Check and restart Codex. Do not overwrite
the entire checkout or restore unrelated configuration/runtime journals.

Finite time cannot guarantee complete discovery for every repository, disk load
or concurrent write. Incomplete coverage stays visible, and historical gaps in
an existing session remain unverified. Oversized language inputs are explicitly
skipped; JSON and archives are not globally ignored. Known ignored build/cache
directories retain their existing scope, with explicit known targets tracked.
Filesystem calls can outlast a cooperative deadline; command workers have the
native outer timeout/process-tree boundary. A stuck live native owner is not
stolen: its work remains unresolved and may require restarting that consumer.
