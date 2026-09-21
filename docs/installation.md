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

Skill-library evolution is under `autonomous-skill-evolution`. Use
`codex-harness skills isolate`, `publish`, `usage` and
`identity --path`. `identity --codex-home` and `usage --codex-home` honor
`[[skills.config]] enabled = false`. Ordinary hooks stay off. On CLI 0.155.0 a
skill added mid-turn is missing from the first same-turn continuation, with or
without compact, and present on the next user turn, after idle `/compact`, on
resume and in a new child without fork. Catalogue listing is not activation.
Disablement and delete can remain listed in that session; observe them with
identity or usage. After publish, read the live `SKILL.md` or identity before
the next use; already loaded tokens are not refunded. Same-turn compact
catalogue injection can omit a late add; recovery is that live read, not
SessionStart/PostToolUse/Stop hooks. Limits and open scenarios live in
[skill evolution](memory/skill-evolution.md).

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
`--core-only`, `--code-tools-only`, `--subscriptions-only`,
`--token-workflow-only` and `--board-only`.

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

Everyday delivery and update are one command. It builds an immutable
candidate from the checkout, installs or updates the core connection with
user-default homes, verifies the installed launcher and prints one receipt;
`--all` chains the scoped component updates and `--reset` is the explicit
repair when `install`/`update` refuse because recorded link ownership no
longer matches (reset removes recorded link objects only, writes a receipt
and never touches targets or regular files). The `--all` component steps run
through the manager this delivery just connected, so their behavior always
matches the delivered checkout instead of the older process that started the
delivery:

```powershell
codex-harness deploy --source "<absolute-kit-checkout>" [--all] [--reset]
```

The explicit candidate flow below remains for scoped, pinned or
no-Cargo situations; `install`, `update` and `check` never compile.

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
& <build>\codex-harness.exe install --board-only --source . --codex-home "$env:USERPROFILE\.codex" --user-home "$env:USERPROFILE"
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
| `global/board.json` and the `board-workflow` skill | `<CODEX_HOME>/harness/bin/bd.exe` (board component) and `<user profile>/.agents/skills/board-workflow/` after core skill linking |
| `global/code-tools.json` and the adopted packages | native MCP registrations in local `<CODEX_HOME>/config.toml` |

The launcher resolves the original CLI through its recorded registration,
distinguishes its own executable, and refuses self-recursion. An in-place
Codex CLI update that changes the registered executable or package digest
still starts that current CLI; the launcher does not treat digest drift as
incompatibility and does not warn when harness enhancements still apply. If
shared defaults are unavailable it still starts the original CLI with the
native arguments and a degraded notice. Core recovery preserves the earlier
owned selection and unrelated configuration.

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
Launch admission follows recorded binary integrity only: while the delivered
build's binaries and metadata verify, ordinary launches keep using it even
after the checkout moves ahead or becomes unavailable, and nothing recompiles,
downloads or rewrites configuration at consumer startup. Check, diagnose and
deploy receipts still state the source-stale or source-unavailable
relationship with its action, and an explicit `build` plus `update` (or
`deploy`) remains the only mechanism that moves new processes to a newer
build; running sessions keep their immutable build until they finish. A
missing, altered or metadata-incompatible build still refuses the affected
runtime with its corrective action. After adding or removing a top-level skill
directory, rerun `update` to reconcile its registration; repeated installation
is idempotent.

Core Install/Update deliver the freshest integrity-verified build of the owned
state when `--build` is omitted; an explicit `--build` still selects exactly
that directory. Delivery adds the new immutable build and re-points the
`<CODEX_HOME>/harness/bin` links, so a manager that already running sessions
hold is never replaced or rewritten. Those sessions keep their build until
they finish, and the next Codex CLI session resolves the delivered one. The
managed MCP registrations name the same stable manager link, so a scoped
`update --code-tools-only` run from an older manager cannot pin it for new
sessions.

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
[Rust migration](evidence/rust-migration.md). Two-consumer quantitative
comparison is not required for this delivery; benefit remains unproven and no
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
