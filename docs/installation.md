# Direct global connection

[Documentation map](README.md) · [Source inventory](../global/kit.psd1)

The checkout is the source of the portable kit. `install.ps1` connects its files
and directories to native Codex locations using symbolic links. It creates no
copied or merged deployment configuration.

The [MCP/LSP extension](code-tools.md) is globally connected and verified under
`connect-global-mcp-lsp`; its [acceptance report](code-tools-verification.md)
records actual consumers, language coverage and environment limits. The default
installer includes its explicit dependency lifecycle and MCP registrations.
`-CoreOnly` selects the previously accepted instructions/profile/skills/agents
connection for an isolated core installation or its regression checks.

Native profiled TUI settings writes modify the linked repository source.
The user explicitly accepted machine-specific values in this shared config,
including trusted project paths. `/model` and trust persistence were both
exercised directly. See the [acceptance report](linked-kit-verification.md).

## Prerequisites

- Windows native and PowerShell **7.4 or later** (`pwsh`). Windows PowerShell 5.1
  is not the supported interpreter.
- Installed Codex CLI exposing file profiles. The compatibility baseline is
  **0.153.4**; the installer checks version/help and performs an actual startup
  probe before committing. A higher version number alone is not proof that its
  behavior is compatible.
- OpenSpec CLI **1.12.0 or later** for the six included OpenSpec skills. The
  observed npm distribution is `@fission-ai/openspec`. Its Node runtime is an
  external prerequisite of that distribution.
- Permission to create symbolic links: enable Windows Developer Mode or run in
  a context that already has the required link privilege. The installer does
  not elevate itself or substitute copies if link creation fails.
- A checkout at a stable, accessible location. Connection tools resolve the
  current account and checkout paths; saved configuration can include machine
  paths. Sign in to Codex separately on each PC.

Check `pwsh --version`, `codex --version` and `openspec --version` before use.
Codex and OpenSpec remain bootstrap prerequisites. Selected MCP/LSP dependencies
are handled separately by the explicit [code-tool lifecycle](code-tools.md).
Install/Update reuse existing compatible UV, managed Python and package
environments. If they are absent, explicit installation provisions UV from its
official checksum-verified Windows distribution, then managed Python and the
required tool environments. Preview and Check do not install packages. Runtime
MCP/LSP startup does not download them. See the [bootstrap evidence](evidence/code-tools-activation.md).

Language analysis also needs the project's ordinary compiler/SDK inputs: the
selected Rust toolchain, the applicable .NET SDK for C#, and a separately supplied
licensed Delphi SDK for Delphi projects. Missing prerequisites remain explicit.
Graphify needs a selected saved graph; repository operations additionally use
Git and GitHub CLI. Nuphus browser operations reuse installed Edge or Chrome.
The [dependency catalogue](../global/code-tools.json) records the per-tool inputs.

## Install and verify

From the checkout in PowerShell:

```powershell
.\install.ps1 -WhatIf
.\install.ps1
.\install.ps1 -Mode Check
```

The script resolves its own source directory, independently of the caller's
working directory. `-WhatIf` inspects sources, prerequisites and conflicts without
writing files or persistent environment values. Actual link creation happens
during installation; a privilege failure rolls back the changes.

Installation records the original CLI path, connects the sources and prepends
`<CODEX_HOME>/harness/bin` to the user's PATH. It verifies command precedence in
the effective machine-plus-user PATH and launches a fresh PowerShell process to
check the ordinary `codex` entry point, complete global instructions and Full
Access. A conflicting earlier machine-level command is reported before changes.
Existing shell aliases/functions can override PATH; resolve such a customization
if it deliberately intercepts `codex`.

Open a new PowerShell terminal after installation so it receives the persistent
PATH change. `codex` can then be started from another repository normally.
Installation's neutral startup check is distinct from the broader development
integration tests for skill/agent discovery, coexistence and recovery.

The standard Codex configuration root is `~/.codex`; an existing `CODEX_HOME` is
respected. The optional `-CodexHome`, `-UserHome`, `-CodexCommand` and
`-PathScope Process` parameters support explicit destinations and disposable
verification. A launcher invocation must use the same effective `CODEX_HOME` as
the installation. Changing `-UserHome` does not change the Windows Known Folder
that native Codex uses for personal skill discovery.

`-DependencyUserHome` explicitly selects the owner whose shared MCP/LSP
installations are discovered, maintained and recovered. It defaults to
`-UserHome`. Independent connection homes can select an existing dependency
owner without rebinding that account's personal skills or changing its PATH:

    .\install.ps1 -UserHome C:\owned-test\bindings -CodexHome C:\owned-test\codex -DependencyUserHome C:\Users\existing-user -PathScope Process

Use the same explicit owner for subsequent Check, Install, Update, Recover and
Disconnect. It is retained in installation metadata and pending transactions;
an owner mismatch preserves the recorded installations and reports a conflict.
This chooses dependency locations, not another account's identity or credentials.
The installer serializes both the connection owner and the dependency owner.

## What is connected

| Repository source | User connection |
| --- | --- |
| `global/principles-of-work.md`, the full global AGENTS content | `<CODEX_HOME>/AGENTS.md` |
| `global/harness.config.toml` | `<CODEX_HOME>/harness.config.toml` |
| Each `.agents/skills/<name>/` and its resources | `<user profile>/.agents/skills/<name>/` |
| `global/agents/` and its resources | `<CODEX_HOME>/agents/codex-harness/` |
| `tools/codex.ps1` | `<CODEX_HOME>/harness/bin/codex.ps1` |
| `global/hooks.json` | `<CODEX_HOME>/hooks.json` |
| `tools/hook.ps1` | `<CODEX_HOME>/harness/bin/hook.ps1` |
| `tools/mcp.ps1` and the source connection policy | Five MCP path registrations and the owned startup-readiness scalar in local `<CODEX_HOME>/config.toml` |

