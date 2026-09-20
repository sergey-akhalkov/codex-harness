## ADDED Requirements

### Requirement: One-action deployment with explicit repair

The kit SHALL provide a single `codex-harness deploy` command that builds an immutable candidate from an absolute source checkout (or reuses an explicit build), installs or updates the core connection with user-default homes, verifies the installed launcher through its own version build identity and executor usage probe, re-reads the installation metadata, and reports one receipt. A relative source SHALL produce an explicit error naming the absolute-path requirement. The command SHALL offer an explicit `--reset` for installations whose recorded link ownership no longer matches reality: reset removes exactly the recorded owned objects that are reparse points or already missing, preserves and reports every regular file or foreign object, writes a receipt before removal, and then performs a normal installation. The ordinary verbs SHALL keep refusing silent repair. An `--all` option SHALL chain the existing scoped component updates after the core connection and report each component outcome in the same receipt.

The kit SHALL provide a `harness-deploy` skill, delivered with the kit's skills, that owns the deployment procedure: everyday one-action delivery, receipt verification, deliberate blocked-installation repair through `deploy --reset`, and the prohibition of out-of-band link retargeting to working-tree builds. The skill SHALL reference the installation guide as the authoritative detail home instead of duplicating it.

#### Scenario: Everyday delivery is one command
- **WHEN** a developer with a changed checkout runs `codex-harness deploy --source <absolute-checkout>`
- **THEN** a new immutable candidate is built and installed, and the receipt reports the installed launcher's build identity and a passing executor usage probe

#### Scenario: A blocked installation is repaired deliberately
- **WHEN** update refuses because an owned link's recorded identity no longer matches and the operator runs `deploy --source <absolute-checkout> --reset`
- **THEN** recorded link objects are removed only where they are reparse points, foreign regular files are preserved and reported, a reset receipt is written, the fresh installation succeeds, and a following `update --preview` validates without ownership conflicts

#### Scenario: The full delivery chain runs in one action
- **WHEN** the operator runs `deploy --source <absolute-checkout> --all`
- **THEN** the core connection is followed by the scoped component updates in order, and the receipt reports each component's outcome and stops at the first failure with prior work preserved
