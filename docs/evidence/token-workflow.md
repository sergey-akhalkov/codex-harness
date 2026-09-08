# Token workflow acceptance

Status: globally activated, verified and archived on 2026-09-08 as [optimize-agent-token-workflow](../../openspec/changes/archive/2026-09-08-optimize-agent-token-workflow/proposal.md). All nine tasks are closed. Six new requirements and one modified lifecycle requirement were synchronized and compared exactly with main specs before archive; strict validation passed for all 12 main capabilities. Archive used `--skip-specs` because the verified inline sync had already finished.

Observed runtime: Codex CLI 0.153.4; PowerShell 7.6.5; RTK release 0.48.0 Windows x64 archive and executable hashes pinned in [global/rtk.json](../../global/rtk.json). OpenSpec strict validation passed with all four planning artifacts and nine implementation/acceptance tasks.

Completed scoped checks in this task:

- `tests/launcher.Tests.ps1`: 157 assertions, including actual native argv/streams/exit/cancellation and routine/standard/demanding effort selection with explicit overrides and unchanged prompt tokens.
- `tests/installer.Tests.ps1 -CodexCommand <recorded-original>`: 24 lifecycle scenarios, 353 assertions. Source/runtime identity must be rechecked if its relevant inputs change.
- `tests/hook-policy.Tests.ps1`: four native semantic/idempotence cases plus malformed-config preservation after the feature editor refactor.
- `tests/token-workflow.Tests.ps1`: 15 assertions with native feature editor and real source links, a dependency stub only. Covers fresh core hooks-off, selection, repeat activation, source relocation, repair, explicit suspension, foreign-file conflict, retained transaction/recovery, disconnect and preexisting Code Mode. Actual artifact/protocol checks remain separate.
- [Semantic fixture evidence](token-semantic.md): actual fresh Serena and CBM tool calls, one scoped apply_patch fix and an independent oracle. Metadata freshness limitations and unpaired timing are explicit.
- New skill frontmatter and scaffold validation passed through the installed skill creator validator.
- `tests/activation.Tests.ps1`: 19 scenarios, 152 assertions for the combined lifecycle. Subsequent changes to RTK classification and its tests did not change that activation mechanism.
- `tests/rtk-adapter.py --adapter <installed adapter> --pytest-python <owned pytest environment>`: all 16 scenarios accepted (15 passed in the full run, followed by the corrected pytest presentation oracle passing alone). Actual child argv including empty strings/quotes/Unicode, cwd/env/stdin/stderr, exits 7/128/101/1, exact-once execution, raw recovery, machine formats, bypass, invalid/missing/hung filter, capture failure, binary and >4 MiB output were exercised. The hung native fixture timed out at the bounded filter path and returned raw. Real Cargo and pytest failures preserved failure details and status. Pytest 8.4.2 was installed only in owned `TEMP/harness-rtk-pytest-2869cd5719034f50815395a9f57b0fc9`; normal tests do not download it. The first pytest oracle incorrectly required the literal `FAILED`, while RTK emitted `[FAIL]` and the correct failure count/detail; the corrected check tests the information rather than that presentation. Raw-file rotation was not separately stress-tested.

The pinned dependency and native adapter were built twice through the real provisioning entry point in owned `TEMP/harness-token-build-0399a6e19df04434b2f5718523e6a2c5`; the second call reused the identified artifact. Global `install.ps1 -TokenWorkflowOnly` and component Check passed. Build source identity: `b18386726ae60a897d96a02bc12718402d41d33907b835e430884bf424342b4d`; installed adapter SHA-256: `4ca2cb8e9426a8688eb91f4b4e62b015265099f234287d82f66c24f9480bb39a`. Identity covers Cargo sources/lock, dependency definition and builder; each build records its binary hash and Cargo version. Host-path builds can have different binary hashes; byte-for-byte reproducibility across hosts is not claimed.

