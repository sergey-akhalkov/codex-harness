# Global hook launcher repair — 2026-09-08

The user reported persistent `Hook failed / hook exited with code 1` across
sessions. Both `~/.codex/hooks.json` and `~/.codex/harness/bin/hook.ps1` were
absent; installation metadata also lacked their records, while MCP registration
and runtime inventories remained present.

## Cause and correction

`Invoke-HarnessInstallCore` constructed a core-only desired inventory even when
the previous installation contained hooks. Its ordinary obsolete-link cleanup
then removed both owned hook links. This path is reproduced; the exact historical
invocation that damaged the user's home was not attributed to a session.

The installer now includes hook links when explicitly requested or when either
hook connection is recorded in validated installation state. Existing conflict,
relocation, missing-link repair and rollback behavior applies to these links.
Fresh core-only installation still leaves hooks absent.

After restoring the links, functional testing exposed a second problem: command
hooks omitted `HARNESS_CODE_TOOLS_REGISTRY`, whereas the native MCP launcher
supplied the installed inventory path. The broker includes that environment
value in its compatibility identity and therefore rejected the same installation
as an older runtime. A fresh broker reproduced the mismatch. Changing only that
environment value produced exactly the endpoint's identity (`registry-identity.json`).
The hook bootstrap now passes the absolute inventory path it actually reads to
its diagnostic child, matching native MCP startup.

This fixes the missing executable instead of suppressing nonzero hook exits.
The [official command-hook contract](https://learn.chatgpt.com/docs/hooks) was
checked on 2026-09-08; the locally reproduced command and actual installed CLI
remain the evidence for this incident.

## Identities and controlled baseline

- Source revision: `dfc2d6af49f2da949b2b1b0ed549a33c62d1eb4b` plus existing dirty
  work. Task implementation changes are limited to `tools/kit.psm1`,
  `tools/hook.ps1` and the added case in `tests/installer.Tests.ps1`.
- Windows PowerShell 7.6.5, Codex CLI 0.153.4, OpenSpec 1.12.0.
- Python: installed Serena environment,
  `C:\Users\noilw\AppData\Roaming\uv\tools\serena-agent\Scripts\python.exe`.
  The WindowsApps `python` alias is not a working interpreter.
- Local evidence directory:
  `C:\Users\noilw\AppData\Local\Temp\codex-hook-exit1-b12keb6k`.
  It retains `baseline.json`, `candidate.json`, original `kit.baseline.psm1`,
  `hook.baseline.ps1`, `installation.before.json`, source hashes and test/repair receipts.

The PreToolUse encoded command from `global/hooks.json` was executed with a
read-only Bash payload from that owned external directory. Before repair it
exited naturally with code 1 in 0.531 seconds and named the missing launcher.
After repair the identical command, cwd and payload exited 0 with `{}` in
0.690 seconds. Each invocation had a 12-second outer bound. No historical
known-good version is claimed.

## Verification

Commands below ran from the repository unless stated otherwise. The installer
test received the original Node-distribution `codex.ps1` path via `-CodexCommand`,
not the harness launcher on PATH.

| Check | Result |
| --- | --- |
| `pwsh -NoProfile -File tests/installer.Tests.ps1 -CodexCommand <original-cli>` on baseline | Failed only the new assertion: core update preview removes connected hooks |
| Same installer suite on candidate | Passed: 24 lifecycle scenarios, 342 assertions, including native external install/Check, repeated updates, relocation, hook repair, foreign replacement, rollback and recovery |
| PowerShell parser on both changed scripts | No parse errors |
| `harness-lsp.diagnostics` on `tools/kit.psm1` | Unavailable: existing PowerShell adapter hit its 20-second deadline; not a clean result |
| Direct installed `PSScriptAnalyzer`, `-Severity Error`, all three changed scripts | No errors |
| Transactional `Invoke-HarnessInstall -IncludeCodeTools -Preview` against the real home | Exactly two new links, no removals or PATH change |
| Same transaction without Preview | Connected, 15 links; native startup read global instructions and existing Full Access defaults |
| Public `install.ps1 -CoreOnly -Mode Update -WhatIf`, then actual Update outside checkout | Empty operation preview; update succeeded and retained both owned hook records |
| Global config/MCP/runtime/subscription metadata hashes before/after | Identical for all five saved files |
| Installed `tests/lsp-installed-reconciliation.py`, run outside checkout using global hooks and native MCP entry | Passed: 20 events/scenarios, including real TypeScript finding/clearance, unchanged reads, large-file unresolved status, Stop/SubagentStop repetition and exactly one delivery in native/command race |
| `pwsh -NoProfile -File tests/hook-encoding.Tests.ps1 -KeepProbe` | Passed: 3 assertions with real TypeScript diagnostics and correction in a Cyrillic workspace despite initial OEM866 console encoding |

The installed functional check initially stopped on an honest unavailable result:
the broker reported an older source identity. The initial stale-process hypothesis
was rejected when a newly started broker reproduced the same mismatch. Original
failed outputs remain in `installed-hooks.log`, `installed-hooks-candidate.log`
and their temporary reconciliation fixtures. No assertion was weakened.

The diagnostic broker retirement attempt waited 30 seconds and remained pending.
Authenticated status showed two leased retired Markdown backends, whose children
were hours old. Scoped recovery terminated that verified broker after checking
its receipt PID/start time, command and separation from control-process ancestors.
One PowerShell descendant survived; it was separately verified by creation time
and exact owned session-details path and stopped. Both the incomplete first
cleanup and its correction are retained. This restart did not fix the environment
mismatch; the bootstrap change did. The session transport was not stopped.

Final installed functional report:
`C:\Users\noilw\AppData\Local\Temp\harness-installed-reconciliation-_qnwurn0\report.json`.
Maximum observed command times: PreToolUse 0.864 s, PostToolUse 3.664 s,
Stop 2.323 s, SubagentStop 2.166 s, all within their configured deadlines.
The native test printed a non-fatal installed Pydantic annotation warning;
protocol assertions and exit status passed.

This is actual installed command/MCP acceptance with owned source fixtures, not
a model-driven interactive session. No product repository or controller was used.

## Activation and recovery

Restored links point directly into this checkout. Sessions already holding the
command definition use the repaired launcher on their next invocation. A process
started while `hooks.json` was absent needs a new session to load the definitions.
Hook definitions, native trust state and model routing were not edited.

The source correction has a saved baseline; the installation transaction retained
its ordinary rollback protection. The original damaged installation JSON is
preserved for incident comparison. Restoring that damaged JSON alone is not a
safe operational rollback because it would discard hook ownership again. To undo
only the source patch, preserve the restored links and avoid core-only updates
until a corrected installer is reapplied.
