# Spec Delta

## MODIFIED Requirements

### Requirement: Two globally available MCP integrations

Serena and Nuphus SHALL be the managed global capabilities, subject to
applicable user overrides. CodeGraph, Graphify and Codebase Memory SHALL be
fully retired: no managed selection, registration, first-party adapter or CLI
route survives in the kit, while an update removes any owned registration
recorded by an earlier version and shared packages, saved graphs and caches
remain inert host residue outside the kit's ownership. Retained integrations SHALL be
discoverable without per-project configuration, manual startup or a special
working directory in supported interactive, non-interactive, resumed/forked
and subagent consumers. Desktop/IDE coverage SHALL continue to require
actual configuration-consumer evidence rather than a CLI-only claim.
Existing processes needing registration reload SHALL be identified
explicitly.

Serena SHALL be delivered independently through the versioned plugin specified by `serena-plugin`; Nuphus SHALL retain its existing lifecycle. A Serena-only installation MUST NOT require or provision Nuphus or harness core. Discovery guarantees apply to the capabilities selected, installed and prepared for that consumer; unavailable preparation SHALL be explicit rather than reported as semantic readiness.

#### Scenario: Codex starts in an unrelated project

- **WHEN** Codex starts outside the checkout with both retained integrations selected and prepared
- **THEN** Serena and Nuphus are available through their respective installed delivery owners, and retired Graphify, Codebase Memory and CodeGraph registrations do not reappear

#### Scenario: Update runs over a pre-retirement installation

- **WHEN** an owned `codegraph` registration exists from an earlier version
- **THEN** Install/Update removes only that owned registration, preserves unrelated registrations and the shared package/state, and reports the retirement

#### Scenario: Several Codex CLI projects are open together

- **WHEN** separate Codex CLI sessions start in different projects through the installed global configuration
- **THEN** every session gets usable Serena operations for its own project context, without extra per-project setup or selecting one globally exclusive project; bounded resource contention is explicit

#### Scenario: A retired integration is still needed locally

- **WHEN** leftover shared packages, saved graphs, caches or account state from Graphify, Codebase Memory or CodeGraph exist on the host
- **THEN** the kit neither reads, restores nor re-registers them, and the managed selection reports only Serena and Nuphus

#### Scenario: A different session entry point is used

- **WHEN** non-interactive, resumed/forked, tool-capable child or supported local desktop/IDE consumers load the same host settings
- **THEN** they discover the accepted global selection with the applicable project context, preserve intentional native overrides, and do not restore retired registrations

#### Scenario: Activation fails

- **WHEN** a required resource, retrieval, coverage or lifecycle check fails
- **THEN** the previous managed configuration remains recoverable, the failure is not declared active, and the missing behavior remains open

#### Scenario: Serena is installed independently

- **WHEN** a consumer installs and prepares only the Serena plugin in native Codex without harness core
- **THEN** Serena is available in unrelated projects without requiring the Nuphus connection, a harness launcher or a harness checkout
- **AND** an existing Nuphus installation remains unchanged

### Requirement: Serena and Nuphus MCP start with Codex CLI

Owned Serena and Nuphus SHALL become ready during Codex CLI startup through
the installed registrations. The registered command MUST remain able to
complete MCP initialize for those servers when its native binaries still
match their recorded hashes, even if the linked checkout later differs from
that build. A manager that is admitted only for integrity-checked
management MUST NOT be the Codex MCP command for Serena or Nuphus unless it
is also admitted for serving. Source-consuming MCP, including
`mcp codebase-memory`, MUST remain gated on a healthy source match.
Ordinary Codex startup MUST NOT download packages, rebuild native source,
alter agent instructions, install hooks or enable telemetry. Unrelated MCP
registrations, user edits and the adopted Serena and audited Nuphus
packages SHALL be preserved. An unavailable Serena or Nuphus process SHALL
remain explicit and MUST NOT disable the other retained MCP server.

For plugin-delivered Serena, the registered command and required portable resources SHALL resolve from the selected verified plugin delivery, independently of harness core and checkout freshness. Before preparation, the native plugin entry point SHALL remain usable for setup/status and report that semantic tools are not ready. The preserved startup guarantees apply to Nuphus through its existing registration.

