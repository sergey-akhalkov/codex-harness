## ADDED Requirements

### Requirement: Three globally available MCP integrations

Serena, CodeGraph from `colbymchenry/codegraph`, and Nuphus SHALL be the
managed global capabilities, subject to applicable user overrides. Graphify
SHALL be retired from the managed selection the same way Codebase Memory was:
an update removes only the owned registration, while shared packages, saved
graphs and explicit local rollback routes remain installed and usable without
per-project configuration. The accepted subscription-efficiency selection SHALL
still be allowed to disable an LSP-bearing integration when its language
functions fail the benefit gate, including Serena; unrelated useful MCP
functions and external installations SHALL be preserved. Retained integrations
SHALL be discoverable without per-project configuration, manual startup or a
special working directory in supported interactive, non-interactive,
resumed/forked and subagent consumers. Desktop/IDE coverage SHALL require
actual configuration-consumer evidence rather than a CLI-only claim. Existing
processes needing registration reload SHALL be identified explicitly.

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

## REMOVED Requirements

### Requirement: Four globally available MCP integrations
**Reason**: Graphify duplicated the graph-discovery role of the selected CodeGraph provider while measured use collapsed to a handful of calls, so it leaves the managed selection to reduce surface and maintenance.
**Migration**: Shared installations, saved graphs and explicit local tools remain; re-adding a registration is a deliberate local action outside the managed selection.

## MODIFIED Requirements

### Requirement: MCP operations preserve useful source capabilities

Each retained integration SHALL preserve its accepted explicit source operations
through a bounded model-facing surface: suitable Serena navigation, references
and semantic edits; CodeGraph bounded symbol search and retained detail reads
with bounded automatic and manual incremental refresh; Nuphus authorized
desktop/browser inspection and interaction. CodeGraph deliberate full indexing,
explicit catch-up and status inspection SHALL remain available through a native
CLI control command outside model sessions, and the CodeGraph MCP tools/list
SHALL expose only the bounded query surface. The Serena model-facing tool list
SHALL exclude memory, onboarding and configuration-introspection tools; native
Git records remain the authoritative memory route, and an explicit escape hatch
SHALL keep an unfiltered debugging view possible. Diagnostic retention SHALL
follow the subscription-efficiency assessment and SHALL distinguish a returned
empty object from authoritative current completion. Tool filtering and
unavailable operations SHALL remain explicit. Representative actual calls,
rather than a tool list, SHALL establish delivered operation scope. Acceptance
mutations SHALL use owned targets. CodeGraph's approximate edges SHALL be
treated as candidates; required exact-reference and refactoring claims SHALL be
checked with current Serena or source evidence.

#### Scenario: Each MCP is exercised by its consumer
- **WHEN** acceptance performs a meaningful read and applicable bounded mutation through each retained MCP
- **THEN** the actual result or owned-target effect and server identity are recorded, with diagnostic uncertainty preserved

#### Scenario: Maintenance moves outside the model session
- **WHEN** a root needs a deliberate full index, explicit catch-up or status inspection
- **THEN** the native CLI control command performs it without a model session, and the MCP catalogue still exposes only the bounded query tools

#### Scenario: Filtered Serena surface stays debuggable
- **WHEN** a debugging session needs the unfiltered Serena tool list
- **THEN** an explicit escape hatch restores it for that client without changing the default model-facing surface

#### Scenario: A required MCP operation fails
- **WHEN** discovery succeeds but an operation selected for delivery fails
- **THEN** that operation remains unverified and its original cause is retained; an unrelated passing call does not establish its support

#### Scenario: Index excludes a maintained input
- **WHEN** a needed source, configuration or specification file is unsupported, oversized, ignored or only partly extracted
- **THEN** coverage reports that limit and the appropriate current semantic or text tool remains usable; a successful exit code does not certify complete source coverage
