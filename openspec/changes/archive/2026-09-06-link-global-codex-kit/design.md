## Context

See [proposal.md](proposal.md) for motivation and [the capability spec](specs/linked-global-kit/spec.md) for acceptance. At planning time the repository contained documentation, the portable principles and six OpenSpec skills, with no installer, shared configuration file, agent catalogue or application test runner. The host global `AGENTS.md` already linked to `global/principles-of-work.md`; the full global instruction source therefore already belonged to the repository.

The local `config.toml` contains Full Access defaults together with model preferences, project trust paths and UI state. Installation preserves this existing base and connects a separate shared profile. Following actual TUI persistence checks, the user explicitly accepted machine-specific settings, including trusted project paths, in the shared repository configuration. The inspected CLI is 0.153.4 on Windows native.

The design is included because global activation crosses configuration/discovery boundaries and needs safe migration, command forwarding and recovery.

## Goals / Non-Goals

**Goals:** Preserve one live source per managed artifact, keep ordinary local `codex` startup automatic, retain the native CLI/configuration hierarchy and make the declared kit reproducible from a fresh checkout. The full global AGENTS content and all repository-owned launch/install tools remain in the checkout.

**Non-Goals:** A new agent runtime, a copied or merged deployment tree, automatic model tasks during installation, installation of Codex itself, or a new functional agent catalogue. The initial verified entry point is Windows PowerShell running the installed CLI command. Desktop/IDE/app-server entry points, other OS installers and undeclared plugins/hooks/MCP packages are not covered by that CLI evidence.

## Decisions

### 1. Keep the base configuration local and link a native file profile

Add a portable `global/harness.config.toml` and link it as `<effective CODEX_HOME>/harness.config.toml`. Transfer the declared reusable settings by preserving their current values, including the explicitly selected Full Access defaults and the existing model/reasoning preferences (`gpt-6-astra`, `xhigh` observed during planning). This is configuration preservation, not a new model recommendation. The initial profile contains reusable preferences; native settings writers may subsequently add machine-specific values to it, as accepted by the user. Authentication, histories, caches and installer state stay local. Record the managed key inventory so new PCs reproduce the shared setup and unrelated local settings keep their native behavior. The host base `config.toml` remains local; no combined deployment file is generated.

The profile uses native Codex layering. There is no merged configuration output. The launcher adds `--profile harness` when the user has not selected another profile. Project configuration and explicit command-line overrides retain native priority.

