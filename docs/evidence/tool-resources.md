# Tool resource incident and delivery

Change: [bound-code-tools-resources](../../openspec/changes/archive/2026-09-08-bound-code-tools-resources/proposal.md). Implementation and global consumer acceptance are complete; the [task list](../../openspec/changes/archive/2026-09-08-bound-code-tools-resources/tasks.md) records the accepted scope.

## Incident and scope

Windows, 8 logical CPUs, 16,555,320 KiB visible RAM. Installed CBM 0.10.8, Serena 1.7.0, Graphify 0.9.55, Nuphus 0.2.2 and Codex CLI 0.153.4. The user excluded a common Codex app-server; independent Codex applications and projects remain supported.

The retained CBM log `~/.cache/codebase-memory-mcp/logs/.worker-log-a24608` records 18,481 files, eight workers, 1,471,298 extracted nodes, a 5,844 MiB RSS peak and Windows allocation error 1455. Its roughly 1,010 MiB advisory budget did not contain actual memory. Saved pmac hashes included 10,888 generated HTML logs. This baseline was preserved from the incident without deliberately repeating a 5.8 GiB workload.

Native CBM frontends each consumed about 2.6% of host CPU during a 12-second idle sample; initially there were 11 frontends plus a daemon. Historical orphan inspection found 23 PSES instances, approximately 894 MiB working set / 1,232 MiB private memory. Their original owners had exited while dedicated cmd.exe and console processes survived.

A later pressure snapshot had only 891,456 KiB physical and 728,132 KiB commit headroom. It included 57 Node processes / 4,523 MiB private, 132 Python / 2,959 MiB, 77 PowerShell / 2,729 MiB, and three Codex processes / 662 MiB. These totals include other applications and are not wholly attributed to this task. Native acceptance workloads were serialized.

## Delivered policy

| Component | Reuse and lifetime |
| --- | --- |
| CBM | Thin MCP clients; native catalogue cached by audited binary SHA256; finite native query processes. One account-wide indexing slot, two workers, 1 GiB advisory budget, 2 GiB aggregate Windows Job commitment limit, 25% CPU hard cap, 600-second deadline, no supervisor retry. |
| CBM retained source | 8 MiB total / 1 MiB per file. Native cross-file passes reread uncached source; source files and semantic passes remain enabled. |
| LSP | One authenticated loopback broker per Codex home; at most four compatible root/language/configuration backends, 300-second idle retirement. Session journals, claims and revisions stay independent. |
| Serena | Shared project-routed broker; at most three native project backends, 300-second idle retirement, serialized operations and client-local activation/history. |
| Graphify | Reuses the verified authenticated HTTP endpoint. Owned native fallback has at most two graph contexts. The adopted external 0.9.55 service retains its own bounded eight-context cache and lifecycle. |
| Nuphus | Lazy native/browser startup, isolated browser state per client, 300-second idle cleanup; account-wide desktop mutation admission. Stale/foreign references are rejected without destroying the current page. |
| Launchers | Global registration directly uses the adopted Python interpreter; compatible entrypoints execute in that process. PowerShell compatibility remains supported. |

Auto-index, auto-watch and graph UI are disabled recoverably. Old watcher subscriptions are retired through native daemon control. Graphs require explicit refresh; MCP instructions expose this policy. Query deadlines are 60 seconds; the installed Codex consumer confirms the actual setting `tool_timeout_sec = 660` for CBM indexing.

The pmac consumer .cbmignore adds only `saturn/Сатурн/mtronnc/logs/` and `openspec/changes/archive/**/evidence/**/*.json`. The latter covers 3,925 generated measurement files, approximately 80 MiB. Files remain on disk; Rust sources, Markdown specifications, archived reports and reference documents remain eligible. No controller/product behavior changed.

Ordinary CBM CLI indexing delegates to an account daemon, so wrapping that client cannot constrain a pre-existing daemon. The adapter uses the audited native worker response-file protocol, retaining native project locks and atomic graph publication. Supported binary SHA256: `b4b403b1d7c4def3785f148b93f345ce8427858f4f5489ce28580c4387a336a6`. Unknown builds fail visibly until reviewed.

## Real pmac acceptance, 2026-09-08

The canonical D-mekha-mtronics-pmac-emulator full index completed with the delivered policy:

