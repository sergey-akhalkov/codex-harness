# Experimental context delivery

2026-09-08, CLI 0.153.4. The accepted [Astra runtime pilot](native-context-contracts.md)
established experimental sampling and preservation of an early constraint after
automatic compaction. The setting is now enabled in the source-linked
`global/harness.config.toml`, using the existing file-profile lifecycle. This
does not enable Fast, native memories or background model calls.

Delivery evidence is under
`%LOCALAPPDATA%/codex-harness-evidence/context-install-1c61e8a4a2004307ac6eb70101e16e3b/`.
`native-profile-parse.json` records native parsing of the directly linked source
and explicit override: true/false respectively, both retaining Astra xhigh.
The native binary SHA-256 is
`444A3F0008050605CAE73CD9B7A2DCAC61294062DFAAB56DD20430FD6498518B`.
Ordinary `-p harness debug prompt-input` and the explicit false override both
started successfully from outside the checkout without a model request. Their
prompt output does not expose the experimental flag and is not runtime proof.

The attempted `app-server -p harness` probe was rejected by this CLI: file
profiles apply to runtime commands, not App Server. A fresh owned App Server
home with its base configuration linked to the profile supplied the separate
native parse/override evidence. It does not pretend to be the ordinary launch.

`tests/installer.Tests.ps1 -CodexCommand <recorded original Codex command>` passed
24 lifecycle scenarios and 356 assertions. The first invocation without that
argument selected the harness wrapper and failed inside the isolated feature
editor; its fixtures remain at
`%TEMP%/codex-installer проба f56cc480ebb84fc3be6516a8b351649f`.
Use the original command from the installation receipt for this legacy suite.

`install.ps1 -Mode Update -CoreOnly -CodexCommand <original>` completed globally;
an absolute `install.ps1 -Mode Check -CoreOnly` from `%TEMP%` returned Connected
with 19 links, 12 skills and 3 agent definitions. Base configuration, code-tools
and token-workflow registry hashes remained unchanged. The core installation
receipt was updated by its lifecycle. No subscription service was restarted.

For a new ordinary task, `codex -c
features.context_management.experimental_mode=false` is the explicit reversible
override. Removing the added table from the owned shared profile rolls back the
default. This delivery leaves the remaining task-2.2 runtime rollback acceptance
open: parsing and model-free startup do not prove the ordinary sampling path.
Other Git-memory/worktree/structured-run acceptance remains in the active spec.