Why this design: current [Advanced Config](https://learn.chatgpt.com/docs/config-file/config-advanced#profiles) documents file profiles and says the persistent `profile = "name"` selector is unsupported since 0.134.0. No generic extra-config `include` was found in the inspected official contract. A profile is a supported additional configuration source, but its automatic selection needs a small launch adapter.

An isolated planning probe on CLI 0.153.4 established that a symlinked profile is read by `--profile harness debug prompt-input`, applies `never` and `danger-full-access`, and retains a marker from the local base. A bounded `mcp add` probe changed the local base while leaving the linked profile unchanged. Both profiles and the ordinary base path are therefore actual consumers, not merely proposed file layouts.

Keeping the base separate preserves existing local settings and native management-command behavior without migration. Actual `/model` and trust-onboarding checks show that the profiled TUI writes into the linked profile source, whereas `mcp add/remove` and an unprofiled app-server writer target the local base. The user accepted this native behavior, including machine-specific paths in the shared file. Document these observed targets; no custom CLI build or separate write-redirection layer is required.

### 2. A transparent repository-owned CLI launcher selects the profile

Keep launcher source under `tools/` in the repository. Register its entry point through a file link in a kit-owned user command directory and update the user's command search path once; existing PATH content remains intact. Save the original resolved Codex executable/entry point in host-local installation metadata before registering the launcher, and reject recursion or unresolved command precedence.

The adapter does no orchestration and does not process model output. It forwards argument boundaries, stdin/stdout/stderr, interactive terminal behavior, cancellation and the exit status to the original CLI. Session commands (ordinary TUI launch, exec, review, resume and fork) select the profile by default. Explicit `--profile`/`-p` wins. `debug prompt-input` can use the same selection for verification. Help, version and management commands are delegated without indiscriminately attaching unsupported flags; the native 0.153.4 CLI rejects `--profile` for app-server.

The implementation must test dispatch against the supported CLI help, including options before a subcommand, config values containing spaces and an end-of-options separator. Resolve the original binary again on an explicit repair if its install path changes; never loop back into the adapter.

A new terminal obtains the persistent path change. Existing terminals receive clear restart/activation instructions. Direct invocation of a different executable path or a client that bypasses this command is outside automatic selection; the install/check report must state this boundary. The bootstrap stores only paths/ownership; it does not duplicate launcher source or configuration content.

### 3. Link instruction and capability sources individually

| Repository source | Host registration |
| --- | --- |
| `global/principles-of-work.md` — full global AGENTS content | `<CODEX_HOME>/AGENTS.md` file link |
| `global/harness.config.toml` | `<CODEX_HOME>/harness.config.toml` file link |
| Each managed `.agents/skills/<name>/` including resources | User `.agents/skills/<name>/` directory link |
| `global/agents/` including agent definitions and resources | `<CODEX_HOME>/agents/codex-harness/` directory link |
| Repository-owned launch tools | Linked user command entry point |

The existing full instruction source keeps its canonical filename; naming the host link `AGENTS.md` makes it the actual global AGENTS source. The maintenance `AGENTS.md` at the repository root is project guidance and must not accidentally become the portable global instructions. No Markdown include stub or separately maintained copy of the principles is introduced.

Individual skill links and one namespaced agent directory link preserve foreign skills/agents already present in their global folders. A whole-root directory replacement would obscure them. A new skill directory requires rerunning install to register it; agent files added inside the linked namespace and edits to connected sources are live for the next reader. Reconnection may remove only links whose recorded owner and old source still match.

[Build skills](https://learn.chatgpt.com/docs/build-skills#where-codex-loads-local-skills) explicitly supports symlinked skill directories. `[[skills.config]]` is not used as an invented extra-directory setting. [Subagents](https://learn.chatgpt.com/docs/agent-configuration/subagents#custom-agents) documents global standalone agent TOML files. Implementation probes on 0.153.4 found that an individual TOML file link is skipped by discovery, while the namespaced directory link is scanned recursively; an actual custom-agent invocation confirmed the latter. The implementation therefore uses that directory connection. Agents and skills must be self-contained or use declared repository resources/dependencies. Connection and resource paths must resolve on the current PC; saved machine-specific configuration values are permitted.

### 4. Declare the complete connected inventory in the repository

Keep the small mapping of managed sources, destinations and external prerequisites with the install tools; a separate extensible manifest framework is unnecessary. The initial inventory includes the shared config, full global instructions and all six existing OpenSpec skills. It supports the agent directory even when no production roles have yet been authored. Record the required OpenSpec CLI for those skills and the supported Codex/PowerShell environment in installation documentation, with their verified versions during implementation.

Every referenced script/template/resource needed by a declared kit capability must be present in the repository or covered by an explicit external prerequisite. Optional machine-installed plugins are not silently advertised as portable kit components. New component classes must meet the same direct-source contract before joining the inventory.

### 5. Use one bounded installer lifecycle

Interface: `install.ps1` connects, `-WhatIf` previews without mutations, `-Mode Check` checks ownership and consumption, and `-Mode Disconnect` removes this installation's registrations. Connection paths derive from the script location and the effective user home; the tools do not depend on a fixed account or checkout drive.

Before activation, validate the CLI, source inventory, link capability, canonical target boundaries, target conflicts, config profile collisions, command resolution and global instruction overrides. Windows Developer Mode or the relevant link privilege may be required. Failure to create a link is an actionable prerequisite failure; copying is never a fallback. A valid existing principles link is adopted with its pre-existing ownership recorded so rollback restores the prior activation state.

Write a minimal local journal under the effective Codex home with installation identity, source root, managed destinations, original command identity and PATH registration changes. It is mutable host state. Apply bounded mutations sequentially and roll back their completed subset on failure. A preview does not need journal writes. For disconnect/rollback, check both ownership and the current source identity; remove links themselves, never recursively traverse their targets. Preserve externally changed destinations and explain the unresolved conflict.

### 6. Validate actual startup and persistence behavior

There is no existing application runner to reuse. Add focused PowerShell checks for lifecycle and argument forwarding, plus real installed-CLI probes in disposable user roots and directories. Use `codex debug prompt-input` for effective defaults and initial instruction/skill discovery where supported; no model request is needed for those checks. Do not pass `--strict-config` to commands that reject it.

Exercise a representative harmless agent fixture through the supported CLI discovery/consumer path; if a model invocation is necessary to prove actual agent use, keep its task bounded and report that evidence separately. Remove the fixture afterward. The profile read probe already performed during planning does not replace verification of the eventual launcher.

Acceptance additionally covers changes to a live source between two launches, an unrelated project AGENTS file, coexistence with local skills/agents, duplicate discovery inside the harness, a different checkout path, explicit profile/CLI overrides, existing state, conflict handling, unavailable links, injected mid-activation failure, relocation and disconnect. Compare local state and tracked artifacts around config-management operations; explicitly exercise ordinary model selection persistence and project trust rather than extrapolating from `mcp add` alone.

Do the first real launch verification early, then expand to lifecycle failure paths. A fresh-checkout exercise must exclude machine-local fixtures and prove all declared source resources are present. Finish with the real user's command entry point outside this repository and keep claims specific to the tested CLI environment.

## Risks / Trade-offs

- Profile selection is not a persistent native setting → a small linked CLI launcher is required; clients bypassing it are explicitly outside this first entry point.
- Native profiled TUI settings writers modify the repository source, including machine-specific trusted paths → this effect is explicitly accepted and documented; verify the link and local authentication/session/cache state are preserved. Resource paths still require actual consumer checks.
- CLI argument/terminal forwarding can break interactive work → verify representative native TUI/exec and management paths, cancellation, exit status and quoted arguments; keep launcher responsibilities minimal.
- Links depend on the checkout and Windows privileges → preflight, explicit broken-source diagnostics and reconnection, with no copy fallback.
- Skill/agent collisions or a global AGENTS override can hide effective sources → detect before activation and preserve existing sources; do not claim successful composition when discovery disagrees.
- Uninstall through a directory link could delete repository sources → record ownership, validate canonical paths and unlink without recursion.
- Current schema can retain legacy fields rejected by current behavior → use current official docs and the installed CLI together. The version observed in planning is evidence, not an untested minimum-version guarantee.

## Migration Plan

1. Implement the portable inventory and direct profile/launcher path, prove it in a disposable host environment, and complete lifecycle checks.
2. Preview connection on the current machine. Keep the base configuration intact and adopt the already-correct global principles link without rewriting its source.
3. Register only missing, conflict-free connections and the verified command entry point; store local recovery metadata.
4. Verify through a fresh terminal outside the harness, publish the actual supported versions and evidence, and leave all tasks open until their checks pass.
5. On failure restore the previous registrations/PATH. On disconnect remove only this installation's still-owned additions and preserve pre-existing activation, repository files and local Codex data.
