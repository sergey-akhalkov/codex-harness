# Automatic diagnostic acceptance evidence

Observed on Windows x64, 2026-09-06, with native Codex CLI 0.153.4 and the
installed shared language backends. The implementation lives in
[server.py](../../tools/lsp/server.py), [backend.py](../../tools/lsp/backend.py)
and [journal.py](../../tools/lsp/journal.py); the global entry points are
[hooks.json](../../global/hooks.json) and [hook.ps1](../../tools/hook.ps1).
Package provisioning and registration lifecycle have separate acceptance
evidence in [the overall report](../code-tools-verification.md).

The native tests actually edit disposable projects outside this checkout.
The model receives automatic feedback before its completion response; the
prompts prohibit explicit diagnostic calls. Direct backend tests establish
language semantics and controlled failure paths separately. The
[language matrix](lsp-languages.md) records fourteen required languages and
available HTML/CSS, including the limits of each selected server.

## Implemented contract and bounds

PreToolUse records an independent SHA256 baseline before the diagnostic MCP
needs to initialize. The SQLite journal is under
`$CODEX_HOME/harness/runtime/lsp`, separated by canonical workspace, session
and agent identity. It hashes current bytes, including pre-dirty, untracked
and Git-ignored source; it does not derive changes from a Git diff or shell
command text. Known explicit file arguments supplement the bounded scan.
The scanner excludes dependency/build/VCS directories, follows links only
inside the applicable root, and preserves previously known source identities.
An incomplete scan is unresolved, never an empty successful check.

Native PostToolUse calls `harness-lsp.diagnostics_after_tool` synchronously.
The supported command handler waits briefly for that owner, then runs the
same adapter in a short-lived process when no native MCP consumer claims the
invocation. This is necessary for the observed native child tool proxy:
the child can call all four MCPs, but its local `mcp_tool` hook manager does
not connect harness-lsp. The real global child probe below verifies the
command path. Connected ordinary consumers retain their warm LSP processes.
Claims and completed-invocation records prevent duplicate checks; repeated
Stop checks compare bytes again before reusing a recent completion.

| Boundary | Implemented limit and failure behavior |
|---|---|
| Pre baseline | Seven seconds shared across at most 32 explicit roots; native command timeout ten seconds |
| Native diagnostic batch | 27 seconds including its ordinary snapshot/waits; native hook timeout 30 seconds; unresolved work remains journaled |
| Command fallback | 25-second internal budget, including a 0.6-second native-claim grace; bounded cleanup within the native 30-second handler timeout |
| Individual analysis | Normally at most 20 seconds; remaining dependent-file budget is shared |
| Source reconciliation | Initial scan at most five seconds, final scan at most two seconds, at most 8 MiB per file; exhaustion is explicit |
| Summary | At most 60 files, 30 diagnostic entries, 12,000 message characters total, 2,000 per message and 1,000 per reason; omitted counts and full report path retained |

Stop and SubagentStop reconcile unfinished or late changes. Their first
unresolved completion can block; an already active Stop hook produces a
system message instead of an infinite retry loop. A parent that only
delegated and had no covered tool invocation is `not-applicable`, which is
not a clean diagnostic claim. A missing baseline after an observed edit
remains unavailable. Timeouts terminate only the fallback-owned process
tree; successful user edits and unrelated consumers remain intact.

Every source result records file, content SHA256, backend, status, and
available diagnostic range/severity/code/source. Current pull responses,
versioned pushes, or an explicitly correlated source-snapshot validator can
establish clearance. Missing/unversioned/uncompleted empty notifications do
not. Backend validation error logs invalidate an otherwise empty response.
The final full source/configuration snapshot detects newly created files as
well as changed/deleted inputs. Pending jobs include the complete source
snapshot generation in their key, so unchanged callers cannot reuse work
started against an older library revision.
Before querying a source, the client synchronizes its already opened changed
or deleted dependency buffers. This prevents a multi-file disk edit from
combining the new caller with an old imported buffer. Closed imports remain
the language server's ordinary current-disk responsibility.

