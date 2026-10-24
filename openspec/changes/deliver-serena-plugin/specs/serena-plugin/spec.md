# Spec Delta

## Purpose

Let users install, prepare and use the accepted Serena language tools through a versioned Codex plugin without installing or building the harness core or retaining its source checkout.

## ADDED Requirements

### Requirement: Independent versioned consumer package

The Serena plugin SHALL install through the existing Codex marketplace mechanism on supported Windows x64 hosts. It SHALL include its prebuilt first-party runtime, required portable data and usage guidance. Ordinary consumers MUST NOT need a harness checkout, harness core/launcher, harness compilation, symlink privileges or manual MCP configuration. Package provenance, supported host versions and external prerequisites SHALL be explicit.

#### Scenario: Install on another PC
- **WHEN** a user installs the released plugin into a supported native Codex with the documented plugin-host prerequisites, without an installed harness or its source
- **THEN** the plugin and its native preparation entry point are available without Rust/MSVC being installed solely to build harness
- **AND** no development-machine path, credential, cache or undeclared file is needed

#### Scenario: Development checkout changes or disappears
- **WHEN** an ordinary installed plugin is used after an unrelated development checkout is edited, moved or removed
- **THEN** its selected package remains usable and its health does not depend on that checkout

#### Scenario: Host version is unsupported
- **WHEN** installation or preparation encounters an unsupported Codex or platform contract
- **THEN** it identifies the incompatible capability and required host action before activation, preserves existing connections, and does not substitute a manual harness installation as successful plugin delivery

### Requirement: Preparation remains accessible before dependencies exist

The installed native entry point SHALL provide dependency status and explicit preparation without requiring Serena, Python or a language backend to start first. Missing semantic dependencies SHALL produce an actionable setup-required result rather than an unexplained transport failure. The plugin SHALL resolve its own executable and resource paths for the user and agent.

#### Scenario: First use lacks Python and Serena
- **WHEN** the plugin is installed but its semantic dependencies are absent
- **THEN** the user can request preparation through the plugin's supplied guidance, receive the missing dependency state and start the supported preparation operation without separately reconstructing paths or installing its control runtime

#### Scenario: Preparation has completed
- **WHEN** dependencies pass the required checks
- **THEN** the next supported native catalogue refresh or new session exposes the ready semantic tools
- **AND** any required session restart is stated explicitly, without claiming current-session discovery before it is observed

### Requirement: Explicit dependency preparation reuses existing owners

One explicit setup/update operation SHALL perform supported discovery, preparation and verification of the selected distributable dependencies. It SHALL reuse verified compatible installations and preserve shared-consumer ownership. Preview and Check SHALL be non-mutating; ordinary session startup and semantic requests MUST NOT download, build, install or upgrade packages. Setup SHALL not require users to execute a sequence of package-manager commands manually.

#### Scenario: Supported distributable dependencies are missing
- **WHEN** explicit preparation runs with network access and only the documented plugin-host prerequisites
- **THEN** it obtains the required plugin-runtime prerequisites and selected provisionable backends from declared sources, verifies them and reports each resulting dependency state
- **AND** prerequisite runtimes are either supplied in the package or prepared by that operation, without a hidden manual bootstrap chain

#### Scenario: Compatible shared installations exist
- **WHEN** discovery finds a compatible verified dependency already used by another consumer
- **THEN** preparation reuses it without creating a redundant permanent installation, changing that consumer's settings or terminating its sessions

#### Scenario: Preparation is interrupted or acquisition fails
- **WHEN** setup cannot finish because of interruption, network failure, compatibility failure or conflicting ownership
- **THEN** it retains useful staged work and recoverable prior state, identifies the failed stage and provides an explicit repeat/recovery action
- **AND** neither a running process nor partial installation is reported as ready

#### Scenario: Repeat and read-only use
- **WHEN** successful preparation is repeated with unchanged inputs, or Check or an ordinary session runs
- **THEN** no duplicate registrations, permanent dependency installations or services are created
- **AND** Check and ordinary startup perform no provisioning or upgrades

### Requirement: Existing semantic behavior and language scope survive packaging

The plugin SHALL preserve the accepted Serena operations, exclusions, diagnostic uncertainty and language selection in `global-code-tools`. Rust and Python remain retained; C++, C# and Pascal/Delphi remain reuse-only conditional capabilities. Project toolchains, licensed SDKs, compilation databases and project-specific inputs remain project responsibilities. Packaging SHALL not enable automatic diagnostics, add languages or treat installation as semantic readiness.

#### Scenario: Ordinary Rust or Python project
- **WHEN** a prepared plugin is used in a normally configured project with the applicable project prerequisites
- **THEN** the agent can perform the retained symbol, definition, reference and semantic edit operations through the installed plugin without harness-specific per-project setup
- **AND** explicit diagnostics retain their completeness/freshness limits and project-native checks remain necessary

#### Scenario: Conditional language has a usable existing backend
- **WHEN** the existing accepted C++, C# or Pascal dependency and project-input conditions are satisfied
- **THEN** the plugin preserves that language's retained operations and limitations through the same adoption policy

