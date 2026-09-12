# Direct global connection

[Documentation map](README.md) · [Source inventory](../global/kit.psd1) ·
[Decisions](project-decisions.md#local-only-runtime-state)

Current selection, 2026-09-08: ordinary diagnostic and Stop hooks remain disabled.
The accepted RTK exception enables native hooks and Code Mode in base settings;
ordinary sessions inherit that selection. Diagnostic definitions remain empty and
cached diagnostic handlers return before analysis. The separate `harness-lsp`
registration and managed diagnostic backend provisioning are retired; explicit
Serena Python discovery is retained and shared packages are preserved. See
[code tools](code-tools.md) and the
[project decisions](project-decisions.md#mcp-language-tools-and-resources).
The authorized RTK exception is globally implemented; it does not revive
diagnostic or Stop hooks. Its component lifecycle is available through
`-TokenWorkflowOnly`; full installation includes it after core and existing
integrations. See [token workflow](token-workflow.md) for the pinned native
dependency, trust, suspension, recovery and task-effort selector.

The checkout is the source of the portable kit. `install.ps1` links instructions,
skills and agents into native Codex locations. The launcher reads shared settings
live through the Rust configuration bridge; it creates no deployed configuration copy.
If harness preparation fails, ordinary `codex` falls back to the original CLI
with the supplied native arguments and local settings. Missing or stale shared
builds do not trigger compilation or block that fallback. A short stderr notice
identifies unavailable shared defaults; Check continues to report harness health.
The installed command bootstrap has a local owned copy so an unavailable
checkout cannot remove the command entry itself.

The [MCP extension](code-tools.md) is globally connected. The default installer
includes its explicit dependency lifecycle and MCP registrations.
`-CoreOnly` selects the previously accepted instructions/profile/skills/agents
connection for an isolated core installation or its regression checks.
On an existing installation, core Install/Update also retains recorded hook
connections, repairs missing managed links and follows a relocated checkout.
It does not provision code tools or change their MCP registrations. A fresh
core-only installation still has no hooks.
`-CodeToolsOnly` reconciles existing MCP connections and resource policy
without provisioning or updating dependencies, core links or subscription routing.

Without an accepted RTK selection, core installation persists
`features.hooks = false` in the native base configuration using the installed
CLI's editor, with a private backup and concurrent-change check. This explicit
selection survives failed connection activation, Recover, Disconnect and archive;
backups are not automatically restored to enabled hooks. Shared settings are
read live and other reusable artifacts remain source links. Code-tools lifecycle output
is concise; add `-Detailed` to an explicit Check for full discovery, dependency
and health records. When RTK is selected, core updates preserve its linked
definition and the current native feature state, including explicit manual
suspension. Ordinary launch inherits the base hook selection so it cannot accidentally
re-enable a suspended exception.

The default installer also includes [subscription model routing](subscription-models.md)
through pinned OpenCodex. `-SubscriptionsOnly` adds, checks or disconnects that
component on an existing kit installation. It requires a native MSI/portable
PowerShell 7.4+ host for Task Scheduler, Node/npm for dependency provisioning,
and separate provider authorization. Microsoft Store PowerShell remains usable
interactively but is not selected for the subscription background task.

## Shared defaults and local TUI writes

Shared defaults stay live in this checkout. Native TUI model and trust writers
target the machine-local `config.toml`. The launcher
reads the live shared file and injects portable defaults with native `-c`.
Explicit `--profile` / `-p`, remote, help and other non-session dispatch keep
their previous bypass. A local base model/reasoning/UI value wins when
set; otherwise the shared default applies. Authentication and MCP base
configuration stay in the local Codex home. No copied or rendered deployed
source body is introduced.

Core Install/Update builds or reuses a verified immutable Rust manager, records
its location, and retires the old shared-profile link through the usual link
journal. This now requires Cargo/Rust during explicit core setup. Ordinary launch
does not build. Source code changes require an explicit core Update; shared
settings and instructions remain live.

Legacy machine settings are merged into the local base before link retirement.
Existing local values win conflicts; a conflict count is reported and both
original documents are preserved under `CODEX_HOME/harness/private-profile-migration`.
This local preparation survives a later failed activation, Recover or Disconnect.
Inspect these local copies to resolve differing preferences or restore the base;
do not restore a writable link into public source. The former managed `harness`
file profile is retired; explicit profiles must be user-owned local files.

Historical profiled TUI `/model` and project-trust confirmation wrote through
the linked shared file. Unprofiled app-server and `mcp add/remove` writers
target the local base. Those observed writer identities remain useful; the
accepted public-pack decision is local persistence, not shared-file accumulation
of machine paths.

## Prerequisites

- Windows native and PowerShell **7.4 or later** (`pwsh`). Windows PowerShell 5.1
  is not the supported interpreter.
- Installed Codex CLI exposing file profiles. The compatibility baseline is
  **0.153.4**; later local instruction loading also used **0.154.0**. The installer
  checks version/help and performs an actual startup probe before committing. A
  higher version number alone is not proof that its behavior is compatible.
- OpenSpec CLI **1.12.0 or later** for the included OpenSpec skills. The observed
  npm distribution is `@fission-ai/openspec`. Its Node runtime is an external
  prerequisite of that distribution.
- Permission to create symbolic links: enable Windows Developer Mode or run in
  a context that already has the required link privilege. The installer does
  not elevate itself or substitute copies if link creation fails.
- A checkout at a stable, accessible location. Connection tools resolve the
  current account and checkout paths. Sign in to Codex separately on each PC.

Check `pwsh --version`, `codex --version` and `openspec --version` before use.
Codex and OpenSpec remain bootstrap prerequisites. Selected MCP dependencies
are handled separately by the explicit [code-tool lifecycle](code-tools.md).
Install/Update reuse existing compatible UV, managed Python and package
environments. If they are absent, explicit installation provisions UV from its
official checksum-verified Windows distribution, then managed Python and the
required tool environments. Preview and Check do not install packages. Runtime
MCP startup does not download them.

Language analysis also needs the project's ordinary compiler/SDK inputs: the
selected Rust toolchain, the applicable .NET SDK for C#, and a separately
supplied licensed Delphi SDK where Delphi is in use. Missing prerequisites
remain explicit. Graphify needs a selected saved graph; repository operations
additionally use Git and GitHub CLI. Nuphus browser operations reuse installed
Edge or Chrome. The [dependency catalogue](../global/code-tools.json) records
the per-tool inputs.

## Install and verify

From the checkout in PowerShell:

```powershell
.\install.ps1 -WhatIf
.\install.ps1
.\install.ps1 -Mode Check
```

The script resolves its own source directory, independently of the caller's
working directory. `-WhatIf` inspects sources, prerequisites and conflicts
without writing files or persistent environment values. Actual link creation
happens during installation; a privilege failure rolls back the changes.

Installation records the original CLI path, connects the sources and prepends
`<CODEX_HOME>/harness/bin` to the user's PATH. It verifies command precedence in
the effective machine-plus-user PATH and launches a fresh PowerShell process to
check the ordinary `codex` entry point, complete global instructions and Full
Access. A conflicting earlier machine-level command is reported before changes.
Existing shell aliases/functions can override PATH; resolve such a customization
if it deliberately intercepts `codex`.

Open a new PowerShell terminal after installation so it receives the persistent
PATH change. `codex` can then be started from another repository normally.
Installation's neutral startup check is distinct from the broader consumer
discovery checks below.

The standard Codex configuration root is `~/.codex`; an existing `CODEX_HOME` is
respected. The optional `-CodexHome`, `-UserHome`, `-CodexCommand` and
`-PathScope Process` parameters support explicit destinations and disposable
verification. A launcher invocation must use the same effective `CODEX_HOME` as
the installation. Changing `-UserHome` does not change the Windows Known Folder
that native Codex uses for personal skill discovery.

`-DependencyUserHome` explicitly selects the owner whose shared MCP installations
are discovered, maintained and recovered. It defaults to `-UserHome`. Independent
connection homes can select an existing dependency owner without rebinding that
account's personal skills or changing its PATH:

    .\install.ps1 -UserHome <owned-bindings> -CodexHome <owned-codex-home> -DependencyUserHome <existing-user-home> -PathScope Process

Use the same explicit owner for subsequent Check, Install, Update, Recover and
Disconnect. It is retained in installation metadata and pending transactions;
an owner mismatch preserves the recorded installations and reports a conflict.
This chooses dependency locations, not another account's identity or credentials.

## What is connected

| Repository source | User connection |
| --- | --- |
| `global/principles-of-work.md`, the full global AGENTS content | `<CODEX_HOME>/AGENTS.md` |
| `global/harness.config.toml` | live shared defaults; native TUI writes target the local base `config.toml` |
| Each `.agents/skills/<name>/` and its resources | `<user profile>/.agents/skills/<name>/` |
| `global/agents/` and its resources | `<CODEX_HOME>/agents/codex-harness/` |
| `tools/codex.ps1` | Explicitly installed content-addressed local bootstrap, linked from `<CODEX_HOME>/harness/bin/codex.ps1` |
| `global/hooks.json` | `<CODEX_HOME>/hooks.json` |
| `tools/hook.ps1` | `<CODEX_HOME>/harness/bin/hook.ps1` |
| `tools/mcp.ps1` and the source connection policy | MCP path registrations and the owned startup-readiness scalar in local `<CODEX_HOME>/config.toml` |

The launcher imports its policy module through the registered repository source
when available. Its small bootstrap is staged under
`<CODEX_HOME>/harness/launchers/<content-hash>/codex.ps1` during explicit core
Install/Update, then selected through the existing recoverable link journal.
This executable bootstrap is the deliberate exception to live source links;
it is not a copied configuration or an automatic fallback for missing link
privileges. If the source or harness metadata is unavailable, it uses the
recorded original CLI where usable or the ordinary PATH alternative, excluding
itself. If the original Codex installation is also missing, it reports that
specific missing dependency. Run `install.ps1 -Mode Update -CoreOnly` from the
available checkout to restore shared behavior. Core recovery preserves earlier
owned selection and unrelated configuration.

The
repository-root `AGENTS.md` remains maintenance guidance for this project; the
portable global instruction text has one authoritative home under `global/`.

The initial skills include the six OpenSpec workflow skills plus later globally
linked skills such as project-memory, isolated-worktree, structured-codex-run,
project-verification and reproduce-regression. Fixed Astra presets have been
retired in favor of direct model/effort selection; the compatible agent source
directory and its link remain without agent TOMLs. Subscription-owned Grok
preset retirement remains part of the orchestration lifecycle work. See
[agent selection](agent-delegation.md) for migration and current visibility limits.
User-owned agents and skill directories are preserved.

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

Full installation also registers MCP references to the current source launcher
and the native root scalar `mcp_optional_startup_grace_ms = 0`.
The source-owned connection function maintains these small host connection
records; it does not deploy a configuration body. The scalar makes the initial
tool catalog wait for each optional server's finite startup timeout, instead of
the default shared one-second grace period. It applies to all optional MCPs in
that consumer, including pre-existing ones; an unavailable optional server still
permits Codex to start. This follows the
[native configuration contract](https://learn.chatgpt.com/docs/config-file/config-reference).

A pre-existing explicit value other than integer `0` produces an actionable
conflict before activation. Existing `0` is retained on Disconnect; a scalar
introduced by the kit is removed. Legacy connections acquire this ownership
record on Install/Update and Check reports degraded until migration. Native TUI
formatting changes are accepted when the recorded values still match. A later
user edit is preserved as a conflict. Pending recovery restores the exact prior
config and ownership metadata; it never guesses ownership after concurrent edits.

## Consumer discovery

Isolated and global consumer checks on CLI 0.153.4 established:

- `--profile harness debug prompt-input` reads the shared defaults, reports
  Full Access, and still includes a marker from the local base file.
- A separate Git project can load its own `AGENTS.md`, trusted project config
  and the global principles together.
- `mcp add` / `mcp remove` and unprofiled app-server config writers change the
  local base; they do not rewrite the shared source.
- Skills are discovered through directory links with canonical source paths and
  without duplicate names for the same checkout source.
- New CLI processes see live edits of connected profile, principles, skill and
  agent sources without reinstall.
- Explicit `-c sandbox_mode="read-only"` overrides the shared profile.
- Replacing `HOME` / `USERPROFILE` in an isolated process does not make Codex
  read `<temp-user>/.agents/skills`. Isolation tests therefore use
  `<CODEX_HOME>/skills`. Actual Known Folder skill discovery is checked after
  real installation, not by that HOME substitution.
- Test auth/history fixtures remain unchanged when probes use an explicit
  auth-source file and isolated homes.

Executable checks live in `tests/consumer.Tests.ps1`, `tests/installer.Tests.ps1`,
`tests/launcher.Tests.ps1`, `tests/tui.Tests.ps1` and
`tests/global-activation.Tests.ps1`. Ordinary consumer runs make no model
requests. `-RunAgent` requires an explicit auth file. Lifecycle tests copy
sources only to simulate an independent checkout; the installer still creates
direct links. A ConPTY attempt to host Codex through the execution tool failed
before start with `CreateProcessW` OS `-1073283067`; writer checks therefore used
real app-server methods generated from the installed CLI schema. That is a
runtime-writer check, not a TUI click check.

## Update and move

For an already installed kit, apply code-tool connection/resource changes with
the scoped coordinator:

```powershell
pwsh -NoProfile -File ./install.ps1 -CodeToolsOnly -Mode Install -WhatIf
pwsh -NoProfile -File ./install.ps1 -CodeToolsOnly -Mode Install
pwsh -NoProfile -File ./install.ps1 -CodeToolsOnly -Mode Check
```

This scope adopts existing discovered packages and uses the same ownership and
transaction checks for registrations, registries and resource settings. It does
not run dependency bootstrap or upgrades. Missing packages remain explicit;
provision them through the full installation lifecycle. `-CodeToolsOnly -Mode Update`
is rejected. Component selectors are mutually exclusive. Keep the same explicit
home and dependency-owner parameters on subsequent scoped operations.

The [resource contract](code-tools.md#resource-limits-and-reuse) bounds CBM
indexing and shares compatible Serena workers. Restart existing native Codex
sessions after migration: they retain previously loaded MCP processes and
catalogues.

Edits to already connected files are read by a new Codex process without another
install. Running sessions do not automatically reload their initial instruction
context. Files added inside a connected skill or agent directory are also live.

After adding/removing a top-level skill directory, rerun `install.ps1` to
reconcile its individual registration. Repeated installation is idempotent. Only
obsolete connections still owned by this kit are removed. Unrelated user
capabilities and conflicting destinations are preserved.

After moving the checkout, run `install.ps1` from its new location. Broken source
references are diagnosed by Check. If Codex itself moved or was reinstalled at a
different path, pass `-CodexCommand` with its real `.ps1` or `.exe` entry point;
never point it back to the harness launcher. For legacy installer suites that
edit native features, pass the original command recorded in installation
metadata rather than the harness wrapper.

## Disconnect and recovery

To disconnect or recover only code tools on an existing kit:

```powershell
pwsh -NoProfile -File ./install.ps1 -CodeToolsOnly -Mode Disconnect
pwsh -NoProfile -File ./install.ps1 -CodeToolsOnly -Mode Recover
```

Scoped Disconnect drains owned shared services, removes still-owned registrations
and releases this `CODEX_HOME` from the account resource receipt. Other
installation owners keep the policy active. The last owner restores original
native settings only where they still match the applied values; later user edits
are preserved. Core links, subscription state and unrelated pending journals
remain intact.

Scoped Recover rolls back an unfinished scoped activation, or finishes journal
cleanup after a durable commit. Preserve pending files; do not delete journals to
unblock a service. A pending full-kit activation must be recovered without
`-CodeToolsOnly`. Shared service admission remains blocked by an unfinished
activation until recovery succeeds.

For the full kit:

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

Stop or reconnect the subscription proxy only from an independent terminal after
sessions that use it have finished. See [subscription models](subscription-models.md).

## CLI scope and native contracts

The kit also supplies the source-linked
[project-verification](../.agents/skills/project-verification/SKILL.md) and
[reproduce-regression](../.agents/skills/reproduce-regression/SKILL.md) skills.
Normal Install/Check/reconnect discovers their directories and nested resources;
references and scripts remain in the authoritative checkout. The optional Windows
process helper depends on the kit's `tools/opencodex-process.ps1` and `.cs`, so
copying an individual script is not a supported deployment. Run verification from
the consumer's actual project and keep command records in its documentation home.

Model evaluation is opt-in. Installation and ordinary documentation checks do
not invoke it. Skill registration alone proves no speed gain.

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

For diagnostics of setting provenance and conflicts from any project, use
`codex-harness-check.ps1 -Json`, equivalent to `install.ps1 -Mode Check -Diagnose`.
The command is part of the same direct-link lifecycle. Report format, native
observation limits and restoration order are in
[source diagnostics](source-diagnostics.md).

Windows sandbox must not launch packaged PowerShell from WindowsApps. Child PATH
entries matching `(?i)\\WindowsApps(?:\\|$)` are filtered so native Codex
selects an already installed desktop PowerShell. Parent and global PATH stay
unchanged.

## Python analysis while developing the pack

The project [pyrightconfig.json](../pyrightconfig.json) describes import roots
of standalone scripts and the local `.venv`. On Windows `.venv` may be a link
to an already connected Serena environment: the Python path is stored in
`serena.paths.python` of `$CODEX_HOME/harness/code-tools.json`.
For Python in `Scripts/python.exe`, the link target is the parent of `Scripts`.
Create the link only when `.venv` is absent; do not replace an existing
environment. The link is excluded from Git and does not copy a machine path into
shared configuration.

CLI check of selected files: `basedpyright --project pyrightconfig.json
--pythonpath <full-path-to-python.exe> <files>`. Use the same installed
BasedPyright recorded in the LSP inventory. Diagnostic rules in the project
config are not disabled. These settings belong to pack development; other
projects continue to choose their own environments. Relative-path contract:
[BasedPyright configuration](https://docs.basedpyright.com/latest/configuration/config-files/).
