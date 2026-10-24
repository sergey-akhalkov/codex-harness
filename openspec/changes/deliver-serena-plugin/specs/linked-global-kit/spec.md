# Spec Delta

## MODIFIED Requirements

### Requirement: Direct source consumption

This source-link contract SHALL apply to components still delivered through the source-linked harness lifecycle and to explicitly selected development connections. Capabilities migrated to a native plugin SHALL use versioned packages for ordinary consumer installation; Serena is the migrated capability in this change. A plugin's portable source remains repository-owned, while its released runtime, configuration, skills and integration data MAY be included in the selected package without requiring a live checkout. This exception MUST NOT move credentials, machine settings or runtime state into a public package or silently convert other components.

The kit SHALL connect its configuration, portable instructions, skills, agent definitions, MCP connection definitions, language integration definitions and hooks by supported path references or filesystem links. Codex and the corresponding integration consumer SHALL read the repository source when consuming those data artifacts. The installer MUST NOT copy, render, merge or cache their contents into deployment files, including as a fallback. Small host-local path registrations and installation metadata SHALL contain references and ownership information rather than duplicated artifact bodies. Compiled first-party executables SHALL be distinguished from source-owned data: installation SHALL connect verified build artifacts with recorded source/build identity, while their complete source remains in the checkout. This executable-artifact allowance MUST NOT permit embedding deployed copies of source-owned configuration, instructions or skills. External packages, language runtimes and generated runtime state SHALL remain distinct from repository-owned configuration and integration source; their installation SHALL not be used to smuggle deployed copies of those artifacts into a package cache.

#### Scenario: A clean host is connected

- **WHEN** the source-linked installer successfully connects a valid checkout for the components it still owns, or a developer explicitly selects source-linked development
- **THEN** each source-linked data artifact resolves to that checkout without a deployed content copy, and each source-linked compiled executable resolves to a verified build identified with that checkout's relevant sources
- **AND** independently installed Serena remains owned by its versioned plugin

#### Scenario: Links or another required capability are unavailable

- **WHEN** a required direct connection cannot be established
- **THEN** installation fails with the specific cause and a corrective action, without substituting copied artifacts or reporting partial activation as success

#### Scenario: MCP or diagnostic integration source is updated

- **WHEN** a connected MCP definition, language integration definition or hook definition changes and a new consumer starts
- **THEN** the consumer reads the updated checkout data through its existing connection without a generated deployment copy, while a changed compiled implementation follows the verified build/update contract

#### Scenario: Ordinary installation of a migrated capability

- **WHEN** an ordinary user installs the released Serena plugin through native Codex
- **THEN** the consumer reads that package's versioned portable resources and prebuilt runtime without a harness checkout or source-link privilege
- **AND** source-linked development remains an explicit alternative rather than a prerequisite

### Requirement: Source updates and checkout availability

The live-source behavior below SHALL apply only to source-linked components and explicit development connections. Ordinary Serena plugin consumers SHALL follow their selected released package; edits, relocation or absence of a development checkout MUST NOT change plugin behavior or degrade its health. New package versions SHALL become active through the explicit native plugin lifecycle with the preserved session-generation guarantees in `serena-plugin`.

A new Codex process SHALL consume edited contents of already connected source-owned data files without a second install or content synchronization. Native executable launch admission SHALL depend on the recorded binary and metadata integrity of the delivered build, not on checkout freshness. Missing, altered or metadata-incompatible builds SHALL refuse the affected runtime with the corrective action. A source-stale or unavailable checkout SHALL be reported with the explicit build/update action that switches subsequent process launches to a newer build, and MUST NOT disable otherwise-admitted runtime. Ordinary consumer startup MUST NOT invoke Cargo, compile or download dependencies. Adding or removing separately registered artifacts SHALL be reconciled by rerunning native installation without copying data artifacts. Checkout relocation SHALL support reconnection and build-identity reconciliation. Verification SHALL identify a moved, deleted or inaccessible source and MUST NOT treat a dangling connection or source-stale binary as healthy. Already running sessions are not required to reload their entire initial context; skills accepted and activated by the autonomous skill evolution workflow SHALL additionally satisfy current-session skill awareness and revision recovery without a user restart. Orchestrated worker succession through `codex resume` is a separate consumer of those updates and SHALL NOT satisfy the current-session awareness requirement.
These harness health checks SHALL NOT block ordinary upstream Codex CLI launch. An installed command bootstrap independent of checkout availability SHALL preserve access to the original CLI when harness source, shared configuration or its required build is unavailable. It SHALL report the degraded harness and use native arguments/local settings without running unverified harness extensions, rebuilding, downloading or rewriting user configuration. Missing upstream Codex itself and recursive command discovery SHALL remain explicit failures, not reasons to invoke an arbitrary replacement.
The installed bootstrap SHALL apply the shared agent CPU policy independently of optional harness feature availability. When it cannot establish or verify the requested cap, it SHALL preserve upstream launch availability with the visible degraded fallback specified by `shared-agent-cpu-budget`; it SHALL NOT report that launch as capped or silently change the normal policy. Explicit uncapped invocation SHALL remain available independently of the shared checkout. Fallback SHALL start the original requested payload at most once and SHALL preserve unrelated sessions' CPU policy and lifecycle.

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
- **AND** the bootstrap establishes the shared CPU cap when possible and otherwise warns that CPU enforcement is not guaranteed

#### Scenario: CPU admission fails during an ordinary launch

- **WHEN** the installed bootstrap cannot establish or verify the shared CPU allowance before starting a usable original CLI
- **THEN** it warns with the cause and recovery action, starts that CLI once without claiming the cap is active, and reports incomplete CPU coverage
- **AND** it preserves the default policy for later launches and does not disable another session's limit

#### Scenario: An explicit uncapped launch is requested during recovery

- **WHEN** the user explicitly requests an uncapped invocation while the shared checkout is unavailable
- **THEN** the installed bootstrap starts the original CLI with the visible requested exception and preserves other sessions and their CPU budgets

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

#### Scenario: Plugin consumer is independent of development source

- **WHEN** a development checkout changes or becomes unavailable while the released Serena plugin and its prepared dependencies remain intact
- **THEN** ordinary Serena consumers retain the selected package behavior and report no checkout-related degradation
- **AND** an explicit plugin update selects a new verified package for new consumers without changing unrelated source-linked components

