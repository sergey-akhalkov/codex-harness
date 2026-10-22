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
degraded state instead of treating it as a healthy kit consumer. If the shared
CPU cap cannot be verified, the requested payload still starts once and the
warning says the cap is not guaranteed. See
[Shared CPU policy](#shared-cpu-policy).

The [MCP extension](code-tools.md) is globally connected by the code-tools
component. On an existing installation, core Install/Update also retains
recorded hook connections, repairs missing managed links and follows a
relocated checkout. The component selectors are mutually exclusive:
`--core-only`, `--code-tools-only`, `--subscriptions-only`,
`--token-workflow-only` and `--board-only`.

## Prerequisites

- Windows x64 with PowerShell 7, a Rust MSVC toolchain (Rust 1.98 or later),
  Cargo, the MSVC C++ linker and the
  Windows SDK. Workspace dependencies are locked in `Cargo.lock`.
- Installed Codex CLI. The compatibility baseline is **0.153.4**; the global
  installation lifecycle also passed with **0.162.0**. Installation resolves the real executable and
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

### First installation on a new machine

Complete prerequisites before starting the immutable build. Reuse existing
tools and preserve the user's CLI, authentication and local settings. Run the
commands below in the owner's **PowerShell 7**, not Windows PowerShell 5.1.
An execution tool's requested shell is not proof of its actual runtime:
check `$PSVersionTable.PSVersion`; if a host still launches 5.1, invoke the
existing `pwsh.exe -NoProfile -Command ...` explicitly. Do not install a second
PowerShell distribution to work around that host.

1. Check `Get-Command pwsh, cargo, rustup, node, npm, openspec, codex` and their
   versions. Open a new terminal after installing prerequisites. A running
   terminal retains its old PATH even after a successful installer.
2. On a machine without a compiler, install Microsoft's Visual Studio Build
   Tools with the **Desktop development with C++** workload and Windows SDK,
   then install Rust's MSVC toolchain. A GUI bootstrapper returning immediately
   does not mean installation finished: wait for its setup process and check
   the final result. `vswhere -latest -products * -requires
   Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath`
   must resolve an installation; the selected SDK must contain x64
   `kernel32.lib` and `ucrt.lib`. See the official
   [Rust installation guide](https://doc.rust-lang.org/book/ch01-01-installation.html)
   and [PowerShell installation guide](https://learn.microsoft.com/en-us/powershell/scripting/install/install-powershell-on-windows).
   When acquiring installers or archives, verify the vendor signature or
   published digest before execution. Keep bootstrap downloads outside the kit.
3. Install OpenSpec with `npm install --global @fission-ai/openspec` if absent.
   For code tools, prepare uv, Python, Serena, basedpyright and rust-analyzer
   before `deploy --all`. The native component adopts these shared packages;
   registering an MCP is not a replacement for installing its runtime.

The following dependency set passed global delivery and real Rust/Python
Serena semantic checks on 2026-10-09: PowerShell 7.6.6, Rust 1.99.0, Node.js
24.21.0 LTS, OpenSpec 1.14.1, uv 0.12.24, Python 3.13.16, Serena 1.7.0 and
basedpyright 1.40.2. This is observed installation evidence, not a promise
about every language or future upstream release. With Node.js and uv already
installed, the explicit preparation commands are:

```powershell
uv python install 3.13
uv python update-shell
uv tool install 'serena-agent==1.7.0'
rustup component add rust-analyzer rust-src
npm install --global --ignore-scripts 'basedpyright@1.40.2'
npm install --global --ignore-scripts '@nuphus/nuphus-mcp@0.2.2'
```

Reuse compatible existing packages instead of reinstalling them. Nuphus
**0.2.2** is the adapter-audited version: a newer npm `latest` may be refused
by `dependencies apply` with a compatibility-audit error. Do not bypass that
audit. The pinned npm install skips package scripts and includes the native
platform package and companion DLL; the harness validates the original
binary when preparing its connection. `dependencies discover --source KIT
--user-home USER` is the model-free way to confirm package discovery.
`dependencies plan` adds release metadata; inspect per-item results rather
than assuming all dependencies are ready from a command's exit code alone.
See the [code-tool lifecycle](code-tools.md#explicit-lifecycle) for capability
acceptance and conditional language support.

The pack defaults to ordinary Codex / GPT-6 Astra at `xhigh` for the lead and
successor, and to the local `ds` profile (DeepSeek V4.1 Flash at `max`) for
executors. Before installation, inspect `global/orchestration.toml`: `default`
requires no separate profile, while any other role profile must already exist
locally. A missing successor profile can fail core Check even when ordinary
Codex and the executor profile work. Connecting the subscription component
does not authorize a provider or create a user's Z.AI profile. Choose billing
routes explicitly; see [subscription setup](subscription-models.md).

The shared defaults set `code_mode_host = true` under `[features]` and
`suppress_unstable_features_warning = true`, so a new deployment inherits the
Code Mode host and does not print the experimental-feature notice. An explicit
machine-local `[features]` value still wins. Code Mode remains an
under-development capability.

Codex does not know a custom provider model's limits unless the profile
declares a catalog. For `ds`, point `model_catalog_json` at a local copy or link
of the compact
[DeepSeek catalog](../global/codex-profiles/deepseek.models.json), for example
`CODEX_HOME/deepseek-models.json`, and keep `[features] code_mode = false`.
Use this compact entry: copying a bundled OpenAI model entry into the catalog
can make DeepSeek print tool calls as DSML text instead of invoking them.
Provider credentials and routing remain local. This removes the
fallback-metadata warning; verify the catalog with
`codex debug models -c model_catalog_json='<path>'`, then start a fresh
`codex exec --profile ds`. The long-thread notice has no disable setting; open a
new thread when it appears. Startup notices already shown in a thread remain
there; open a new Codex process after changing these settings.

The shipped orchestration profile also sets `self_reflection_enabled = true`.
Ordinary interactive `codex` automatically prepares its current session's
reflection transport and project lifecycle; no separate lead-start or
`delivery-improvement enable` command is required. Existing unmanaged sessions
take this update on their next ordinary launch. Finished sessions, historical
acceptances and their pending stages remain dormant. An omitted key is true
for compatibility. Set it to `false` to stop new
reflection stages while ordinary lead, executor and acceptance work continues;
retained results and pending work are preserved.
Core delivery installs the distinct `executor-self-reflection` and
`lead-self-reflection` skills and resolves their role instructions. See
[accepted-task self-reflection](rust-native.md#accepted-task-self-reflection)
for automatic activation, delivery and recovery.
Ordinary acceptance uses the installed kit configuration when its source is a
consumer project, with or without an explicit `--source`. A selected kit supplies
its own configuration; consumer pool provenance remains unchanged. Default-true bindings
omit the field from the legacy-compatible acceptance metadata; false is
explicit. An acceptance made while false still succeeds, records that
permission in its receipt, and remains ineligible for reflection if the value is
re-enabled before the next poll: the closure is consumed as an existing ignored
event, not a new refusal.

For a first installation, when `codex-harness` is not yet on PATH, bootstrap
the manager with Cargo's ordinary development profile and let **deploy**
publish the optimized immutable release build:

```powershell
$kit = (Resolve-Path .).Path
$state = Join-Path $env:LOCALAPPDATA 'codex-harness-native'
cargo run --locked --jobs 2 -p codex-harness -- deploy --source $kit --state $state --all
```

This avoids a separate optimized bootstrap followed by another optimized
publication build. The delivered binaries remain immutable release artifacts;
never install links to `target/debug` or `target/release`. Do not start a
second build/deploy against the same state while the first one owns it.
Build output may be quiet while Cargo optimizes: retain the process handle
and inspect `STATE/staging/<candidate>/cargo.log` when needed. Silence or a
poll timeout is not evidence that the operation stopped.

Complete the required [Ponytail setup](#ponytail-setup) below after core delivery.

### Handoff to an agent on another PC

The receiving agent runs [first installation](#first-installation-on-a-new-machine)
on the target PC with that owner's tools, paths, authentication and settings.
Preparing another Windows account, VM or Sandbox on the development machine is
not a prerequisite for this handoff. Do not copy the development account's
package caches, credentials or hook-trust records.

After installation, run the documented Check, open a fresh ordinary Codex
session outside the harness checkout, and verify discovery and actual use of
the installed capabilities. Repeat the installation to check that local settings
survive. Report the selected versions, command results and any remaining native
trust step. Use [targeted recovery](#completion-and-targeted-recovery) if delivery
is incomplete; keep target-machine evidence with its owner. A written handoff
does not claim that the other PC has already passed these checks.

### Ponytail setup

Ponytail is a required part of normal setup and is enabled by default. Install
the original plugin with Codex's native commands; do not copy its skills or
rules. The [marketplace declaration](../.agents/plugins/marketplace.json) selects
the inspected upstream revision. Its MIT license and attribution remain in the
package. Git and Node must be on PATH. The exercised baseline is Codex CLI
0.163.0 and Node 24.18.0; this does not establish minimum compatible versions.

First run `codex plugin list --json`. If Ponytail is already installed, keep that
native installation and inspect its selection instead of adding a duplicate.
Do not reset deliberate disablement. For a new installation, run these commands
after the core delivery above (`$kit` is the absolute checkout path):

```powershell
codex plugin marketplace add $kit --json
codex plugin add ponytail@coding-agents-harness --json
codex features enable hooks
```

Open ordinary Codex and use `/hooks` to review and trust the original Ponytail
hooks. Then start a fresh session outside the harness checkout. Native plugin
listing must show Ponytail installed and enabled, and the session must receive
its guidance and discover the supplied skills, including `ponytail-help`.
Package presence alone does not prove loading. `codex-harness check` verifies
the harness connection; use native plugin listing, `/hooks` and the fresh
session to verify Ponytail. Keep any native error visible and resolve its stated
cause before claiming setup is complete.

Repeat setup starts with native listing; do not blindly repeat `plugin add`,
which can reset the enabled choice. The plugin lives in the selected Codex home
and needs no project-local setup. Updates and removal use Codex's native plugin
commands and the [upstream guide](https://github.com/DietrichGebert/ponytail/blob/09c434ab73ca43761c5d8b0c5d341926c2fba710/INSTALL.md).

The original plugin resets mode on resume and can share mode state between
sessions. On the exercised Codex 0.163.0 host, different projects in the same
Codex home also used shared fallback state; independent modes are not promised.
Compaction retains the stored mode in the inspected upstream hooks. Temporary
mode changes differ from persistent plugin disablement. See the original
[mode-state implementation](https://github.com/DietrichGebert/ponytail/blob/09c434ab73ca43761c5d8b0c5d341926c2fba710/hooks/ponytail-runtime.js).

### Everyday delivery and update

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

`deploy --preview` without `--build` first selects an integrity-verified build
from the same checkout in the manager's owned state (or the explicit `--state`).
When one is available, planning does not invoke Cargo. The receipt identifies
the selected directory and digest; `build_selection = verified-existing` records
this reuse, and `build_check.status` reports whether the source is current or
`source-stale`. An older intact build can support planning; the preview does not
certify changed source as compiled, tested or installed. Explicit `--build`
selection remains authoritative. With no eligible build, including first use,
the ordinary preparation path and its errors still apply. An applying deploy
independently prepares and validates its candidate as before.

Preview preserves installation links, settings, build selection and reset
targets. `--all --preview` reports each component's plan through its existing
owner; missing prerequisites remain explicit failures and do not disappear
from the receipt. Avoiding preview compilation establishes this bounded
behavior, not an end-to-end time or token saving.

### Goal recovery policy

`install` and `update` with `--core-only` accept
`--goal-recovery enabled|disabled`. New installations use the accepted bounded
default (enabled); an update without the option retains the recorded choice, so
an explicit `disabled` survives later updates until it is explicitly changed.
The choice is recorded in `installation.json` and in the generated
`native-launch.json` registration. Registration schema 3 carries the policy;
an existing schema-2 registration is legacy and stays recovery-disabled until
an update writes schema 3.

`codex-harness check --core-only` reports `goalRecovery.configured` and
`goalRecovery.support` (`declared` for schema 3, `legacy` for schema 2) with
`exercised: false`. The read-only check establishes configuration and declared
support, not that an overload recovery ran. Disabling recovery affects new
sessions; an already-running session keeps its owner. Rollback restores the
previous registration and settings and never reopens a blocked goal.

### Fast local delivery

`deploy` without `--fast` is the release/publication route: a fresh target,
fat LTO and the complete delivery and runtime checks. Use it for final
delivery and for any candidate another person or machine will consume. For an
explicit local iteration candidate, add `--fast`:

```powershell
codex-harness deploy --source "<absolute-kit-checkout>" --fast [--all] [--preview]
```

`--fast` selects the maintained `delivery-fast` profile (opt-level 3 inherited
from release, `strip = true`, `lto = "off"` and `codegen-units = 16`). It does
not change ordinary `deploy`, and it never falls back to release: an
unavailable fast profile or candidate, or a `--build` for another profile,
fails before installation links change. The receipt's top-level `profile` must
be `delivery-fast` for this choice and `release` for ordinary deployment. Fast
and release candidates are not interchangeable, even when both exist for the
same source. Keep fast candidates on the local developer installation; release
candidates remain the publication output. Both profiles retain the same
immutable-build, ownership, integrity, installed-runtime and recovery checks;
the selector changes the compiler profile and fast target eligibility.

`--fast --preview` filters existing candidates by the fast profile and does not
compile when an eligible verified build is available. `--fast --all --preview`
reports that core plan and each component plan without changing installation
state.

Fast mode has one persistent compiler target per key under the owned short
`%TEMP%\chx` root. The key covers the rustc/cargo versions, target triple,
profile and build-affecting manifest, lockfile, toolchain, build-script and
Cargo host-configuration inputs; the target record binds it to the installation
state, stores the full source identity and last successful build, and is leased
while in use. An unchanged source identity may reuse the target. After source
changes, reuse requires every changed identity path to be a regular file whose
modification time is later than that last success; otherwise preparation uses
fresh scratch. That conservative fallback protects a restored or otherwise
untrustworthy timestamp instead of trusting Cargo's mtime comparison.

A target whose ownership, record or lease evidence is unavailable or
mismatched, or which is actively leased, is not reused or removed; preparation
uses fresh scratch. A foreign or ambiguous `%TEMP%\chx` root is preserved and
fast preparation reports the explicit ownership error instead of inheriting
it. An owned target for this installation with a free lease and no use for
seven days is retired during a later fast preparation.
Active entries and foreign or ambiguous entries are preserved. Ordinary
release deployment never acquires the fast target. The fast target is
additional regenerable disk state; no current size measurement is retained for
it. Lease and reclamation mechanics belong to the
[native build lifecycle](rust-native.md#fast-local-delivery-profile).

The root `[workspace.metadata.harness]` delivery list is the source-owned
declaration. When valid, both profiles compile its exact declared set
(currently seven executables) in one Cargo invocation. A missing or invalid
declaration retains the existing two-phase source-manager query; it is not
guessed, omitted or silently widened.

Retained observations from 2026-10-22, on one Windows x64 host with
rustc/cargo 1.98.1, `x86_64-pc-windows-msvc` and the shared heavy budget. The
current native command uses four Cargo jobs for both profiles, selected from
matched no-contention runs. These are single runs and foundation evidence, not
matched repeatability:

| Route | Retained observation |
| --- | --- |
| Release baseline at source `4bd9ea12` | 617.757813 s wall (phase 1 518 s, phase 2 96 s); outer-Job peak 3,256,029,184 bytes; seven artifacts 35,290,624 bytes total; manager 27,249,664 bytes |
| Release matched, one Cargo job | 531.344 s phase sum; 532.006 s heavy payload; 3.1047 GiB peak; seven artifacts 35,405,312 bytes |
| Release matched, two Cargo jobs | 442.122 s phase sum; 443.775 s heavy payload; 2.5392 GiB peak; seven artifacts 35,405,312 bytes |
| Release matched, four Cargo jobs | 408.376 s phase sum; 409.979 s heavy payload; 2.6239 GiB peak; seven artifacts 35,405,312 bytes |
| `delivery-fast` foundation at source `f96fe382`, two Cargo jobs | 2m34s cold; manager 29,714,944 bytes; opt-level 3, LTO off, 16 codegen units and stripped symbols |
| Fast matrix, cold, four Cargo jobs | 117.715 s wall; 117.500 s payload; 2.683 GiB peak; seven artifacts 40,254,464 bytes |
| Real fast deployment, four Cargo jobs | 119.339 s; seven declared binaries; candidate healthy |
| One-invocation cold fast, earlier one-job sample | 410.755 s |
| Two-phase fallback cold fast, one job | 675.446 s |
| Unchanged source with a verified candidate | 4.2 s; `build_selection=verified-existing`; no Cargo |
| Earlier fast-target reuse samples, pre-four-job | 124.943 s and 290.112 s |

The one-invocation and fallback runs built the same seven declared executables
and produced identical normalized dep-info sets. The current four-job samples
meet the 360 s cold-fast target; the earlier one-job sample did not. The
unchanged-source sample meets the five-second reuse target. No current
leaf-rebuild or single-binary/artifact-reuse measurement establishes the 120 s
leaf target or the 20-60 s stretch target. These samples cover build and reuse,
not a current startup or representative runtime comparison between fast and
release. Queue conditions, source, toolchain, target and machine changes
invalidate transfer; these numbers are not a universal or matched same-job
speedup claim.

Fast compiled-input reuse is implemented. Each immutable fast candidate records
the union of source-relative compiled inputs from all seven dep-info files plus
build-affecting manifest, lockfile, toolchain and Cargo host-configuration
identities. A fast candidate may be reused when every recorded compiled and
build-affecting input matches, even if only noncompiled test or documentation
files changed. A test-like path that is compiled invalidates; missing,
malformed, empty, ambiguous or extra-unreferenced evidence forces a rebuild.
Release candidates still require the full checkout identity. The reused
candidate is not rewritten: a content-addressed selection record references the
same verified binary hashes and the current checkout relationship.

The matched release comparison adopted four Cargo jobs: 22.937% lower heavy
payload than one job, with a 5.3761 GiB memory margin. The test-feedback
measurement separated 98.645 s of warm-target compilation from 1532.633 s of
execution across 1,716 selected cases; it retained 64 ignored conditions and one
deterministic pre-existing token-audit failure, and adopted no test runner or
serialization change.

For a test- or fixture-only change that does not exercise delivery, run the
focused [Cargo feedback route](rust-native.md#development-feedback-without-a-publication-build)
instead of deploying. Deployment is not a test runner and is needed only when
the task itself exercises compilation, installation, the receipt, the launcher
or installed runtime behavior. When delivery is needed, a fast deployment may
reuse the verified candidate if the changed file is outside dep-info; a release
deployment still requires exact source identity. A compiled fixture or test
path is a real input and invalidates the candidate.

Preview and applying delivery retain their source, build, profile and ownership
checks; applying additionally verifies the installed launcher. A preview may
select an older verified build and report it as source-stale; it does not
certify changed source as compiled, tested or accepted, and it grants no
authority to the later applying step.

Applying fast and release delivery with both profile candidates present,
interrupted delivery/CLI recovery, and delivery outside this checkout remain
unvalidated (OpenSpec tasks 8.3 and 8.4). The scoped build observations above
do not establish those installed-lifecycle scenarios.

For connected code tools, delivery requires real Serena semantic acceptance
before link changes and through the installed manager afterward. The receipt's
`verification.serena.status` must be `semantic-ready`; `not-connected` records
an installation without code tools. Failed verification exits nonzero. See the
[acceptance scope and standalone check](code-tools.md#explicit-lifecycle).

The explicit candidate flow below remains for scoped, pinned or
no-Cargo situations; `install`, `update` and `check` never compile.

Build an immutable candidate from the checkout, then run the candidate manager:

```powershell
cargo run --locked --jobs 2 -p codex-harness -- build --source . --state "$env:LOCALAPPDATA\codex-harness-native"
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

### Completion and targeted recovery

Read the final delivery receipt: require `status = deployed`, the intended
`profile`, the intended `build_identity`, every requested component's `ok = true`,
`verification.executor_probe_ok = true`, and, with code tools connected,
`verification.serena.status = semantic-ready`. A `partial` receipt means
installation has changed state but verification is unfinished. Preserve that
state and investigate the reported error; do not reset a valid installation
because an acceptance probe failed.

If Serena reports `overview missed the fixture symbol`, inspect the retained
evidence path and the worker log under its `codex-home/harness/serena-home`.
An empty symbol result is not success. In the observed first-start failure,
rust-analyzer did not signal readiness within 120 seconds; the same backend
subsequently analyzed the retained project and the unchanged semantic probe
passed. Check `rustup component list --installed` for `rust-src`, run
`rust-analyzer analysis-stats <evidence>/rust` to establish backend readiness,
then rerun the bounded semantic check after the prerequisite or readiness
condition changed:

```powershell
codex-harness mcp serena-check --source $kit --registry "$env:USERPROFILE/.codex/harness/code-tools.json"
codex-harness deploy --source $kit --all
```

Keep the failed evidence; repeated empty results require diagnosis, not a
retry loop or a weaker assertion. A verified unchanged build is reused by
deploy, so this recovery does not require another full compilation.

Finally open a new PowerShell 7 terminal **outside the kit checkout**:

```powershell
Get-Command codex, codex-harness
codex --version
codex mcp list --json
rtk --version
bd --version
codex-harness check --core-only --codex-home "$env:USERPROFILE/.codex" --user-home $env:USERPROFILE
```

The check's `orchestration.roles` lists each responsibility role resolved from
the recorded source root: `resolved` with its owned revision, or `gap` with
the exact missing input. A gap describes what that role's sessions would not
receive; it fails nothing else, and a managed dispatch or resume refuses
before any launcher or model request while those inputs are missing, keeping
its pool slot bound and recoverable.

Both Codex commands must resolve under `CODEX_HOME/harness/bin`, and the MCP
list must show enabled `serena` and `nuphus` when code tools were requested.
Restart Codex sessions that loaded the old MCP/configuration. Persistent User
PATH is not injected into existing terminal processes: if an automation host
must refresh its current process, preserve its host-specific tool paths rather
than overwriting PATH with only User/Machine values. An explicit absolute path
to the upstream Codex executable bypasses the harness launcher.

CPU-policy warnings are separate from launcher/configuration success. A host
Job hierarchy can refuse account-job placement while ordinary Codex still
launches once. Check the reported enforcement and restart boundary and test
from a plain interactive shell; a configured 75% policy is not proof of kernel
enforcement. See [Shared CPU policy](#shared-cpu-policy). Installation and these
checks make no model/provider calls; external subscription authorization remains
an explicit separate step.

## What is connected

| Repository source | User connection |
| --- | --- |
| `global/principles-of-work.md`, the full global AGENTS content | `<CODEX_HOME>/AGENTS.md` |
| `global/harness.config.toml` | live shared defaults; native TUI writes target the local base `config.toml` |
| `global/roles.toml` and `global/roles/*.md` | resolved by managed lead/executor sessions from the recorded source root; no deployed copy |
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
User-owned agents and skill directories are preserved. Core skill linking
delivers every top-level kit skill, including the loop guidance `team-lead`,
`board-workflow` and `self-improvement-loop`, and the manager binary carries
the matching `feedback` and `improve` verbs; an `update` after a skill is
added or removed reconciles that set. The installed loop consumes the
consumer project's Beads board and OpenSpec CLI, keeps its run directory, spec
file, qualification record and retained receipts as local private inputs, and
surfaces its recovery cursor through `improve status` without a model call;
operation lives in the `self-improvement-loop` skill. The controller contract,
commands and open acceptance are in
[Rust native](rust-native.md#improvement-controller).

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

## Shared CPU policy

Core `install` and `update` connect the account CPU policy. They create
`$env:LOCALAPPDATA\coding-agents-harness\cpu-budget\shared-cpu-policy.json`
only when that record is absent, with a 75% host ceiling. An existing record
is preserved, including a later edit and a ceiling equal to the retired 50%
batch default. A 50% ceiling sets `cpu_policy.ambiguous_shared_ceiling` and is
not rewritten. If account storage cannot be opened, install reports the policy
unavailable, writes nothing and terminates nothing. `--preview` does not write
the record, create a job or stop a process.
Code-tools-only update does not establish this policy, and neither install nor
update rewrites the heavy-command budget. Slot defaults are in
[Heavy-command concurrency](#heavy-command-concurrency).

```powershell
& <build>\codex-harness.exe install --core-only --preview --source . --codex-home "$env:USERPROFILE\.codex" --user-home "$env:USERPROFILE"
& <build>\codex-harness.exe update --core-only --source . --codex-home "$env:USERPROFILE\.codex" --user-home "$env:USERPROFILE"
& <build>\codex-harness.exe check --core-only --codex-home "$env:USERPROFILE\.codex" --user-home "$env:USERPROFILE"
```

Install and update JSON field `cpu_policy`, and core check fields `cpu_policy`
and `cpu_budget`, report the ceiling, escape hatch, activation and restart
boundary. `measured_consumption` stays `not-sampled`: kernel readback is not a
consumption measurement. Check does not write policy, create a job, sample CPU
or call a model. Activation stays `incomplete` while an ordinary registered
route is outside the account job, while the policy record is not usable, or
while process enumeration does not finish. A route outside the job is named in
`restart_boundary`, which says the installation does not terminate it; restart
that process after its work can stop.
Unknown or inaccessible members are not counted as covered.

A committed `disconnect --core-only` prints a stderr notice that default
coverage is no longer provided, preserves the policy record and running work,
and does not change the heavy-command budget. `disconnect --preview` does not
print that notice. `recover` finishes installation journals; it is not the CPU
coverage withdrawal. Selectors and batch safeguards are in
[Rust native](rust-native.md#shared-agent-cpu-allowance). The decision is in
[project decisions](project-decisions.md#shared-agent-cpu-budget).

## Heavy-command concurrency

Install, update and disconnect do not rewrite the machine-local heavy-command
policy. A missing slot count defaults to 2. A missing aggregate memory limit
equals the effective per-tree limit. Admitted trees share one account envelope
enforced by `JOB_OBJECT_LIMIT_JOB_MEMORY`. Each admission holds the legacy
`heavy-command.lock` gate, so an older exclusive lock still blocks a current
caller; slot files alone do not admit. A caller past the bound prints one
queue line with the busy and total slot counts and available holder
descriptions, or that the legacy lock is held. Slot count 1 restores
serialized admission. The shared CPU ceiling is unchanged. Commands are in
[Rust native](rust-native.md#heavy-command-budget). The decision is in
[project decisions](project-decisions.md#heavy-command-concurrency).

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
Committed core disconnect also prints the shared CPU coverage withdrawal in
[Shared CPU policy](#shared-cpu-policy). Preview does not.

Role activation has no deployed copy to roll back: managed sessions resolve
`global/roles.toml` and the fragments it names from the recorded source root
([What is connected](#what-is-connected)), so Disconnect removes the delivered
links while both roles stay resolvable, and the `check` report keeps naming
each role's revision or its exact missing input.

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

Windows sandbox must not launch packaged PowerShell from WindowsApps. Executor
dispatch reads the native runtime's effective permission policy and filters
WindowsApps from the child PATH only for sandboxed execution. Unsandboxed
execution preserves the owner's existing PowerShell 7, including a packaged
installation. A missing permitted PowerShell 7 fails before model work instead
of selecting Windows PowerShell 5.1. Dispatch records the prepared child
environment so an existing terminal process cannot replace it with stale PATH.
Parent and global PATH stay unchanged; no shell is installed.
