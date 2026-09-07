# Markdown links and repeated Stop feedback

Verified on Windows with globally installed Codex CLI 0.153.4 on 2026-09-07.
Sources: [Markdown client](../../tools/lsp/markdown_client.py),
[journal](../../tools/lsp/journal.py), [service](../../tools/lsp/server.py).
[OpenSpec tasks](../../openspec/changes/fix-markdown-stop-hooks/tasks.md).

## Causes and correction

The reported audit links into a sibling `opencode-kit` checkout. The client
rejected those exact local resources before the Markdown server could check
them. Explicit local link targets now grant bounded read-only access: at most
2,048 dependency paths per source and 8 MiB per file read. External documents
cannot grant further dependencies; network resources, unrelated files and
external directory enumeration remain unavailable. Removing a link revokes
its allowance on the next parse.

A fresh server also exposed a missing `markdown.occurrencesHighlight.enabled`
setting: its first diagnostic pull logged an analysis failure. The client now
supplies the setting expected by the installed server. Standard watched-file
notifications invalidate cached sibling text; the server's custom watchers
alone tracked existence and missed changed headings. Missing files and
fragments still produce actual language diagnostics.

The command companion previously treated pending failed work as evidence that
the native handler had not completed. It now compares a verified source,
configuration and registry signature stored with native completion. A failed
analysis can be delivered completely while remaining unresolved in the journal.
Incomplete scans and exceptions cannot create a reusable receipt; later input
changes require reconciliation.

Stop delivery is atomic across handlers and keyed by semantic outcome in the
existing workspace/session/agent journal. Repeated identical feedback is silent,
including across continuation turn IDs. New failures can request one continuation;
an active Stop and successful clearance do not block. Stop output contains a
short summary and the full report path instead of the entire report JSON.
The official [hook contract](https://learn.chatgpt.com/docs/hooks), checked
2026-09-07, distinguishes continuation via `decision: block`, the
`stop_hook_active` guard and informational `systemMessage` output.

## Verification

| Check | Observed result |
|---|---|
| [Real Markdown regressions](../../tests/lsp-markdown-workspace.py) | 4 passed: sibling links with encoded spaces, navigation, changed heading and correction, missing files/fragments, access revocation, rejected network/unrelated paths and bounded reads |
| [Stop regressions](../../tests/lsp-stop-delivery.py) | 9 passed with real SQLite journals: repeated/concurrent handlers, clean and active Stop, failed checks, session/revision changes, later source/configuration/registry writes, incomplete scans and failed report delivery |
| [Progress regressions](../../tests/lsp-hook-progress.py) | 5 passed |
| [Pre-budget regressions](../../tests/lsp-pre-budget.py) | 8 passed |
| [Adapter regressions](../../tests/lsp-adapter.py), `--real` | 33 passed, including installed TypeScript, Python, JSON and XML backends |
| `install.ps1 -Mode Check` | Connected; global links and code-tool protocol ready |
| Documentation and OpenSpec | All eight changed Markdown documents checked clean by the real backend; strict change validation and `git diff --check` passed |

The exact reported audit revision
`278b054a7443c28282ef8adf31d038d4bcffd7326971e3d9cb1a20f13fde41e3`
was reproduced and then checked with the corrected real Markdown backend:
`clean`, zero diagnostics. The audit's old limitation is retained as historical
evidence with a link to this resolution.

## Global native acceptance

[The existing native runner](../../tests/lsp-native.Tests.ps1) gained a
`-MarkdownSibling` fixture. With `-Language markdown -MarkdownSibling
-UseGlobalHome -RunAgent -KeepProbe`, a fresh unprofiled app-server used the
real global hook registration in a temporary workspace outside this checkout.
Two native patches introduced an absent sibling heading, then corrected it.
The model received `link.no-such-header-in-file`; correction returned current
empty diagnostics. All nine native acceptance assertions passed.

The two Stop handlers each completed once, with no continuation and no output
entries: native MCP in 32 ms, command companion in 1,110 ms. No scoped report
was unresolved. The bounded fixture exited 0 in 45.326 seconds. The later
8 MiB read guard was exercised by the real Markdown regression suite; the
native model run was not repeated for that boundary-only addition.

Private evidence is retained under
`%TEMP%/harness-markdown-native-253b410cc43c4e5db78bde9c81a456cd/`:
`result.json`, `native/report.json`, `native/scoped-diagnostics.json` and
`native/private-agent-events.json`. No test hook registration replaced the
global source, and no existing user process or journal was cleared.

## Activation and limits

The global hook and launcher sources are already linked to this checkout.
New Codex consumers load the correction. Already running MCP processes retain
imported Python code: restart an existing Codex session once to adopt it.
The maintenance session continued showing its old Markdown error while the
fresh external consumer passed.

This verifies the reported failure and repetition behavior. Unavailable
backends and incomplete checks remain explicit; it does not establish a clean
result for all existing repository diagnostics. Linked target changes are
reconciled on the next Markdown diagnostic pull, not by a new background
filesystem monitor. Existing timeout limits and model routing are unchanged.
