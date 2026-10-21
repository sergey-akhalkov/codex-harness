# Design

## Context

See [proposal.md](proposal.md) for the confirmed scope. Ponytail already supplies a native Codex plugin, hooks and skills; Codex supplies installation and trust.

Original Ponytail 5.3.2 revision `09c434ab73ca43761c5d8b0c5d341926c2fba710` was inspected and installed in an owned isolated target. Native hook review, policy injection and core/help skill reads were observed outside this checkout with Codex CLI 0.163.0 and Node 24.18.0. These are exercised versions, not inferred minimums or final-delivery proof.

Sources: [installation](https://github.com/DietrichGebert/ponytail/blob/09c434ab73ca43761c5d8b0c5d341926c2fba710/INSTALL.md), [manifest](https://github.com/DietrichGebert/ponytail/blob/09c434ab73ca43761c5d8b0c5d341926c2fba710/.codex-plugin/plugin.json), [rules](https://github.com/DietrichGebert/ponytail/blob/09c434ab73ca43761c5d8b0c5d341926c2fba710/skills/ponytail/SKILL.md), [hooks](https://github.com/DietrichGebert/ponytail/blob/09c434ab73ca43761c5d8b0c5d341926c2fba710/hooks/claude-codex-hooks.json), [license](https://github.com/DietrichGebert/ponytail/blob/09c434ab73ca43761c5d8b0c5d341926c2fba710/LICENSE).

## Goals / Non-Goals

Use the original package and existing native commands. Provide a setup sequence another agent can execute without development-account state. Keep one owner for equivalent rules.

Do not integrate the prepared custom package lifecycle merely because it exists. No Ponytail fork, copied policy, new installer, exhaustive lifecycle campaign or efficiency benchmark is required.

## Decisions

### Native installation

Put the verified marketplace/plugin commands, prerequisites and native trust step in the existing guide. A small native marketplace declaration may select the tested revision; it is Codex input, not a new management system. Do not extend the kit manifest or installation metadata just to track the package.

Check native listing before repeating an add command: native add can reset the enabled choice. Preserve explicit disablement and unrelated configuration. Use native errors and remedies without a second state model.

### Default hooks

Use the original default mode and native `/hooks` review. A listed package alone does not prove activation. Its six skill routes remain in the original package.

The current harness hook decision can disable all hooks when RTK is off. Verify that interaction and correct only the existing decision if it prevents Ponytail from working. Keep retired diagnostic and Stop handlers inactive; this correction does not authorize custom provisioning.

### Instruction ownership

Reuse the prepared comparison in `docs/global-instructions.md`. Remove only equivalent local instructions; retain additional obligations, scope authorization, project rules and specialized workflows. Protected OpenSpec resources stay unchanged.

Observe Ponytail loading before publishing dependent removal. Keep existing global links and source/accounting limits; no new accounting CLI is needed.

## Risks / Trade-offs

- Native trust needs an explicit first-use review; verify a fresh session afterward.
- Resume resets mode and sessions can share state, including a fallback on the exercised host. These original limitations are accepted and must be documented, not fixed or hidden.
- Existing installations can be disabled deliberately; inspect before repeating installation.
- An already-running session can retain older guidance; final verification uses a fresh session.

## Delivery and verification

1. Reuse completed package and native-loading qualification.
2. Verify and publish the native setup sequence, prerequisites and mode limitations; check the existing hook-selection interaction.
3. Publish reviewed deduplication after the plugin is available.
4. Observe the final composition outside the harness, including the selected external project read-only. Verify relevant resources, source hygiene, links and strict OpenSpec validity.

No separate Windows environment, project tasks, custom rollback/removal campaign or claim of already-completed other-PC execution is required.
