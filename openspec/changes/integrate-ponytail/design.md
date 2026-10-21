# Design

## Context

See [proposal.md](proposal.md) for motivation and the three [integration](specs/ponytail-integration/spec.md), [instruction ownership](specs/global-working-principles/spec.md) and [installation](specs/linked-global-kit/spec.md) deltas for the behavior contract.

Current implementation facts:

- `global/kit.json` declares the live first-party instruction, skill, role and hook sources. Global `AGENTS.md` points to `global/principles-of-work.md`; `global/roles.toml` owns responsibility membership.
- `crates/codex-harness/src/install_cli.rs` routes ordinary core connection and `deploy`. `crates/harness-core/src/core_install.rs` owns preflight, installation locking, registration effects and recovery. Its current hook-feature decision reads the RTK selection only and disables hooks otherwise. Adding a plugin without changing that decision would not complete delivery.
- `installation_state.rs`, `installation_metadata.rs` and the existing dependency lifecycle provide ownership and recovery foundations. Extend those owners for the actual plugin effects rather than introduce another installer, registry or report protocol.
- `crates/codex-harness/tests/installation_state.rs` already exercises native entry points, preview, relocation, foreign ownership and pending recovery. These are appropriate integration points for additional Ponytail cases.
- Deployment-preview reuse is specified by `linked-global-kit`. Ponytail must preserve that behavior without compiling solely for preview or overwriting independently delivered installation changes.

Upstream inspection on 2026-10-21 identified Ponytail **5.3.2**, revision **`09c434ab73ca43761c5d8b0c5d341926c2fba710`**. Native installation at this exact revision was exercised with Codex CLI 0.163.0; isolated upstream hooks were exercised with Node 24.18.0. Native trust review, full policy injection and actual core/help skill reads passed in a fresh managed native session outside this checkout; final integrated lifecycle acceptance remains unfinished. Its Codex manifest exposes `skills/` and `hooks/claude-codex-hooks.json`, which declares `SessionStart`, `SubagentStart` and `UserPromptSubmit`. The upstream installation guide's reference to two hooks is therefore not the complete event inventory. The package declares MIT licensing and no npm runtime dependency set; its hooks still require Node and their referenced resources.

The upstream marketplace entry points at `main`. Pinning only a marketplace checkout does not establish that the plugin's own source selection is immutable. Local Codex CLI 0.163.0 exposes native marketplace registration, plugin installation and removal; successful help inspection does not prove hook behavior or a minimum supported version.

Relevant sources, inspected for this design:

