# Linked Global Kit Specification

## Purpose

Make a checkout of codex-harness directly available to Codex CLI as a reusable global configuration and capability source, with reproducible host setup and no deployed copies of repository artifacts.

## Requirements

### Requirement: Complete portable kit source

The repository SHALL own the complete source of its declared global toolkit: the full global AGENTS.md content, shared configuration, managed skills and their referenced resources, managed agent definitions, connection/launch tools and dependency declarations. The host-global AGENTS.md SHALL directly reference that full repository-owned instruction source. A fresh checkout with documented external prerequisites SHALL be sufficient to reproduce the declared kit without retrieving undeclared files from another PC. Connection and managed resource paths SHALL resolve on the current machine. A catalogue entry or dependency declaration SHALL not imply an unimplemented capability is delivered.

#### Scenario: The kit is deployed on a new PC
- **WHEN** a fresh checkout is connected on another Windows account with the documented prerequisites
- **THEN** every declared managed capability and its required resources are available from the checkout or an explicitly declared external dependency, without depending on files from the former PC; saved machine-specific settings may remain in the shared configuration

#### Scenario: The global instruction source is inspected
- **WHEN** the user follows the active global AGENTS.md connection
- **THEN** the complete instruction content is stored under version control in this repository and appears in new sessions through that connection

### Requirement: Direct source consumption

The kit SHALL connect its configuration, portable instructions, skills, agent definitions, MCP connection definitions, language integration definitions and hooks by supported path references or filesystem links. Codex and the corresponding integration consumer SHALL read the repository source when consuming an artifact. The installer MUST NOT copy, render, merge or cache repository artifact contents into deployment files, including as a fallback. Small host-local path registrations and installation metadata SHALL contain references and ownership information rather than duplicated artifact bodies. External packages, language runtimes and generated runtime state SHALL be distinguished from repository-owned configuration and integration source; their installation SHALL not be used to smuggle deployed copies of those artifacts into a package cache.

#### Scenario: A clean host is connected
- **WHEN** the installer successfully connects a valid checkout
- **THEN** every managed artifact resolves to its source in that checkout and no deployed content copy is created

#### Scenario: Links or another required capability are unavailable
- **WHEN** a required direct connection cannot be established
- **THEN** installation fails with the specific cause and a corrective action, without substituting copied artifacts or reporting partial activation as success

#### Scenario: MCP or diagnostic integration source is updated
- **WHEN** a connected MCP definition, language integration or hook source changes and a new consumer starts
- **THEN** the consumer reads the updated checkout source through its existing connection without a generated deployment copy

### Requirement: Reproducible global entry point

On the initial supported Windows native environment, `install.ps1` SHALL discover its checkout independently of the caller's working directory and connect an already installed supported Codex CLI. A new terminal's ordinary `codex` command SHALL select the repository configuration automatically for local session commands, without a manual permissions selection, profile flag, or harness working directory. Explicit invocation overrides SHALL remain effective under documented Codex precedence. The installer SHALL report the effective user directories, CLI version and supported entry point.

#### Scenario: Codex starts in another repository
- **WHEN** a new terminal starts a normal local Codex session in a repository outside the harness
- **THEN** the repository configuration is loaded automatically together with applicable host and project configuration

#### Scenario: The checkout has a different path
- **WHEN** installation runs from a checkout with spaces or non-ASCII characters on another Windows account
- **THEN** it uses that checkout and account's resolved paths without references to the development machine

#### Scenario: The user deliberately overrides a default
- **WHEN** the user supplies an explicit profile or a supported session configuration override
- **THEN** the entry point preserves the user's choice and documents the resulting scope instead of silently forcing the kit default

### Requirement: Shared configuration and local state

The shared configuration SHALL provide `approval_policy = "never"` and `sandbox_mode = "danger-full-access"` as the previously selected Full Access default and SHALL record the declared reusable model, reasoning and tool preferences needed to reproduce the kit. Existing local configuration outside explicitly managed defaults SHALL remain active under native configuration precedence. Authentication, session history, caches and installer state SHALL remain outside tracked repository artifacts. The entire checkout MUST NOT become `CODEX_HOME`. Configuration-management operations SHALL retain native persistence behavior: settings, including model preferences and machine-specific trusted project paths, MAY be written through the linked profile into the repository. Documentation SHALL identify the observed write targets and this accepted shared-file effect. The installer SHALL preserve the existing local base configuration without automatically migrating its contents.

#### Scenario: Existing machine configuration is present
- **WHEN** a host with provider/model preferences, trusted project paths and saved authentication is connected
- **THEN** those unrelated settings and data remain available and the shared Full Access defaults are effective for a new normal session

#### Scenario: A setting is persisted through its native writer
- **WHEN** a supported ordinary Codex operation persists a model preference or machine-specific setting after connection
- **THEN** its native write target is preserved, including the repository profile for profiled TUI model selection and project trust; the direct connection remains intact and authentication, session history, caches and installer state stay outside the repository

### Requirement: Global capability discovery

