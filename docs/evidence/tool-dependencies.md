# Shared dependency lifecycle evidence

Observed on Windows x64, 2026-09-06. This report covers the dependency portions
of tasks 2.3–2.5, the relevant language provisioning/update requirements, and
7.1/7.3. Global registration, combined rollback and missing UV/Python bootstrap
have their separate [activation report](code-tools-activation.md).

The reusable sources are [the catalogue](../../global/code-tools.json),
[discovery](../../tools/code-tools/discovery.py),
[dependency lifecycle](../../tools/code-tools/dependencies.py),
[missing LSP providers](../../tools/code-tools/lsp_provision.py) and
[missing native MCP providers](../../tools/code-tools/mcp_provision.py).
Runtime launchers read the generated registry; they do not import provisioning
code or run package managers.

## Preview, ownership and recovery

Plan reads official npm, PyPI, GitHub, NuGet and Rust channel metadata, rejecting
prereleases and retaining an explicit metadata failure instead of claiming the
installed release is current. It does not install missing conditional languages,
downgrade newer installations, or update the project compiler as a side effect.
Discovery distinguishes graphifyy from the unrelated npm Graphify command,
checks installed identity/provenance and missing executable paths, and preserves
local modifications. Read-only process inspection matches absolute installation
boundaries and exposes PID/executable without command-line secrets. It also
works for an alternate fixture UserHome on the same Windows account.

Replacement uses an exclusive shared-installation lock, a fresh consumer
snapshot, staged compatibility checks and exact prior-tree identity. A durable
journal precedes the first directory move. Recovery verifies recorded source
hashes and ownership; unknown edits or live/unresolved consumers remain pending.
The exact old directory and manager metadata are retained. Graphify's protected
manifest participates in the same transaction through ReplaceFileW, preserving
its Windows DACL. New directories and individual OCR artifacts have inverse
journals before activation. No unrelated consumer is force-stopped.

Malformed owned journals are reported individually as pending without preventing
recovery of valid peers. Missing prior hashes cannot be treated as proof of an
already restored absent installation. New file activation refuses concurrent
creation, and direct symbolic-link transaction paths are rejected.

An additive .NET runtime install records its installer hash and prior SDK/runtime
lists before invoking the installer. A later rollback without an implemented
manager-aware inverse reports that effect as pending; it never claims removal
or deletes a project SDK. Prior standalone provisions do not belong to later
combined activation transactions.

## Actual updates and preserved selections

| Dependency | Observed result | Compatibility and preservation evidence |
|---|---|---|
| Codebase Memory | Shared npm 0.10.5 → 0.10.8 | Official npm/native integrity, original packaged-file audit, staged and post-promotion actual index/query; old package retained; existing public launcher responds |
| Graphify | Shared UV graphifyy 0.9.44 → 0.9.55 | Actual stdio and authenticated HTTP graph query; 88,513 nodes and graph bytes preserved; corrected exact Python launcher identity and protected manifest; existing UV/OpenCode wrapper preserved |
| TypeScript/JavaScript | Shared TLS 5.1.3 → 6.0.0 | Original package audit; staged/post-update TS and JS error→clean/symbols; complete TypeScript 5.9.3 compiler tree unchanged; old prefix and lock retained |
| PSES | 4.4.0 retained; 4.7.0 staged and compatible | Official 4.7 bundle passed real error→clean/symbols with bundled PSScriptAnalyzer. Active shared consumers prevented promotion; no forced termination. Update remains pending consumers |
| Rust Analyzer | Existing rustup 1.97.1 retained | Current stable metadata identifies compiler cohort 1.98.1; rust-analyzer-preview itself uses placeholder 0.0.0. No automatic compiler/toolchain update. Existing cohort passes actual language checks |
| Nuphus | npm 0.2.2 original native companion selected | Five original companion files match the official tarball. Local modified wrapper/extra patched binary remain untouched. Source proxy applies the audited schema fix to the original binary; actual desktop/browser/OCR checks pass |

The resulting state does not claim all available updates were applied. PSES is
pending quiescence, Rust is held by the project-toolchain policy, and the local
Nuphus modifications are preserved. These are named, evidenced holds rather
than silently discarded releases. An unimplemented future backend-specific
update or unresolved identity remains pending; this report is not evidence that
every future upstream release is compatible.

## Actual missing dependency provisions

The current host originally lacked the selected basedpyright, shared VS Code
JSON/HTML/CSS server package, its Markdown parser, native Taplo, NeoCMake,
ShellCheck/Bash, Roslyn runtime package, native LemMinX and minimal FPC prerequisite.
Their explicit installation paths ran against real distributions and existing
shared runtimes, followed by the applicable actual language checks in the
[language report](lsp-languages.md). Markdown reuses the same VS Code package;
only markdown-it was added to its shared prefix while preserving existing lock
records. .NET 10.0.11 was added alongside runtime 8.0.30 in the existing dotnet
installation; project SDK 8.0.424 and its runtime remained unchanged.

