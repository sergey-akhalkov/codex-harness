## REMOVED Requirements

### Requirement: Enforced bounded indexing

**Reason**: The kit no longer indexes repositories; every CodeGraph indexing,
watch and admission path is deleted with the integration, so there is no
indexing operation left to bound.
**Migration**: Host-side saved indexes stay inert residue; any future indexing
capability reintroduces its own bounded-resource requirement.

### Requirement: Deliberate scope and honest freshness

**Reason**: Freshness semantics described watcher-backed CodeGraph queries,
which no longer exist. Serena/source remain the current evidence route, and
their contracts live in the code-tools specification.

### Requirement: Shared project resources across sessions

**Reason**: Index, watcher and cross-client graph backend reuse is deleted
with CodeGraph. Serena's compatible same-project worker reuse remains covered
by "Compatible language-server reuse" and "Safe reuse across MCP
integrations".

### Requirement: Bounded index and response storage

**Reason**: There are no owned indexes, staging generations or retained
CodeGraph response details left to store or bound after the removal.

## MODIFIED Requirements

### Requirement: Global delivery and measured acceptance

The kit SHALL install and update reusable resource policy, launchers and
services globally with recoverable configuration changes and documented
restart boundaries. Uninstall or rollback SHALL preserve later user edits and
unrelated tools. Acceptance SHALL include owned resource-exhaustion and crash
fixtures, same-project reuse and cross-project isolation, representative real
MCP calls from outside this checkout, and measured idle/active process memory
and CPU. Reports SHALL identify versions, inputs, enforcement scope and any
unverified limits. The change SHALL preserve separate Codex CLI applications
and SHALL not install or enable a common Codex app-server.

#### Scenario: Delivery is accepted
- **WHEN** the resource optimization is declared complete
- **THEN** global activation and all associated tasks have evidence, the resource regression is resolved within its enforced scope, and no shared Codex server was introduced
