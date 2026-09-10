## MODIFIED Requirements

### Requirement: Deliberate scope and honest freshness

The kit SHALL disable uncontrolled automatic Codebase Memory indexing and watch registration for its delivered configuration, preserving existing indexes and explicit indexing operations. It SHALL provide reusable setup for project ignore rules and apply approved generated-log exclusions to the locally selected large acceptance repository. Product code and specification documents SHALL remain accessible. Query responses SHALL make the explicit-refresh policy discoverable and SHALL not claim automatic freshness. Restoring automatic watching SHALL require an explicit policy change rather than an implicit retry or startup fallback.

#### Scenario: A source edit occurs during an ordinary session
- **WHEN** files change without an explicit index request
- **THEN** the kit does not start a background full rebuild, and the agent can distinguish retained graph information from current source

#### Scenario: The affected consumer is indexed
- **WHEN** the locally selected large acceptance repository is indexed with the delivered ignore policy
- **THEN** its generated HTML log directory is excluded while representative product symbols and specification documents remain available through the appropriate code or text tools

### Requirement: Global delivery and measured acceptance

The kit SHALL install and update reusable resource policy, launchers and services globally with recoverable configuration changes and documented restart boundaries. Uninstall or rollback SHALL preserve later user edits and unrelated tools. Acceptance SHALL include owned resource-exhaustion and crash fixtures, same-project reuse and cross-project isolation, representative real MCP calls from outside this checkout, the locally selected large acceptance repository indexing case, and measured idle/active process memory and CPU. Reports SHALL identify versions, inputs, enforcement scope and any unverified limits. The change SHALL preserve separate Codex CLI applications and SHALL not install or enable a common Codex app-server.

#### Scenario: Delivery is accepted
- **WHEN** the resource optimization is declared complete
- **THEN** global activation and all associated tasks have evidence, the resource regression is resolved within its enforced scope, and no shared Codex server was introduced
