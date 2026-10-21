## MODIFIED Requirements

### Requirement: One-action deployment with explicit repair

The kit SHALL provide a single `codex-harness deploy` command that builds an immutable candidate from an absolute source checkout (or reuses an explicit build), installs or updates the core connection with user-default homes, verifies the installed launcher through its own version build identity and executor usage probe, re-reads the installation metadata, and reports one receipt. A relative source SHALL produce an explicit error naming the absolute-path requirement. The command SHALL offer an explicit `--reset` for installations whose recorded link ownership no longer matches reality: reset removes exactly the recorded owned objects that are reparse points or already missing, preserves and reports every regular file or foreign object, writes a receipt before removal, and then performs a normal installation. The ordinary verbs SHALL keep refusing silent repair. An `--all` option SHALL chain the existing scoped component updates after the core connection and report each component outcome in the same receipt.

For `deploy --preview` without an explicit build, the kit SHALL reuse an eligible integrity-verified existing build selected by the existing build-selection authority when one is available, without compiling solely to produce that preview. The plan SHALL identify both the selected build and requested source and disclose any freshness limitation; preview reuse SHALL NOT certify new source as compiled, tested, installed or approved. Explicit build selection SHALL remain authoritative. When no eligible existing build is available, the existing candidate preparation path SHALL remain available with its normal errors and safeguards. A later applying deployment SHALL independently enforce its source/build and ownership checks. Preview SHALL preserve installation links, selections, settings and reset targets, including component previews requested with `--all`.

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

#### Scenario: A planning preview reuses a verified available build
- **WHEN** an operator requests `deploy --preview` without `--build` and an eligible verified existing build is available
- **THEN** the plan uses that identified build without invoking compilation, reports its relationship to the requested source, and leaves the installation unchanged

#### Scenario: Preview reuse does not grant applying authority
- **WHEN** a plan used an older verified build for changed source and the operator later requests applying deployment
- **THEN** normal source/build verification and preparation apply independently, and the earlier preview cannot authorize delivery of stale or altered executable bytes

#### Scenario: Explicit selection and unavailable builds remain honest
- **WHEN** the operator supplies an explicit build, or implicit preview finds no eligible verified build
- **THEN** explicit selection retains its existing meaning, unavailable or altered artifacts are never reported as verified, and the normal preparation or error path applies when required

#### Scenario: Full or repair preview does not perform installation effects
- **WHEN** the preview also requests component updates or reset
- **THEN** every requested plan is reported through existing component owners while links, settings, selected runtime and reset targets remain unchanged
