## Purpose

Provide reproducible discovery, reuse, installation and compatible updates of shared MCP and language-server dependencies while preserving existing consumers, data and recovery paths.

## ADDED Requirements

### Requirement: Existing dependency discovery and reuse

The kit SHALL discover existing installations before proposing installation or update. Its inventory SHALL identify the package or executable, actual version, resolved path, installation manager or owner, active consumer and health evidence. It SHALL reuse a working compatible installation and its applicable language-server cache instead of creating another permanent installation solely for Codex. Ambiguous command names and locally modified installations MUST NOT be selected or replaced silently.

#### Scenario: Tools are already installed for OpenCode
- **WHEN** discovery finds the existing Serena, Codebase Memory, graphifyy, Nuphus and language servers
- **THEN** the proposed Codex connection uses those resolved dependencies and identifies which require an update or repair

#### Scenario: Two packages expose the same command name
- **WHEN** both graphifyy and an unrelated package expose `graphify` or `graphify-mcp`
- **THEN** selection verifies the intended distribution and uses its resolved executable rather than accepting the first same-named command

#### Scenario: A dependency contains local modifications
- **WHEN** discovery detects a modified installation or cannot establish safe ownership for an update
- **THEN** it reports the uncertainty and preserves the installation until a compatible preservation or migration path is established

### Requirement: Current and compatible updates

An explicit kit install/update operation SHALL check official release or package metadata for newer stable versions of selected MCP/LSP dependencies, recording the source and check date. It SHALL update the existing shared installation when compatibility is established. An incompatible newer release SHALL produce an explicit retained-version result with evidence and a corrective action; silently skipping an available update or downgrading a newer compatible installation is prohibited. Runtime session startup and file diagnostics MUST NOT perform package upgrades. Version pinning SHALL record its reason and be reconsidered by the next explicit update operation.

#### Scenario: A compatible newer stable release exists
- **WHEN** an update finds a newer release and the relevant integration checks pass
- **THEN** the common installation becomes the selected version for Codex and existing consumers, without creating a separate Codex installation

#### Scenario: A newer release breaks a required operation
- **WHEN** validation finds a regression or an unsupported contract in the newer release
- **THEN** the working version remains or is restored, the attempted update is reported as unsuccessful, and the incompatibility is recorded without claiming the newer version works

#### Scenario: The user starts a normal session
- **WHEN** a session starts or requests diagnostics
- **THEN** it uses the selected installed versions without modifying package installations or project dependency lockfiles

### Requirement: Reproducible provisioning of missing dependencies

The kit SHALL declare the acquisition source, version policy, runtime prerequisites, language mapping and verification method for every selected dependency. Missing MCPs and required language servers SHALL be provisionable through the kit lifecycle on the supported environment. Already satisfied prerequisites SHALL be reused. User-owned licensed SDKs or unavailable runtimes SHALL be reported as specific prerequisites rather than replaced, downloaded from an unofficial source or treated as successful support. A new host MUST NOT require an undeclared file or neighboring opencode-kit checkout from the development machine.

#### Scenario: A clean host has only the documented bootstrap prerequisites
- **WHEN** the kit provisions its selected tools from a fresh checkout
- **THEN** it obtains missing distributable dependencies from declared sources, resolves current-host paths and verifies their integration

#### Scenario: A language needs a separately supplied SDK
- **WHEN** its required SDK is unavailable
- **THEN** the language is reported as unavailable with the exact prerequisite and remains unverified until that prerequisite and its checks are satisfied

### Requirement: Non-mutating preview and repeatability

Preview SHALL show reuse, downloads, updates, affected consumers, connection changes and rollback implications without changing files, packages, services, credentials or persistent environment values. Repeating successful install/update with unchanged inputs SHALL not duplicate registrations, persistent server installations or services. A network failure SHALL be distinguished from a result that no update exists.

#### Scenario: The user previews an update
- **WHEN** preview runs against an existing host
- **THEN** it reports the intended changes and conflicts while the host and checkout retain their prior state

#### Scenario: Release metadata is unavailable
- **WHEN** the official metadata request fails
- **THEN** the report identifies the failed check and preserves the known working installation without claiming that it is current

### Requirement: Shared consumer preservation and recovery

Before changing a shared dependency, the kit SHALL account for active consumers, local configuration, caches and data formats. It SHALL prevent concurrent conflicting updates and preserve a recoverable previous version and any state affected by migration. Temporary staging or rollback versions SHALL be identified separately from active installations. An update MUST NOT force-stop unrelated sessions, erase indexes or overwrite another consumer's configuration. A failed activation SHALL restore the prior usable connection and dependency state, or report the exact unrecovered changes without reporting success.

#### Scenario: OpenCode is using a dependency selected for update
- **WHEN** replacing it would interrupt that consumer or violate a recorded installation identity
- **THEN** the update is safely coordinated or reported as pending until the consumer is quiescent, and existing connection/ownership metadata stays consistent with the actual selected version

#### Scenario: Validation fails after package replacement
- **WHEN** the new version fails an MCP or LSP integration check
- **THEN** the previous usable version and relevant data/configuration are restored and the original failure remains visible

#### Scenario: Another updater is already active
- **WHEN** a second operation targets the same shared installation
- **THEN** the operations do not modify it concurrently and the second operation reports the contention

### Requirement: Check and disconnect respect dependency ownership

Check SHALL distinguish installed, connected, callable, current, degraded and unverified states. Disconnect SHALL remove only kit-owned connections and runtime resources, preserving shared packages, existing OpenCode integration, user data and pre-existing services. A rollback or cleanup MUST NOT traverse a filesystem link into a source checkout or delete an adopted cache.

#### Scenario: A configured server cannot initialize
- **WHEN** Check sees a configuration entry but the actual MCP handshake or required call fails
- **THEN** it reports the server as unhealthy with the cause instead of treating registration as working support

#### Scenario: Codex integration is disconnected
- **WHEN** disconnect completes
- **THEN** kit-owned activation is removed while adopted tools and their other consumers remain usable
