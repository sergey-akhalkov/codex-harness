## MODIFIED Requirements

### Requirement: Direct source consumption

The kit SHALL connect its configuration, portable instructions, skills, agent definitions, MCP connection definitions, language integration definitions and hooks by supported path references or filesystem links. Codex and the corresponding integration consumer SHALL read the repository source when consuming those data artifacts. The installer MUST NOT copy, render, merge or cache their contents into deployment files, including as a fallback. Small host-local path registrations and installation metadata SHALL contain references and ownership information rather than duplicated artifact bodies. Compiled first-party executables SHALL be distinguished from source-owned data: installation SHALL connect verified build artifacts with recorded source/build identity, while their complete source remains in the checkout. This executable-artifact allowance MUST NOT permit embedding deployed copies of source-owned configuration, instructions or skills. External packages, language runtimes and generated runtime state SHALL remain distinct from repository-owned configuration and integration source; their installation SHALL not be used to smuggle deployed copies of those artifacts into a package cache.

#### Scenario: A clean host is connected
- **WHEN** the installer successfully connects a valid checkout
- **THEN** every managed data artifact resolves to its source in that checkout without a deployed content copy, and every managed compiled executable resolves to a verified build identified with that checkout's relevant sources

#### Scenario: Links or another required capability are unavailable
- **WHEN** a required direct connection cannot be established
- **THEN** installation fails with the specific cause and a corrective action, without substituting copied artifacts or reporting partial activation as success

#### Scenario: MCP or diagnostic integration source is updated
- **WHEN** a connected MCP definition, language integration definition or hook definition changes and a new consumer starts
- **THEN** the consumer reads the updated checkout data through its existing connection without a generated deployment copy, while a changed compiled implementation follows the verified build/update contract

### Requirement: Reproducible global entry point

On the initial supported Windows native environment, `codex-harness.exe` SHALL provide native install, update, check, recover and disconnect operations and resolve its intended checkout from explicit source selection or verified registration independently of the caller's working directory. A documented Rust-native bootstrap with declared prerequisites SHALL connect an already installed supported Codex CLI. A new terminal's ordinary `codex` command SHALL select the repository configuration automatically for local session commands, without a manual permissions selection, profile flag, or harness working directory. Explicit invocation overrides SHALL remain effective under documented Codex precedence. The installer SHALL report the effective user directories, CLI version and supported native entry point. Upgrade SHALL migrate existing owned script-based connections and document equivalent native commands without requiring manual recreation of the kit.

#### Scenario: Codex starts in another repository
- **WHEN** a new terminal starts a normal local Codex session in a repository outside the harness
- **THEN** the repository configuration is loaded automatically together with applicable host and project configuration

#### Scenario: The checkout has a different path
- **WHEN** installation runs with a selected checkout containing spaces or non-ASCII characters on another Windows account
- **THEN** it uses that checkout and account's resolved paths without references to the development machine

#### Scenario: The user deliberately overrides a default
- **WHEN** the user supplies an explicit profile or a supported session configuration override
- **THEN** the entry point preserves the user's choice and documents the resulting scope instead of silently forcing the kit default

#### Scenario: Existing script installation is upgraded
- **WHEN** the native installer upgrades an owned working installation created by `install.ps1`
- **THEN** native command connections replace its supported script entry points, ordinary `codex` remains available, and existing foreign state and recovery information are preserved

### Requirement: Source updates and checkout availability

A new Codex process SHALL consume edited contents of already connected source-owned data files without a second install or content synchronization. Changed first-party executable source SHALL require an explicit verified build/update before the affected native executable runs; stale, missing or altered builds SHALL be reported with the corrective action. Ordinary consumer startup MUST NOT invoke Cargo, compile or download dependencies. Adding or removing separately registered artifacts SHALL be reconciled by rerunning native installation without copying data artifacts. Checkout relocation SHALL support reconnection and build-identity reconciliation. Verification SHALL identify a moved, deleted or inaccessible source and MUST NOT treat a dangling connection or source-stale binary as healthy. Already running sessions are not required to reload their initial context.

These harness health checks SHALL NOT block ordinary upstream Codex CLI launch. An installed command bootstrap independent of checkout availability SHALL preserve access to the original CLI when harness source, shared configuration or its required build is unavailable. It SHALL report the degraded harness and use native arguments/local settings without running unverified harness extensions, rebuilding, downloading or rewriting user configuration. Missing upstream Codex itself and recursive command discovery SHALL remain explicit failures, not reasons to invoke an arbitrary replacement.

#### Scenario: A connected file changes
- **WHEN** a shared setting, principle, skill body or agent definition is edited in the checkout and a new process starts
- **THEN** Codex observes the new source content without a reinstall

#### Scenario: The registered set changes
- **WHEN** source artifacts are added or removed and installation is rerun
- **THEN** the live registrations are reconciled, removing only obsolete kit-owned registrations and preserving unrelated capabilities

#### Scenario: The checkout is moved
- **WHEN** verification encounters the old location and installation is subsequently run with the new checkout
- **THEN** verification reports the stale source and reconnection updates only kit-owned references and the applicable build identity to the new location

#### Scenario: Native implementation changes
- **WHEN** relevant Rust source or locked build inputs change after installation
- **THEN** Check and the affected invocation report the outdated build without automatic compilation, and successful explicit update activates a verified candidate with recoverable prior state

#### Scenario: Harness is unavailable during an ordinary Codex launch
- **WHEN** the installed launcher encounters stale or missing shared build inputs, an unavailable checkout/module, or missing or malformed harness registration while the original CLI remains installed
- **THEN** ordinary Codex launches once with the user's native arguments, local settings, cwd and streams preserved, reports degraded harness behavior, and does not require a successful harness update first

#### Scenario: A running process uses the previous build
- **WHEN** an explicit update validates a new native build while another session still uses the old executable
- **THEN** activation does not overwrite that running artifact and cleanup preserves it until owned use can safely end

#### Scenario: Stale manager needs to update itself
- **WHEN** source freshness differs but the last accepted manager still passes binary-integrity and metadata-compatibility checks
- **THEN** Check and explicit update/recover/disconnect remain usable without starting the obsolete ordinary runtime; an unavailable or altered manager instead requires the documented native bootstrap recovery path