#### Scenario: Conditional backend or project input is missing
- **WHEN** an optional backend, SDK, compiler information or required project input is absent or ambiguous
- **THEN** the affected language remains explicitly unavailable with the exact reason and corrective action, without blocking unrelated ready languages or downloading reuse-only components

### Requirement: Project and process lifecycle remains isolated

The plugin SHALL preserve canonical project/worktree binding, compatible service reuse, resource bounds, request identities, cancellation and bounded recovery under the existing `global-code-tools` and `bounded-tool-resources` contracts. One client's project change or closure MUST NOT retarget or stop another client's work. Connection lifetime SHALL follow the client without a fixed session expiry or background package maintenance.

#### Scenario: Two projects remain open
- **WHEN** two native Codex sessions use the installed plugin against different projects and one closes or changes its active project
- **THEN** the other continues receiving its own project's results, with existing resource-admission behavior and no cross-project edits

#### Scenario: Client returns after worker retirement
- **WHEN** an open client requests an operation after its idle worker retired or an eligible transient transport failure occurred
- **THEN** bounded recovery restores the selected project and request identity under the existing retry policy, without replaying an indeterminate mutation

### Requirement: Readiness distinguishes package presence from useful operation

Status SHALL distinguish package installation, dependency preparation, MCP availability and verified language operations. A failure SHALL retain its stage, cause and next supported action. A plugin listing, successful handshake or empty diagnostic result MUST NOT establish semantic readiness. Verification SHALL use owned mutable targets and identify its actual package/backend versions and coverage.

#### Scenario: Backend starts but semantic requests fail
- **WHEN** MCP initializes but a required symbol or edit check fails
- **THEN** the affected operation remains failed or unverified, the original error is available, and setup/delivery does not report complete readiness

#### Scenario: User checks an existing installation
- **WHEN** read-only Check inspects the installed package and selected dependencies
- **THEN** it reports the observed state and applicable limits without changing packages, project files, registrations or native settings

### Requirement: Native package lifecycle preserves retained state and sessions

Codex's supported plugin lifecycle SHALL own package installation, version selection, disablement and removal. Writable dependency, ownership and recovery state SHALL live outside replaceable package contents. An update SHALL retain the runtime/resources needed by open sessions and activate the selected version for new sessions. Failed dependency activation SHALL preserve or restore the previous usable state and report any unrecovered effects.

#### Scenario: Update while a project session is open
- **WHEN** the user explicitly updates the plugin and an older session later needs to restart its broker
- **THEN** the older session retains its verified runtime generation and project context while a fresh session uses the selected new version
- **AND** no executable or required resource still owned by that older session is overwritten or deleted

#### Scenario: Native package removal
- **WHEN** the user disables or removes the plugin through the supported native mechanism
- **THEN** new consumers stop receiving its tools according to the native lifecycle, projects and adopted dependencies remain intact, and active owned resources finish or recover without stopping unrelated clients
- **AND** retained local state is not misrepresented as an enabled installation

### Requirement: Legacy Serena migration has one recoverable owner

Migration SHALL identify and replace only the existing harness-owned Serena connection after the plugin candidate is verified. Conflicting or foreign connections SHALL be preserved and reported. An interrupted cutover SHALL remain recoverable without simultaneous competing managed Serena providers. Nuphus, executors, other components, credentials, local preferences and shared dependencies SHALL retain their current ownership and behavior.

#### Scenario: Existing full harness installation migrates
- **WHEN** a verified plugin replaces the recorded legacy Serena connection
- **THEN** a fresh consumer sees one selected Serena provider, Nuphus remains on its existing connection, and unrelated settings and packages are preserved
- **AND** subsequent supported core/code-tools lifecycle operations do not recreate the superseded Serena registration

#### Scenario: Cutover is interrupted or a foreign edit is found
- **WHEN** migration stops between preparation and activation, or ownership no longer matches
- **THEN** it reports the actual active state and a bounded recovery action, preserves the previous usable route or its recovery inputs, and never overwrites the foreign change

### Requirement: Independent delivery is accepted through actual consumers

Acceptance SHALL cover native plugin installation and use outside the harness checkout with no core/launcher dependency, including a clean host boundary, repeated preparation, update, interruption/recovery and removal. It SHALL retain existing real Serena semantic and lifecycle checks. Model-free protocol tests and fixtures SHALL supplement, not replace, fresh native Codex discovery and actual tool use; unexercised environments remain explicitly unverified.

#### Scenario: First usable independent installation
- **WHEN** a released candidate is installed into a supported clean Windows environment without harness source/core or compiler prerequisites solely for harness
- **THEN** its initial preparation succeeds and real Rust/Python navigation, cross-file references and an owned semantic edit/readback work through the installed native consumer with the applicable project prerequisites

#### Scenario: Existing and conditional consumers remain supported
- **WHEN** migration and independent delivery are accepted
- **THEN** checks exercise concurrent project isolation, preserved open-session generations, retained failure recovery and the adopted conditional backends where available
- **AND** missing mandatory evidence keeps the corresponding task open rather than weakening the support contract or claiming a fixture proves a real consumer
