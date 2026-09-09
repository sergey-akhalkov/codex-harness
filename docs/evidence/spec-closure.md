# OpenSpec closure audit — 2026-09-08

The user requested implementation and archival of all 13 active changes.
This record covers the changes actually reconciled so far; remaining changes
stay active until their complete implementation and acceptance are proven.

| Archived change | Requirement evidence retained and rechecked |
| --- | --- |
| require-relevant-skills | [Policy scenarios and activation](mandatory-skills.md); full current source loaded through native prompt-input in two external Git roots, including local AGENTS.md composition |
| prefer-verified-global-mcp | [Actual parent/child MCP operations](mcp-tool-selection.md); current global link hash and both native prompt-input checks |
| fix-diagnostic-hook-timeouts | [Original failure, timing and native delivery](hook-timeouts.md); current progress, pre-budget and real adapter suites |
| fix-markdown-stop-hooks | [Native Markdown error/clearance and boundaries](markdown-stop-hooks.md); current real sibling-link tests and Stop/companion suite |
| stabilize-diagnostic-reconciliation | [Discovery, recovery, transport, native child and read-only consumer acceptance](diagnostic-reconciliation.md); current reconciliation, delivery, deadlines and installed transport tests |
| fix-core-update-hook-links | [Original failure and global restoration](core-update-hook-links.md); current complete installer suite and real installed hook/MCP acceptance |

All six changes had complete planning artifacts and implementation tasks.
Their ADDED requirements were merged into the main specs without removing
existing content, compared in full after synchronization, and strictly validated
before moving the changes to `openspec/changes/archive/2026-09-08-<name>`.
Inbound links and the moved reconciliation evidence link were updated.

## Current verification

Commands ran from the harness unless stated otherwise. Python means the
installed Serena Python 3.13 executable, not the WindowsApps alias.
Codex CLI is 0.153.4; PowerShell is 7.6.5.

| Command / entry point | Result |
| --- | --- |
| `codex -C <external-root> debug prompt-input`, two existing owned roots | Full source matched; second also contained its local project instructions; no model calls |
| `pwsh -NoProfile -File tests/installer.Tests.ps1 -CodexCommand <original-cli>` | 24 lifecycle scenarios, 342 assertions passed, including hook preservation, relocation, rollback, collisions and disconnect |
| `python tests/lsp-hook-progress.py` | 5 passed |
| `python tests/lsp-pre-budget.py` | 9 passed |
| `python tests/lsp-stop-delivery.py` | 16 passed |
| `python tests/lsp-markdown-workspace.py` | 4 passed with the installed Markdown backend |
| `python tests/lsp-reconciliation.py` | 6 passed |
| `python tests/lsp-adapter.py --real` | 33 passed in 73.647 seconds |
| Absolute `tests/lsp-installed-reconciliation.py` entry from TEMP | 20 scenarios passed through installed hooks/config, real TypeScript error/clearance, race deduplication and explicit incomplete states |
| `openspec validate --specs --strict` | 7 main specs passed after synchronization |
| Individual strict validation before each move | All six changes passed |

The original CLI path came from the existing installation's `codexCommand`.
The first installer run used the PATH harness wrapper and failed only its
separate-checkout startup with recursive invocation; that attempt remains in
the native tool transcript. Selecting the recorded original executable passed
the complete suite without code or assertion changes. The first prompt audit
expected an absent phrase; the corrected assertion compares the complete source.

Primary current receipts, OpenSpec inputs and diagnostic source SHA256 values:
`%TEMP%/harness-spec-audit-8ae152f990de442da00b60c3a6fc9466/`.
Installed global report:
`%TEMP%/harness-installed-reconciliation-7bc_k_nu/report.json`.
Original TypeScript and Markdown native model receipts remain at their owning
evidence links; their process receipts show natural exit 0 and their reports
confirm actual patches, agent-visible findings and authoritative correction.

The current checks complement those historical native model runs; no new
model run, shared service restart, production write or opencode-kit execution
was needed for these six archives. Workspace-wide automatic diagnostics remain
incomplete during concurrent work on the large pre-existing diff. This record
does not claim a clean check of all repository files or completed remaining specs.

## 2026-09-08 subscription, delegation and resource archives

A later audit archived five completed changes after comparing every delta
requirement with its main spec, running applicable deterministic checks and
strict OpenSpec validation. CLI archive used `--skip-specs` because the
verified inline merge had already finished. The routing main already used the
newer Grok-middle wording and was left unchanged; overwriting it with the older
`grok_reviewer` delta would have failed archive sync.

