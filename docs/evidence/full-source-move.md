# Full source relocation acceptance

Observed on Windows x64, 2026-09-06, native Codex 0.153.4 and PowerShell 7.6.5.
[tests/full-source-move.Tests.ps1](../../tests/full-source-move.Tests.ps1)
passed **40 assertions** through the real full installer and native MCP consumers.
It made no model requests and selected reuse, so it acquired no packages.

The test copied the reusable source files into an owned source directory, used
an isolated connection home and CODEX_HOME, and explicitly selected the existing
account's shared packages through DependencyUserHome. PathScope Process confined
PATH changes to the test process. It then performed:

1. Full Install, reusing the existing compatible packages and creating 12 direct
   source links. All five registrations named the copied source launcher.
2. Actual Check: Serena, Codebase Memory, Graphify, Nuphus and harness-lsp completed
   their MCP handshake and tools/list checks. A new native consumer activated an
   outside TypeScript fixture, introduced error2322 through Serena, observed it,
   corrected the file and observed clean diagnostics.
3. Shutdown, move of the owned source directory to another path with spaces and
   Cyrillic characters, and full Install to reconnect every source link and MCP
   launcher path. Another new consumer repeated the five-server check and the
   TypeScript error/clearance sequence.
4. Full Disconnect, restoring the test process PATH and removing the owned
   registration, hook link and installation state while preserving the fixture's
   foreign configuration, data and pre-existing skill.

The real user's global config, installation/registration/inventory files, Serena
project configuration, personal skill links, persistent user PATH and inventoried
existing entrypoint files retained their captured bytes or link targets. No
shared package was installed or updated. The source copies were test inputs;
deployment used direct links and native path references throughout.

Command:

    pwsh -NoLogo -NoProfile -File tests/full-source-move.Tests.ps1

Retained machine-local evidence:

    TEMP/harness-full-move-8d92132fe7634304a422dd17eafb695f/report.json
    TEMP/harness-full-move-8d92132fe7634304a422dd17eafb695f/install-first.json
    TEMP/harness-full-move-8d92132fe7634304a422dd17eafb695f/first-check.json
    TEMP/harness-full-move-8d92132fe7634304a422dd17eafb695f/moved-check.json

This is a real full connection lifecycle with an explicitly shared dependency
owner, not a new Windows account or a clean physical PC. Native Codex still uses
the actual account's Windows Known Folder for personal skill discovery. The
separate [bootstrap evidence](code-tools-activation.md) covers actual missing UV,
managed Python, Serena and Graphify provisioning. This relocation test calls
diagnostics explicitly; automatic model-visible feedback is demonstrated by the
separate [native MCP edit acceptance](lsp-native-mcp.md).

The 40-assertion run preceded the later native startup-readiness scalar change.
Its migration, rollback and native formatting behavior have separate coverage in
the registration and combined activation tests.