`tests/rtk-measure.py --adapter <CODEX_HOME>/harness/bin/harness-rtk.exe` passed against the installed binary. Three raw/optimized read-only pairs per case, owned external fixture, same raw-output oracle; records at `TEMP/harness-rtk-measure-v4eari23/report.json`:

| Command | Net output-byte reduction | Raw median | Adapter + shell + filter median |
| --- | ---: | ---: | ---: |
| Git log, 80 commits | 97.45% | 302 ms | 348 ms |
| rg, 200 matches | 94.68% | 284 ms | 328 ms |
| Git short status | 0%, identical passthrough | 320 ms | 360 ms |

The last case deliberately retains already compact output. A first test incorrectly required status compression; actual RTK behavior showed no reduction, and the acceptance oracle was corrected to require unchanged output. Full Git history and all search matches survive in raw capture. These are byte counts, not tokenizer measurements. The measurement includes calling the native adapter hook but excludes Codex's hook dispatcher.

`tests/token-workflow-native.ps1 -TrustHook -RunModelProbes` then passed against actual global configuration in `TEMP/harness-token-native-09fe4514e6cf4c3a9710afd59e431969`. Two named Astra threads used the globally discovered skill and executed the independent Code Mode batch. Actual rewritten command `harness-rtk.exe compact git log -n 80` returned 253 bytes from 11,759 raw bytes; all 80 commits were independently verified in the capture. The second command returned native exit 1 and its missing-file error remained visible. Another external root saw the same sole enabled, trusted RTK definition. Native hook hash: `sha256:1914caf6f4ea4f558efc894660fb096e808a44e7de9061c688d172c05450f135`.

| Effective Astra effort | Oracle | Task elapsed | Reported input / cached input / output tokens |
| --- | --- | ---: | ---: |
| low | latest subject + failed call passed | 26.82 s | 55,077 / 35,712 / 180 |
| xhigh | same oracle passed | 22.51 s | 55,184 / 35,712 / 282 |

The native turn context confirms both efforts. One pair proves functionality and the matched oracle, not a speed advantage for low effort: it was slower in this sample. Cached input is part of input, not an additional quantity. Reported reasoning-output tokens were zero for both; no hidden reasoning volume or subscription discount is inferred. Keep the conservative default; lower effort is an available task choice with acceptance checks.

Native hook events took 256/272 and 285/313 ms for the two-call batches; skill-read hooks took 382 ms on the first thread and 238 ms on the second. This is additional native dispatch latency, separate from the adapter table. The user explicitly accepted the reported 250–313 ms cost after the first 30–60 ms measurements; the initial design threshold was updated with that decision. First-call evidence is retained. No end-to-end raw-vs-optimized model-task speedup or weekly quota saving is claimed.

Two initial trust probes were preserved as failures: the TUI needed separated text/Enter input, and selecting the linked profile made native trust state land in its source. The final probe reviews the base consumer without a profile, exits normally, and verifies trust through a new native API connection. Only the temporary project entries and RTK trust section introduced by those failed probes were removed from the reusable profile; existing user content was preserved. The accepted hash lives in machine-local base state.

Global Update and a subsequent fresh native configuration/trust probe passed without model calls. Retired pre/post/stop handlers returned silently when invoked explicitly; their separate RTK handler does not restore diagnostics. Local links in all nine owning/new planning documents and changed PowerShell syntax passed. Detailed native events, rollouts, `verified-summary.json` and per-file SHA-256 `source-inputs.json` remain under the evidence root; the report does not treat an enabled flag as successful execution.

The [native acceptance script](../../tests/token-workflow-native.ps1) uses owned external Git fixtures and the installed subscription route. `-TrustHook` reviews only the single expected RTK definition through ordinary native TUI. `-RunModelProbes` explicitly performs two named Astra tasks with the same answer oracle. It records process/session evidence, output and elapsed time, and must not infer weekly quota savings.