TS/JS dependent files come from the actual TypeScript project service.
Other selected languages conservatively recheck their same-language files
from the bounded workspace snapshot. JSON/JSONC and XSD/DTD can supply
arbitrarily named schemas, so their edits invalidate applicable project
analysis. Large cohorts can remain pending; this is not a claim to complete
every project within 30 seconds. Read-only unchanged tools do not start a
new full-project analysis. No formatter, automatic fix or runtime dependency
installer is called by this path. Diagnostic messages are explicitly labeled
as data, not instructions, before model delivery.

## Native evidence and task mapping

The probe identifiers below are directories named `lsp-native-<id>` under
the verification account's `%TEMP%`. They retain native events, scoped full
diagnostic reports and fixture sources. Successful wrapper assertion counts
support the named scenario, not untested behavior.

| Tasks | Actual entry point and observed result | Retained probe |
|---|---|---|
| 1.3, 4.3, 4.4 | Native TS patch introduces 2322, correction receives current empty; sixteen language records separately validate actual edits, automatic hooks and model-visible feedback | `harness-lsp-native-evidence.json`; [matrix](lsp-languages.md) |
| 1.4, 4.2, 4.3 | Shell edits an actually pre-dirty tracked file and creates an untracked file, then exits 7; both errors and both corrections are automatic; original sentinel/exit survive; 11 assertions | `20e6f854143d4ed0a4b58bcc96374dd6` |
| 1.4, 4.3, 4.5 | A real shell command yields before a delayed write, is polled, and receives error then clearance; seven assertions | `19ec10a180244575af14e74678e40d59` |
| 1.4, 4.2, 4.3 | Disposable native MCP renames the source and creates an importing file; old identity is deleted, surviving identities reconciled, original MCP result preserved; 11 assertions | `b70f3bb117f444de85307aba8db931e9` |
| 4.3, 6.2 | Actual globally registered Serena performs two edits in a path with spaces and Cyrillic; model quotes error/clean and the two previously unknown SHA prefixes; 14 assertions plus owned-process cleanup | [Unicode Serena evidence](lsp-native-mcp.md) |
| 4.4 | Only tsconfig strict changes false→true→false; unchanged source receives 2322 then authoritative empty; eight assertions | `10cbfb5786c34cb9b24868c43e3312a8` |
| 1.5, 4.2, 4.5 | First patch completes before an intentionally 35-second delayed MCP handshake; independent command fallback delivers its actual error before immediate completion; eight assertions | `a2092abf5522495483c299c6116f5269` |
| 1.5, 4.3, 4.5, 6.2 | Real global child calls Serena, Codebase, Graphify and Nuphus successfully, then makes two native patches and receives 2322/clean automatically through command fallback; child returns before parent completion | `25c26b8628354bae8f97e9ab85fc9ccf` |
| 4.5 | Actual native edit with a backend that exits 37 produces explicit unresolved feedback, no false clearance and bounded completion; intended edit remains; seven assertions | `29b08452be2a46f7a808acd90890d6f2` |
| 4.2, 6.2 | Actual native edit in an explicitly approved additional root receives error/clean while the same-named primary-root file stays unchanged; ten assertions | `3f8e8517272b4455843946daf16ab4e0` |

The child events independently show four successful `mcpToolCall` responses,
including Nuphus evaluation `6 * 7 = 42`, before the two child patches. Its
original six-assertion wrapper report predates the additional four-MCP
assertions now in the runner; the retained call events, rather than that old
assertion count, establish the four reads. Native child Pre/Post share the
parent session identifier but carry the child's transcript pathname.
SubagentStop uses its `agent_transcript_path`; the implementation hashes
the pathname and never parses unstable transcript contents.

## Targeted regression and corrected failures

