# Bounded tool resources

## Purpose

Keep globally connected code tools responsive and bounded on a resource-constrained workstation while preserving useful code intelligence and isolation between concurrent projects and Codex sessions.

## Requirements

### Requirement: Enforced bounded indexing

Every Codebase Memory index operation launched through the kit SHALL have a finite execution deadline, one account-wide indexing admission slot, and enforced Windows process-tree limits. The initial policy SHALL permit at most two extraction workers, 2 GiB aggregate committed memory and 25 percent of host CPU for the indexing job. Internal advisory budgets MUST NOT be represented as OS enforcement. Concurrent requests SHALL wait within their deadline or receive an explicit busy result. Resource denial, timeout or cancellation SHALL preserve the previously committed index and SHALL not trigger unbounded retries or an unbounded fallback.

#### Scenario: The indexer ignores its internal budget
- **WHEN** a supervised worker attempts allocations beyond the configured hard limit
- **THEN** Windows refuses excess committed memory, the request reports failure, owned descendants are reclaimed, and an existing committed index remains readable

#### Scenario: Three sessions request indexing
- **WHEN** three sessions request indexing concurrently
- **THEN** at most one kit-owned indexing job runs and waiting or busy responses respect finite deadlines

### Requirement: Deliberate scope and honest freshness

The kit SHALL disable uncontrolled automatic Codebase Memory indexing and watch registration for its delivered configuration, preserving existing indexes and explicit indexing operations. It SHALL provide reusable setup for project ignore rules and apply approved generated-log exclusions to the locally selected large acceptance repository. Product code and specification documents SHALL remain accessible. Query responses SHALL make the explicit-refresh policy discoverable and SHALL not claim automatic freshness. Restoring automatic watching SHALL require an explicit policy change rather than an implicit retry or startup fallback.

#### Scenario: A source edit occurs during an ordinary session
- **WHEN** files change without an explicit index request
- **THEN** the kit does not start a background full rebuild, and the agent can distinguish retained graph information from current source

#### Scenario: The affected consumer is indexed
- **WHEN** the locally selected large acceptance repository is indexed with the delivered ignore policy
- **THEN** its generated HTML log directory is excluded while representative product symbols and specification documents remain available through the appropriate code or text tools

### Requirement: Owned tool process lifecycle

Long-lived local tool services and temporary workers SHALL own their child processes so that normal exit, abrupt owner death and initialization failure do not leave working or suspended descendants. Windows ownership SHALL be established before child execution and SHALL not depend solely on atexit or process-name searches. Historical cleanup SHALL verify executable, creation identity, relevant invocation and missing original ownership immediately before cleanup; unrelated or ambiguously owned trees SHALL be preserved and reported.

#### Scenario: A language-server owner crashes
- **WHEN** an owned service with language-server grandchildren is forcibly terminated in an acceptance fixture
- **THEN** its child tree terminates without affecting a separate fixture or the controlling Codex process

### Requirement: Compatible language-server reuse

Retained explicit language operations SHALL reuse compatible project-scoped services where accepted by the subscription-efficiency assessment. Different roots and incompatible configurations SHALL NOT share document state. Backend caches and idle service lifetime SHALL be bounded. Requests and results SHALL retain their originating client and project identity; unavailable or unverified checks SHALL remain explicit. Resource reuse SHALL NOT restore rejected hooks, diagnostic carriers, pre-edit baselines, pending reconciliation or Stop delivery. No separate harness diagnostic broker SHALL be mandatory when no accepted capability uses it.

#### Scenario: Two sessions inspect the same project
- **WHEN** compatible same-project language requests arrive from two sessions
- **THEN** both reuse the compatible retained backend while their project and request identities remain separate

#### Scenario: Projects or settings differ
- **WHEN** requests use another canonical root or changed effective compiler settings
- **THEN** the service selects a distinct or refreshed backend and does not return another project's document results

#### Scenario: An explicit client exits while work is running
- **WHEN** a short-lived client disconnects after its check was admitted
- **THEN** admitted work drains or is cancelled under its owner's finite lifetime without automatic hook retries or duplicate delivery

#### Scenario: The client that started a shared broker closes
- **WHEN** the first Windows MCP client closes while another client still uses its newly started shared LSP or Serena broker
- **THEN** the same broker and compatible backend remain available to the second client, and normal idle or explicit retirement still reclaims their children

### Requirement: Safe reuse across MCP integrations

Serena SHALL reuse a backend for compatible clients of the same canonical project while routing project activation per client so it cannot switch another client's active project. Graphify SHALL reuse its verified shared endpoint where compatible and preserve explicit repository context. Nuphus SHALL preserve isolated lazy browser state and element-reference ownership unless shared execution provides equivalent isolation; incompatible state SHALL remain separate rather than being silently merged. All retained tool processes SHALL have bounded idle retirement. Ordinary startup SHALL avoid unnecessary resident shell wrappers without changing tool schemas, STDIO cleanliness or required operations.

#### Scenario: Serena clients choose different projects
- **WHEN** one client activates another project
- **THEN** its subsequent calls reach that project's backend and other clients retain their selected project

#### Scenario: Browser clients coexist
- **WHEN** two clients inspect separate owned browser targets
- **THEN** one client's tabs or element references cannot cause an action in the other client's target, and unused browser engines are not started

### Requirement: Global delivery and measured acceptance

The kit SHALL install and update reusable resource policy, launchers and services globally with recoverable configuration changes and documented restart boundaries. Uninstall or rollback SHALL preserve later user edits and unrelated tools. Acceptance SHALL include owned resource-exhaustion and crash fixtures, same-project reuse and cross-project isolation, representative real MCP calls from outside this checkout, the locally selected large acceptance repository indexing case, and measured idle/active process memory and CPU. Reports SHALL identify versions, inputs, enforcement scope and any unverified limits. The change SHALL preserve separate Codex CLI applications and SHALL not install or enable a common Codex app-server.

#### Scenario: Delivery is accepted
- **WHEN** the resource optimization is declared complete
- **THEN** global activation and all associated tasks have evidence, the resource regression is resolved within its enforced scope, and no shared Codex server was introduced
