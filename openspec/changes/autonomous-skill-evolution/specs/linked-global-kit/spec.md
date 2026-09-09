## MODIFIED Requirements

### Requirement: Source updates and checkout availability

A new Codex process SHALL consume edited contents of already connected source files without a second install or content synchronization. Adding or removing separately registered artifacts SHALL be reconciled by rerunning installation without copying. Checkout relocation SHALL support reconnection. Verification SHALL identify a moved, deleted or inaccessible source and MUST NOT treat a dangling connection as healthy. Already running sessions are not required to reload their entire initial context; skills accepted and activated by the autonomous evolution workflow SHALL additionally satisfy current-session skill awareness and revision recovery without a user restart.

#### Scenario: A connected file changes
- **WHEN** a shared setting, principle, skill body or agent definition is edited in the checkout and a new process starts
- **THEN** Codex observes the new source content without a reinstall

#### Scenario: The registered set changes
- **WHEN** source artifacts are added or removed and installation is rerun
- **THEN** the live registrations are reconciled, removing only obsolete kit-owned registrations and preserving unrelated capabilities

#### Scenario: The checkout is moved
- **WHEN** verification encounters the old location and installation is subsequently run from the new checkout
- **THEN** verification reports the stale source and reconnection updates only kit-owned references to the new location

#### Scenario: An accepted skill changes in a running session
- **WHEN** the autonomous workflow activates a created, updated or restored skill
- **THEN** the current session can apply the accepted revision before its next relevant action and recover it after compaction without requiring a new process
- **AND** recovery uses a currently allowed ordinary-CLI path; restoring ordinary diagnostic, context or Stop hooks, or substituting App Server-only success, does not satisfy this scenario

## ADDED Requirements

### Requirement: Autonomous managed skill registration

The kit SHALL expose the evolution workflow and its required runtime resources globally through its reproducible installation lifecycle. After accepted shared skill publication or retirement, the workflow SHALL reconcile only the affected kit-owned registrations and ownership state without requiring a user to rerun installation manually. This operation SHALL preserve direct source consumption, source ownership, rollback and disconnect semantics; it MUST NOT reinstall unrelated dependencies, rewrite unrelated configuration or restart shared services. Project-owned skill packages and their records SHALL survive kit disconnection.

#### Scenario: New common skill is accepted outside the harness
- **WHEN** an authorized evolution episode in another repository promotes a skill to the canonical kit source
- **THEN** its scoped global registration is reconciled automatically and the originating session and another project can use it through live source references

#### Scenario: Registration target has foreign ownership
- **WHEN** scoped registration encounters a conflicting foreign destination or an active installation transaction
- **THEN** it preserves the existing state, reports the conflict or busy condition and leaves publication pending or safely recovered without claiming complete activation

#### Scenario: Installation moves or disconnects
- **WHEN** the kit is reconnected at a new location or disconnected
- **THEN** evolution registrations follow the same ownership-aware lifecycle while project skills, knowledge records and unrelated capabilities remain intact