[lsp-adapter.py](../../tests/lsp-adapter.py) passed 33 tests with `--real`,
using installed servers. These include actual TS dependent errors and
clearance, arbitrary JSON schemas, Python imported return-type changes and
XML schema changes that invalidate unchanged consumers. A real Git worktree
shares metadata with the primary repository while concurrent same-named
symbols produce different expected errors. Shutting down one workspace's
backend leaves the second warmed backend working and clears its error.

Controlled tests cover late configuration creation, a new source arriving
before Stop delivery, an old empty push arriving after a newer error,
pending caller work after an intervening library change, missing baseline,
partial scans, preserved-mtime byte changes, summary bounds, and a hung
fallback whose unrelated Python consumer survives. Controlled scheduling
is used to make the races reproducible; those tests do not replace native
entry-point evidence. There is no filesystem watcher queue: the equivalent
loss-of-coverage boundary is an exhausted/incomplete hash scan, tested as
explicit unresolved status.

The [independent boundary review](lsp-review.md) reproduced and rechecked
four failures: a recent Stop hiding a later write, omitted unchanged
dependents, Rust channel path escape, and a source created during final
delivery. A further controlled pending-job counterexample is retained as
`harness-pending-source-generation.json`; the regression prevents reuse of
its earlier source generation. Rust project channels now select verified
installed names inside the canonical rustup toolchain root; project paths
cannot supply an executable. A host-owned explicit custom registration is
separate from a project toolchain file.

A real warmed Python counterexample initially produced a false error when
both caller and imported return type changed in one batch: the import still
had old open-buffer contents. The corrected regression performs two clean
batches on the same client, verifies both buffers were already open, then
changes both files and obtains current clean results. Its independent
rerun also passes both clean batches in 2.27 seconds. The original false
error is retained in `harness-python-warm-batch.json`; the correction is
`harness-python-warm-batch-corrected.json`.

Earlier native failures remain visible. Child native MCP hooks were not
connected and required the command fallback. Unicode hook input was
initially decoded as OEM866 and now has a real UTF-8 correction check.
An implicit-any config fixture retained a legitimate suggestion after
correction, so it was replaced with a strict-null fixture that actually
clears. The TOML runner originally failed an overly strict multiline final
assertion; its retained native events pass the separate evidence validator,
without claiming that original wrapper passed. C++ repeated-text freshness
and CSS validator failure corrections are detailed in the language matrix.

Additional roots use the launcher's validated `--add-dir` JSON environment
contract, retained by Pre, plus actual native shell `workdir` arguments.
The native test exercises that explicit root contract; launcher tests
separately check argument parsing. An unprofiled consumer bypassing both
the launcher and that environment only has its observed cwd/workdir roots;
unobserved extra roots have no independent baseline and are not claimed
covered. This is not an inferred undocumented hook payload field.

## Reproduction

The following checks reuse the selected installations and do not install
packages. Model-backed probes explicitly require `-RunAgent`; all their
source edits target newly created disposable directories.

```powershell
& $ExistingSerenaPython -B tests/lsp-adapter.py --real
pwsh -NoLogo -NoProfile -File tests/lsp-native.Tests.ps1 -RunAgent -Scenario shell -KeepProbe
pwsh -NoLogo -NoProfile -File tests/lsp-native.Tests.ps1 -RunAgent -Scenario child -UseGlobalHome -ChildReadMcps -KeepProbe
pwsh -NoLogo -NoProfile -File tests/lsp-native.Tests.ps1 -RunAgent -Scenario delayed -KeepProbe
pwsh -NoLogo -NoProfile -File tests/lsp-native.Tests.ps1 -RunAgent -Scenario failed -KeepProbe
```

The [native hook contract evidence](code-tools-native.md) records installed
version verification, trust and response-envelope behavior. Current official
[hook documentation](https://learn.chatgpt.com/docs/hooks) and
[subagent configuration](https://learn.chatgpt.com/docs/agent-configuration/subagents)
were checked against the observed installed behavior. Backend-specific
contracts and source limitations are linked from the language matrix and
independent review.
