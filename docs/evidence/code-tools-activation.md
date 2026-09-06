# Combined activation and bootstrap evidence

Observed on Windows x64, 2026-09-06. Native Codex 0.153.4 and PowerShell 7.6.5.
This report covers the installer transaction and missing UV/Python packages.
It does not replace the four-MCP operation and language acceptance reports.

## Implemented ownership and recovery

[install.ps1](../../install.ps1) holds the connection and dependency owners' mutexes across the
[combined coordinator](../../tools/activation.psm1). Core links/state/PATH,
the native-owned MCP references/readiness scalar and their exact prior state bytes, and both
code-tools.json/lsp-servers.json remain recoverable until one durable commit.
The outer activation-pending.json is written before effects. Component pending
records are removed only after its committed marker has been flushed.

Recover rolls back uncommitted work and completes cleanup of committed work.
It verifies current bytes against recorded before/after identities before
restoring files. Core links retain their original source-target ownership
checks. Concurrent config, registration metadata, installation metadata or
registry edits are preserved with incompleteUpdate and specific errors.
Independent components can recover while a conflicting component remains pending.
CoreOnly cannot bypass a pending combined operation.

Exact-block/prefix registration Disconnect, all pending Recover, and core/registry recovery
run in PowerShell without Python. If native TUI normalization changed the block,
Disconnect uses an existing uv-managed stdlib Python for semantic ownership proof;
it does not require the adopted Serena environment. With every interpreter gone
and a normalized block, it preserves settings with an explicit unresolved result.
Check without the adopted Serena environment
reports degraded, callable=false. Dependency-directory rollback uses the separate
stdlib Python recovery API with the exact coordinator transaction ID, preserving
active consumers and unknown edits. An existing uv-managed base interpreter is
a valid lifecycle interpreter; it is not advertised as a ready MCP environment.

The coordinator does not claim that additive package effects disappeared when
their result has no inverse journal. Those remain incomplete. Dependency-owned
directory and file additions use their own durable inverse journals; a system
.NET runtime addition remains explicitly pending when a manager-aware inverse
is unavailable. Prior standalone installations are not rolled back by a later
unrelated coordinator transaction.

## Checks

- tests/installer.Tests.ps1: 21 scenarios, 125 assertions. Real isolated
  install.ps1/core Install/Check/Disconnect and native neutral startup, plus
  ownership, collision, PATH, rollback and concurrency checks.
- tests/activation.Tests.ps1: 19 scenarios, 152 assertions. Real direct links,
  PATH and installed native Codex TOML editor; only package acquisition/discovery
  and neutral core startup are substituted. Failure after core, registration,
  each registry and before commit restores exact prior bytes/absence. Update and
  Disconnect rollback, independent recovery with concurrent edits, metadata
  conflicts, no-Python Disconnect/Recover and unrecorded additive effects are
  exercised. Separate PowerShell children exit with code86 at registration,
  the first registry and after durable commit; a new invocation recovers correctly.
  The actual Graphify promotion result and durable directory journal also pass
  through a later coordinator failure: the prior package is restored and a
  repeated Recover has no false pending update.
  An explicit DependencyUserHome is retained in both committed and pending owner
  metadata; mismatched Check/Recover preserves the recorded state. Legacy MCP
  readiness migration stays degraded even with a protocol-ready health fixture,
  and Disconnect restores the previous scalar absence without Python.
- tests/code_tools_registration_test.py: 17 tests with the real native editor.
  Deferred registration, absent config restoration, exact noncanonical prior
  metadata and intervening state edits are covered.
Native startup readiness covers absent/previous-zero restoration, nonzero and
  boolean/float conflicts, legacy migration, pending rollback, later user edits,
  quoted key formatting, nested same-named keys and multiline foreign text.
  Exact PowerShell disconnection verifies the scalar boundary using its byte
  length and SHA256; ownership metadata does not copy preceding user settings.
- Native TUI normalization is accepted only when every present owned MCP table
  matches its recorded parameters. BEGIN/END markers are not deletion boundaries:
  the TUI can place unrelated project sections inside them. Structural TOML
  statement spans preserve those sections, comments, quoted keys, BOMs and
  header-like text inside multiline strings. Tests exercise the actual native
  MCP editor rewrite; read-only registration Check also passed against the real
  TUI-normalized user config with zero reconciliation operations.