| Archived change | Requirement evidence retained and rechecked |
| --- | --- |
| adaptive-agent-delegation | [Delegation acceptance](agent-delegation.md); current native consumer, usage, evidence and isolated-lifecycle receipts remain on disk |
| connect-subscription-model-routing | [Subscription routing verification](subscription-routing-verification.md); current routing suite 206 assertions, source validation and retained consumer receipts |
| enable-subscription-autorestart | [Runtime recovery](subscription-autorestart.md); current host SHA-256 still matches; routing-module drift is the later ConfigureRestart implementation and passed 206 assertions |
| stabilize-grok-delegation | [Grok reliability](grok-reliability.md); current oracle, guards, config and revalidated global continuation receipt |
| bound-code-tools-resources | [Tool resources](tool-resources.md); current ownership, lifecycle, CBM adapter and shared-service unit checks |

Created main specs: [grok-delegation-reliability](../../openspec/specs/grok-delegation-reliability/spec.md) and [bounded-tool-resources](../../openspec/specs/bounded-tool-resources/spec.md). Archive paths: `openspec/changes/archive/2026-09-08-<name>/`. Inbound docs links were retargeted. Remaining active changes are outside this audit.

| Command / entry point | Result |
| --- | --- |
| `pwsh -NoLogo -NoProfile -File tests/subscription-routing.Tests.ps1` | 206 assertions passed |
| `pwsh -NoLogo -NoProfile -File tests/subscription-service-recovery.Tests.ps1` | 8 controlled host cases passed; host SHA-256 `E28D122CAD108815FC194287C9C39EFC5776F91B97B834101A3A66B5C243F705` |
| `python -B tests/grok-continuation-oracle.Tests.py` | 9 passed |
| bun `tests/grok-guards.mjs` against installed OpenCodex 2.44.0 | PASS |
| `pwsh ... tests/subscription-config.Tests.ps1 -PackageRoot <installed-package>` | 10 scenarios passed |
| `python -B tests/delegation-usage.py` | 34 passed |
| `python -B tests/agent-delegation-evidence.py` | 4 passed |
| `python -B tests/tool-resource-lifecycle.py` | 11 passed |
| `pwsh ... tests/tool-resource-lifecycle.Tests.ps1` | 19 assertions passed |
| `python -B tests/process-ownership.py` | 11 passed |
| `python -B tests/lsp-shared.py` | 10 passed, 8 skipped without `--real` |
| `python -B tests/tool-resources.py` | 9 passed |
| `python -B tests/serena-shared.py` | 9 passed, 1 skipped without `--real` |
| `pwsh ... tests/grok-continuation.Tests.ps1 -RevalidateEvidence <global-receipt>` | 16 PASS against retained global continuation |
| `openspec validate --specs --strict` | 14 main specs passed |
| Individual strict validation before each move | All five changes passed |

Python is the adopted Serena 3.13 interpreter. Isolated scheduler probes, live
model probes, service stop and paid-API checks were not rerun. Historical
private receipts remain on disk; this audit did not reopen live subscription
destructive probes. Concurrent unrelated dirty work was preserved and not
committed.

## Native workflows — 2026-09-09

Archived [adopt-project-memory-and-native-workflows](../../openspec/changes/archive/2026-09-09-adopt-project-memory-and-native-workflows/proposal.md)
after all 16 tasks passed their applicable checks. The
[requirement map](native-workflows.md) links actual Git-memory/fresh-clone,
worktree/conflict, structured Astra and experimental-context/override/rollback
consumers. The final owned lifecycle and ordinary native discovery preserve
project memory, user configuration and unrelated capabilities; the real global
core Check reports 19 links, 12 skills and three agents without proxy restart.

The four new main specs contain all 15 requirements and 25 scenarios from the
accepted deltas. Exact post-sync comparison and strict validation passed:
18 main specs, zero failures. The move retained `.openspec.yaml`, adjusted nine
documents' relative links, and the subsequent reference check resolved 163
local file targets. The owned lifecycle evidence includes `archive-receipt.json`
with the source/target and pre-move file hashes. Runtime observations stay in
the owning evidence/design; main specs retain behavioral requirements.

Four other changes remain active: `migrate-harness-to-rust`,
`improve-installed-tool-workflows`, `autonomous-skill-evolution` and
`accelerate-verified-delivery`. This archive does not declare them complete.
The requested main-repository commit/push still follows completion of all active
changes; no main-repository commit or push was made by this archive.
