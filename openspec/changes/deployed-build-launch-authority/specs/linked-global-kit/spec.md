## MODIFIED Requirements

### Requirement: Source updates and checkout availability

A new Codex process SHALL consume edited contents of already connected source-owned data files without a second install or content synchronization. Native executable launch admission SHALL depend on the recorded binary and metadata integrity of the delivered build, not on checkout freshness. Missing, altered or metadata-incompatible builds SHALL refuse the affected runtime with the corrective action. A source-stale or unavailable checkout SHALL be reported with the explicit build/update action that switches subsequent process launches to a newer build, and MUST NOT disable otherwise-admitted runtime. Ordinary consumer startup MUST NOT invoke Cargo, compile or download dependencies. Adding or removing separately registered artifacts SHALL be reconciled by rerunning native installation without copying data artifacts. Checkout relocation SHALL support reconnection and build-identity reconciliation. Verification SHALL identify a moved, deleted or inaccessible source and MUST NOT treat a dangling connection or source-stale binary as healthy. Already running sessions are not required to reload their entire initial context; skills accepted and activated by the autonomous skill evolution workflow SHALL additionally satisfy current-session skill awareness and revision recovery without a user restart. Orchestrated worker succession through `codex resume` is a separate consumer of those updates and SHALL NOT satisfy the current-session awareness requirement.

These harness health checks SHALL NOT block ordinary upstream Codex CLI launch. An installed command bootstrap independent of checkout availability SHALL preserve access to the original CLI when harness source, shared configuration or its required build is unavailable. It SHALL report the degraded harness and use native arguments/local settings without running unverified harness extensions, rebuilding, downloading or rewriting user configuration. Missing upstream Codex itself and recursive command discovery SHALL remain explicit failures, not reasons to invoke an arbitrary replacement.

#### Scenario: A connected file changes
- **WHEN** a shared setting, principle, skill body or agent definition is edited in the checkout and a new process starts
- **THEN** Codex observes the new source content without a reinstall

#### Scenario: The registered set changes
- **WHEN** source artifacts are added or removed and installation is rerun
- **THEN** the live registrations are reconciled, removing only obsolete kit-owned registrations and preserving unrelated capabilities

#### Scenario: The checkout is moved
- **WHEN** verification encounters the old location and installation is subsequently run from the new checkout
- **THEN** verification reports the stale source and reconnection updates only kit-owned references and the applicable build identity to the new location

#### Scenario: Native implementation changes
- **WHEN** relevant Rust source or locked build inputs change after installation
- **THEN** Check reports the outdated build without automatic compilation, affected native invocations continue to run on the integrity-verified delivered build, and successful explicit update activates a verified candidate with recoverable prior state

#### Scenario: Checkout advances without a deploy
- **WHEN** relevant first-party source changes after delivery while the recorded binaries still match, and a source-consuming runtime command starts afterwards
- **THEN** the command launches on the delivered integrity-verified build without refusal, recompilation or downloads, Check and diagnose report the source-ahead status with the explicit deploy action, and a later explicit deploy switches only subsequently started processes to the new build

#### Scenario: Harness is unavailable during an ordinary Codex launch
- **WHEN** the installed launcher encounters missing, altered or metadata-incompatible shared build inputs, an unavailable checkout/module, or missing or malformed harness registration while the original CLI remains installed
- **THEN** ordinary Codex launches once with the user's native arguments, local settings, cwd and streams preserved, reports degraded harness behavior, and does not require a successful harness update first

#### Scenario: A running process uses the previous build
- **WHEN** an explicit update validates a new native build while another session still uses the old executable
- **THEN** activation does not overwrite that running artifact and cleanup preserves it until owned use can safely end

#### Scenario: Stale manager needs to update itself
- **WHEN** source freshness differs but the last accepted manager still passes binary-integrity and metadata-compatibility checks
- **THEN** Check and explicit update/recover/disconnect remain usable without starting the obsolete ordinary runtime; an unavailable or altered manager instead requires the documented native bootstrap recovery path

#### Scenario: An accepted skill changes in a running session
- **WHEN** the autonomous workflow activates a created, updated or restored skill
- **THEN** the current session can apply the accepted revision before its next relevant action and recover it after compaction without requiring a new process
- **AND** recovery uses a currently allowed ordinary-CLI path; restoring ordinary diagnostic, context or Stop hooks, or substituting App Server-only success, does not satisfy this scenario