- The earlier actual ./install.ps1 -WhatIf in the existing user home passed and reported
  Preview Install, two pending direct-link operations and unchanged PATH.
  This read-only check did not globally activate the MCP/LSP change.
- After native readiness was added, a read-only registration Check of the actual
  TUI-normalized global config reports exactly one operation: register
  mcp_optional_startup_grace_ms=0. Its config SHA256 remains unchanged. Global
  activation and CLI readiness acceptance are reported separately.
- [Full source relocation](full-source-move.md): 40 actual assertions with the
  full installer, shared installed dependencies, five real MCPs, outside-project
  TypeScript diagnostics, a moved source directory, a new consumer and Disconnect.

Commands use the recorded original Codex command from
CODEX_HOME/harness/installation.json, never the already connected harness launcher.
The final 19/152 activation run after readiness metadata used
TEMP/harness-activation-299d9ad9e8a44d2d813e46fb8cb0b2f4.

## Explicit missing-dependency bootstrap

Install/Update reuse existing UV, managed Python, Serena and Graphify environments.
If UV is absent, the installer downloads the official stable Windows standalone
archive, verifies its published SHA256, and writes only previously absent uv.exe
and uvx.exe in the shared user .local/bin directory. No shell profile, global
Python selection or Windows registry is changed. The official release redirect
and checksum sidecar are used when GitHub's anonymous REST quota is exhausted.

Missing managed Python is installed through UV with an explicit shared
UV_PYTHON_INSTALL_DIR, --no-bin and --no-registry. The compatible package
requirements are Serena1.7.0 and graphifyy[mcp]0.9.55. Existing environments are
reused; Graphify project data is not copied or invented. Reusable source wrappers
remain in the checkout. Only package managers run during explicit Install/Update.

Each new UV environment, wrapper file and base runtime has an ownership record.
Rollback requires unchanged source/data and no active consumers, then uses native
UV uninstall for the exact owned package/runtime. Derived .pyc/.pyo caches are
excluded from source identity: UV's cold wheel build can create those caches after
base Python installation. Source changes continue to block removal.
An interruption before the post-install ownership fingerprint remains pending;
recovery does not guess ownership of a partially installed shared package.

The opt-in command:

    pwsh -NoLogo -NoProfile -File tests/bootstrap.Tests.ps1 -RunProvisioning -IncludeGraphify

passed 17 assertions with PATH restricted to PowerShell and System32 and a
disposable UserHome. It exercised missing-dependency preview with no writes,
honestly degraded Check, actual download/install, MCP module imports, exact reuse,
source-change protection, derived-bytecode tolerance and complete package rollback.
The official distributions were UV0.12.10, CPython3.13.15, Serena1.7.0 and
Graphifyy0.9.55. The UV x64 archive SHA256 was
f65744f94072152b1f86ba2aace4d01f1124d9a8ecb235805039e3718c36cac2.

The successful fixture's download/evidence root is
TEMP/harness-bootstrap-504d364eee7a4810a70f9d823000da3e.
Its Serena, Graphify, Python, UV executables and pending journals were removed by
the tested recovery. Existing user installations were not replaced.

Earlier exploratory fixtures remain under TEMP. The old fingerprint experiment
harness-bootstrap-ad5fdcb3490e489a8fe71ab2113bc9dc removed its Serena environment
but preserved base Python/UV after an identity mismatch. Newly generated .pyc
files were examined and removed within that exact fixture; the legacy full
identity still differed, so no package deletion was forced. No probe process
remains for it. The current implementation and final test above use the corrected
source/data identity. The quota-failure fixture
harness-bootstrap-64a8a1a54dad4ce98d772dbf4fb076eb never installed a package.
No automatic-approval denial from the earlier Serena cleanup was retried.

Official contract sources:
[UV installation](https://docs.astral.sh/uv/getting-started/installation/),
[installer options](https://docs.astral.sh/uv/reference/installer/),
[managed Python](https://docs.astral.sh/uv/guides/install-python/),
[environment variables](https://docs.astral.sh/uv/configuration/environment/),
[UV0.12.10 release](https://github.com/astral-sh/uv/releases/tag/0.12.10).
Installed UV help was checked for python find/install/uninstall and tool install.