The kit SHALL expose the existing portable principles, the repository's OpenSpec skills, repository-owned agent definitions and declared MCP/LSP/hook capabilities to sessions outside the harness while preserving applicable project instructions and unrelated user capabilities. Discovery SHALL refer to live source files. An empty agent source directory SHALL be reported accurately; supporting agent loading does not imply delivery of an unrequested agent catalogue. Duplicate discovery through repository and global paths SHALL not result in conflicting copies of the same managed source. Global MCP and automatic diagnostics activation SHALL not rely solely on a CLI-only profile when another supported installed local Codex consumer does not load it; native configuration consumption and explicit overrides SHALL be verified for each applicable entry point.

#### Scenario: Instructions and skills are consumed elsewhere
- **WHEN** Codex starts in an unrelated repository with its own AGENTS.md
- **THEN** the global principles and project instructions are in the initial context and the managed skills are discoverable from the checkout

#### Scenario: An agent definition is connected
- **WHEN** a valid repository-owned agent definition is registered
- **THEN** Codex discovers that definition globally and uses its repository source while unrelated personal agents remain discoverable

#### Scenario: Codex starts within the harness
- **WHEN** global registration and project discovery both reach a managed skill
- **THEN** there is one effective source identity or documented native deduplication, with no separately maintained content or ambiguous conflicting definition

#### Scenario: Global code tools are consumed elsewhere
- **WHEN** a new supported local Codex consumer starts in another project after global activation
- **THEN** the declared MCP/LSP capabilities and automatic edit diagnostics are available without project-local kit configuration, together with preserved unrelated capabilities

### Requirement: Source updates and checkout availability

A new Codex process SHALL consume edited contents of already connected source files without a second install or content synchronization. Adding or removing separately registered artifacts SHALL be reconciled by rerunning installation without copying. Checkout relocation SHALL support reconnection. Verification SHALL identify a moved, deleted or inaccessible source and MUST NOT treat a dangling connection as healthy. Already running sessions are not required to reload their initial context.

#### Scenario: A connected file changes
- **WHEN** a shared setting, principle, skill body or agent definition is edited in the checkout and a new process starts
- **THEN** Codex observes the new source content without a reinstall

#### Scenario: The registered set changes
- **WHEN** source artifacts are added or removed and installation is rerun
- **THEN** the live registrations are reconciled, removing only obsolete kit-owned registrations and preserving unrelated capabilities

#### Scenario: The checkout is moved
- **WHEN** verification encounters the old location and installation is subsequently run from the new checkout
- **THEN** verification reports the stale source and reconnection updates only kit-owned references to the new location

### Requirement: Bounded and repeatable installation

The installer SHALL provide a non-mutating preview and validate prerequisites, source files, target ownership and path boundaries before activation. Repeating a successful installation SHALL not duplicate links, configuration selection or path registration. An existing connection to the same source SHALL be recognized; a conflicting foreign file, link, capability name or higher-priority global instruction override SHALL be reported without replacement or concealment. Existing unrelated global capabilities SHALL remain usable.

#### Scenario: A compatible installation already exists
- **WHEN** installation runs again against the same checkout and host
- **THEN** it recognizes the current direct connections and leaves them effective without duplicate registrations

#### Scenario: A target is owned by another source
- **WHEN** preview or installation encounters a conflicting file, link or instruction override
- **THEN** it identifies the conflict and required resolution without overwriting or shadowing the existing content

### Requirement: Failure recovery and disconnect

The installer SHALL keep enough host-local ownership information to undo its own registrations. On a failed activation it SHALL restore the pre-activation connection state or report exactly which changes remain and how to recover. Disconnect SHALL remove only registrations still owned by this installation, preserve source files and unrelated local state, and leave the original Codex command usable. It MUST NOT recursively delete a source through a filesystem link.

#### Scenario: Activation fails after one mutation
- **WHEN** a later activation step fails
- **THEN** the completed mutations are rolled back and the original cause remains visible, with any rollback failure and outstanding change explicitly reported

#### Scenario: A managed target was replaced externally
- **WHEN** disconnect finds that a recorded destination now belongs to another file or link
- **THEN** it preserves that destination and reports the ownership mismatch

#### Scenario: The kit is disconnected
- **WHEN** disconnect succeeds
- **THEN** the original CLI and unrelated configuration/capabilities remain usable, kit-owned registrations are removed, and checkout/authentication/session files remain intact

### Requirement: Evidence from the actual consumer

Acceptance SHALL exercise the installed CLI entry point in both a neutral directory and a separate repository with project instructions. Checks SHALL prove effective shared and local configuration, live source updates, skill discovery, representative agent consumption, coexistence and recovery scenarios. File hashes, mocks or the existence of links alone SHALL not count as consumer evidence. Verification SHALL distinguish actual global activation from isolated tests and SHALL leave no test-only capability active.

#### Scenario: The installer reports successful activation
- **WHEN** activation is declared verified
- **THEN** evidence identifies the CLI version, effective source paths, representative observed behavior and tested limitations

#### Scenario: A disposable agent is used to verify loading
- **WHEN** a representative agent is exercised because no production role exists yet
- **THEN** the report identifies it as a fixture and removes its test registration without claiming a delivered production agent catalogue
