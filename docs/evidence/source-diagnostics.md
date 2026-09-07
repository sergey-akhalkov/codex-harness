# Source diagnostics acceptance

2026-09-07, Windows, PowerShell 7.6.5, native Codex CLI 0.153.4. Scope: [source diagnostics](../source-diagnostics.md), including its global installation lifecycle. No model-backed probes were required.

## Behaviour and lifecycle

- `tests/source-diagnostics.Tests.ps1 -NativeCodex <registered vendor codex.exe>`: **23 assertions passed** using actual app-server reads and isolated consumer/home fixtures. Covers clean report, profile provenance, three simultaneous conflicts (project setting, duplicate skill and retargeted link), restoration, explicit false, disabled untrusted layer, profile trust uncertainty, invalid TOML, secret suppression, independent link evidence and unchanged config/auth/history. A configured SessionStart command did not execute.
- `tests/source-diagnostics-failures.Tests.ps1`: **passed**. A deliberately hung executable exceeded a one-second RPC budget; the report was incomplete, returned within ten seconds including cleanup, discarded its stderr sentinel, terminated the owned child and preserved its parent.
- `tests/installer.Tests.ps1 -CodexCommand <registered original wrapper>`: **21 scenarios / 137 assertions passed**. Real isolated install/Check/disconnect plus injected lifecycle failures. The new command has a direct owned source link, survives repeat/update and checkout relocation, participates in rollback/recovery, and is removed by owned disconnect. Tampered destinations and foreign state remain protected.
- `tests/launcher.Tests.ps1`: **147 assertions passed**, including native forwarding, output, exit and cancellation behaviour. No launcher implementation changes were needed.

The first native fixtures exposed the RPC's highest-first layer order; reversing it before profile insertion fixed a missed project override. Lifecycle tests caught the new destination missing from the metadata allowlist; the exact destination was added without widening permitted roots. A Windows temporary working-directory lock was avoided by launching native readers from the parent temp directory and sending the requested cwd explicitly. Final fixture runs cleaned up successfully.

## Global consumer

Preview through `Invoke-HarnessInstall -IncludeCodeTools` showed exactly one added link and no PATH change. Activation used the registered Codex home, user home, dependency owner and original wrapper. Existing hook links were retained; subscription service lifecycle was not invoked.

From `C:\Users\noilw\AppData\Local\Temp`, ordinary command discovery resolved `C:\Users\noilw\.codex\harness\bin\codex-harness-check.ps1`. Its source was this checkout; JSON reported **healthy**, native 0.153.4, **13 connected links**, **12 skills**, and harness reasoning `xhigh`. One measured invocation took **704 ms** inside the diagnostic. This is a local observation, not a comparative benchmark or a claim of weekly quota savings.

SHA-256 comparisons before and after activation/inspection confirmed unchanged `config.toml`, `auth.json`, `history.jsonl`, `harness.config.toml` and `hooks.json`. Both existing captured consumer process identities (PID plus creation time) remained present. Normal `install.ps1 -Mode Check -CoreOnly` returned **Connected**. Already-running session and MCP/LSP freshness remained explicitly **unknown**.

## Evidence limits

Native `config/read` and `skills/list` observations are real; selected-profile effective values are reconstructed from native-parsed layers because 0.153.4 app-server rejects file profiles. The report labels this distinction. Profiles affecting project loading/skills, managed requirements and unverified CLI versions return incomplete. No claim is made about loaded revisions of existing servers, model quality, subscription savings or the full security of other audit candidates.

The source audit has a separate [bounded recheck](audit-recheck.md). Other pre-existing repository changes retain their own evidence records; these checks do not re-certify every capability in the kit.

Final integration checks: `tests/activation.Tests.ps1` passed **19 scenarios / 152 assertions**, including combined rollback and hard-crash recovery in isolated homes with subscription lifecycle stubbed. `openspec validate --specs --strict` passed all six main specs; strict validation of the new change passed. A deterministic check resolved **376 local Markdown file links**, with zero missing targets; native Markdown diagnostics were clean for the new documentation/spec files. `git diff --check` passed. These link checks establish local target existence, not freshness of remote documentation.