| Measurement | Observed result |
| --- | --- |
| Discovered files | 3,668 |
| Wall time | 27.874 seconds |
| Sampled peak private memory | 2,117,566,464 bytes, about 1.972 GiB |
| Sampled peak working set | 1,983,877,120 bytes, about 1.848 GiB |
| Sampled CPU time | 43.547 seconds; approximately 19.5% of eight logical CPUs over the measured interval |
| Published graph | 128,315 nodes / 219,315 edges |
| Skipped / partial parse | 0 skipped; 118 partially parsed files explicitly reported by native coverage |

A fresh named graph also completed in 22.576 seconds at 2,102,546,432 bytes peak private memory. Earlier bounded attempts failed at the enforced allocation limit: ordinary CLI supervision allowed native retry; a direct worker with the default retained-source cache exceeded the cap; one extraction worker selected a different sequential pipeline and also failed. These are failed probes, not accepted indexes. Two workers with the small source cache resolved the observed case.

Native warnings remain visible: lsp_surface.serialize_failed prevented saving an incremental optimization surface, configlinker.truncated reported its 8,192-item cap, and ignored-file details were capped at 2,000 entries. The full cross-file pass ran and reported 18,219 LSP overrides. Successful publication does not imply complete parsing or exhaustive relationships; use coverage and source fallback.

Machine-local receipts under %LOCALAPPDATA%/codex-tool-resources/: pmac-index-final.json (canonical result and measurement), pmac-clean-name-result.json (fresh graph), pmac-index-acceptance.json and pmac-index-serial.json (failed probes). Original RSS and new private-memory numbers use different accounting; working set is listed separately.

The exact installed CBM registration was then launched from an owned directory outside the checkout. Project identity and ready graph counts matched. Search found `guarded_direct_listing.read_listing`, source retrieval succeeded, and a depth-one trace returned `read_with_recheck` plus cross-file `Driver.pre_send_recheck` with native confidence/strategy labels. Specification text remained searchable. Only the owned temporary `pmac-resource-probe-17a07fb3ef` graph was deleted after acceptance.

Native coverage reported `coverage_unavailable / metadata_changed` for representative Rust and Markdown paths even though an independent read-only comparison found their saved SHA256, nanosecond mtime and size exactly equal to current files. This upstream coverage signal remains a limitation; it was not relabelled clean or used as proof of missing source. Receipts: cbm-global-acceptance.json, cbm-global-symbols.json and cbm-global-semantic.json in the same machine-local directory.

Three new CBM clients accumulated 0 CPU seconds during a 6.012-second idle sample. Their main Python processes used 46.2–46.5 MiB private memory each, plus small interpreter shims and consoles. No native indexer/frontend remained in their owned trees; a console briefly outlived immediate SDK closure, and a later PID/creation-time check found all captured identities gone. A separate four-second host snapshot had 5,742.7 MiB physical memory available and three old CBM frontends still using 6.88% host CPU in total. Those pre-existing clients require session restart. This changing host snapshot is not a controlled attribution of total RAM savings; see host-after.json.

## Verification and activation

- Windows ownership: 11/11 native checks, no skips. Atomic admission, crash/normal cleanup, descendants, Unicode pipes, initialization failure and CPU limits passed. Independent commitment was 67,059,712 bytes under a 67,108,864-byte fixture limit; excess allocation returned 1455. Peak-job telemetry can include denied commitments, so that field alone is not an enforcement oracle. Receipt: %TEMP%/process-ownership-uyo4p_2z/report.json.
- LSP: seven native shared-service cases; existing native adapter suite 33/33; final shared unit suite 10/10, including bounded obsolete queued work. Activation-pending rejection exercised through a real daemon.
- Serena: eight native routing/lifecycle cases passed: compatible reuse, distinct roots, authentication, eviction, idle retirement and crash cleanup of 19 descendants. Receipt: %TEMP%/serena-shared-native-hfu9qf1r/report.json.
- Graphify: real owned STDIO fallback and authenticated HTTP reuse passed with ten tools, preserved saved graph bytes, and an explicit missing-project error. Receipt: %TEMP%/harness-graphify-705gpt5i/report.json.
- Nuphus: 18 native isolation/lifecycle assertions passed; idle retirement with a one-second fixture setting completed in 1.163 seconds. Final global registration from an owned external directory passed nine checks with all 38 tools: stale references preserved the page and valid references still worked. No owned descendant remained after closure. Receipt: %TEMP%/nuphus-resources-9b5_b7hw/report.json.
- Registration: 18/18 tests, including native codex mcp get --json and preservation of unrelated TOML bytes. Resource lifecycle: 11 Python tests and 19 PowerShell assertions. Existing activation: 19 scenarios / 152 assertions. Scoped activation: 56 assertions.
- CBM final adapter: nine tests passed, including policy drift rejection before native launch and reclamation of a late writer grandchild before reading results. A cold native catalogue returned all 15 tools, warm lookup spawned none, and a forced failed refresh preserved the exact committed database hash and readable old symbol. Configuration reads close SQLite connections deterministically. Receipt: %TEMP%/cbm-resource-evidence-co_afh47/report.json.

