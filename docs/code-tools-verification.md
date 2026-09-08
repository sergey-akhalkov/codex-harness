# Global MCP/LSP acceptance

Historical acceptance of the 2026-09-06 selection. The current
[efficiency selection](evidence/subscription-efficiency.md) disables all hooks,
retires the separate harness diagnostic carrier and retains explicit Python
operations through Serena. The results below are preserved as historical evidence.

Observed on Windows x64 on 2026-09-06: Codex CLI 0.153.4, PowerShell 7.6.5,
OpenSpec 1.12.0. The archived change is
[connect-global-mcp-lsp](../openspec/changes/archive/2026-09-06-connect-global-mcp-lsp/tasks.md).
All 45 tasks are complete. Global activation, actual consumer operations,
automatic diagnostics and the tested installation lifecycle have passed.
The change was archived on 2026-09-06 at the user's request, after synchronizing
all four delta specs: three new capabilities and two modified linked-kit
requirements. Existing linked-kit scenarios were preserved. Environment limits
and retained versions below are part of this acceptance record.

Before archival, `openspec validate connect-global-mcp-lsp --strict` passed;
OpenSpec apply state was `all_done` (45 complete, zero remaining). Static parsing
passed for 37 Python and 30 PowerShell sources, both global JSON files, and
208 local links/anchors across 33 Markdown files. These checks complement the
actual runtime evidence below; they do not substitute for it.

## Delivered operation evidence

The real account has twelve direct source links and five native MCP path
registrations: Serena, Codebase Memory, Graphify, Nuphus and the diagnostic
adapter. The fifth endpoint supplies LSP operations; it is not a fifth upstream
MCP from opencode-kit. Seven native hook handlers are trusted through the real
TUI. No project-local connection file is needed. Existing user configuration,
credentials and source links are preserved.

| Requirement/tasks | Actual evidence | Scope |
|---|---|---|
| Native source consumption, 1.1–1.3 | [Native contract](evidence/code-tools-native.md), twenty assertions including real CLI/MCP edit, native patch, unknown hook nonce, trust invalidation and live source changes | Native CLI and unprofiled app-server, not a simulated hook response |
| Discovery and lifecycle, 2.1–2.6 | [Dependencies](evidence/tool-dependencies.md), [combined activation/recovery](evidence/code-tools-activation.md) | Official acquisition, exact ownership, shared consumer holds, failure recovery; substitute boundaries stated |
| Serena, 3.1 | [Serena](evidence/mcp-serena.md) and tests/lsp-native-mcp.Tests.ps1 | Two project symbol/reference/edit checks; actual global Unicode-project edits, automatic 2322 and clearance |
| Codebase Memory, 3.2 | [Codebase](evidence/mcp-codebase.md) and tests/mcp-codebase.py | Actual independent indexing and query, same-named Cyrillic roots, saved indexes survive restart; private Windows rendezvous |
| Graphify, 3.3 | [Graphify](evidence/mcp-graphify.md) and tests/global-code-tools.Tests.ps1 -ReadPublicRepository | Actual selected graph, authenticated HTTP and stdio fallback; explicit Unicode Git worktree read-only PR query |
| Nuphus, 3.4 | [Nuphus](evidence/mcp-nuphus.md), dependency OCR evidence | Actual owned window/browser interaction and offline OCR; original upstream executable audited |
| Selected languages, 5.1–5.15 | [Eighteen-row matrix](evidence/lsp-languages.md) | Fourteen required plus available HTML/CSS: actual native errors and current empty clearance; YAML/QML explicitly unavailable |
| Automatic edit/stop semantics, 1.4–1.5/4.1–4.5 | [Automatic diagnostic evidence](evidence/automatic-diagnostics.md) | Native shell exit7, yielded late write, MCP multi-file rename, delayed first handshake, missing backend, stale/current empty, configuration and dependent errors |
| Workspace and generation isolation, 6.2 | Automatic diagnostic evidence and [independent review](evidence/lsp-review.md) | Actual concurrent Git worktree, additional roots, Unicode paths, child identity and independent backend shutdown; controlled timing where necessary |
| Ordinary entry points, 6.1 | tests/global-session-lifecycle.Tests.ps1 -RunAgent -ReasoningEffort low, eight assertions | Linked global exec, persisted CLI resume and unprofiled native history fork all perform actual Graphify calls |
| Existing consumers and recovery, 6.3 | Dependency, activation and full-source-relocation reports; actual OpenCode Graphify validators | Reuse/update/rollback/conflict/reconnect/disconnect checks preserve credentials, indexes, graph and foreign settings; shared updates retain existing launchers and ownership |
| Regression checks, 6.4 | Launcher 147, core installer 21 scenarios/125 assertions, combined activation 19/152, registration 17, consumer 18, real global activation 64 | Actual native editor/consumers/TUI where applicable; lifecycle acquisition substitutes are distinguished in the owning reports |
| Material-risk review, 6.5 | Independent dependency/Graphify recovery review and LSP boundary review | Corrected shared manifest/rollback, native TOML ownership, six source freshness/selection counterexamples; affected checks repeated |
| Fresh source and reuse, 7.3 | [Full source relocation](evidence/full-source-move.md), forty assertions, plus actual isolated acquisitions | Fresh source copy, full Install, all-five-server Check, outside TS edit/clearance, move/reconnect and Disconnect; actual user's global connections/packages preserved |
| Global calls, 7.2 | tests/global-code-tools.Tests.ps1, eight assertions | New unprofiled native consumer outside the checkout: all four MCPs, seven trusted hooks, actual explicit repository operation, preserved foreign config |
| Subagent calls and automatic diagnostics, 1.5/6.1/7.2 | Global native child fixture lsp-native-25c26b8628354bae8f97e9ab85fc9ccf | Child calls all four MCPs; actual two patches produce automatic 2322 and clean feedback through bounded command fallback |
| Global semantic edits, 4.3/7.2 | [Final native Serena fixture](evidence/lsp-native-mcp.md) lsp-native-mcp-56d60cdc5f074f6d8842bf476661d6d7, fourteen assertions | Rerun after all freshness corrections; no shell, patch or manual diagnostic calls; model quotes both unknown hook revision prefixes, original Serena responses preserved |
| Documentation and final reconciliation, 7.4–7.5 | This requirement mapping, the language matrix, current official contracts in the owning reports and strict OpenSpec validation | Local links and source syntax checked; test registrations disconnected, owned probe processes shut down, retained artifacts distinguished below |