The launcher imports its module from its resolved repository source. The
repository-root `AGENTS.md` remains maintenance guidance for this project; the
portable global instruction text has one authoritative home under `global/`.

The initial skills are `openspec-apply-change`, `openspec-archive-change`,
`openspec-explore`, `openspec-propose`, `openspec-sync-specs` and
`openspec-update-change`. The agent source folder currently contains no production
role definitions. Add self-contained agent TOML files there when a role is
needed; a supported namespaced directory link lets Codex discover them live.

Shared keys are declared in `global/kit.psd1`. The initial profile preserves the
existing model/reasoning preference (`gpt-6-astra`, `xhigh`), Full Access
(`approval_policy = "never"`, `sandbox_mode = "danger-full-access"`), the user
approval reviewer and reusable notice/pet preferences. This is a preserved
configuration choice; model access still depends on the account and CLI.

The base `<CODEX_HOME>/config.toml` stays local. Native profiles layer shared
defaults above it, with project settings and explicit CLI overrides taking
priority. Authentication, history, caches and installer state stay on the host.
Installation metadata stores paths/ownership and versions, not copies of source
bodies. Installation preserves the existing base file without migrating its
contents into the profile.

Full installation also registers five MCP references to the current source
launcher and the native root scalar `mcp_optional_startup_grace_ms = 0`.
The source-owned [connection function](../tools/code-tools/registration.py)
maintains these small host connection records; it does not deploy a configuration
body. The scalar makes the initial tool catalog wait for each optional server's
finite startup timeout, instead of the default shared one-second grace period.
It applies to all optional MCPs in that consumer, including pre-existing ones;
an unavailable optional server still permits Codex to start. This follows the
[native configuration contract](https://learn.chatgpt.com/docs/config-file/config-reference).

A pre-existing explicit value other than integer `0` produces an actionable
conflict before activation. Existing `0` is retained on Disconnect; a scalar
introduced by the kit is removed. Legacy connections acquire this ownership
record on Install/Update and Check reports degraded until migration. Native TUI
formatting changes are accepted when the recorded values still match. A later
user edit is preserved as a conflict. Pending recovery restores the exact prior
config and ownership metadata; it never guesses ownership after concurrent edits.

Native persistence follows the selected consumer. In the tested profiled TUI,
`/model` and project-trust confirmation write into `harness.config.toml`, through
the link to `global/harness.config.toml` in the checkout. The shared file may
therefore accumulate model preferences and paths from different PCs. Native
`mcp add/remove` management commands and unprofiled app-server config writers
target the local base instead. The launcher preserves this behavior; it does
not redirect writes or create a synchronized configuration copy.

## Update and move

Edits to already connected files are read by a new Codex process without another
install. Running sessions do not automatically reload their initial instruction
context. Files added inside a connected skill or agent directory are also live.

After adding/removing a top-level skill directory, rerun `install.ps1` to reconcile
its individual registration. Repeated installation is idempotent. Only obsolete
connections still owned by this kit are removed. Unrelated user capabilities
and conflicting destinations are preserved.

After moving the checkout, run `install.ps1` from its new location. Broken source
references are diagnosed by Check. If Codex itself moved or was reinstalled at a
different path, pass `-CodexCommand` with its real `.ps1` or `.exe` entry point;
never point it back to the harness launcher.

## Disconnect and recovery

```powershell
.\install.ps1 -Mode Disconnect -WhatIf
.\install.ps1 -Mode Disconnect
```

Disconnect removes the kit's still-owned additions and its owned PATH entry. It
preserves repository files, local Codex state and connections that already
existed before installation, including an adopted global principles link.
Externally replaced destinations are reported and left intact.

A local operation lock prevents concurrent installation for the same user. The
pending journal covers the activation transaction and its final startup check.
A normal failure attempts rollback and retains the original cause. If the
process was interrupted or recovery could not finish, preserve the metadata and
run:

```powershell
.\install.ps1 -Mode Recover -WhatIf
.\install.ps1 -Mode Recover
```

Use the same explicit home parameters, if any, as for installation. Recovery
validates recorded ownership and refuses to overwrite unrelated changes. It
unlinks directory references without recursively traversing repository sources.

## CLI scope and native contracts

Automatic profile selection covers local TUI, exec, review, resume, fork and
`debug prompt-input`. An explicit profile wins. Management/help/version commands
keep their native invocation; remote sessions and app-server are not given an
unsupported local profile flag. The original CLI executable remains available
at its recorded installation path.

- [File profiles and config locations](https://learn.chatgpt.com/docs/config-file/config-advanced#profiles)
- [Configuration precedence](https://learn.chatgpt.com/docs/config-file/config-basic#configuration-precedence)
- [Global instruction discovery](https://learn.chatgpt.com/docs/agent-configuration/agents-md)
- [Skill discovery and symbolic directory links](https://learn.chatgpt.com/docs/build-skills#where-codex-loads-local-skills)
- [Custom agents](https://learn.chatgpt.com/docs/agent-configuration/subagents#custom-agents)

On the tested Windows CLI 0.153.4, individual agent TOML symlinks were skipped by
discovery; a namespaced directory link was consumed successfully by an actual
custom agent. The kit uses that verified connection. Current native config
writers were checked separately from the launcher and their evidence is scoped
to the exercised operations. Unprofiled app-server writers do not establish the
write target of a profiled TUI; its accepted shared-file persistence is described
above.
