## MODIFIED Requirements

### Requirement: Three globally available MCP integrations

Serena, CodeGraph from `colbymchenry/codegraph`, and Nuphus SHALL be the
managed global capabilities, subject to applicable user overrides. Graphify
SHALL remain retired from the managed selection the same way Codebase Memory
was: an update removes only the owned registration, while the shared package
and saved state remain installed and usable through an explicit local route
without per-project configuration. The accepted subscription-efficiency
selection SHALL still be allowed to disable an LSP-bearing integration when
its language functions fail the benefit gate, including Serena; unrelated
useful MCP functions and external installations SHALL be preserved. Retained
integrations SHALL be discoverable without per-project configuration, manual
startup or a special working directory in supported interactive,
non-interactive, resumed/forked and subagent consumers. Desktop/IDE coverage
SHALL require actual configuration-consumer evidence rather than a CLI-only
claim. Existing processes needing registration reload SHALL be identified
explicitly.

#### Scenario: Codex starts in an unrelated project
- **WHEN** Codex starts outside the checkout after this change is active
- **THEN** Serena, CodeGraph and Nuphus are available, CodeGraph selects the intended root, and retired Graphify and Codebase Memory registrations do not reappear

#### Scenario: Several Codex CLI projects are open together
- **WHEN** separate Codex CLI sessions start in different indexed projects through the installed global configuration
- **THEN** every session has usable CodeGraph operations and automatic refresh for its own root, without extra per-project setup or selecting one globally exclusive project; bounded resource contention is explicit

#### Scenario: A retired integration is still needed locally
- **WHEN** the user deliberately wants Graphify or Codebase Memory for a specific task
- **THEN** the shared package and saved state remain usable through an explicit local route, and the managed selection does not silently restore the registration

#### Scenario: A different session entry point is used
- **WHEN** non-interactive, resumed/forked, tool-capable child or supported local desktop/IDE consumers load the same host settings
- **THEN** they discover the accepted global selection with the applicable project context, preserve intentional native overrides, and do not restore retired registrations

#### Scenario: Activation fails
- **WHEN** a required resource, retrieval, coverage or lifecycle check fails
- **THEN** the previous managed configuration remains recoverable, the failure is not declared active, and the missing behavior remains open

### Requirement: CodeGraph MCP starts with Codex CLI

Owned CodeGraph SHALL become ready during Codex CLI startup through the
installed registration, together with the other retained MCP servers. The
registered command MUST remain able to complete MCP initialize when its native
binaries still match their recorded hashes, even if the linked checkout later
differs from that build. A manager that is admitted only for integrity-checked
management MUST NOT be the Codex MCP command unless it is also admitted for
serving. Ordinary Codex startup MUST NOT download packages, rebuild native
source, alter agent instructions, install hooks or enable telemetry. Unrelated
MCP registrations, user edits, published CodeGraph/Node and existing indexes
SHALL be preserved. An unavailable CodeGraph process SHALL remain explicit and
MUST NOT disable the other retained MCP servers.

#### Scenario: Codex CLI starts after later source edits
- **WHEN** a consumer starts Codex CLI through the installed launcher after the linked checkout has changed relative to the recorded native build, and the recorded CodeGraph binaries still match
- **THEN** CodeGraph completes MCP initialize and is present in the session catalogue with the other retained MCP servers, without a rebuild or extra per-session setup

#### Scenario: Interactive, non-interactive and child sessions share the same host settings
- **WHEN** interactive, non-interactive, resumed/forked or tool-capable child consumers load the same installed host configuration
- **THEN** each session gets a usable CodeGraph MCP connection for its current project root, or an explicit CodeGraph startup error that does not omit the other retained servers

#### Scenario: Native binaries no longer match the recorded build
- **WHEN** the registered CodeGraph command is missing, hash-mismatched or otherwise not admitted for serving
- **THEN** Codex CLI still starts, CodeGraph is omitted with an explicit startup failure, and Serena and Nuphus remain available

## REMOVED Requirements

### Requirement: Graphify lifecycle and repository selection

**Reason**: Graphify is retired from the managed selection; the kit no longer
provides a managed Graphify connection, shared-endpoint reuse or
repository-scoped operations, and the first-party adapter sources are removed.
The remaining behavior is the retirement guard that removes an owned
registration recorded by pre-retirement versions.

**Migration**: Users who need Graphify keep using their own local installation,
saved graphs and explicit local route outside the kit; kit updates remove only
the owned registration and preserve unrelated installations, saved graphs and
other user configuration.
