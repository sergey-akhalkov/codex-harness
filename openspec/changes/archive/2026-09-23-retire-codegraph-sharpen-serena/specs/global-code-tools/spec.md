## RENAMED Requirements

- FROM: `### Requirement: Three globally available MCP integrations`
- TO: `### Requirement: Two globally available MCP integrations`

## REMOVED Requirements

### Requirement: Rust-owned CodeGraph integration

**Reason**: CodeGraph is fully retired and its first-party modules, CLI routes
and executable acceptance fixtures are deleted. No CodeGraph integration
remains to own, so the language-ownership requirement has no object.
**Migration**: The published package, bundled Node and any saved host state
are inert residue outside kit ownership; any future graph tool is a new
deliberate adoption under the current Rust rule.

### Requirement: Reproducible and reversible CodeGraph delivery

**Reason**: There is no CodeGraph delivery to reproduce or roll back after
the full removal; the generic registration journal covers removal of any
owned retired registration and recovery of interrupted activations.
**Migration**: Update removes an owned `codegraph` registration recorded by
an earlier version while preserving unrelated settings and host residue.

### Requirement: CodeGraph MCP starts with Codex CLI

**Reason**: No managed CodeGraph registration exists, so startup readiness
for it is no longer a deliverable.
**Migration**: Serena and Nuphus keep their startup contract; a fresh session
catalogue no longer contains CodeGraph.

### Requirement: Check reports CodeGraph serving admission

**Reason**: With no registration or serving command, serving admission for
CodeGraph cannot be reported; Check instead reports the retirement through
the registration journal without mutating configuration.

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

#### Scenario: Codex starts in an unrelated project
- **WHEN** Codex starts outside the checkout after this change is active
- **THEN** Serena and Nuphus are available, and retired Graphify, Codebase Memory and CodeGraph registrations do not reappear

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

### Requirement: MCP operations preserve useful source capabilities

Each retained integration SHALL preserve its accepted explicit source
operations through a bounded model-facing surface: suitable Serena
navigation, references, semantic edits and explicit diagnostics; Nuphus
authorized desktop/browser inspection and interaction. The Serena
model-facing tool list SHALL exclude memory, onboarding,
configuration-introspection tools and `search_for_pattern`; native Git
records remain the authoritative memory route, an explicit escape hatch
SHALL keep an unfiltered debugging view possible, and literal text, regex,
configuration and document search SHALL route to scoped native search
(`rg`). Diagnostic retention SHALL follow the
subscription-efficiency assessment and SHALL distinguish a returned empty
object from authoritative current completion. Tool filtering and
unavailable operations SHALL remain explicit. Representative actual calls,
rather than a tool list, SHALL establish delivered operation scope.
Acceptance mutations SHALL use owned targets. Required exact-reference and
refactoring claims SHALL be checked with current Serena or source evidence.

#### Scenario: Each MCP is exercised by its consumer
- **WHEN** acceptance performs a meaningful read and applicable bounded mutation through each retained MCP
- **THEN** the actual result or owned-target effect and server identity are recorded, with diagnostic uncertainty preserved

#### Scenario: Maintenance moves outside the model session
- **WHEN** any retired integration's host residue needs maintenance or deletion
- **THEN** it is handled outside the kit with explicit user action, and no managed MCP catalogue, registration or kit CLI route is restored

#### Scenario: Filtered Serena surface stays debuggable
- **WHEN** a debugging session needs the unfiltered Serena tool list
- **THEN** an explicit escape hatch restores it for that client without changing the default model-facing surface

#### Scenario: A required MCP operation fails
- **WHEN** discovery succeeds but an operation selected for delivery fails
- **THEN** that operation remains unverified and its original cause is retained; an unrelated passing call does not establish its support

#### Scenario: Index excludes a maintained input
- **WHEN** a needed source, configuration or specification file is unsupported, oversized, ignored or only partly extracted by any indexed or semantic route
- **THEN** coverage reports that limit and the appropriate current semantic or text tool remains usable; a successful exit code does not certify complete source coverage

#### Scenario: Literal text search stays native
- **WHEN** a session needs literal text, regex, configuration or document matches
- **THEN** the managed Serena catalogue offers no `search_for_pattern`, the call is rejected explicitly if attempted, and scoped `rg` remains the documented route

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

#### Scenario: Codex CLI starts after later source edits
- **WHEN** a consumer starts Codex CLI through the installed launcher after the linked checkout has changed relative to the recorded native build, and the recorded binaries still match
- **THEN** Serena and Nuphus complete MCP initialize and are present in the session catalogue, without a rebuild or extra per-session setup

#### Scenario: Interactive, non-interactive and child sessions share the same host settings
- **WHEN** interactive, non-interactive, resumed/forked or tool-capable child consumers load the same installed host configuration
- **THEN** each session gets usable Serena and Nuphus MCP connections, or an explicit startup error for the failed server that does not omit the other retained servers

#### Scenario: Source-consuming MCP stays gated
- **WHEN** the same source-stale manager with matching hashes is asked to serve `mcp codebase-memory`
- **THEN** that command is refused, Serena and Nuphus remain callable, and ordinary Codex startup does not rebuild native source

### Requirement: Workspace and session isolation

Project-sensitive calls, language-server state and diagnostics SHALL be bound
to a canonical project/worktree root. Concurrent sessions and subagents MUST
NOT select another project's mutable workspace or consume its diagnostic
results. Changes to project roots and user-approved additional roots SHALL be
resolved explicitly. Shared immutable installations are permitted; an
intentionally shared service SHALL retain its identity instead of being
represented as the current project's state.

#### Scenario: Two projects use the same symbol names
- **WHEN** two concurrent sessions navigate and edit their respective projects
- **THEN** each receives only the applicable project results and closing one session does not invalidate the other

#### Scenario: A worktree or nested project changes the applicable root
- **WHEN** a session operates in that root
- **THEN** the selected project, configuration, cache identity and reported file paths agree with that root