- [Ponytail Codex manifest](https://github.com/DietrichGebert/ponytail/blob/09c434ab73ca43761c5d8b0c5d341926c2fba710/.codex-plugin/plugin.json), [marketplace](https://github.com/DietrichGebert/ponytail/blob/09c434ab73ca43761c5d8b0c5d341926c2fba710/.agents/plugins/marketplace.json), [license](https://github.com/DietrichGebert/ponytail/blob/09c434ab73ca43761c5d8b0c5d341926c2fba710/LICENSE).
- [Upstream rule owner](https://github.com/DietrichGebert/ponytail/blob/09c434ab73ca43761c5d8b0c5d341926c2fba710/skills/ponytail/SKILL.md), [hook configuration](https://github.com/DietrichGebert/ponytail/blob/09c434ab73ca43761c5d8b0c5d341926c2fba710/hooks/claude-codex-hooks.json), [mode-state implementation](https://github.com/DietrichGebert/ponytail/blob/09c434ab73ca43761c5d8b0c5d341926c2fba710/hooks/ponytail-runtime.js), [installation](https://github.com/DietrichGebert/ponytail/blob/09c434ab73ca43761c5d8b0c5d341926c2fba710/INSTALL.md).
- [Official plugin packaging and native hook trust](https://developers.openai.com/plugins/build/plugins), [hook events and context](https://learn.chatgpt.com/docs/hooks).

## Goals / Non-Goals

**Goals:** Use the upstream package as the policy owner, keep the existing Rust installation lifecycle authoritative, and make selection, trust, consumption and recovery distinguishable. Deliver a small native-consumer proof before changing the live instruction composition.

**Non-Goals:** A generic plugin-management framework, a fork or Rust rewrite of Ponytail, new support claims for other agents, Claude-specific band/dashboard integration, changed model routing, or an efficiency benchmark campaign. Existing acceptance and protected external OpenSpec workflows remain intact.

## Decisions

### 1. Native plugin package with one portable revision declaration

Use the Codex plugin lifecycle to install the original upstream repository. Add a small harness-owned native marketplace declaration under `.agents/plugins/marketplace.json`, referenced by the kit inventory. Its Ponytail source points to the authoritative repository and the accepted full commit revision. Keep the pin in that declaration; installation metadata records the resolved observation, not a second independently edited version preference.

Register the local marketplace through the supported native command and install its Ponytail entry. Resolve the actual installed package identity rather than trust a version label or the marketplace revision. Handle a compatible existing upstream installation as adopted where native identity permits; report incompatible or duplicate sources without silently migrating ownership.

This avoids copying rules into the harness or relying on the upstream moving branch. A link to `AGENTS.md` alone is insufficient: it would neither compose both owners through the existing global file nor deliver the plugin's lifecycle and skill behavior. Downloading raw skill files would create a second update lifecycle and omit hooks.

### 2. Integrate with ordinary core delivery and existing recovery

Ordinary core installation and `deploy` include Ponytail; no new required component flag or standalone setup script is introduced. Extend the current installation reports, metadata and recovery stages with selected package identity, owned versus adopted registrations, trust/activation state and actionable failures. Reuse the existing installation locks and dependency preflight. Preserve the existing non-mutating preview contract, including full deployment previews.

Keep the selected Codex executable, native plugin operations and Node runtime explicit. Reuse compatible installed dependencies and existing supported provisioning paths. Missing prerequisites fail preflight with a next action; a new system runtime installation must not happen as an invisible session-start side effect. Ponytail's JavaScript remains third-party code; maintained integration, fixtures and checks remain Rust.

Retain the prior accepted package selection and sufficient local recovery material before switching. Native plugin operations can have effects beyond a filesystem link, so recovery must cover the actual config, registration and package-selection effects and report incomplete restoration separately. Disconnect removes only still-owned registrations; a native remove operation must not be used on an adopted package or discard user data merely to simplify cleanup.

Native Codex 0.163.0 discovery accepts an ordinary package-cache directory but rejects a version-directory symlink or junction. Prepare the original package through native commands in an isolated home, then publish an ordinary directory through the existing `Registration` journal alongside guarded configuration changes. Extend that owner with the required directory effect; do not add a second journal. Record intent before renaming, retain the prior tree, and guard publication, recovery and finalization against changed identities and foreign replacements. Recovery restores both the prior package and configuration; a completed update keeps the prior package available for explicit rollback. Native `plugin add` enables its selection even when it was explicitly disabled, so preparation must carry forward the target's disabled selection before publication.

Native `hooks/list` preserves configuration, plugin selection and package contents but can create Codex runtime databases, logs and managed skill copies. Preview must not run that probe against the target home. Inspect package identity through read-only Git/resource checks and use an owned temporary home for any required native capability probe; report unavailable target activation evidence explicitly.

### 3. Native trust is an accepted setup boundary

Preserve the native `/hooks` review. Do not synthesize approval fingerprints, mark the plugin as enterprise-managed, or copy its hooks into a privileged harness definition to avoid trust. Initial package installation may complete while activation remains pending. The normal receipt and Check expose that state and the next step; a fresh session after trust establishes loading. Updated hook definitions may legitimately require another native review.

Replace the current RTK-only hook-feature decision with the accepted selection of RTK and Ponytail, preserving explicit disablement and unrelated settings. Do not reconnect the empty ordinary-hook definition to old diagnostic handlers. The specific upstream events are reviewed from the selected manifest; broad hook enablement is not a reason to reactivate retired handlers.

### 4. Keep upstream policy and harness supplements separate

Use the upstream default `full` mode initially. The plugin owns its rules and skills; global `AGENTS.md` remains linked to the harness supplements. Native project guidance and controlling instructions retain their normal authority. The supplements identify their Ponytail dependency and the meaning of missing or deliberately disabled guidance without reproducing the missing rules.

Use the existing `docs/global-instructions.md` ownership section for the durable coverage map. Compare meaning across the common principles, developer guidance, role fragments, owned skills and source-generated briefs. Classify affected obligations as upstream-owned, retained supplement or protected external workflow. Link to the accepted revision and source section; do not paste an upstream manual into the map. Preserve specification requirements as outcome contracts even when their instruction owner changes.

Initial boundaries from the inspected text:

| Concern | Upstream coverage | Harness responsibility retained |
| --- | --- | --- |
| Small complete change, local reuse, platform/stdlib preference, avoiding unrequested abstractions | Ponytail core rules | Required research beyond local reuse, total lifecycle cost and accepted operating conditions |
| Reading affected code and updating callers, fixtures and checks | Ponytail core rules | Exact assignment paths, committed inputs, tool coverage and installation ownership |
| Small checks for non-trivial logic and preservation of validation | Ponytail core rules | Specification-required native checks, real-consumer evidence, recovery and truthful verification limits |
| Review, audit and related Ponytail commands | Original upstream skills | Existing specialized workflows only where their purpose and obligations differ |
| OpenSpec, delegation, MCP selection, publication boundaries and runtime recovery | Not replaced by the inspected core rules | Existing harness owners and protected external workflows |

This table identifies comparison boundaries, not an assertion that every affected paragraph has already been audited. Do not retire a specialized skill merely because its name resembles an upstream command. Do not replace the harness's complete acceptance with Ponytail's minimum small-test rule or let shortest-diff advice remove required work.

Extend existing instruction accounting to include the actual upstream context and invoked skill bodies alongside harness layers. Native caches and generated hook context are consumption mechanisms, not new source owners. Mechanical accounting and semantic coverage are separate checks; no token or quota savings follow from deleting local text alone.

### 5. Qualify the native path before consolidating instructions

The first usable result is the pinned original plugin loading in a fresh ordinary Codex session from an owned outside-checkout directory, together with the existing harness and project guidance. Establish package identity, native trust, actual hook output and skill discovery. Reuse the current native context/check route; do not build a new probe framework.

Before removing local coverage, exercise fresh start, resume, compaction, executor startup, explicit mode changes and concurrent sessions in the same and different repositories. The inspected runtime uses project-keyed mode files when project context is present and a shared fallback otherwise; it does not provide session-keyed isolation. Isolated hook execution confirmed that resume resets to the default and another same-project startup overwrites the prior mode, while compaction retains the current stored mode. Do not infer isolation from separate worktrees or plugin installation alone.

The user explicitly selected integration of the original package with these mode limitations documented in `docs/installation.md`, rather than a Ponytail correction or fork. Preserve upstream behavior and distinguish temporary `off`/mode controls from persistent plugin disablement, which the installation lifecycle must still preserve. Do not impose a one-session restriction, copy the policy, or claim native consumption from a hook fixture. Actual missing context, trust failures and installation defects remain qualification failures; the accepted upstream mode scope is not one.

### 6. Keep delivery and observation claims separate

Check inspects declared versus resolved revision, resource availability, selections and native trust/activation information that the installed CLI actually exposes. It performs no download or model call. A historical successful probe is not proof of loading in every current session; unavailable evidence is reported as unverified. Unexpected native startup/hook errors remain visible and ordinary Codex retains the existing degraded-launch behavior.

Acceptance uses local isolated installation and the installed launcher in a neutral directory and the user-selected existing external project with its own instructions. Per the user's explicit read-only boundary, that project qualifies package selection, composed guidance and relevant Ponytail skill/resource consumption; no project task or file change is authorized. Disposable test homes and synthetic failure inputs supplement these consumers. Per the revised delivery decision, `docs/installation.md` supplies installation, check and recovery instructions for the receiving agent on another PC; no separate Windows machine, VM or account must be prepared in this implementation session. Target-machine execution belongs to that agent and remains unverified until it runs. Native model-backed checks remain explicit, visible and subject to the existing routing and resource rules.

Run the remaining native session-lifecycle matrix against one complete isolated candidate containing the proposed final package, installer and supplements, before publishing the live instruction-source update. The same observed run can satisfy upstream lifecycle qualification and final composition acceptance when their required conditions and exact inputs coincide. Keep every required scenario; repeat affected checks when a correction changes those inputs or leaves a concrete concern unresolved.

## Risks / Trade-offs

- **Shared mode state can interfere across sessions.** This original upstream behavior is explicitly accepted with documentation. Observe the native scope before consolidation and state the resume reset and shared-mode consequences without promising isolation or changing upstream code.
- **A trusted installation can later have missing or changed hooks.** Preserve native trust invalidation, expose pending/degraded states and verify recovery rather than silently installing a fallback policy.
- **Live source updates can remove rules before an older host installs Ponytail.** Deliver dependency awareness and the plugin connection before publishing the consolidated source; document explicit update/trust requirements and qualify fresh consumers after the final source change. Do not claim an old running session reloaded.
- **Native package operations can change user-owned state.** Preflight identity and ownership, retain recovery inputs, and test interruption and foreign replacement through the real operation boundary.
- **Upstream changes can remove relied-on guidance.** Keep an immutable pin and reassess only affected coverage before promoting a candidate; preserve the old accepted selection on failure.
- **Hook context can increase rather than reduce instruction load.** Measure the complete composed input with existing accounting. The acceptance goal is correct ownership and behavior, not an unmeasured reduction.

## Migration Plan

1. Qualify the inspected candidate through native installation and the bounded session/concurrency checks. Record a supported Codex/Node baseline and any actual constraints in the existing installation guide during implementation.
2. Add the portable declaration and lifecycle integration, including preview, pending trust, coexistence, update, interruption, recovery and disconnect. Verify a real outside-project path while the existing instruction coverage is still intact.
3. Complete the semantic coverage map and remove equivalent harness-owned instructions. Reconcile affected role/skill accounting, operating guides and project decisions. Do not edit external OpenSpec workflows or rewrite historical evidence as a current pass.
4. Deploy the final composition, then complete local isolated-installation and real-consumer acceptance, including unchanged required project checks and all lifecycle cases. Deliver the other-PC installation/check/recovery handoff in `docs/installation.md` for its receiving agent. Leave unexecuted local tasks open and do not claim target-machine execution from the handoff alone.
5. For rollback, restore the prior accepted harness source and package selection through their existing owners. Because first-party instructions are live, rolling back only a binary or plugin is insufficient after consolidation. Preserve unrelated settings, adopted packages and user session state; report unresolved restoration before conflicting use.