The successful global MCP report is at
`CODEX_HOME/harness/verification/global-mcp-1cf0174b794d4caabfdfb58f09241bd5/report.json`.
It observed Graphify's actual saved graph with 88,513 nodes, 172,936 edges and
4,173 communities and Nuphus's actual browser evaluation result 42. These are
operation evidence, separate from the installer's protocol-ready health check.
The final ordinary Install reports Connected with twelve source links; the
subsequent Check reports all five MCP endpoints protocol-ready. Their compact
machine-local records are `TEMP/harness-global-code-tools-final-install.json`
and `TEMP/harness-global-code-tools-final-check.json`.
Native edit traces and complete diagnostic reports remain in the named private
fixture directories under `%TEMP%`; their machine paths are not deployment inputs.

The final ordinary session report is
`CODEX_HOME/harness/verification/session-lifecycle-b3af587a19524752bbb69457bb29c501/report.json`.
Earlier first/resume turns lacked tools because the initial optional-MCP
catalogue was collected after only the default 1,000 ms grace. The same failed
session succeeded with the documented native readiness setting; ordinary
Install then registered it globally. The final test uses that global connection
without a test-only readiness override, including a short low-effort turn.
The setting waits for finite per-server startup timeouts and keeps unavailable
servers optional. [Official native configuration contract](https://learn.chatgpt.com/docs/config-file/config-reference).

## Current-host updates

The common Codebase Memory installation was updated from 0.10.5 to 0.10.8,
Graphifyy from 0.9.44 to 0.9.55 and typescript-language-server from 5.1.3 to 6.0.0.
Previous versions and recovery journals remain in machine-local rollback storage.
TypeScript 5.9.3 and the saved graph were preserved. Real OpenCode Graphify
ownership validators accept the selected package/manifest and original launcher.

PSES 4.7.0 passed staged checks, but 4.4.0 remains selected because other sessions
use the shared installation. Rust Analyzer 1.97.1 remains paired with the existing
project toolchain. These explicit holds mean `all_updates_applied=false`; they
are not missing language support. The next explicit Update rechecks them.

## Verification limits and retained artifacts

No second physical Windows machine or clean VM is available. Real acquisitions
in isolated user roots and fresh checkout relocation are reported separately;
they must not be described as a full clean-VM test. Installed Desktop Appx/IDE
consumers were not found in the bounded local inventory. The real unprofiled
app-server proves that global configuration does not depend on the CLI profile,
but it is not a claim that an absent desktop application was exercised.

The native subagent's MCP hook manager cannot resolve the parent's connected
adapter in the tested version. The source-owned command fallback handles the
same child journal and delivers actual results before return. Failed, missing,
stale or timed-out analysis remains explicit and never becomes a clean report.

Test-only registrations were isolated and disconnected, and owned verification
processes were shut down using their captured identities. Earlier failed probes
are retained as failures; only subsequent actual passing evidence supports the
corresponding claims. Two rejected early backend alternatives remain installed
but unselected: their first provisioning lacked a complete inverse ownership
journal, so deletion could not establish safe restoration of prior state. The
[dependency report](evidence/tool-dependencies.md) records that bounded audit.

Automatic approval review rejected deletion of two earlier Serena test roots
and one probe-only LSP journal with the reason “blocked by policy.” Those exact
artifacts remain; the rejected cleanup was not retried by another route.