#### Scenario: Codex CLI starts after later source edits

- **WHEN** a consumer starts Codex CLI through the installed launcher after the linked checkout has changed relative to the recorded native build, and the recorded binaries still match
- **THEN** Serena and Nuphus complete MCP initialize and are present in the session catalogue, without a rebuild or extra per-session setup

#### Scenario: Interactive, non-interactive and child sessions share the same host settings

- **WHEN** interactive, non-interactive, resumed/forked or tool-capable child consumers load the same installed host configuration
- **THEN** each session gets usable Serena and Nuphus MCP connections, or an explicit startup error for the failed server that does not omit the other retained servers

#### Scenario: Source-consuming MCP stays gated

- **WHEN** the same source-stale manager with matching hashes is asked to serve `mcp codebase-memory`
- **THEN** that command is refused, Serena and Nuphus remain callable, and ordinary Codex startup does not rebuild native source

#### Scenario: Plugin startup without a harness checkout

- **WHEN** native Codex starts with a prepared Serena plugin and no installed harness core or source checkout
- **THEN** Serena initializes from its selected package with no build, download or per-session configuration
- **AND** an unprepared installation reports its preparation action instead of pretending semantic tools are ready

### Requirement: MCP registrations resolve the delivered manager

For servers still delivered by harness core, managed Codex MCP registrations SHALL name the
stable manager link under the installation home rather than a single frozen
build path, so a new Codex CLI session resolves the currently delivered
manager without rewriting the registration. The recorded command MUST resolve
into an integrity-verified build when the registration is written, and a
scoped code-tools operation MUST NOT pin an older manager for later sessions.
Unrelated registrations, adopted packages and running sessions SHALL remain
unchanged.

Plugin-delivered Serena SHALL instead resolve the runtime and portable resources of the version selected by native Codex plugin installation. It MUST NOT depend on the core manager link, a development checkout or a global `codex-harness` PATH entry. The plugin lifecycle SHALL preserve integrity, unrelated registrations, adopted packages and open-session generations. A later core or scoped code-tools operation MUST NOT recreate a migrated legacy Serena registration.

#### Scenario: New session after a manager delivery

- **WHEN** a newer verified core manager build has been delivered and a new Codex CLI session loads the existing core-owned MCP registrations
- **THEN** its core-delivered MCP servers start from the fresh manager through the stable link, with no registration rewrite required
- **AND** independently installed Serena continues to follow its native plugin version

#### Scenario: Scoped update from the previous manager

- **WHEN** a code-tools Install/Update runs from the previous manager
- **THEN** any retained core-owned command remains the stable link and the next session resolves the delivered build instead of the older one
- **AND** a migrated Serena connection is not duplicated or switched away from its selected plugin

#### Scenario: Plugin update selects a new package

- **WHEN** a verified Serena plugin version is selected by native Codex and a fresh session starts
- **THEN** its Serena connection uses that package's verified runtime and resources without a core install, source checkout or global manager path

### Requirement: Open Serena sessions retain their manager generation

An open managed Serena session SHALL retain a consistent manager generation for broker identity and launch after an installation update. A subsequent broker restart SHALL preserve the session's selected project and response identity without launching a different build through a changed installation link. Existing integrity, ownership and bounded recovery checks SHALL remain effective.

The same guarantee SHALL apply to plugin updates and removal: package-cache replacement MUST NOT invalidate the verified runtime or portable resources an open session needs to serve requests or restart its broker. New sessions SHALL follow native plugin selection while old sessions retain only the resources required by their owned generation.

#### Scenario: Installed manager link changes before broker restart

- **WHEN** the installed manager link changes while a Serena session remains open and its broker later exits
- **THEN** the next symbol query returns the selected project's symbol body with its original request id using the session's retained manager generation

#### Scenario: New session after delivery

- **WHEN** a new Serena session starts through the updated installation link
- **THEN** it uses the newly installed manager generation without replacing the older session's live broker

#### Scenario: Plugin cache changes before broker restart

- **WHEN** native plugin update or removal changes the package cache while an existing Serena session still owns its runtime generation and its broker later exits
- **THEN** the session can recover under the existing bounded policy using its retained verified generation and project context, without selecting an unrelated binary or preventing native package removal

