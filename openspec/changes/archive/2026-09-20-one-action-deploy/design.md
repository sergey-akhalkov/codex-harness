## Context

See proposal.md. The building blocks already exist and are verified:
`native_build::prepare` creates journaled immutable candidates,
`core_install::connect` installs or updates the core connection,
`native_build::delivery_build` picks the freshest published build, and each
scoped component has its own lifecycle verb. What is missing is composition,
one command with sane defaults, an exit from identity-blocked state, and
verification of what was actually installed.

## Goals / Non-Goals

**Goals:**

- Everyday delivery and update in one command with default homes.
- A deliberate, reported repair for identity-blocked installations.
- Proof in the same receipt that the installed launcher serves the new build.
- Clear, source-specific errors instead of reused legacy-metadata text.

**Non-Goals:**

- No new package registry, network fetch or auto-update daemon; the source
  checkout remains the package.
- No weakening of the identity guards in the ordinary verbs: they keep
  refusing silent repair, and only `deploy --reset` bypasses them explicitly.
- No change to the config split: shared defaults stay live in
  `global/harness.config.toml`, private TUI writes stay in the local base
  `config.toml`; linking the whole local config into the repository would
  funnel private values into public sources.

## Decisions

- **Compose existing primitives, add no second installer.** `deploy` calls
  `native_build::prepare` and `core_install::connect` unchanged; a new verb
  that reimplemented linking would double the failure surface it removes.
- **Reset removes links, never targets.** A recorded owned object is removed
  only while it is a reparse point (deleting a symlink or junction removes the
  link, not its target) or already absent; a regular file or directory at a
  recorded path is preserved and reported as foreign. A reset receipt is
  written before anything is removed, so the action is auditable even if the
  following install fails. Simpler full-wipe alternatives would destroy
  user-owned state and violate the kit's preservation rules.
- **Verification probes the installed artifact, not the candidate.** The
  receipt records what `harness/bin/codex-harness.exe --version` and
  `executor --help` answer after installation, because those are exactly the
  observations a consumer session relies on; a candidate-only check would
  repeat the stale-launcher failure this change removes.
- **`--all` chains the existing scoped updates rather than a combined
  activation.** Combined activation is explicitly unfinished in the current
  lifecycle; chaining keeps each component's own ownership checks and
  journals, and the receipt reports per-component outcomes.

## Risks / Trade-offs

- [Reset on a machine with hand-placed regular files at recorded paths] →
  Those are preserved and reported, never removed; install then either
  re-links around them or fails loudly with the receipt intact.
- [Deploy hides the multi-component split from advanced users] → Scoped verbs
  remain unchanged and documented; `--all` is a chain, not a replacement.
- [Concurrent sessions holding old binaries during deploy] → Immutable builds
  and link replacement keep running processes alive; verification reads the
  new link, and this matches the existing delivery model.

## Migration Plan

Ship the verb alongside the existing commands; documentation presents
`deploy` as the everyday route and the scoped verbs for special cases.
Acceptance delivers this machine's installation through `deploy --reset`,
replacing the manual repair script. Rollback is the existing recover/build
selection; the reset receipt names everything that was removed.
