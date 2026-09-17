# Direct global connection

[Documentation map](README.md) · [Source inventory](../global/kit.json) ·
[Decisions](project-decisions.md#local-only-runtime-state)

Current selection: ordinary diagnostic and Stop hooks remain disabled. The
accepted RTK exception enables native hooks and Code Mode in base settings;
ordinary sessions inherit that selection. The separate `harness-lsp`
registration and managed diagnostic backend provisioning are retired; the
adopted Serena package is started through the native boundary. See
[code tools](code-tools.md) and the
[project decisions](project-decisions.md#mcp-language-tools-and-resources).

The checkout is the source of the portable kit. The native manager links
instructions, skills and agents into native Codex locations and installs
immutable Rust builds for the launcher, manager, diagnostic alias and helpers.
The launcher reads shared settings live; it creates no deployed configuration
copy. If harness preparation fails, ordinary `codex` falls back to the original
CLI with the supplied native arguments and local settings, prints a short
degraded notice and never compiles or downloads anything. Check reports the
degraded state instead of treating it as a healthy kit consumer.

The [MCP extension](code-tools.md) is globally connected by the code-tools
component. On an existing installation, core Install/Update also retains
recorded hook connections, repairs missing managed links and follows a
relocated checkout. The component selectors are mutually exclusive:
`--core-only`, `--code-tools-only`, `--subscriptions-only` and
`--token-workflow-only`.

## Prerequisites

- Windows x64 with a Rust MSVC toolchain, Cargo, the MSVC C++ linker and the
  Windows SDK. Workspace dependencies are locked in `Cargo.lock`.
- Installed Codex CLI. The compatibility baseline is **0.153.4**; **0.154.0** is
  the current verified upstream. Installation resolves the real executable and
  performs a model-free startup handoff before committing.
- OpenSpec CLI **1.12.0 or later** for the included OpenSpec skills.
- Permission to create symbolic links: enable Windows Developer Mode or use a
  context that already has the required link privilege. The manager does not
  elevate itself or substitute copies when link creation fails.
- A checkout at a stable, accessible location.

Codex and OpenSpec remain bootstrap prerequisites. Selected MCP dependencies are
handled by the explicit [code-tool lifecycle](code-tools.md); ordinary startup
and Check never provision or download packages.

## Install and verify

Build an immutable candidate from the checkout, then run the candidate manager:

```powershell
cargo run --release --locked -p codex-harness -- build --source . --state "$env:LOCALAPPDATA\codex-harness-native"
# The command prints the immutable build directory, e.g. state\builds\<identity>.
& <build>\codex-harness.exe install --core-only --source . --build <build> --codex-home "$env:USERPROFILE\.codex" --user-home "$env:USERPROFILE" --preview
& <build>\codex-harness.exe install --core-only --source . --build <build> --codex-home "$env:USERPROFILE\.codex" --user-home "$env:USERPROFILE"
& <build>\codex-harness.exe check --core-only --codex-home "$env:USERPROFILE\.codex" --user-home "$env:USERPROFILE"
```

`--preview` (the native equivalent of `-WhatIf`) inspects sources, prerequisites
and conflicts without writing files or persistent environment values. Actual
link creation happens during installation; a privilege failure rolls the
activation back through the journal. Update, Recover and Disconnect follow the
same shape (`update`, `recover`, `disconnect`), and the complete old-to-new
command mapping lives in
[native Rust commands](rust-native.md#supported-command-mapping).

Installation records the resolved original CLI, connects the source links and
prepends `<CODEX_HOME>/harness/bin` to the configured PATH scope (User by
default). Explicit destinations are supported with `--codex-home`,
`--user-home`, `--dependency-user-home`, `--upstream` and
`--path-scope Process`; use the same values for later operations. An existing
installation keeps its recorded PATH scope. Open a new terminal after
installation so it receives a persistent PATH change, then start `codex` from
any repository.

`--dependency-user-home` explicitly selects the owner whose shared MCP
installations are discovered, maintained and recovered; it defaults to
`--user-home`. Independent connection homes can select an existing dependency
owner without rebinding that account's personal skills or PATH:

    & <build>\codex-harness.exe install --core-only --source . --build <build> --codex-home <owned-codex-home> --user-home <owned-bindings> --dependency-user-home <existing-user-home> --path-scope Process

The dependency owner is retained in installation metadata and pending
transactions; an owner mismatch preserves the recorded installation and reports
a conflict.

Install the remaining components with the same candidate:

```powershell
& <build>\codex-harness.exe install --code-tools-only --source . --codex-home "$env:USERPROFILE\.codex" --user-home "$env:USERPROFILE" --dependency-user-home "$env:USERPROFILE"
& <build>\codex-harness.exe install --token-workflow-only --source . --codex-home "$env:USERPROFILE\.codex" --user-home "$env:USERPROFILE"
& <build>\codex-harness.exe install --subscriptions-only --source . --codex-home "$env:USERPROFILE\.codex" --user-home "$env:USERPROFILE"
```

## What is connected

| Repository source | User connection |
| --- | --- |
| `global/principles-of-work.md`, the full global AGENTS content | `<CODEX_HOME>/AGENTS.md` |
| `global/harness.config.toml` | live shared defaults; native TUI writes target the local base `config.toml` |
| Each `.agents/skills/<name>/` and its resources | `<user profile>/.agents/skills/<name>/` |
| `global/agents/` and its resources | `<CODEX_HOME>/agents/codex-harness/` |
| Immutable build binaries | `<CODEX_HOME>/harness/bin` launcher, manager, inspect/observe helpers and the `codex-harness-check.exe` diagnostic alias |
| `global/rtk-hooks.json` | `<CODEX_HOME>/hooks.json` (token-workflow component) and `harness/bin/{rtk,harness-rtk}.exe` |
| `global/code-tools.json` and the adopted packages | native MCP registrations in local `<CODEX_HOME>/config.toml` |

The launcher resolves the original CLI through its recorded registration,
distinguishes its own executable, and refuses self-recursion or a wrong
upstream. If shared defaults are unavailable it still starts the original CLI
with the native arguments and a degraded notice. Core recovery preserves the
earlier owned selection and unrelated configuration.

The repository-root `AGENTS.md` remains maintenance guidance for this project;
the portable global instruction text has one authoritative home under `global/`.

Fixed Astra presets are retired in favor of direct model/effort selection; the
compatible agent source directory and its link remain without agent TOMLs.
User-owned agents and skill directories are preserved.

Shared keys are declared in `global/kit.json` and live in
`global/harness.config.toml`: the model/reasoning preference
(`gpt-6-astra`, `xhigh`), Full Access (`approval_policy = "never"`,
`sandbox_mode = "danger-full-access"`), the reviewer and reusable notice
preferences. Model access still depends on the account and CLI.

## Update and move

Apply connection or MCP changes with the scoped components:

```powershell
& <build>\codex-harness.exe update --code-tools-only --source . --codex-home "$env:USERPROFILE\.codex" --user-home "$env:USERPROFILE" --dependency-user-home "$env:USERPROFILE"
& <build>\codex-harness.exe check  --code-tools-only --source . --codex-home "$env:USERPROFILE\.codex" --user-home "$env:USERPROFILE" --dependency-user-home "$env:USERPROFILE"
```

Scoped operations adopt already discovered packages and reuse the same
ownership and transaction checks for registrations, registries and resource
settings; they never acquire packages. Missing packages remain explicit and are
provisioned through the explicit [dependency lifecycle](code-tools.md).

Source data is read live by a new Codex process without another install.
Executable changes require an explicit `build` plus `update`; a source-stale
build is reported by Check with its update action, and ordinary launch never
compiles. After adding or removing a top-level skill directory, rerun `update`
to reconcile its registration; repeated installation is idempotent.

After moving the checkout, run `update` from its new location; broken source
references are diagnosed by Check. If Codex itself moved, pass `--upstream` with
its real entry point - never the harness launcher.

## Disconnect and recovery

```powershell
& <build>\codex-harness.exe disconnect --code-tools-only --source . --codex-home "$env:USERPROFILE\.codex" --user-home "$env:USERPROFILE"
& <build>\codex-harness.exe recover    --code-tools-only --source . --codex-home "$env:USERPROFILE\.codex" --user-home "$env:USERPROFILE"
& <build>\codex-harness.exe disconnect --core-only --codex-home "$env:USERPROFILE\.codex" --user-home "$env:USERPROFILE"
& <build>\codex-harness.exe recover    --core-only --codex-home "$env:USERPROFILE\.codex" --user-home "$env:USERPROFILE"
```

Scoped Disconnect drains owned shared services, removes still-owned
registrations and releases this `CODEX_HOME` from the account resource receipt;
other owners keep the policy active. Externally replaced destinations are
reported and left intact, repository files are preserved, and connections that
already existed before installation stay untouched.

A local operation lock serializes installation per user. Recover rolls back an
unfinished activation or finishes journal cleanup after a durable commit;
preserve pending journals and never delete them to unblock a service. Use the
same explicit home parameters as for installation.

Stop or reconnect the subscription proxy only from an independent terminal
after the sessions that use it have finished. See
[subscription models](subscription-models.md).

## CLI scope and native contracts

The kit also supplies the source-linked
[project-verification](../.agents/skills/project-verification/SKILL.md) and
[reproduce-regression](../.agents/skills/reproduce-regression/SKILL.md) skills,
whose native helpers are `harness-observe.exe` and `harness-inspect.exe`.
References and resources remain in the authoritative checkout; copying an
individual executable is not a supported deployment. Run verification from the
consumer's actual project and keep command records in its documentation home.

Model evaluation is opt-in. Installation and ordinary documentation checks do
not invoke it. Controlled native baseline/candidate pairs are described in
[Rust migration](evidence/rust-migration.md); benefit remains unproven and no
model or quota speedup is claimed.

- [Configuration precedence](https://learn.chatgpt.com/docs/config-file/config-basic#configuration-precedence)
- [Global instruction discovery](https://learn.chatgpt.com/docs/agent-configuration/agents-md)
- [Skill discovery and symbolic directory links](https://learn.chatgpt.com/docs/build-skills#where-codex-loads-local-skills)
- [Custom agents](https://learn.chatgpt.com/docs/agent-configuration/subagents#custom-agents)

For diagnostics of setting provenance and conflicts from any project, use the
native `codex-harness-check.exe` alias (`check --diagnose`). Report format,
native observation limits and restoration order are in
[source diagnostics](source-diagnostics.md).

Windows sandbox must not launch packaged PowerShell from WindowsApps. Child PATH
entries matching `(?i)\\WindowsApps(?:\\|$)` are filtered so native Codex
selects an already installed desktop PowerShell. Parent and global PATH stay
unchanged.