FPC 3.2.2 was extracted from the official installer and source archive without
running the installer, registering an IDE, changing PATH or replacing the Delphi
SDK. Only the compiler driver/backend, units and compiler/RTL/package sources
needed by pasls were activated. Actual `-n -iV/-iTO/-iTP` queries confirmed
3.2.2/win32/i386. SDK discovery reuses installed registry entries and project
unit/include settings; absence of the licensed Delphi SDK remains explicit.

The rejected npm Taplo build reported that LSP was not included. The attempted
UV cmake-language-server had incompatible pygls dependencies and, after a
compatible constraint, still lacked required diagnostics. Native alternatives
were selected and proved. These failed alternatives are not recorded as working
support.

The bounded cleanup audit for task 7.5 examined the two task-created rejected
alternatives: Marksman 2026-02-08 under the shared Serena `Marksman` cache and
UV cmake-language-server 0.1.11 under the existing UV tools root. Both executable
hashes match their `codex-harness-dependencies` provisioning markers. Neither
is selected in the generated global registry, and the host process snapshot
found no consumers. A targeted search of source-kit configuration, active
OpenCode configuration, Codex configuration and Serena configuration found no
configured callers.

Both are preserved because these early attempts preceded durable dependency
transactions: neither has a pre-effect create-directory journal or recorded
whole-tree prior identity. The old Marksman install result also has no journal;
the CMake receipt includes the later pygls 1.3.1 constraint and a public UV
entrypoint outside the environment. Standard Recover therefore cannot prove
restoration to prior absence. No historical ownership or inverse was fabricated,
and no unrelated source-kit installation was changed. Current tree hashes,
exact paths, marker checks, caller-search scope and process results are retained
in `%TEMP%/harness-rejected-alternative-cleanup.json`. This records the reason
for preservation; it does not claim that those two filesets were removed or
that every possible dynamic consumer was enumerated.

## Fresh fixture and failure evidence

- Codebase Memory and Nuphus were actually absent in separate alternate user
  roots. Their native providers installed official distributions, passed real
  protocol/operation checks, reused the exact installation tree on repetition,
  and standard dependency Recover restored prior absence. The retained report
  is `%TEMP%/harness-fresh-mcp-recovery.json`.
- A fresh OCR fixture explicitly acquired the official detector, recognizer and
  dictionary. Detector/recognizer SHA256 values matched bytes already exercised
  by offline OCR; they are labelled measured compatibility fingerprints, not
  checksums published by upstream. Actual `desktop_perceive` returned four OCR
  elements from an owned test-page PNG with downloads disabled and all three
  cache hashes unchanged. Standard Recover restored absence for all three
  files. Reports: `%TEMP%/harness-nuphus-models-fresh.json`,
  `harness-nuphus-models-fresh-ocr.json`, and
  `harness-nuphus-models-fresh-recovered.json`.
- Codebase probes set the upstream-supported CBM_RUNTIME_DIR to a new private
  nonce path, isolating Windows rendezvous as well as cache/project data.
  Two sequential actual six-operation probes used distinct daemon PIDs and
  verified cooperative shutdown. A forced initialization timeout also verified
  absence of its private daemon afterward. No host TEMP ACL or admission guard
  was weakened. Reports: `%TEMP%/harness-codebase-isolation.json` and
  `harness-codebase-failed-probe-cleanup.json`.
- [Dependency tests](../../tests/test_dependencies.py): 31 passing tests cover
  metadata errors, release selection, locks, integrity, archive traversal,
  local-change preservation, interrupted promotion, protected auxiliary files,
  conflicting file creation, OCR mismatch/reuse/recovery, corrupt journal peers
  and absent prior identity.
- [Discovery tests](../../tests/code_tools_discovery_test.py): 16 passing tests,
  including real host CIM observation for an alternate UserHome and FPC target
  metadata distinct from the host architecture.
- [Missing LSP provider tests](../../tests/test_lsp_provision.py): six passing
  tests cover ownership, preserved project inputs and Rust component inverse
  guards. These manager/inverse tests use substitutes and do not establish an
  actual missing Rust component installation.

The source of the three OCR URLs is Nuphus's official
[model contract](https://github.com/mrpulor-gh/nuphus-mcp/blob/master/crates/nuphus-mcp/src/models.rs).
The dictionary fallback is the same official PaddleOCR upstream GitHub source.
Cached existing models are reused without network access; a changed download
cannot replace the selected compatibility artifact automatically.

## Boundary of clean-machine evidence

There was no second physical Windows machine or clean VM. The real missing LSP
installations ran on this host with its existing Node, PowerShell, Serena,
rustup and licensed Delphi SDK. The new missing Rust/TLS/PSES/pasls/clangd
provider code is not collectively proved by a clean end-to-end Windows install:
PSES staging and TLS shared update have actual compatibility proof, while the
missing Rust component add/remove path currently has substitute-based checks.
The fresh native MCP/OCR and UV/Python/Serena/Graphify bootstrap checks are real
isolated acquisitions with standard recovery, as recorded above and in the
activation report. They do not by themselves close every scenario of task 7.3.