Global LSP and Serena semantic calls passed from outside the checkout: two clients reused a backend, a TypeScript 2322 error cleared after correction, and Serena returned resources.py symbols. Global Graphify queried its explicitly identified `D:/mekha/mtronics/graphify-knowledgebase` saved graph (88,513 nodes / 172,936 edges), whose bytes remained unchanged. During three-second idle samples, main thin LSP processes used about 47 MiB each and Serena about 23 MiB; all sampled thin/shim CPU deltas were zero. Broker memory alone was about 61 / 24 MiB respectively and excludes native language-worker memory. Receipt: %TEMP%/harness-global-resources-27p732pw/report.json.

A stronger cold-start lifecycle probe found a defect: closing the first Windows SDK client killed its newly launched shared broker while the second client still needed it. Both LSP and Serena returned connection-refused. The failed receipt is retained at %TEMP%/harness-global-resources-6y0j979r/report.json.

The corrected startup uses built-in local WMI to create the broker as the same verified user, outside the starter's Job. Environment is passed through STDIN into the native process environment, never through command lines or handoff files. The service owns its own Job before loading broker code, has a finite readiness watchdog, preserves script import paths, and adds no resident launcher interpreter, scheduled task or installed Windows service. Owned fixtures proved Unicode environment/logs, same SID, sibling imports, independence from a 192 MiB client Job, normal/crash cleanup and unpublished-startup timeout. Receipt: %TEMP%/service ownership-yqqyrj4i/report.json; original JobGuard suite remained 11/11 at %TEMP%/process-ownership-drnxsuir/report.json.

The exact global two-client regression then passed for both tools: after the starter closed, LSP broker 27788 / backend 28268 and Serena broker 31192 / project worker 19464 retained their identities, and the second client completed semantic calls. Explicit retirement returned retired for both brokers after client closure. Receipt: %TEMP%/harness-global-resources-r8s1c5sh/report.json. Source review found and corrected the intermediate script-import regression; no remaining blocker was found in this bounded review.

Actual global `install.ps1 -CodeToolsOnly -Mode Install` and `-Mode Check` passed. All five MCP registrations became protocol-ready. This scoped route used existing dependencies and preserved unrelated core/subscription changes; it did not install a Codex server.

Historical cleanup checks exact registered PSES executable, -File script and harness runtime paths, cmd /c arguments, missing owner and PID creation times. Final fresh cleanup retired PID 27708; the next audit returned no eligible candidates. Receipts: %LOCALAPPDATA%/codex-tool-resources/pses-cleanup.json and pses-cleanup-final.json. User Codex sessions and unrelated services were retained.

## Recovery and evidence boundaries

Use the [installation lifecycle](../installation.md), including scoped Install, Check, Recover and Disconnect. Resource ownership is account/cache scoped with a set of Codex-home owners: one owner's disconnect does not restore settings still needed by another; last-owner restoration changes only values that still equal the applied policy. Later user changes remain intact. Interrupted transactions remain recoverable.

Enforcement applies to delivered kit entrypoints; direct native invocations can bypass supervision. Existing MCP processes have loaded old code and must be replaced by restarting their Codex sessions; registration does not retrofit them. Automatic LSP diagnostics in this heavily changing workspace reported incomplete checks and cannot establish a globally clean codebase. Shared pools bound duplicate servers and queued work; they do not eliminate analysis CPU after source changes.

Primary contracts: [CBM 0.10.8 configuration](https://github.com/DeusData/codebase-memory-mcp/blob/v0.10.8/docs/CONFIGURATION.md), [retained-source cache and fallback reads](https://github.com/DeusData/codebase-memory-mcp/blob/v0.10.8/src/pipeline/pass_parallel.c), [native worker and CLI dispatch](https://github.com/DeusData/codebase-memory-mcp/blob/v0.10.8/src/main.c), [Windows Job Objects](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects), [extended startup attributes](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-updateprocthreadattribute).


Final global scoped Check passed after the service-bootstrap correction: connected registrations, active resource policy and protocol-ready health. Receipt: %LOCALAPPDATA%/codex-tool-resources/global-check-final.txt. OpenSpec strict validation and the scoped local-link check (108 targets) passed.

